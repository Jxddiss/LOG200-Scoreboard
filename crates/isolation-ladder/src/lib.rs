//! Enchaîne les parties entre équipes, une paire par slot libre, et ajoute une ligne par partie au
//! JSONL. Les équipes sont celles de `ladder.toml` plus les équipes inscrites qui ont un jar
//! accepté ou une session distante ouverte ; la liste est relue avant chaque paire.

mod api;
mod arena;
mod command;
mod config;
mod db;
mod hub;
mod live;
mod model;
mod remote;
mod roster;
mod schedule;
mod spectate;
mod standings;
mod store;
mod submit;
mod sync;
mod team_id;
mod thermal;
mod tournament;

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use clap::{Parser, Subcommand};
use rand::{Rng, RngExt};

use isolation_core::board::Move;
use isolation_core::notation;
use isolation_core::record::{GameRecord, TournamentGame, rfc3339_at};
use isolation_core::rules::random_opening;

use arena::{Arena, Fixture, GameError, Sigterm};
use config::Config;
use db::Db;
use hub::Hub;
use live::Live;
use remote::Registry;
use roster::{Team, multithread_game, roster};
use schedule::{Announcements, Scheduler};
use standings::Standings;
use team_id::TeamId;
use thermal::Thermal;
use tournament::admin::{self, Request};
use tournament::{Organizer, Pending};

/// Pause après une partie annulée : une config cassée ne doit pas faire tourner le ladder à vide.
const RETRY_DELAY: Duration = Duration::from_secs(5);
/// Avant de rejouer une partie de tournoi annulée sur un `no_show` : le plus souvent, une machine
/// surchargée ou trop chaude, qu'on laisse souffler.
const NO_SHOW_RETRY_DELAY: Duration = Duration::from_secs(30);
const IDLE_DELAY: Duration = Duration::from_secs(10);
const LOG_RETENTION: Duration = Duration::from_hours(60 * 24);

#[derive(Parser)]
#[command(
    name = "isolation-ladder",
    version,
    about = "Ladder Isolation : parties en continu et soumission des jars"
)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(required = true)]
    config: Option<PathBuf>,
    #[arg(long)]
    rounds: Option<u64>,
}

#[derive(Subcommand)]
enum Command {
    AddTeam {
        id: TeamId,
        #[arg(long, default_value = submit::DEFAULT_TEAMS)]
        teams: PathBuf,
    },
    /// Requête lue par le ladder en marche.
    Tournament {
        #[command(subcommand)]
        action: TournamentAction,
    },
}

#[derive(Subcommand)]
enum TournamentAction {
    Start {
        config: PathBuf,
    },
    Resume {
        config: PathBuf,
        id: String,
        #[arg(value_name = "MATCH")]
        match_id: String,
    },
}

pub fn cli() -> ExitCode {
    let cli = Cli::parse();
    let result = match (cli.command, cli.config) {
        (Some(Command::AddTeam { id, teams }), _) => submit::add_team(&teams, id)
            .map(|token| println!("{token}"))
            .map_err(anyhow::Error::from),
        (Some(Command::Tournament { action }), _) => request(action),
        (None, Some(path)) => run(&path, cli.rounds),
        (None, None) => unreachable!("clap exige la configuration sans sous-commande"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e:#}");
            ExitCode::from(2)
        }
    }
}

fn load(path: &Path) -> anyhow::Result<Config> {
    fs::read_to_string(path)
        .with_context(|| path.display().to_string())?
        .parse()
        .with_context(|| path.display().to_string())
}

fn request(action: TournamentAction) -> anyhow::Result<()> {
    let (path, request) = match action {
        TournamentAction::Start { config } => (config, Request::Start),
        TournamentAction::Resume { config, id, match_id } => (config, Request::Resume { id, match_id }),
    };
    let sent = admin::send(&load(&path)?, &request)?;
    println!(
        "requête déposée dans {} : le ladder la lit sous {} s",
        sent.display(),
        IDLE_DELAY.as_secs()
    );
    Ok(())
}

/// PID 1 sans init : chaque bac à sable laisse un `containerd-shim` orphelin, rattaché à nous, que
/// personne n'attend. Les zombies s'accumulent jusqu'à la limite de processus du conteneur, puis plus
/// aucun joueur ne démarre (`no_show` partout). L'image lance un init (`docker-init`) devant nous.
fn warn_if_init() {
    if std::process::id() == 1 {
        eprintln!(
            "ATTENTION : isolation-ladder est le PID 1, sans init pour ramasser les orphelins. Les \
             zombies vont s'accumuler jusqu'à bloquer les parties : lancer le conteneur avec \
             `init: true` (compose) ou `docker run --init`."
        );
    }
}

