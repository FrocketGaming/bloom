//! Connects keyboard listening, the matcher and text insertion.

use crate::config::InjectMode;
use crate::matcher::{Hit, Matcher};
use crate::state::{AppState, PendingForm};
use crate::{palette, platform, template};
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
            let mut last_window = 0isize;
            for ev in rx {
                let window = platform::foreground_window();
                if window != last_window {
                    matcher.reset();
                    last_window = window;
                }
                // Never expand inside Bloom's own windows, or while paused. In a
                // password field, don't even remember what was typed.
                if state.paused.load(Ordering::Relaxed)
                    || platform::is_own_window(window)
                    || platform::password_field_focused()
                {
                    matcher.reset();
                    continue;
                }
                let triggers = state.store.read().unwrap().triggers.clone();
                if let Some(hit) = matcher.feed(ev, &triggers) {
                    expand(&app, &state, hit);
                }
            }
        })
        .expect("spawn engine thread");
}

fn expand(app: &AppHandle, state: &Arc<AppState>, hit: Hit) {
    let Some(snippet) = state.store.read().unwrap().get(&hit.id).cloned() else {
        return;
    };
    let segs = template::parse(&snippet.body);

    platform::wait_for_typing_to_settle();
    platform::backspace(hit.delete);

    if !template::fields(&segs).is_empty() {
        // The trigger is erased before the palette opens, so remember it:
        // pressing Esc has to leave the user with the text they typed rather
        // than nothing at all.
        let restore = format!(
            "{}{}",
            snippet.trigger.as_deref().unwrap_or_default(),
            hit.trailing
        );
        *state.prev_window.lock().unwrap() = Some(platform::foreground_window());
        *state.pending_form.lock().unwrap() = Some(PendingForm {
            id: snippet.id.clone(),
            trailing: hit.trailing,
            restore,
        });
        palette::open(app, Some(snippet.id));
        return;
    }

    let rendered = template::render(&segs, &HashMap::new());
    deliver(state, &rendered.text, &hit.trailing, rendered.cursor_back);
}

/// Inserts text into the focused app, then places the cursor.
pub fn deliver(state: &AppState, text: &str, trailing: &str, cursor_back: usize) {
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
    platform::cursor_left(cursor_back + trailing.chars().count());
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
