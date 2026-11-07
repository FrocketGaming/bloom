//! Connects keyboard listening, the matcher and text insertion.

use crate::config::InjectMode;
use crate::matcher::{Hit, KeyEvent, Matcher, Nav};
use crate::state::{AppState, PendingForm};
use crate::undo::{self, Undo};
use crate::usage;
use crate::{dropdown, palette, platform, template};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

pub fn start(app: AppHandle, state: Arc<AppState>) {
    let (tx, rx) = mpsc::channel();
    platform::start_listener(tx);
    platform::start_password_watch();
    platform::start_caret_probe();

    thread::Builder::new()
        .name("bloom-engine".into())
        .spawn(move || {
            let mut matcher = Matcher::default();
            let mut menu: Option<dropdown::Menu> = None;
            // Set when Esc closes the list, so it stays shut until the typed
            // text no longer looks like the start of a trigger.
            let mut dismissed = false;
            let mut last_window = 0isize;
            for ev in rx {
                // Any key ends the chance to take back the last expansion, so it
                // is taken out here and only used if this key is Backspace.
                let undo = state.undo.lock().unwrap().take();
                let window = platform::foreground_window();
                if window != last_window {
                    matcher.reset();
                    close_menu(&app, &mut menu);
                    last_window = window;
                }
                // Never expand inside Bloom's own windows, or while paused. In a
                // password field, don't even remember what was typed.
                if state.paused.load(Ordering::Relaxed)
                    || platform::is_own_window(window)
                    || platform::password_field_focused()
                {
                    matcher.reset();
                    close_menu(&app, &mut menu);
                    continue;
                }
                if ev == KeyEvent::Backspace
                    && let Some(undo) = undo
                    && undo.window == window
                    && state.config.read().unwrap().undo_with_backspace
                {
                    take_back(&state, &mut matcher, undo);
                    continue;
                }
                if let KeyEvent::Nav(nav) = ev {
                    handle_nav(&app, &state, &mut matcher, &mut menu, &mut dismissed, nav);
                    continue;
                }
                let triggers = state.store.read().unwrap().triggers.clone();
                if let Some(hit) = matcher.feed(ev, &triggers) {
                    close_menu(&app, &mut menu);
                    dismissed = false;
                    expand(&app, &state, hit, None);
                    continue;
                }
                let suggestion = if state.config.read().unwrap().suggestions {
                    matcher.suggest(&triggers)
                } else {
                    None
                };
                match suggestion {
                    None => {
                        dismissed = false;
                        close_menu(&app, &mut menu);
                    }
                    Some(_) if dismissed => {}
                    Some(suggestion) => {
                        menu = dropdown::open(&app, &state, suggestion, menu.take());
                    }
                }
            }
        })
        .expect("spawn engine thread");
}

/// Backspace right after an expansion: erase it and type the trigger back.
fn take_back(state: &AppState, matcher: &mut Matcher, undo: Undo) {
    // The Backspace the user pressed has already removed the last character,
    // and Bloom waits for their typing to settle before adding its own.
    platform::wait_for_typing_to_settle();
    platform::backspace(undo.erase.saturating_sub(1));
    platform::type_text(&undo.restore);
    // Bloom's own typing is invisible to the matcher; tell it what is on
    // screen, so the user can carry on from the restored trigger.
    matcher.seed(&undo.restore);
    // A use that was taken back was a mistake, so it should not count.
    usage::forget_use(state, &undo.usage_key);
}

/// Remembers how to take back an expansion that was just delivered, or forgets
/// the previous one if this expansion cannot be taken back. `typed` is what the
/// user had typed before the expansion replaced it.
pub fn remember_undo(
    state: &AppState,
    text: &str,
    trailing: &str,
    cursor_back: Option<usize>,
    typed: String,
    usage_key: String,
) {
    let inserted = format!("{text}{trailing}");
    let undo = undo::plan(
        platform::foreground_window(),
        &inserted,
        cursor_steps(cursor_back, trailing),
        typed,
        usage_key,
    );
    *state.undo.lock().unwrap() = undo;
}

fn close_menu(app: &AppHandle, menu: &mut Option<dropdown::Menu>) {
    if menu.take().is_some() {
        dropdown::close(app);
    }
}

/// Acts on a key the open suggestion list has taken.
fn handle_nav(
    app: &AppHandle,
    state: &Arc<AppState>,
    matcher: &mut Matcher,
    menu: &mut Option<dropdown::Menu>,
    dismissed: &mut bool,
    nav: Nav,
) {
    let Some(open) = menu.as_mut() else { return };
    match nav {
        Nav::Up => dropdown::select(app, state, open, -1),
        Nav::Down => dropdown::select(app, state, open, 1),
        Nav::Close => {
            *dismissed = true;
            close_menu(app, menu);
        }
        Nav::Accept => {
            let id = open.selected_id();
            let typed = open.typed.clone();
            close_menu(app, menu);
            matcher.reset();
            *dismissed = false;
            if let Some(id) = id {
                let hit = Hit {
                    id,
                    delete: typed.chars().count(),
                    trailing: String::new(),
                };
                // Cancelling a form puts back what was actually typed.
                expand(app, state, hit, Some(typed));
            }
        }
    }
}