/// Base ouverte, ancien `games.jsonl` importé puis renommé si elle n'a encore aucune partie.
fn open_db(cfg: &Config) -> anyhow::Result<Db> {
    let path = &cfg.db;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    let db = Db::open(path).with_context(|| path.display().to_string())?;
    let old = &cfg.out;
    if old.exists() {
        if db.is_empty()? {
            let count = db.import_jsonl(old).with_context(|| old.display().to_string())?;
            let done = old.with_extension("jsonl.migrated");
            fs::rename(old, &done).with_context(|| done.display().to_string())?;
            eprintln!("{count} parties importées de {} dans {}", old.display(), path.display());
        } else {
            eprintln!(
                "ATTENTION : {} ignoré, {} contient déjà des parties",
                old.display(),
                path.display()
            );
        }
    }
    Ok(db)
}

/// Matchmaking, classement et tournoi en cours, rebâtis depuis la base.
fn restore(cfg: &Config, rounds: Option<u64>) -> anyhow::Result<(Db, Scheduler, Standings, Organizer)> {
    let db = open_db(cfg)?;
    let games = db.games().with_context(|| cfg.db.display().to_string())?;
    let scheduler = Scheduler::from_history(&games, &cfg.session);
    let standings =
        Standings::from_history(cfg.data_dir(), &games, &db).with_context(|| cfg.data_dir().display().to_string())?;
    let tournaments = Organizer::new(cfg, rounds.is_none(), &games)?;
    tournaments.publish_preview(cfg, &standings);
    Ok((db, scheduler, standings, tournaments))
}

fn run(path: &Path, rounds: Option<u64>) -> anyhow::Result<()> {
    warn_if_init();
    let sigterm = Sigterm::listen().context("écoute de SIGTERM")?;
    let cfg = load(path)?;
    let logs = cfg.logs_dir();
    fs::create_dir_all(&logs).with_context(|| logs.display().to_string())?;
    let (db, scheduler, mut standings, mut tournaments) = restore(&cfg, rounds)?;
    let mut live = Live::new(cfg.data_dir().to_path_buf()).with_context(|| cfg.data_dir().display().to_string())?;

    let (reports, received) = mpsc::channel();
    let hub = live_hub(&reports);
    let remote = remote_registry(&cfg, &reports);
    let arena = Arc::new(Arena::new(cfg, Arc::clone(&hub)));
    arena.drain_on(sigterm);
    arena.sweep();
    let _api = api::serve(&arena, &hub, remote.as_ref())?;
    let cfg = arena.config();

    let mut rng = rand::rng();
    let mut waiting = false;
    let mut warned = HashSet::new();
    let mut pairs = Pairs::new(scheduler);
    let mut thermal = Thermal::new(cfg.thermal.as_ref());
    loop {
        if !arena.draining() && rounds.is_none_or(|n| pairs.round < n) {
            tournaments.tick(cfg, &standings, rng.random());
            tournaments.poll(cfg, &standings, rng.random());
            let cool = thermal.as_mut().is_none_or(Thermal::cool);
            if cool {
                launch_tournament(&arena, &mut tournaments, &mut pairs.busy, &mut pairs.running, &reports);
            }
            let teams = roster(cfg, remote.as_ref(), &mut warned);
            let waiting_teams = tournaments.waiting();
            pairs.cancel_stale(cfg, &teams, &waiting_teams);
            if cool {
                pairs.launch_announced(&arena, &teams, &reports, &mut rng);
            }
            if cool && pairs.running.iter().sum::<usize>() < cfg.slots() && pairs.open(rounds) {
                let unavailable = pairs.unavailable(&waiting_teams, None);
                let running = pairs.running;
                let fits = |a: &Team, b: &Team| {
                    let multithread = multithread_game(cfg, a, b);
                    running[usize::from(multithread)] < cfg.slots_of(multithread)
                };
                if let Some((a, b)) =
                    pairs
                        .scheduler
                        .next_pair(&teams, &unavailable, fits, |id| standings.rating(id), &mut rng)
                {
                    waiting = false;
                    let pair = [a.clone(), b.clone()];
                    pairs.launch(&arena, &teams, pair, &reports, &mut rng);
                    continue;
                }
                if pairs.running == [0; 2] && !waiting {
                    eprintln!("en attente d'au moins deux équipes ({} pour l'instant)", teams.len());
                    waiting = true;
                }
            }
        } else if pairs.running == [0; 2] && arena.smoke_tests() == 0 {
            break;
        }
        let Ok(report) = received.recv_timeout(IDLE_DELAY) else {
            continue;
        };
        match report {
            Report::Wake => {}
            Report::Started { red, black, game } => published(live.start(&red, &black, game.as_ref())),
            Report::Assigned { red, black, id } => published(live.assign(&red, &black, &id)),
            Report::Played {
                red,
                black,
                outcome,
                game,
            } => {
                published(live.end(&red, &black));
                let saved = save(cfg, &db, &mut standings, outcome, &red, &black)?;
                tournaments.publish_preview(cfg, &standings);
                if let Some(game) = game {
                    tournaments.record(cfg, &game, saved.as_ref());
                }
            }
            Report::Resting(rest) => {
                if !arena.draining() && pairs.open(rounds) {
                    let teams = roster(cfg, remote.as_ref(), &mut warned);
                    let rating = |id: &str| standings.rating(id);
                    pairs.announce(cfg, &teams, &tournaments.waiting(), rest, rating, &mut rng);
                }
            }
            Report::Done { pair, multithread } => {
                pairs.done(&pair, multithread);
                prune_logs(&logs, LOG_RETENTION);
            }
        }
    }
    hub.shutdown();
    Ok(())
}

