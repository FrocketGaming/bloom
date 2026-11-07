//! Watches typed characters and decides when a trigger has been typed.
//!
//! When one trigger is the start of another (";sig" and ";sign"), Bloom waits
//! for the next key: typing "n" expands ";sign", typing anything else expands
//! ";sig" and keeps that extra character.

use crate::snippets::Trigger;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyEvent {
    Char(char),
    Backspace,
    /// Anything that moves the cursor or changes focus: forget what was typed.
    Reset,
}

#[derive(Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    /// Characters to erase: the trigger plus any trailing characters.
    pub delete: usize,
    /// Characters typed after the trigger that must be re-typed after the expansion.
    pub trailing: String,
}

struct Pending {
    start: usize,
    len: usize,
    id: String,
}

#[derive(Default)]
pub struct Matcher {
    buf: Vec<char>,
    pending: Option<Pending>,
}

/// Longest trigger the editor accepts. Bloom only remembers as much typing as
/// your longest trigger needs, so this also caps how much it ever holds.
pub const MAX_TRIGGER: usize = 64;

/// How many typed characters are worth remembering: the longest trigger, plus
/// the character before it for the start-of-word check. Nothing at all when
/// there are no triggers.
fn memory_limit(trigs: &[Trigger]) -> usize {
    trigs
        .iter()
        .map(|t| t.text.len())
        .max()
        .map_or(0, |longest| longest + 1)
}

impl Matcher {
    pub fn reset(&mut self) {
        self.buf.clear();
        self.pending = None;
    }

