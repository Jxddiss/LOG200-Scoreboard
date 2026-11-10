//! Base SQLite du ladder, source de vérité des parties et des soumissions. Les requêtes sont dans
//! `sql/`, le schéma dans `sql/schema.sql` (décrit dans docs/formats.md).

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, named_params};

use isolation_core::board::Color;
use isolation_core::record::{GameRecord, Jsonl};

const SCHEMA_VERSION: i64 = 1;

mod sql {
    pub const SCHEMA: &str = include_str!("sql/schema.sql");
    pub const IS_EMPTY: &str = include_str!("sql/is_empty.sql");
    pub const GAMES: &str = include_str!("sql/games.sql");
    pub const GAME: &str = include_str!("sql/game.sql");
    pub const INSERT_GAME: &str = include_str!("sql/insert_game.sql");
    pub const INSERT_MOVE: &str = include_str!("sql/insert_move.sql");
    pub const INSERT_RATING: &str = include_str!("sql/insert_rating.sql");
    pub const CLEAR_RATINGS: &str = include_str!("sql/clear_ratings.sql");
}

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

fn to_json<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(sql::SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// Connexion en lecture seule, pour l'API.
    pub fn open_read(path: &Path) -> rusqlite::Result<Self> {
        let conn =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        self.conn.query_row(sql::IS_EMPTY, [], |r| r.get(0))
    }

    pub fn insert_game(&self, game: &GameRecord) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        insert(&tx, game)?;
        tx.commit()
    }

    /// Toutes les parties, dans l'ordre où elles ont été jouées.
    pub fn games(&self) -> rusqlite::Result<Vec<GameRecord>> {
        let mut stmt = self.conn.prepare(sql::GAMES)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|text| {
            serde_json::from_str(&text?)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
        })
        .collect()
    }

    /// La ligne JSON d'une partie, telle qu'enregistrée.
    pub fn game_json(&self, id: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self.conn.prepare_cached(sql::GAME)?;
        let mut rows = stmt.query(named_params! { ":id": id })?;
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
        tx.execute(sql::CLEAR_RATINGS, [])?;
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
    let tournament = game.tournament.as_ref();
    conn.prepare_cached(sql::INSERT_GAME)?.execute(named_params! {
        ":id": game.id,
        ":session": game.session,
        ":ts": game.ts,
        ":time_ms": result.time_ms,
        ":red_team": game.red.team,
        ":red_build": game.red.build,
        ":red_remote": game.red.remote,
        ":black_team": game.black.team,
        ":black_build": game.black.build,
        ":black_remote": game.black.remote,
        ":winner": name(&result.winner),
        ":reason": name(&result.reason),
        ":invalid_red": result.invalid.red,
        ":invalid_black": result.invalid.black,
        ":duration_ms": result.duration_ms,
        ":plies": result.moves.len(),
        ":opening": to_json(&result.opening)?,
        ":tournament_id": tournament.map(|t| &t.id),
        ":tournament_match": tournament.map(|t| &t.match_id),
        ":tournament_game": tournament.map(|t| t.game),
        ":record": to_json(game)?,
    })?;
    let seq = conn.last_insert_rowid();
    let mut stmt = conn.prepare_cached(sql::INSERT_MOVE)?;
    // L'ouverture a un nombre pair de demi-coups : le rouge joue le premier coup client.
    for (ply, mv) in result.moves.iter().enumerate() {
        let color = if ply % 2 == 0 { Color::Red } else { Color::Black };
        stmt.execute(named_params! {
            ":game": seq,
            ":ply": ply,
            ":color": name(&color),
            ":move": mv,
            ":ms": result.move_ms.get(ply),
        })?;
    }
    Ok(())
}

fn insert_rating(conn: &Connection, r: &Rating) -> rusqlite::Result<()> {
    conn.prepare_cached(sql::INSERT_RATING)?.execute(named_params! {
        ":game_id": r.game,
        ":team": r.team,
        ":all_rating": r.all[0],
        ":all_delta": r.all[1],
        ":session_rating": r.session[0],
        ":session_delta": r.session[1],
    })?;
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
