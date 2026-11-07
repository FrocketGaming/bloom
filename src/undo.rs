//! Taking back an expansion with Backspace.
//!
//! Right after Bloom expands a trigger, Backspace as the very next key erases
//! the expansion and types the trigger back, so a mistaken expansion costs one
//! key press. This module decides whether an expansion can be taken back and
//! how much to erase; the engine does the erasing.

use unicode_segmentation::UnicodeSegmentation;

/// The longest expansion that can be taken back. Erasing more than this means
/// that many key presses, which is slow and not worth the risk.
pub const MAX_ERASE: usize = 1000;

/// What it takes to undo the last expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct Undo {
    /// The window the expansion went into. Undo only applies there.
    pub window: isize,
    /// How many characters (as the user sees them, not code points) to erase.
    pub erase: usize,
    /// What the user had typed before the expansion, to type back.
    pub restore: String,
    /// Which snippet was used, so taking the expansion back also takes back the
    /// use that was counted for it.
    pub usage_key: String,
}

/// What to remember about an expansion, or `None` when it cannot be taken back:
/// the cursor was placed somewhere other than the end (the user's next key is
/// meant for that spot), nothing was typed that could be restored, or the text
/// is empty or too long to erase.
///
/// `cursor_steps` is how far left of the end the cursor was put; `typed` is the
/// text that was erased to make way for the expansion.
pub fn plan(
    window: isize,
    inserted: &str,
    cursor_steps: usize,
    typed: String,
    usage_key: String,
) -> Option<Undo> {
    if cursor_steps != 0 || typed.is_empty() {
        return None;
    }
    // Most apps remove one character as displayed with each Backspace, so an
    // emoji with a skin tone, or an accented letter built from two code points,
    // counts once. A few older controls remove one code point at a time and
    // would leave part of such text behind. Counting this way errs towards
    // leaving text, never towards removing something the user typed.
    let erase = inserted.graphemes(true).count();
    if erase == 0 || erase > MAX_ERASE {
        return None;
    }
    Some(Undo {
        window,
        erase,
        restore: typed,
        usage_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undo(inserted: &str, steps: usize, typed: &str) -> Option<Undo> {
        plan(7, inserted, steps, typed.to_string(), "trigger:;x".into())
    }

    #[test]
    fn plain_text_is_erased_one_key_per_character() {
        let u = undo("Best,\nYour Name", 0, ";sig").unwrap();
        assert_eq!(u.erase, 15);
        assert_eq!(u.restore, ";sig");
        assert_eq!(u.window, 7);
        assert_eq!(u.usage_key, "trigger:;x");
    }

    #[test]
    fn characters_built_from_several_code_points_count_once() {
        // Backspace removes each of these in one press.
        assert_eq!(
            undo("e\u{301}", 0, ";x").unwrap().erase,
            1,
            "e + combining accent"
        );
        assert_eq!(
            undo("👍🏽", 0, ";x").unwrap().erase,
            1,
            "thumbs up + skin tone"
        );
        assert_eq!(
            undo("👨‍👩‍👧", 0, ";x").unwrap().erase,
            1,
            "a family joined by ZWJ"
        );
        assert_eq!(undo("¯\\_(ツ)_/¯", 0, ";shrug").unwrap().erase, 9);
    }

    #[test]
    fn a_line_break_counts_once() {
        assert_eq!(undo("a\r\nb", 0, ";x").unwrap().erase, 3);
        assert_eq!(undo("a\nb", 0, ";x").unwrap().erase, 3);
    }

    #[test]
    fn a_cursor_placed_inside_the_text_cannot_be_undone() {
        assert!(undo("Hello", 3, ";x").is_none());
        // Even one step left of the end means the next key is meant for that spot.
        assert!(undo("Hello", 1, ";x").is_none());
    }

    #[test]
    fn nothing_typed_means_nothing_to_restore() {
        assert!(undo("Hello", 0, "").is_none());
    }

    #[test]
    fn empty_and_very_long_expansions_cannot_be_undone() {
        assert!(undo("", 0, ";x").is_none());
        assert!(undo(&"a".repeat(MAX_ERASE), 0, ";x").is_some());
        assert!(undo(&"a".repeat(MAX_ERASE + 1), 0, ";x").is_none());
    }
}
