//! Connects keyboard listening, the matcher and text insertion.

use crate::config::InjectMode;
use crate::matcher::{Hit, KeyEvent, Matcher, Nav};
use crate::state::{AppState, PendingForm};
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
    let segs = template::parse(&snippet.body);

    platform::wait_for_typing_to_settle();
    platform::backspace(hit.delete);

    if !template::fields(&segs).is_empty() {
        // The trigger is erased before the palette opens, so remember it:
        // pressing Esc has to leave the user with the text they typed rather
        // than nothing at all.
        let restore = restore.unwrap_or_else(|| {
            format!(
                "{}{}",
                snippet.trigger.as_deref().unwrap_or_default(),
                hit.trailing
            )
        });
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
