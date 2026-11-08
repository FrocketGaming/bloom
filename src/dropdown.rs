//! The caret dropdown: a short list of snippets shown next to the text cursor.
//!
//! The window never takes focus, so the app being typed into keeps its cursor.
//! The engine decides when to open it; this module draws it and tracks which
//! row is highlighted.

use crate::matcher::Suggestion;
use crate::platform::{self, CaretPosition};
use crate::state::AppState;
use crate::template;
use crate::usage;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

pub const LABEL: &str = "dropdown";

/// Most rows shown at once.
const MAX_ROWS: usize = 6;
/// Window width in logical pixels.
const WIDTH: f64 = 360.0;
/// Logical pixels for one row, the list's padding, and the preview strip.
const ROW_HEIGHT: f64 = 37.0;
const LIST_PADDING: f64 = 12.0;
const PREVIEW_HEIGHT: f64 = 68.0;
/// Gap between the caret and the window, in logical pixels.
const GAP: f64 = 4.0;

#[derive(Clone, Serialize)]
struct Row {
    trigger: String,
    name: String,
    /// True when picking it opens a fill-in form first.
    form: bool,
}

#[derive(Clone, Serialize)]
struct Rows {
    rows: Vec<Row>,
    selected: usize,
    preview: String,
}

/// An open dropdown: what it lists, which row is highlighted, and where it sits.
pub struct Menu {
    ids: Vec<String>,
    selected: usize,
    /// What the user has typed so far, which is erased when a snippet is picked.
    pub typed: String,
    caret: CaretPosition,
}