    pub fn feed(&mut self, ev: KeyEvent, trigs: &[Trigger]) -> Option<Hit> {
        match ev {
            KeyEvent::Reset => {
                self.reset();
                None
            }
            KeyEvent::Backspace => {
                self.buf.pop();
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|p| self.buf.len() < p.start + p.len)
                {
                    self.pending = None;
                }
                None
            }
            KeyEvent::Char(c) => {
                let hit = self.push(c, trigs);
                if c == '\n' || c == '\t' {
                    self.reset();
                }
                hit
            }
        }
    }

    fn boundary_ok(&self, t: &Trigger, start: usize) -> bool {
        !t.word || start == 0 || !self.buf[start - 1].is_alphanumeric()
    }

    fn push(&mut self, c: char, trigs: &[Trigger]) -> Option<Hit> {
        self.buf.push(c);

        if let Some(p) = self.pending.take() {
            let tail = &self.buf[p.start..];
            let could_grow = trigs
                .iter()
                .any(|t| t.text.len() >= tail.len() && t.text.starts_with(tail));
            if could_grow {
                self.pending = Some(p);
            } else {
                let trailing: String = self.buf[p.start + p.len..].iter().collect();
                self.buf.clear();
                return Some(Hit {
                    id: p.id,
                    delete: p.len + trailing.chars().count(),
                    trailing,
                });
            }
        }

        let best = trigs
            .iter()
            .filter(|t| !t.text.is_empty() && self.buf.ends_with(&t.text))
            .filter(|t| self.boundary_ok(t, self.buf.len() - t.text.len()))
            .max_by_key(|t| t.text.len());

        if let Some(t) = best {
            let start = self.buf.len() - t.text.len();
            let tail = &self.buf[start..];
            let longer_exists = trigs
                .iter()
                .any(|o| o.text.len() > tail.len() && o.text.starts_with(tail));
            if longer_exists {
                self.pending = Some(Pending {
                    start,
                    len: t.text.len(),
                    id: t.id.clone(),
                });
                self.forget_old(memory_limit(trigs));
                return None;
            }
            self.reset();
            return Some(Hit {
                id: t.id.clone(),
                delete: t.text.len(),
                trailing: String::new(),
            });
        }

        self.forget_old(memory_limit(trigs));
        None
    }

    /// Drops everything older than `limit` characters. A trigger waiting for
    /// the next key, and the character before it, are always kept; both fit
    /// within the limit because that trigger can still grow into a real one.
    fn forget_old(&mut self, limit: usize) {
        if self.buf.len() <= limit {
            return;
        }
        let mut drop = self.buf.len() - limit;
        if let Some(p) = &self.pending {
            drop = drop.min(p.start.saturating_sub(1));
        }
        self.buf.drain(..drop);
        if let Some(p) = &mut self.pending {
            p.start -= drop;
        }
    }

    #[cfg(test)]
    fn remembered(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trig(t: &str, word: bool) -> Trigger {
        Trigger {
            text: t.chars().collect(),
            word,
            id: t.to_string(),
        }
    }

    fn typed(m: &mut Matcher, s: &str, trigs: &[Trigger]) -> Vec<Hit> {
        s.chars()
            .filter_map(|c| m.feed(KeyEvent::Char(c), trigs))
            .collect()
    }

    #[test]
    fn simple_trigger() {
        let t = vec![trig(";date", false)];
        let mut m = Matcher::default();
        let hits = typed(&mut m, "today is ;date", &t);
        assert_eq!(
            hits,
            vec![Hit {
                id: ";date".into(),
                delete: 5,
                trailing: String::new()
            }]
        );
    }

    #[test]
    fn prefix_triggers_wait_for_next_key() {
        let t = vec![trig(";sig", false), trig(";sign", false)];
        let mut m = Matcher::default();
        assert!(typed(&mut m, ";sig", &t).is_empty());
        assert_eq!(
            typed(&mut m, " ", &t),
            vec![Hit {
                id: ";sig".into(),
                delete: 5,
                trailing: " ".into()
            }]
        );
        let mut m = Matcher::default();
        assert_eq!(typed(&mut m, ";sign", &t)[0].id, ";sign");
        let mut m = Matcher::default();
        let hits = typed(&mut m, ";sig\n", &t);
        assert_eq!(hits[0].trailing, "\n");
    }

    #[test]
    fn backspace_and_reset() {
        let t = vec![trig(":ty", false)];
        let mut m = Matcher::default();
        typed(&mut m, ":tx", &t);
        m.feed(KeyEvent::Backspace, &t);
        assert_eq!(typed(&mut m, "y", &t).len(), 1);
        typed(&mut m, ":t", &t);
        m.feed(KeyEvent::Reset, &t);
        assert!(typed(&mut m, "y", &t).is_empty());
    }

    #[test]
    fn word_boundary() {
        let t = vec![trig("btw", true)];
        let mut m = Matcher::default();
        assert!(typed(&mut m, "abtw", &t).is_empty());
        assert_eq!(typed(&mut m, " btw", &t).len(), 1);
    }

    #[test]
    fn semicolon_triggers_avoid_everyday_text() {
        let t = vec![trig(";fu", true), trig(";sig", true)];
        let mut m = Matcher::default();
        // Code and prose with colons and semicolons don't fire.
        assert!(typed(&mut m, "Self::full_name(); x;fu", &t).is_empty());
        m.reset();
        // At the start of a line or after a space, they do.
        assert_eq!(typed(&mut m, ";fu", &t)[0].id, ";fu");
        assert_eq!(typed(&mut m, "Thanks ;sig", &t)[0].id, ";sig");
    }

    #[test]
    fn remembers_only_the_longest_trigger_plus_one() {
        let t = vec![trig(";hi", true), trig(";shrug", true)];
        let mut m = Matcher::default();
        typed(&mut m, "my password is hunter2 and more text", &t);
        assert_eq!(m.remembered(), ";shrug".len() + 1);
        // Still matches, including the start-of-word check.
        assert_eq!(typed(&mut m, " ;shrug", &t)[0].id, ";shrug");
        assert!(typed(&mut m, "x;hi", &t).is_empty());
    }

    #[test]
    fn remembers_nothing_without_triggers() {
        let mut m = Matcher::default();
        typed(&mut m, "anything at all", &[]);
        assert_eq!(m.remembered(), 0);
    }

    #[test]
    fn a_waiting_trigger_survives_trimming() {
        // ";s" could still become ";sign", so it waits; trimming must keep it.
        let t = vec![trig(";s", false), trig(";sign", false)];
        let mut m = Matcher::default();
        assert!(typed(&mut m, "lots of earlier text ;s", &t).is_empty());
        assert!(m.remembered() <= ";sign".len() + 1);
        let hits = typed(&mut m, "!", &t);
        assert_eq!(hits[0].id, ";s");
        assert_eq!(hits[0].trailing, "!");
    }
}
