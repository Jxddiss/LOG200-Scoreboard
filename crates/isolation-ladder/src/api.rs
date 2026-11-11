use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use http_body_util::LengthLimitError;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio::task::spawn_blocking;

use isolation_core::remote::PATH as REMOTE_PATH;

use crate::arena::Arena;
use crate::db::{Db, Stat};
use crate::hub::Hub;
use crate::remote::{self, Registry, Remote};
use crate::spectate;
use crate::submit::{Submissions, SubmitError};

/// Serves the API while it lives.
pub struct Api {
    _runtime: Runtime,
}

/// One router on every configured address: the existing reverse proxies keep their routes.
pub fn serve(arena: &Arc<Arena>, hub: &Arc<Hub>, registry: Option<&Arc<Registry>>) -> anyhow::Result<Option<Api>> {
    let cfg = arena.config();
    let mut router = Router::new();
    let mut announced: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    if let Some(submit) = &cfg.submit {
        router = router.merge(submit_routes(Arc::new(Submissions::new(
            Arc::clone(arena),
            submit.clone(),
        ))));
        announced
            .entry(&submit.listen)
            .or_default()
            .push("soumissions : PUT http://{}/api/submit");
    }
    router = router.merge(read_routes(cfg.db.clone()));
    if let Some(live) = &cfg.live {
        router = router.merge(
            Router::new()
                .route(spectate::PATH, get(spectate::watch))
                .with_state(Arc::clone(hub)),
        );
        announced
            .entry(&live.listen)
            .or_default()
            .push("direct : ws://{}/api/live");
    }
    if let (Some(remote), Some(submit), Some(registry)) = (&cfg.remote, &cfg.submit, registry) {
        let state = Remote::new(Arc::clone(registry), submit.teams.clone(), cfg.time_ms);
        router = router.merge(Router::new().route(REMOTE_PATH, get(remote::upgrade)).with_state(state));
        announced
            .entry(&remote.listen)
            .or_default()
            .push("équipes distantes : ws://{}/api/remote");
    }
    if announced.is_empty() {
        return Ok(None);
    }
    let router = router.fallback(not_found).layer(DefaultBodyLimit::disable());
    let runtime = Runtime::new()?;
    for (address, services) in announced {
        let listener = runtime
            .block_on(TcpListener::bind(address))
            .with_context(|| format!("écoute de l'API sur {address}"))?;
        let local = listener.local_addr()?.to_string();
        for service in services {
            eprintln!("{}", service.replace("{}", &local));
        }
        runtime.spawn(
            axum::serve(
                listener,
                router.clone().into_make_service_with_connect_info::<SocketAddr>(),
            )
            .into_future(),
        );
    }
    Ok(Some(Api { _runtime: runtime }))
}

fn submit_routes(submissions: Arc<Submissions>) -> Router {
    Router::new()
        .route("/api/health", get(async || "ok\n").fallback(not_found))
        .route(
            "/api/version",
            get(async || concat!(env!("CARGO_PKG_VERSION"), "\n")).fallback(not_found),
        )
        .route("/api/submit", put(submit).post(submit).fallback(not_found))
        .with_state(submissions)
}

/// Lectures publiques de la base, chacune sur sa propre connexion en lecture seule.
fn read_routes(db: PathBuf) -> Router {
    let recent = Recent {
        db: db.clone(),
        cache: Mutex::new(None),
    };
    Router::new()
        .route("/api/games/{id}", get(game))
        .route("/api/stats/{name}", get(stats))
        .with_state(Arc::new(db))
        .merge(Router::new().route("/api/games", get(recent_games)).with_state(Arc::new(recent)))
}

const RECENT_GAMES: usize = 5000;
const RECENT_TTL: Duration = Duration::from_secs(60);

/// Les dernières parties, relues au plus une fois par `RECENT_TTL` quel que soit le nombre d'appels.
struct Recent {
    db: PathBuf,
    cache: Mutex<Option<(Instant, Bytes)>>,
}