/// Inserts the snippet a trigger (or the suggestion list) picked. `restore` is
/// the text to put back if a fill-in form is cancelled; by default that is the
/// trigger plus whatever followed it.
fn expand(app: &AppHandle, state: &Arc<AppState>, hit: Hit, restore: Option<String>) {
    let Some(snippet) = state.store.read().unwrap().get(&hit.id).cloned() else {
        return;
    };
    let segs = template::parse_resolved(&snippet.body, &state.variables.read().unwrap());

    platform::wait_for_typing_to_settle();
    platform::backspace(hit.delete);

    // What the user typed, erased above to make way for the expansion. It is
    // put back if a form is cancelled, or if the expansion is taken back.
    let typed = restore.unwrap_or_else(|| {
        format!(
            "{}{}",
            snippet.trigger.as_deref().unwrap_or_default(),
            hit.trailing
        )
    });

    if !template::fields(&segs).is_empty() {
        // The trigger is erased before the palette opens, so remember it:
        // pressing Esc has to leave the user with the text they typed rather
        // than nothing at all.
        *state.prev_window.lock().unwrap() = Some(platform::foreground_window());
        *state.pending_form.lock().unwrap() = Some(PendingForm {
            id: snippet.id.clone(),
            trailing: hit.trailing,
            restore: typed,
        });
        palette::open(app, Some(snippet.id));
        return;
    }

    let rendered = template::render(&segs, &HashMap::new());
    deliver(state, &rendered.text, &hit.trailing, rendered.cursor_back);
    usage::record_use(state, &snippet);
    remember_undo(
        state,
        &rendered.text,
        &hit.trailing,
        rendered.cursor_back,
        typed,
        usage::key_of(&snippet),
    );
}

/// How far left of the end of the inserted text the cursor goes. A body with a
/// `{{cursor}}` marker puts it there, which is also before whatever the user
/// typed after the trigger. A body without one leaves it at the very end, after
/// that character, where the user expects to carry on typing.
fn cursor_steps(cursor_back: Option<usize>, trailing: &str) -> usize {
    cursor_back.map_or(0, |back| back + trailing.chars().count())
}

/// Inserts text into the focused app, then places the cursor.
pub fn deliver(state: &AppState, text: &str, trailing: &str, cursor_back: Option<usize>) {
    let full = format!("{text}{trailing}");
    if full.is_empty() {
        return;
    }
    let (mode, threshold) = {
        let c = state.config.read().unwrap();
        (c.inject, c.clipboard_threshold)
    };
    let paste = match mode {
        InjectMode::Keys => false,
        InjectMode::Clipboard => true,
        // Paste long or multi-line text, and anything with characters the
        // keyboard layout has no key for (¯, ツ, emoji).
        InjectMode::Auto => {
            full.contains('\n')
                || full.chars().count() > threshold
                || !platform::can_type_as_keys(&full)
        }
    };
    platform::wait_for_typing_to_settle();
    if paste {
        platform::paste_text(&full);
    } else {
        platform::type_text(&full);
    }
    platform::cursor_left(cursor_steps(cursor_back, trailing));
}

/// Closes the palette and gives focus back to the window it covered (Esc, or
/// the hotkey pressed again). A cancelled fill-in form gets its trigger
/// re-typed, so the text the user typed isn't silently destroyed.
pub fn close_palette(state: &Arc<AppState>) {
    let form = state.pending_form.lock().unwrap().take();
    let prev = state.prev_window.lock().unwrap().take();
    let Some(prev) = prev else { return };
    let restore = form.map(|f| f.restore).unwrap_or_default();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        platform::focus_window(prev);
        // If Windows wouldn't hand focus back, typing the trigger now would
        // drop it into whatever *is* focused. Losing the text beats inserting
        // it into the wrong window.
        if restore.is_empty() || platform::foreground_window() != prev {
            return;
        }
        thread::sleep(Duration::from_millis(90));
        platform::wait_for_typing_to_settle();
        platform::type_text(&restore);
    });
}

/// The palette lost focus to another window: drop the pending form without
/// pulling focus back, since the user has already moved on.
pub fn dismiss_palette(state: &AppState) {
    state.pending_form.lock().unwrap().take();
    state.prev_window.lock().unwrap().take();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_cursor_marker_the_cursor_stays_at_the_end() {
        assert_eq!(cursor_steps(None, ""), 0);
        // The character typed after the trigger was re-typed after the text;
        // the cursor must not move back over it.
        assert_eq!(cursor_steps(None, "!"), 0);
    }

    #[test]
    fn a_cursor_marker_counts_the_text_after_it_and_the_trailing_character() {
        assert_eq!(cursor_steps(Some(3), ""), 3);
        assert_eq!(cursor_steps(Some(3), "!"), 4);
        // A marker at the very end still sits before the trailing character.
        assert_eq!(cursor_steps(Some(0), "!"), 1);
        assert_eq!(cursor_steps(Some(0), ""), 0);
    }
}
