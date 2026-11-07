//! How often and how recently each snippet is used, so lists can put the ones
//! you reach for first.
//!
//! Every use adds one to a snippet's score, and a score halves every
//! `HALF_LIFE_DAYS`. Snippets you use all the time stay on top, and a new
//! favourite overtakes an old one within days, without keeping any history:
//! only a score and the moment it was last changed are stored.
//!
//! A snippet is recognised by its trigger, or by its file and name when it has
//! none, so the record survives editing, reordering and deleting other
//! snippets. Renaming a trigger starts that snippet over.

use crate::files;
use crate::snippets::Snippet;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long it takes a score to halve.
pub const HALF_LIFE_DAYS: f64 = 14.0;
const SECONDS_PER_DAY: f64 = 86_400.0;
/// Scores below this are forgotten when the file is written.
const FORGET_BELOW: f64 = 0.01;
/// How often changes are written out.
const SAVE_EVERY: Duration = Duration::from_secs(5);

const HEADER: &str = "# Bloom's record of how often you use each snippet, so that lists can put your\n\
# favourites first. It is rewritten automatically; delete it to start over.\n\n";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    key: String,
    /// The score as of `updated`.
    score: f64,
    /// Seconds since 1970.
    updated: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct FileContents {
    #[serde(default)]
    entry: Vec<Entry>,
}

/// The scores of every snippet that has been used.
#[derive(Debug, Default)]
pub struct Usage {
    /// Key -> (score as of the time, that time).
    scores: HashMap<String, (f64, u64)>,
    changed: bool,
}

/// The current time, in seconds since 1970.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// What identifies a snippet in the record: its trigger, or its file and name
/// when it has none.
fn key(trigger: Option<&str>, file: &str, name: &str) -> String {
    match trigger {
        Some(trigger) => format!("trigger:{trigger}"),
        None => format!("search:{file}::{name}"),
    }
}

/// Whether the user wants their use recorded and lists ranked by it.
pub fn enabled(state: &AppState) -> bool {
    state.config.read().unwrap().rank_by_use
}

/// What identifies `snippet` in the record.
pub fn key_of(snippet: &Snippet) -> String {
    key(snippet.trigger.as_deref(), &snippet.file, &snippet.name)
}

/// Counts one use of `snippet`.
pub fn record_use(state: &AppState, snippet: &Snippet) {
    if !enabled(state) {
        return;
    }
    state.usage.lock().unwrap().record(&key_of(snippet), now());
}

/// Takes back a use that the user undid.
pub fn forget_use(state: &AppState, key: &str) {
    if !enabled(state) {
        return;
    }
    state.usage.lock().unwrap().forget(key, now());
}

/// `score` after the time between `from` and `to` has passed.
fn faded(score: f64, from: u64, to: u64) -> f64 {
    let days = to.saturating_sub(from) as f64 / SECONDS_PER_DAY;
    score * 0.5_f64.powf(days / HALF_LIFE_DAYS)
}