impl Recent {
    // Le verrou reste pris pendant la lecture : les appels simultanés attendent au lieu de relire la base.
    fn get(&self) -> rusqlite::Result<Bytes> {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, body)) = &*cache
            && at.elapsed() < RECENT_TTL
        {
            return Ok(body.clone());
        }
        match Db::open_read(&self.db).and_then(|db| db.recent_jsonl(RECENT_GAMES)) {
            Ok(text) => {
                let body = Bytes::from(text);
                *cache = Some((Instant::now(), body.clone()));
                Ok(body)
            }
            Err(e) => match &*cache {
                Some((_, stale)) => {
                    eprintln!("lecture de la base : {e} ; dernières parties servies depuis le cache");
                    Ok(stale.clone())
                }
                None => Err(e),
            },
        }
    }
}

async fn recent_games(State(recent): State<Arc<Recent>>) -> Response {
    match blocking(move || recent.get()).await {
        Ok(body) => (
            [(CONTENT_TYPE, "application/x-ndjson"), (CACHE_CONTROL, "public, max-age=60")],
            body,
        )
            .into_response(),
        Err(e) => unavailable(&e),
    }
}

fn json(cache: &'static str, body: String) -> Response {
    ([(CONTENT_TYPE, "application/json"), (CACHE_CONTROL, cache)], body).into_response()
}

fn unavailable(error: &rusqlite::Error) -> Response {
    eprintln!("lecture de la base : {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "base indisponible\n").into_response()
}

async fn game(State(db): State<Arc<PathBuf>>, UrlPath(id): UrlPath<String>) -> Response {
    match blocking(move || Db::open_read(&db)?.game_json(&id)).await {
        // Une partie finie ne change plus : un CDN peut la garder indéfiniment.
        Ok(Some(body)) => json("public, max-age=31536000, immutable", body),
        Ok(None) => (StatusCode::NOT_FOUND, "partie inconnue\n").into_response(),
        Err(e) => unavailable(&e),
    }
}

#[derive(Deserialize)]
struct StatsQuery {
    session: Option<String>,
    team: Option<String>,
}

async fn stats(
    State(db): State<Arc<PathBuf>>,
    UrlPath(name): UrlPath<String>,
    Query(query): Query<StatsQuery>,
) -> Response {
    let stat = match name.as_str() {
        "teams" => Stat::Teams,
        "head-to-head" => Stat::HeadToHead,
        "openings" => Stat::Openings,
        "builds" if query.team.is_some() => Stat::Builds,
        "builds" => return (StatusCode::BAD_REQUEST, "paramètre team manquant\n").into_response(),
        _ => return (StatusCode::NOT_FOUND, "statistique inconnue\n").into_response(),
    };
    let StatsQuery { session, team } = query;
    match blocking(move || Db::open_read(&db)?.stats(stat, session.as_deref(), team.as_deref())).await {
        // Le rythme de rafraîchissement de la page, comme /data.
        Ok(body) => json("public, max-age=15", body),
        Err(e) => unavailable(&e),
    }
}

async fn not_found() -> SubmitError {
    SubmitError::NotFound
}

async fn submit(
    State(submissions): State<Arc<Submissions>>,
    headers: HeaderMap,
    body: Body,
) -> Result<String, SubmitError> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let team = blocking({
        let submissions = Arc::clone(&submissions);
        move || submissions.authenticate(authorization.as_deref())
    })
    .await?;
    let jar = to_bytes(body, submissions.max_body())
        .await
        .map_err(|error| body_error(error, submissions.max_jar_mb()))?;
    blocking(move || submissions.submit(&team, &jar))
        .await
        .map(|accepted| format!("{accepted}\n"))
}

fn body_error(error: axum::Error, max_mb: u64) -> SubmitError {
    let error = error.into_inner();
    if error.is::<LengthLimitError>() {
        SubmitError::TooLarge { max_mb }
    } else {
        SubmitError::Read(std::io::Error::other(error))
    }
}

/// The registry is a file and the smoke test plays two games: neither may block the runtime.
pub async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    spawn_blocking(work)
        .await
        .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn a_body_over_the_limit_is_too_large() {
        let limit = 8;
        let error = to_bytes(Body::from(vec![0; limit + 1]), limit).await.unwrap_err();
        assert_eq!(body_error(error, 1).status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