/// Paires du ladder : slots occupés, équipes prises et paires annoncées pendant la pause d'un slot.
struct Pairs {
    scheduler: Scheduler,
    /// Paires en cours, par genre de slot (ordinaire, multithread), pause comprise.
    running: [usize; 2],
    busy: HashSet<String>,
    announced: Announcements,
    round: u64,
}

impl Pairs {
    fn new(scheduler: Scheduler) -> Self {
        Self {
            scheduler,
            running: [0; 2],
            busy: HashSet::new(),
            announced: Announcements::default(),
            round: 0,
        }
    }

    /// Une paire de plus tient dans `rounds`, annonces comprises.
    fn open(&self, rounds: Option<u64>) -> bool {
        let announced = u64::try_from(self.announced.len()).unwrap_or(u64::MAX);
        rounds.is_none_or(|n| self.round.saturating_add(announced) < n)
    }

    /// Équipes hors tirage. `resting` : la paire en pause dont le slot est à pourvoir, comptée libre.
    fn unavailable(&self, waiting: &HashSet<String>, resting: Option<&[String; 2]>) -> HashSet<String> {
        let busy = self
            .busy
            .iter()
            .filter(|id| resting.is_none_or(|pair| !pair.contains(id)));
        busy.chain(waiting)
            .cloned()
            .chain(self.announced.teams().map(String::from))
            .collect()
    }

    fn publish(&self, cfg: &Config, teams: &[Team]) {
        let queued = self.scheduler.publish(cfg.data_dir(), teams, &self.announced);
        queued.unwrap_or_else(|e| eprintln!("   file d'attente non publiée : {e}"));
    }

    fn launch(
        &mut self,
        arena: &Arc<Arena>,
        teams: &[Team],
        [a, b]: [Team; 2],
        reports: &Sender<Report>,
        rng: &mut impl Rng,
    ) {
        let cfg = arena.config();
        self.round += 1;
        self.running[usize::from(multithread_game(cfg, &a, &b))] += 1;
        self.scheduler.record(&a.id, &b.id, 2);
        self.busy.extend([a.id.clone(), b.id.clone()]);
        self.publish(cfg, teams);
        let opening = random_opening(cfg.opening_plies, rng);
        eprintln!(
            "== paire {} : {} contre {} (ouverture {})",
            self.round,
            a.id,
            b.id,
            notation::join(&opening)
        );
        spawn_pair(Arc::clone(arena), a, b, opening, reports.clone());
    }

