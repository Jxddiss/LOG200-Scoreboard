//! Base SQLite du ladder, source de vérité des parties et des soumissions (schéma dans docs/formats.md).

use std::fs;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

use isolation_core::board::Color;
use isolation_core::record::{GameRecord, Jsonl};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS games (
    seq INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    session TEXT NOT NULL,
    ts TEXT NOT NULL,
    time_ms INTEGER NOT NULL,
    red_team TEXT NOT NULL,
    red_build TEXT,
    red_remote INTEGER NOT NULL,
    black_team TEXT NOT NULL,
    black_build TEXT,
    black_remote INTEGER NOT NULL,
    winner TEXT NOT NULL,
    reason TEXT NOT NULL,
    invalid_red INTEGER NOT NULL,
    invalid_black INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    plies INTEGER NOT NULL,
    opening TEXT NOT NULL,
    tournament_id TEXT,
    tournament_match TEXT,
    tournament_game INTEGER,
    record TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS games_session ON games(session);
CREATE INDEX IF NOT EXISTS games_red ON games(red_team);
CREATE INDEX IF NOT EXISTS games_black ON games(black_team);
CREATE TABLE IF NOT EXISTS moves (
    game INTEGER NOT NULL REFERENCES games(seq),
    ply INTEGER NOT NULL,
    color TEXT NOT NULL,
    move TEXT NOT NULL,
    ms INTEGER,
    PRIMARY KEY (game, ply)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS ratings (
    game_id TEXT NOT NULL,
    team TEXT NOT NULL,
    all_rating REAL NOT NULL,
    all_delta REAL NOT NULL,
    session_rating REAL NOT NULL,
    session_delta REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS ratings_team ON ratings(team);
CREATE TABLE IF NOT EXISTS submissions (
    ts TEXT NOT NULL,
    team TEXT NOT NULL,
    hash TEXT NOT NULL,
    verdict TEXT NOT NULL,
    detail TEXT NOT NULL
);
";

pub struct Db {
    conn: Connection,
}

/// Rating d'une équipe après une partie : `[rating, variation]`, toutes sessions puis dans la session.
pub struct Rating {
    pub game: String,
    pub team: String,
    pub all: [f64; 2],
    pub session: [f64; 2],
}

fn name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[allow(dead_code)] // TEMP étape 2 : open_read, conn et game_json servent à l'API.
impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// Connexion en lecture seule, pour l'API.
    pub fn open_read(path: &Path) -> rusqlite::Result<Self> {
        let conn =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        self.conn
            .query_row("SELECT NOT EXISTS (SELECT 1 FROM games)", [], |r| r.get(0))
    }

    pub fn insert_game(&self, game: &GameRecord) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        insert(&tx, game)?;
        tx.commit()
    }

    /// Toutes les parties, dans l'ordre où elles ont été jouées.
    pub fn games(&self) -> rusqlite::Result<Vec<GameRecord>> {
        let mut stmt = self.conn.prepare("SELECT record FROM games ORDER BY seq")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|text| {
            let text = text?;
            serde_json::from_str(&text)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
        })
        .collect()
    }

    /// La ligne JSON d'une partie, telle qu'enregistrée.
    pub fn game_json(&self, id: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT record FROM games WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        rows.next()?.map(|r| r.get(0)).transpose()
    }

    /// Migration depuis l'ancien `games.jsonl` : tout en une transaction, lignes illisibles ignorées.
    pub fn import_jsonl(&self, path: &Path) -> anyhow::Result<usize> {
        let jsonl = Jsonl::parse(&fs::read_to_string(path)?);
        for skipped in &jsonl.skipped {
            eprintln!("{} ligne {} ignorée : {}", path.display(), skipped.line, skipped.reason);
        }
        let tx = self.conn.unchecked_transaction()?;
        for game in &jsonl.games {
            insert(&tx, game)?;
        }
        tx.commit()?;
        Ok(jsonl.games.len())
    }

    /// Remplace toute la table : les ratings se recalculent au démarrage.
    pub fn replace_ratings(&self, ratings: impl IntoIterator<Item = Rating>) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM ratings", [])?;
        for rating in ratings {
            insert_rating(&tx, &rating)?;
        }
        tx.commit()
    }

    pub fn add_ratings(&self, ratings: impl IntoIterator<Item = Rating>) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for rating in ratings {
            insert_rating(&tx, &rating)?;
        }
        tx.commit()
    }
}

fn insert(conn: &Connection, game: &GameRecord) -> rusqlite::Result<()> {
    let result = &game.result;
    let record = serde_json::to_string(game).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let opening =
        serde_json::to_string(&result.opening).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let tournament = game.tournament.as_ref();
    conn.prepare_cached(
        "INSERT INTO games (id, session, ts, time_ms, red_team, red_build, red_remote, black_team,
             black_build, black_remote, winner, reason, invalid_red, invalid_black, duration_ms, plies,
             opening, tournament_id, tournament_match, tournament_game, record)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
    )?
    .execute(params![
        game.id,
        game.session,
        game.ts,
        result.time_ms,
        game.red.team,
        game.red.build,
        game.red.remote,
        game.black.team,
        game.black.build,
        game.black.remote,
        name(&result.winner),
        name(&result.reason),
        result.invalid.red,
        result.invalid.black,
        result.duration_ms,
        result.moves.len(),
        opening,
        tournament.map(|t| &t.id),
        tournament.map(|t| &t.match_id),
        tournament.map(|t| t.game),
        record,
    ])?;
    let mut stmt = conn.prepare_cached("INSERT INTO moves (game, ply, color, move, ms) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    let seq = conn.last_insert_rowid();
    // L'ouverture a un nombre pair de demi-coups : le rouge joue le premier coup client.
    for (ply, mv) in result.moves.iter().enumerate() {
        let color = if ply % 2 == 0 { Color::Red } else { Color::Black };
        stmt.execute(params![seq, ply, name(&color), mv, result.move_ms.get(ply)])?;
    }
    Ok(())
}