impl Menu {
    /// The snippet that Enter or Tab would insert.
    pub fn selected_id(&self) -> Option<String> {
        self.ids.get(self.selected).cloned()
    }
}

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Area {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Gives the dropdown its no-focus window style and rounded corners.
pub fn setup(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    if let Ok(hwnd) = w.hwnd() {
        platform::make_non_activating(hwnd.0 as isize);
        platform::round_corners(hwnd.0 as isize);
    }
}

/// Picks the top-left corner for a window of `size` (width, height) next to the
/// caret: below it if there is room, otherwise above, and always inside `screen`.
fn place(caret: Area, size: (i32, i32), gap: i32, screen: Area) -> (i32, i32) {
    let (width, height) = size;
    let below = caret.bottom + gap;
    let y = if below + height <= screen.bottom {
        below
    } else {
        caret.top - gap - height
    };
    let y = y.min(screen.bottom - height).max(screen.top);
    let x = caret.left.min(screen.right - width).max(screen.left);
    (x, y)
}

/// The first couple of non-blank lines of a snippet, as written. Placeholders
/// stay as typed so nothing (the clipboard, say) is read just to draw a preview.
fn preview_of(body: &str) -> String {
    body.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Opens the dropdown for `suggestion`, or updates it if `previous` is open.
/// Returns `None` (and closes it) when the app won't say where its text cursor
/// is: typing and instant expansion carry on as usual, just without a list.
pub fn open(
    app: &AppHandle,
    state: &AppState,
    suggestion: Suggestion,
    previous: Option<Menu>,
) -> Option<Menu> {
    let mut ids = suggestion.ids;
    // Most used first, and before cutting the list down to what fits, so a
    // favourite is never left out.
    rank_by_use(state, &mut ids);
    ids.truncate(MAX_ROWS);

    // The list stays where it opened while the user types, instead of
    // following the cursor and jittering.
    let caret = match &previous {
        Some(menu) => Some(menu.caret),
        None => platform::caret_position(),
    };
    let Some(caret) = caret else {
        close(app);
        return None;
    };
    let selected = match &previous {
        Some(menu) if menu.ids == ids => menu.selected,
        _ => 0,
    };

    let menu = Menu {
        ids,
        selected,
        typed: suggestion.text,
        caret,
    };
    render(app, state, &menu);
    platform::set_list_keys(true);
    Some(menu)
}

/// Puts the snippets used most (and most recently) first. Snippets with equal
/// scores, which includes ones never used, keep their order.
fn rank_by_use(state: &AppState, ids: &mut [String]) {
    if !usage::enabled(state) {
        return;
    }
    let store = state.store.read().unwrap();
    state.usage.lock().unwrap().rank(ids, usage::now(), |id| {
        store.get(id).map(usage::key_of).unwrap_or_default()
    });
}

/// Moves the highlight up (`-1`) or down (`1`), wrapping around.
pub fn select(app: &AppHandle, state: &AppState, menu: &mut Menu, step: isize) {
    let count = menu.ids.len() as isize;
    if count == 0 {
        return;
    }
    menu.selected = (menu.selected as isize + step).rem_euclid(count) as usize;
    render(app, state, menu);
    platform::set_list_keys(true);
}

/// Hides the dropdown and gives the arrow, Enter, Tab and Esc keys back.
pub fn close(app: &AppHandle) {
    platform::set_list_keys(false);
    if let Some(w) = window(app)
        && let Ok(hwnd) = w.hwnd()
    {
        platform::hide_window(hwnd.0 as isize);
    }
}

fn render(app: &AppHandle, state: &AppState, menu: &Menu) {
    let Some(w) = window(app) else { return };
    let Ok(hwnd) = w.hwnd() else { return };
    let Some(monitor) = app
        .monitor_from_point(menu.caret.left as f64, menu.caret.top as f64)
        .ok()
        .flatten()
    else {
        return;
    };

    let store = state.store.read().unwrap();
    let variables = state.variables.read().unwrap();
    let mut rows = Vec::new();
    let mut preview = String::new();
    for (index, id) in menu.ids.iter().enumerate() {
        let Some(snippet) = store.get(id) else {
            continue;
        };
        if index == menu.selected {
            preview = preview_of(&snippet.body);
        }
        rows.push(Row {
            trigger: snippet.trigger.clone().unwrap_or_default(),
            name: snippet.name.clone(),
            form: !template::fields(&template::parse_resolved(&snippet.body, &variables))
                .is_empty(),
        });
    }
    drop(store);
    drop(variables);

    let scale = monitor.scale_factor();
    let height = LIST_PADDING + ROW_HEIGHT * rows.len() as f64 + PREVIEW_HEIGHT;
    let size = ((WIDTH * scale) as i32, (height * scale) as i32);
    let screen = Area {
        left: monitor.position().x,
        top: monitor.position().y,
        right: monitor.position().x + monitor.size().width as i32,
        bottom: monitor.position().y + monitor.size().height as i32,
    };
    let caret = Area {
        left: menu.caret.left,
        top: menu.caret.top,
        right: menu.caret.right,
        bottom: menu.caret.bottom,
    };
    let (x, y) = place(caret, size, (GAP * scale) as i32, screen);

    let _ = app.emit_to(
        LABEL,
        "dropdown-rows",
        Rows {
            rows,
            selected: menu.selected,
            preview,
        },
    );
    platform::place_without_activating(hwnd.0 as isize, x, y, size.0, size.1);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Area = Area {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    fn caret(left: i32, top: i32) -> Area {
        Area {
            left,
            top,
            right: left + 2,
            bottom: top + 20,
        }
    }

    #[test]
    fn opens_below_the_caret_when_there_is_room() {
        assert_eq!(place(caret(100, 200), (300, 150), 4, SCREEN), (100, 224));
    }

    #[test]
    fn flips_above_near_the_bottom_of_the_screen() {
        // Below would run past the bottom edge, so it sits above the caret.
        assert_eq!(place(caret(100, 1040), (300, 150), 4, SCREEN), (100, 886));
    }

    #[test]
    fn stays_inside_the_right_edge() {
        assert_eq!(place(caret(1900, 200), (300, 150), 4, SCREEN).0, 1620);
    }

    #[test]
    fn stays_inside_a_monitor_that_does_not_start_at_zero() {
        let left_monitor = Area {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let (x, _) = place(caret(-1930, 200), (300, 150), 4, left_monitor);
        assert_eq!(x, -1920);
    }

    #[test]
    fn never_leaves_the_top_when_neither_side_fits() {
        let short = Area {
            left: 0,
            top: 0,
            right: 800,
            bottom: 100,
        };
        assert_eq!(place(caret(10, 40), (300, 150), 4, short).1, 0);
    }

    #[test]
    fn preview_skips_blank_lines_and_keeps_placeholders() {
        let body = "Hi {{field:Name}},\n\nThanks for meeting on {{date}}.\n\nBest";
        assert_eq!(
            preview_of(body),
            "Hi {{field:Name}},\nThanks for meeting on {{date}}."
        );
    }
}
