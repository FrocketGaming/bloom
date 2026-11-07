//! The caret dropdown: a short list of snippets shown next to the text cursor.
//!
//! The window never takes focus, so the app being typed into keeps its cursor.

use crate::platform::{self, CaretPosition};
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

pub const LABEL: &str = "dropdown";

/// Window width in logical pixels.
const WIDTH: f64 = 300.0;
/// Logical pixels for one row, the list's padding, and the preview strip.
const ROW_HEIGHT: f64 = 37.0;
const LIST_PADDING: f64 = 12.0;
const PREVIEW_HEIGHT: f64 = 68.0;
/// Gap between the caret and the window, in logical pixels.
const GAP: f64 = 4.0;

#[derive(Clone, Serialize)]
pub struct Row {
    pub trigger: String,
    pub name: String,
    /// True when picking it opens a fill-in form first.
    pub form: bool,
}

#[derive(Clone, Serialize)]
struct Rows {
    rows: Vec<Row>,
    selected: usize,
    preview: String,
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

/// Shows `rows` next to the caret without taking focus.
pub fn show_at(
    app: &AppHandle,
    caret: &CaretPosition,
    rows: &[Row],
    selected: usize,
    preview: &str,
) {
    let Some(w) = window(app) else { return };
    let Ok(hwnd) = w.hwnd() else { return };
    let Some(monitor) = app
        .monitor_from_point(caret.left as f64, caret.top as f64)
        .ok()
        .flatten()
    else {
        return;
    };

    let scale = monitor.scale_factor();
    let height = LIST_PADDING + ROW_HEIGHT * rows.len() as f64 + PREVIEW_HEIGHT;
    let size = ((WIDTH * scale) as i32, (height * scale) as i32);
    let screen = Area {
        left: monitor.position().x,
        top: monitor.position().y,
        right: monitor.position().x + monitor.size().width as i32,
        bottom: monitor.position().y + monitor.size().height as i32,
    };
    let caret_area = Area {
        left: caret.left,
        top: caret.top,
        right: caret.right,
        bottom: caret.bottom,
    };
    let (x, y) = place(caret_area, size, (GAP * scale) as i32, screen);

    let _ = app.emit_to(
        LABEL,
        "dropdown-rows",
        Rows {
            rows: rows.to_vec(),
            selected,
            preview: preview.to_string(),
        },
    );
    platform::place_without_activating(hwnd.0 as isize, x, y, size.0, size.1);
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.hide();
    }
}

/// Demo for checking the window by hand: with `BLOOM_DROPDOWN_DEMO` set, a
/// fixed list follows the text cursor in whatever app has focus. The real
/// triggering arrives with the key handling in a later step.
pub fn start_demo(app: &AppHandle) {
    if std::env::var_os("BLOOM_DROPDOWN_DEMO").is_none() {
        return;
    }
    let rows = vec![
        Row {
            trigger: ";sig".into(),
            name: "Email signature".into(),
            form: false,
        },
        Row {
            trigger: ";fu".into(),
            name: "Meeting follow-up".into(),
            form: true,
        },
        Row {
            trigger: ";date".into(),
            name: "Today's date".into(),
            form: false,
        },
    ];
    let preview = "Hi {{field:Name}},\n\nThanks for meeting on {{date}}.";
    let handle = app.clone();
    let spawned = thread::Builder::new()
        .name("bloom-dropdown-demo".into())
        .spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(250));
                let in_own_window = platform::is_own_window(platform::foreground_window());
                match platform::caret_position() {
                    Some(caret) if !in_own_window => show_at(&handle, &caret, &rows, 1, preview),
                    _ => hide(&handle),
                }
            }
        });
    if spawned.is_err() {
        eprintln!("bloom: couldn't start the dropdown demo");
    }
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
}