fn insert_rating(conn: &Connection, r: &Rating) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO ratings (game_id, team, all_rating, all_delta, session_rating, session_delta)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![r.game, r.team, r.all[0], r.all[1], r.session[0], r.session[1]])?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use isolation_core::record::{GameResult, InvalidCounts, Player, RECORD_VERSION, Reason, TournamentGame};

    pub(crate) fn game(id: &str, red: &str, black: &str, winner: Color) -> GameRecord {
        let player = |team: &str| Player {
            team: team.into(),
            build: None,
            anchor: false,
            remote: false,
        };
        GameRecord {
            id: id.into(),
            session: "s1".into(),
            ts: "2026-10-24T12:00:00.000Z".into(),
            red: player(red),
            black: player(black),
            tournament: None,
            result: GameResult {
                v: RECORD_VERSION,
                time_ms: 500,
                opening: vec!["G1-I1-C12".into(), "F12-D10-F1".into()],
                moves: vec!["I1-F4-A7".into(), "D10-C10-I7".into(), "F4-F6-F10".into()],
                winner,
                reason: Reason::NoMoves,
                invalid: InvalidCounts { red: 0, black: 1 },
                duration_ms: 1500,
                move_ms: vec![10, 20, 30],
            },
        }
    }

    pub(crate) fn memory() -> Db {
        Db::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn games_come_back_identical_and_in_order() {
        let db = memory();
        assert!(db.is_empty().unwrap());
        let mut first = game("g2", "a", "b", Color::Red);
        first.red.build = Some("h1".into());
        first.black.remote = true;
        first.tournament = Some(TournamentGame {
            id: "t".into(),
            match_id: "W1".into(),
            game: 2,
        });
        let second = game("g1", "b", "a", Color::Black);
        db.insert_game(&first).unwrap();
        db.insert_game(&second).unwrap();
        assert!(!db.is_empty().unwrap());
        assert_eq!(db.games().unwrap(), vec![first.clone(), second]);
        let json: GameRecord = serde_json::from_str(&db.game_json("g2").unwrap().unwrap()).unwrap();
        assert_eq!(json, first);
        assert_eq!(db.game_json("absent").unwrap(), None);
    }

    #[test]
    fn columns_and_moves_are_queryable() {
        let db = memory();
        db.insert_game(&game("g1", "a", "b", Color::Black)).unwrap();
        let (winner, reason, plies, invalid): (String, String, i64, i64) = db
            .conn()
            .query_row(
                "SELECT winner, reason, plies, invalid_black FROM games WHERE id = 'g1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (winner.as_str(), reason.as_str(), plies, invalid),
            ("black", "no_moves", 3, 1)
        );
        let moves: Vec<(String, String, i64)> = db
            .conn()
            .prepare("SELECT color, move, ms FROM moves JOIN games ON seq = game WHERE id = 'g1' ORDER BY ply")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(moves[0], ("red".into(), "I1-F4-A7".into(), 10));
        assert_eq!(moves[1].0, "black");
        assert_eq!(moves.len(), 3);
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let db = memory();
        db.insert_game(&game("g1", "a", "b", Color::Red)).unwrap();
        assert!(db.insert_game(&game("g1", "a", "b", Color::Red)).is_err());
        assert_eq!(db.games().unwrap().len(), 1);
    }

    #[test]
    fn import_reads_a_jsonl_and_skips_broken_lines() {
        let path = std::env::temp_dir().join(format!("isolation-db-import-{}.jsonl", std::process::id()));
        let games = [game("g1", "a", "b", Color::Red), game("g2", "b", "a", Color::Red)];
        let mut text: String = games.iter().map(|g| serde_json::to_string(g).unwrap() + "\n").collect();
        text.push_str("{\"id\": \"cut");
        fs::write(&path, text).unwrap();
        let db = memory();
        assert_eq!(db.import_jsonl(&path).unwrap(), 2);
        assert_eq!(db.games().unwrap(), games);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ratings_are_replaced_then_appended() {
        let db = memory();
        let rating = |game: &str, team: &str| Rating {
            game: game.into(),
            team: team.into(),
            all: [1520.0, 20.0],
            session: [1510.0, 10.0],
        };
        db.replace_ratings([rating("g1", "a"), rating("g1", "b")]).unwrap();
        db.replace_ratings([rating("g1", "a")]).unwrap();
        db.add_ratings([rating("g2", "a")]).unwrap();
        let count: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM ratings WHERE team = 'a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