impl Usage {
    /// Reads the record. A missing file means nothing has been used yet, and an
    /// unreadable one starts over rather than getting in the way.
    pub fn load(path: &Path) -> Usage {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Usage::default(),
            Err(e) => {
                eprintln!("bloom: couldn't read usage.toml, starting over: {e}");
                return Usage::default();
            }
        };
        match toml::from_str::<FileContents>(&text) {
            Ok(file) => Usage {
                scores: file
                    .entry
                    .into_iter()
                    .map(|e| (e.key, (e.score, e.updated)))
                    .collect(),
                changed: false,
            },
            Err(e) => {
                eprintln!("bloom: usage.toml has an error, starting over: {e}");
                Usage::default()
            }
        }
    }

    /// The score of a snippet at `now`. Zero if it has never been used.
    pub fn score(&self, key: &str, now: u64) -> f64 {
        self.scores
            .get(key)
            .map_or(0.0, |&(score, updated)| faded(score, updated, now))
    }

    /// Counts one use.
    pub fn record(&mut self, key: &str, now: u64) {
        let score = self.score(key, now) + 1.0;
        self.scores.insert(key.to_string(), (score, now));
        self.changed = true;
    }

    /// Takes back one use, for an expansion the user undid.
    pub fn forget(&mut self, key: &str, now: u64) {
        if !self.scores.contains_key(key) {
            return;
        }
        let score = (self.score(key, now) - 1.0).max(0.0);
        self.scores.insert(key.to_string(), (score, now));
        self.changed = true;
    }

    /// Orders `items` with the most used first. Items with equal scores, which
    /// includes everything never used, keep the order they came in.
    pub fn rank<T>(&self, items: &mut [T], now: u64, key_of: impl Fn(&T) -> String) {
        items.sort_by(|a, b| {
            self.score(&key_of(b), now)
                .total_cmp(&self.score(&key_of(a), now))
        });
    }

    /// The text to write, if anything changed since the last time. Scores that
    /// have faded to nothing are dropped.
    fn snapshot(&mut self, now: u64) -> Option<String> {
        if !self.changed {
            return None;
        }
        self.scores
            .retain(|_, &mut (score, updated)| faded(score, updated, now) >= FORGET_BELOW);
        let mut entry: Vec<Entry> = self
            .scores
            .iter()
            .map(|(key, &(score, updated))| Entry {
                key: key.clone(),
                score,
                updated,
            })
            .collect();
        entry.sort_by(|a, b| a.key.cmp(&b.key));
        self.changed = false;
        match toml::to_string(&FileContents { entry }) {
            Ok(body) => Some(format!("{HEADER}{body}")),
            Err(e) => {
                eprintln!("bloom: couldn't prepare usage.toml: {e}");
                None
            }
        }
    }
}

/// Writes the record now if it changed.
pub fn save_now(state: &AppState) {
    let text = state.usage.lock().unwrap().snapshot(now());
    let Some(text) = text else { return };
    if let Err(e) = files::write_atomic(&state.paths.usage_file, &text) {
        eprintln!("bloom: couldn't write usage.toml: {e}");
        // Try again next time rather than losing the change.
        state.usage.lock().unwrap().changed = true;
    }
}