    /// Tire la paire qui prendra le slot de `rest` à la fin de sa pause, et l'annonce.
    fn announce(
        &mut self,
        cfg: &Config,
        teams: &[Team],
        waiting: &HashSet<String>,
        rest: Rest,
        rating: impl Fn(&str) -> f64,
        rng: &mut impl Rng,
    ) {
        let unavailable = self.unavailable(waiting, Some(&rest.pair));
        let fits = |a: &Team, b: &Team| multithread_game(cfg, a, b) == rest.multithread;
        let Some((a, b)) = self.scheduler.next_pair(teams, &unavailable, fits, rating, rng) else {
            return;
        };
        eprintln!("   annonce : {} contre {} à {}", a.id, b.id, rest.at);
        let pair = [a.clone(), b.clone()];
        self.announced.add(rest.pair, rest.multithread, pair, rest.at);
        self.publish(cfg, teams);
    }

    fn cancel_stale(&mut self, cfg: &Config, teams: &[Team], waiting: &HashSet<String>) {
        if self.announced.cancel_stale(teams, &self.busy, waiting) {
            eprintln!("   annonce annulée : nouveau jar, équipe absente ou tournoi");
            self.publish(cfg, teams);
        }
    }

    /// Lance les annonces dont la pause est finie, sauf si un tournoi a pris leur slot.
    fn launch_announced(&mut self, arena: &Arc<Arena>, teams: &[Team], reports: &Sender<Report>, rng: &mut impl Rng) {
        let cfg = arena.config();
        for (pair, multithread) in self.announced.take_ready() {
            if self.running[usize::from(multithread)] < cfg.slots_of(multithread) {
                self.launch(arena, teams, pair, reports, rng);
            } else {
                eprintln!(
                    "   annonce {} contre {} annulée : slot pris par le tournoi",
                    pair[0].id, pair[1].id
                );
                self.publish(cfg, teams);
            }
        }
    }

    fn done(&mut self, pair: &[String; 2], multithread: bool) {
        self.running[usize::from(multithread)] -= 1;
        for id in pair {
            self.busy.remove(id);
        }
        self.announced.rested(pair);
    }
}

fn published(written: io::Result<()>) {
    if let Err(e) = written {
        eprintln!("   parties en cours non publiées : {e}");
    }
}

fn save(
    cfg: &Config,
    db: &Db,
    standings: &mut Standings,
    outcome: Result<GameRecord, GameError>,
    red: &str,
    black: &str,
) -> anyhow::Result<Option<GameRecord>> {
    let record = match outcome {
        Ok(record) => record,
        Err(e) => {
            eprintln!("   partie {red} contre {black} annulée : {e}");
            return Ok(None);
        }
    };
    db.insert_game(&record).with_context(|| cfg.db.display().to_string())?;
    if let Err(e) = standings.record(&record, db) {
        eprintln!("   classement non publié : {e}");
    }
    let result = &record.result;
    eprintln!(
        "   {red} (rouge) contre {black} (noir) : {} gagne ({:?}, {} coups)",
        result.winner,
        result.reason,
        result.moves.len()
    );
    Ok(Some(record))
}

fn launch_tournament(
    arena: &Arc<Arena>,
    tournaments: &mut Organizer,
    busy: &mut HashSet<String>,
    running: &mut [usize; 2],
    reports: &Sender<Report>,
) {
    let cfg = arena.config();
    for p in tournaments.pending(busy) {
        let multithread = multithread_game(cfg, &p.red, &p.black);
        if running[usize::from(multithread)] < cfg.slots_of(multithread) {
            running[usize::from(multithread)] += 1;
            busy.extend([p.red.id.clone(), p.black.id.clone()]);
            tournaments.start(cfg, &p);
            spawn_game(Arc::clone(arena), p, reports.clone());
        }
    }
}

/// Compte rendu d'un thread de paire, ou réveil à l'arrivée d'une équipe distante.
#[expect(clippy::large_enum_variant, reason = "six messages par paire")]
enum Report {
    Wake,
    Started {
        red: String,
        black: String,
        game: Option<TournamentGame>,
    },
    /// La partie a obtenu un slot et son id.
    Assigned {
        red: String,
        black: String,
        id: String,
    },
    Played {
        red: String,
        black: String,
        outcome: Result<GameRecord, GameError>,
        game: Option<TournamentGame>,
    },
    /// Les deux parties de la paire sont jouées : son slot entre en pause jusqu'à `at`.
    Resting(Rest),
    Done {
        pair: [String; 2],
        multithread: bool,
    },
}

struct Rest {
    pair: [String; 2],
    multithread: bool,
    at: String,
}

