//! Paires du ladder : slots occupés, équipes prises et paires annoncées pendant la pause d'un slot.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use rand::Rng;

use isolation_core::notation;
use isolation_core::rules::random_opening;

use crate::arena::Arena;
use crate::config::Config;
use crate::play::{Report, Rest, spawn_game, spawn_pair};
use crate::roster::{Team, multithread_game};
use crate::schedule::{Announcements, Scheduler};
use crate::tournament::Organizer;

pub struct Pairs {
    pub scheduler: Scheduler,
    /// Paires en cours, par genre de slot (ordinaire, multithread), pause comprise.
    pub running: [usize; 2],
    busy: HashSet<String>,
    announced: Announcements,
    pub round: u64,
}

impl Pairs {
    pub fn new(scheduler: Scheduler) -> Self {
        Self {
            scheduler,
            running: [0; 2],
            busy: HashSet::new(),
            announced: Announcements::default(),
            round: 0,
        }
    }

    pub fn idle(&self) -> bool {
        self.running == [0; 2]
    }

    /// Une paire de plus tient dans `rounds`, annonces comprises.
    pub fn open(&self, rounds: Option<u64>) -> bool {
        let announced = u64::try_from(self.announced.len()).unwrap_or(u64::MAX);
        rounds.is_none_or(|n| self.round.saturating_add(announced) < n)
    }

    /// Équipes hors tirage. `resting` : la paire en pause dont le slot est à pourvoir, comptée libre.
    pub fn unavailable(&self, waiting: &HashSet<String>, resting: Option<&[String; 2]>) -> HashSet<String> {
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

    pub fn launch(
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

    /// Lance les matchs de tournoi prêts qui trouvent un slot libre.
    pub fn launch_tournament(&mut self, arena: &Arc<Arena>, tournaments: &mut Organizer, reports: &Sender<Report>) {
        let cfg = arena.config();
        for p in tournaments.pending(&self.busy) {
            let multithread = multithread_game(cfg, &p.red, &p.black);
            if self.running[usize::from(multithread)] < cfg.slots_of(multithread) {
                self.running[usize::from(multithread)] += 1;
                self.busy.extend([p.red.id.clone(), p.black.id.clone()]);
                tournaments.start(cfg, &p);
                spawn_game(Arc::clone(arena), p, reports.clone());
            }
        }
    }

    /// Tire la paire qui prendra le slot de `rest` à la fin de sa pause, et l'annonce.
    pub fn announce(
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

    pub fn cancel_stale(&mut self, cfg: &Config, teams: &[Team], waiting: &HashSet<String>) {
        if self.announced.cancel_stale(teams, &self.busy, waiting) {
            eprintln!("   annonce annulée : nouveau jar, équipe absente ou tournoi");
            self.publish(cfg, teams);
        }
    }

    /// Lance les annonces dont la pause est finie, sauf si un tournoi a pris leur slot.
    pub fn launch_announced(
        &mut self,
        arena: &Arc<Arena>,
        teams: &[Team],
        reports: &Sender<Report>,
        rng: &mut impl Rng,
    ) {
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

    pub fn done(&mut self, pair: &[String; 2], multithread: bool) {
        self.running[usize::from(multithread)] -= 1;
        for id in pair {
            self.busy.remove(id);
        }
        self.announced.rested(pair);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rand::SeedableRng;

    use super::*;
    use crate::roster::roster;

    #[test]
    fn the_resting_pair_counts_free_and_the_announcement_waits_for_its_slot() {
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
}