/// Writes changes to disk every few seconds, off the typing thread.
pub fn start_saver(state: Arc<AppState>) {
    let spawned = thread::Builder::new()
        .name("bloom-usage".into())
        .spawn(move || {
            loop {
                thread::sleep(SAVE_EVERY);
                save_now(&state);
            }
        });
    if spawned.is_err() {
        eprintln!("bloom: couldn't start saving which snippets you use");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    const T0: u64 = 1_700_000_000;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn each_use_adds_one() {
        let mut usage = Usage::default();
        assert_eq!(usage.score("a", T0), 0.0);
        usage.record("a", T0);
        usage.record("a", T0);
        usage.record("a", T0);
        assert!(near(usage.score("a", T0), 3.0));
    }

    #[test]
    fn a_score_halves_every_half_life() {
        let mut usage = Usage::default();
        usage.record("a", T0);
        let half_life = (HALF_LIFE_DAYS as u64) * DAY;
        assert!(near(usage.score("a", T0 + half_life), 0.5));
        assert!(near(usage.score("a", T0 + 2 * half_life), 0.25));
    }

    #[test]
    fn recording_later_fades_the_old_score_first() {
        let mut usage = Usage::default();
        usage.record("a", T0);
        let half_life = (HALF_LIFE_DAYS as u64) * DAY;
        usage.record("a", T0 + half_life);
        assert!(near(usage.score("a", T0 + half_life), 1.5));
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_inflate_scores() {
        let mut usage = Usage::default();
        usage.record("a", T0);
        assert!(near(usage.score("a", T0 - 5 * DAY), 1.0));
    }

    #[test]
    fn frequent_use_beats_one_recent_use_and_a_new_favourite_overtakes_an_old_one() {
        let half_life = (HALF_LIFE_DAYS as u64) * DAY;
        let mut usage = Usage::default();
        // "old" was used twenty times, two half-lives ago.
        for _ in 0..20 {
            usage.record("old", T0);
        }
        // "new" has been used four times this week.
        let now = T0 + 2 * half_life;
        for _ in 0..4 {
            usage.record("new", now);
        }
        // 20 / 4 = 5 against 4: still just ahead.
        assert!(usage.score("old", now) > usage.score("new", now));
        // A few more uses of the new one and it takes over.
        for _ in 0..3 {
            usage.record("new", now);
        }
        assert!(usage.score("new", now) > usage.score("old", now));
        // A single use never beats a steady habit.
        let mut once = Usage::default();
        for _ in 0..10 {
            once.record("habit", now);
        }
        once.record("odd", now);
        assert!(once.score("habit", now) > once.score("odd", now));
    }

    #[test]
    fn ranking_puts_the_most_used_first_and_keeps_the_order_of_ties() {
        let mut usage = Usage::default();
        usage.record("b", T0);
        usage.record("c", T0);
        usage.record("c", T0);
        let mut items = vec!["a", "b", "c", "d"];
        usage.rank(&mut items, T0, |i| i.to_string());
        // c (2), b (1), then a and d, never used, in the order they came.
        assert_eq!(items, vec!["c", "b", "a", "d"]);
    }

    #[test]
    fn forgetting_takes_back_one_use_and_never_goes_below_zero() {
        let mut usage = Usage::default();
        usage.record("a", T0);
        usage.record("a", T0);
        usage.forget("a", T0);
        assert!(near(usage.score("a", T0), 1.0));
        usage.forget("a", T0);
        usage.forget("a", T0);
        assert_eq!(usage.score("a", T0), 0.0);
        // Something never used is left alone, and does not appear in the file.
        usage.forget("never", T0);
        assert!(!usage.scores.contains_key("never"));
    }

    #[test]
    fn a_trigger_identifies_a_snippet_and_a_search_only_one_uses_file_and_name() {
        assert_eq!(
            key(Some(";sig"), "personal.toml", "Signature"),
            "trigger:;sig"
        );
        assert_eq!(
            key(None, "personal.toml", "Office address"),
            "search:personal.toml::Office address"
        );
        // Renaming the snippet does not matter when there is a trigger.
        assert_eq!(
            key(Some(";sig"), "personal.toml", "Renamed"),
            key(Some(";sig"), "other.toml", "Signature")
        );
    }

    fn temp_file(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bloom-usage-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("usage.toml")
    }

    #[test]
    fn the_record_survives_a_round_trip_through_the_file() {
        let path = temp_file("roundtrip");
        let mut usage = Usage::default();
        usage.record("trigger:;sig", T0);
        usage.record("trigger:;sig", T0);
        usage.record("search:p.toml::Address", T0 + DAY);
        let text = usage.snapshot(T0 + DAY).unwrap();
        assert!(text.starts_with("# Bloom's record"));
        fs::write(&path, text).unwrap();
        let loaded = Usage::load(&path);
        assert!(near(loaded.score("trigger:;sig", T0), 2.0));
        assert!(near(loaded.score("search:p.toml::Address", T0 + DAY), 1.0));
        assert!(!loaded.changed, "loading is not a change");
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn nothing_is_written_until_something_changes() {
        let mut usage = Usage::default();
        assert!(usage.snapshot(T0).is_none());
        usage.record("a", T0);
        assert!(usage.snapshot(T0).is_some());
        assert!(usage.snapshot(T0).is_none(), "written once, then quiet");
    }

    #[test]
    fn scores_that_have_faded_away_are_dropped_from_the_file() {
        let mut usage = Usage::default();
        usage.record("old", T0);
        usage.record("fresh", T0 + 400 * DAY);
        let text = usage.snapshot(T0 + 400 * DAY).unwrap();
        assert!(!text.contains("old"), "{text}");
        assert!(text.contains("fresh"));
    }

    #[test]
    fn a_missing_or_broken_file_starts_over_quietly() {
        let path = temp_file("broken");
        assert_eq!(Usage::load(&path).score("a", T0), 0.0);
        fs::write(&path, "this is not [valid toml").unwrap();
        assert_eq!(Usage::load(&path).score("a", T0), 0.0);
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