/// Registre des équipes distantes, si `[remote]` est configuré : il réveille la boucle à chaque arrivée.
fn remote_registry(cfg: &Config, reports: &Sender<Report>) -> Option<Arc<Registry>> {
    cfg.remote.as_ref()?;
    let wake = reports.clone();
    let interval = cfg.submit.as_ref().map_or(0, |submit| submit.min_interval_s);
    Some(Registry::new(Duration::from_secs(interval), move || {
        let _ = wake.send(Report::Wake);
    }))
}

/// `live.json` apprend l'id de chaque partie lancée.
fn live_hub(reports: &Sender<Report>) -> Arc<Hub> {
    let assigned = reports.clone();
    Hub::new(move |id, red, black| {
        let _ = assigned.send(Report::Assigned {
            red: red.into(),
            black: black.into(),
            id: id.into(),
        });
    })
}

/// Joue la paire dans les deux couleurs, avec la même ouverture. L'arène attribue un slot à chaque
/// partie. Pendant l'arrêt, la seconde couleur n'est pas jouée.
fn spawn_pair(arena: Arc<Arena>, a: Team, b: Team, opening: Vec<Move>, reports: Sender<Report>) {
    thread::spawn(move || {
        for (red, black) in [(&a, &b), (&b, &a)] {
            let retry = play_and_report(&arena, &reports, red, black, &opening, None);
            if arena.draining() {
                break;
            }
            if let Some(delay) = retry {
                thread::sleep(delay);
            }
        }
        if !arena.draining() {
            let pause = Duration::from_secs(arena.config().pause_s);
            let _ = reports.send(Report::Resting(Rest {
                pair: [a.id.clone(), b.id.clone()],
                multithread: multithread_game(arena.config(), &a, &b),
                at: rfc3339_at(SystemTime::now() + pause),
            }));
            thread::sleep(pause);
        }
        report_done(&arena, &reports, a, b);
    });
}

fn spawn_game(arena: Arc<Arena>, p: Pending, reports: Sender<Report>) {
    thread::spawn(move || {
        let fixture = Fixture {
            game: p.game,
            time_ms: p.time_ms,
            forfeit_counts: p.forfeit_counts,
        };
        if let Some(delay) = play_and_report(&arena, &reports, &p.red, &p.black, &p.opening, Some(&fixture)) {
            thread::sleep(delay);
        }
        report_done(&arena, &reports, p.red, p.black);
    });
}

/// Si la partie a échoué : l'attente avant la suivante.
fn play_and_report(
    arena: &Arena,
    reports: &Sender<Report>,
    red: &Team,
    black: &Team,
    opening: &[Move],
    fixture: Option<&Fixture>,
) -> Option<Duration> {
    let game = fixture.map(|f| f.game.clone());
    let _ = reports.send(Report::Started {
        red: red.id.clone(),
        black: black.id.clone(),
        game: game.clone(),
    });
    let outcome = arena.play(red, black, opening, fixture, !(red.is_bot() && black.is_bot()));
    let retry = match &outcome {
        Ok(_) => None,
        Err(GameError::NoShow(_)) => Some(NO_SHOW_RETRY_DELAY),
        Err(_) => Some(RETRY_DELAY),
    };
    let _ = reports.send(Report::Played {
        red: red.id.clone(),
        black: black.id.clone(),
        outcome,
        game,
    });
    retry
}

fn report_done(arena: &Arena, reports: &Sender<Report>, a: Team, b: Team) {
    let multithread = multithread_game(arena.config(), &a, &b);
    let _ = reports.send(Report::Done {
        pair: [a.id, b.id],
        multithread,
    });
}

