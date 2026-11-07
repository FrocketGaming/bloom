//! Connects the keyboard hook, the matcher and text insertion.

use crate::config::InjectMode;
use crate::matcher::{Hit, Matcher};
use crate::state::{AppState, PendingForm};
use crate::{palette, platform, template};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use tauri::AppHandle;

pub fn start(app: AppHandle, state: Arc<AppState>) {
    let (tx, rx) = mpsc::channel();
    platform::start_hook(tx);

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
                // Never expand inside Bloom's own windows, or while paused.
                if state.paused.load(Ordering::Relaxed) || platform::is_own_window(window) {
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

    platform::wait_for_modifiers_released();
    platform::backspace(hit.delete);

    if !template::fields(&segs).is_empty() {
        *state.prev_window.lock().unwrap() = Some(platform::foreground_window());
        *state.pending_form.lock().unwrap() = Some(PendingForm {
            id: snippet.id.clone(),
            trailing: hit.trailing,
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
        InjectMode::Auto => full.contains('\n') || full.chars().count() > threshold,
    };
    platform::wait_for_modifiers_released();
    if paste {
        platform::paste_text(&full);
    } else {
        platform::type_text(&full);
    }
    platform::cursor_left(cursor_back + trailing.chars().count());
}