/// Supprime les journaux plus vieux que `retention`. Un échec ne doit pas arrêter le ladder.
fn prune_logs(dir: &Path, retention: Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > retention));
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_logs_removes_only_old_files() {
        let dir = std::env::temp_dir().join(format!("isolation-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (old, new) = (dir.join("old.log"), dir.join("new.log"));
        fs::write(&old, "").unwrap();
        fs::write(&new, "").unwrap();
        let expired = std::time::SystemTime::now() - LOG_RETENTION - Duration::from_hours(24);
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(expired)
            .unwrap();
        prune_logs(&dir, LOG_RETENTION);
        assert!(!old.exists());
        assert!(new.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_resting_pair_counts_free_and_the_announcement_waits_for_its_slot() {
        use rand::SeedableRng;
        let dir = std::env::temp_dir().join(format!("isolation-announce-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let teams = ["a", "b", "c", "d"]
            .map(|id| format!("[[team]]\nid = \"{id}\"\ncmd = [\"bot\"]\n"))
            .concat();
        let toml = format!(
            "referee = [\"r\"]\ntime_ms = 100\nsession = \"s\"\nout = {out:?}\n{teams}",
            out = dir.join("games.jsonl")
        );
        let cfg: Config = toml.parse().unwrap();
        let teams = roster(&cfg, None, &mut HashSet::new());
        for seed in 0..20 {
            let mut pairs = Pairs::new(Scheduler::default());
            pairs.scheduler.record("a", "b", 2);
            pairs.busy.extend(["a", "b", "c"].map(String::from));
            pairs.running = [2, 0];
            let rest = Rest {
                pair: ["a".into(), "b".into()],
                multithread: false,
                at: "2026-11-05T12:01:00.000Z".into(),
            };
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            pairs.announce(&cfg, &teams, &HashSet::new(), rest, |_| 1500.0, &mut rng);
            let announced: Vec<&str> = pairs.announced.teams().collect();
            assert!(announced.contains(&"d") && !announced.contains(&"c"), "{announced:?}");
            let unavailable = pairs.unavailable(&HashSet::new(), None);
            assert!(announced.iter().all(|id| unavailable.contains(*id)));
            let file: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(dir.join("queue.json")).unwrap()).unwrap();
            assert_eq!(file["next"][0]["teams"], serde_json::json!(announced));
            assert!(pairs.announced.take_ready().is_empty());
            pairs.done(&["a".into(), "b".into()], false);
            assert_eq!(pairs.announced.take_ready().len(), 1);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn registered_team_named_like_a_fixed_team_is_reported_once() {
        let dir = std::env::temp_dir().join(format!("isolation-roster-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let toml = format!(
            "referee = [\"r\"]\ntime_ms = 100\nsession = \"s\"\n\
             [submit]\nteams = {teams:?}\njars = {jars:?}\n\
             [[team]]\nid = \"niveau-1\"\ncmd = [\"bot\"]\n",
            teams = dir.join("teams.toml"),
            jars = dir.join("jars"),
        );
        let cfg: Config = toml.parse().unwrap();
        let submit = cfg.submit.clone().unwrap();
        for id in ["niveau-1", "eq01", submit::SMOKE_OPPONENT] {
            let id: TeamId = id.parse().unwrap();
            submit::add_team(&submit.teams, id.clone()).unwrap();
            let jar = submit.current_jar(&id);
            fs::create_dir_all(jar.parent().unwrap()).unwrap();
            fs::write(jar, "").unwrap();
        }
        let mut warned = HashSet::new();
        for _ in 0..2 {
            let ids: Vec<String> = roster(&cfg, None, &mut warned).into_iter().map(|t| t.id).collect();
            assert_eq!(ids, ["niveau-1", "eq01", submit::SMOKE_OPPONENT]);
        }
        assert_eq!(
            warned,
            HashSet::from(["niveau-1".into(), submit::SMOKE_OPPONENT.into()])
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_old_games_file_is_imported_once_then_renamed() {
        let dir = std::env::temp_dir().join(format!("isolation-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data")).unwrap();
        let toml = format!(
            "referee = [\"r\"]\ntime_ms = 100\nsession = \"s\"\nout = {out:?}\ndb = {db:?}\n",
            out = dir.join("data/games.jsonl"),
            db = dir.join("state/ladder.db"),
        );
        let cfg: Config = toml.parse().unwrap();
        let games = [
            db::tests::game("g1", "a", "b", isolation_core::board::Color::Red),
            db::tests::game("g2", "b", "a", isolation_core::board::Color::Black),
        ];
        let text: String = games.iter().map(|g| serde_json::to_string(g).unwrap() + "\n").collect();
        fs::write(&cfg.out, text).unwrap();

        assert_eq!(open_db(&cfg).unwrap().games().unwrap(), games);
        assert!(!cfg.out.exists());
        assert!(dir.join("data/games.jsonl.migrated").exists());

        fs::write(&cfg.out, serde_json::to_string(&games[0]).unwrap() + "\n").unwrap();
        assert_eq!(open_db(&cfg).unwrap().games().unwrap().len(), 2);
        assert!(cfg.out.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
