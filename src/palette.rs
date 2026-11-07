//! Showing and hiding the search palette window.

use crate::engine;
use crate::platform;
use crate::state::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

pub const LABEL: &str = "palette";
pub const WIDTH: f64 = 640.0;

#[derive(Clone, Serialize)]
struct OpenPayload {
    /// Set when a trigger needs its form filled in.
    form: Option<String>,
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Opens the palette on the monitor under the mouse, in the upper third of the screen.
pub fn open(app: &AppHandle, form: Option<String>) {
    let Some(w) = window(app) else { return };
    let state = app.state::<Arc<AppState>>();
    if form.is_none() {
        *state.pending_form.lock().unwrap() = None;
        let fg = platform::foreground_window();
        if !platform::is_own_window(fg) {
            *state.prev_window.lock().unwrap() = Some(fg);
        }
    }

    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let width = (WIDTH * scale) as i32;
        let x = m.position().x + (m.size().width as i32 - width) / 2;
        let y = m.position().y + (m.size().height as f64 * 0.2) as i32;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }

    let _ = app.emit_to(LABEL, "palette-open", OpenPayload { form });
    let _ = w.show();
    if let Ok(hwnd) = w.hwnd() {
        platform::focus_window(hwnd.0 as isize);
    }
    let _ = w.set_focus();
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.hide();
    }
}

pub fn toggle(app: &AppHandle) {
    match window(app) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let state = app.state::<Arc<AppState>>().inner().clone();
            hide(app);
            engine::close_palette(&state);
        }
        _ => open(app, None),
    }
}

pub fn resize(app: &AppHandle, height: f64) {
    if let Some(w) = window(app) {
        // Never taller than the screen: a snippet with a lot of fill-in fields
        // used to be clipped, with the Insert hint out of reach.
        let tallest = w
            .current_monitor()
            .ok()
            .flatten()
            .map(|m| (m.size().height as f64 / m.scale_factor()).floor() - 24.0)
            .unwrap_or(860.0)
            .max(200.0);
        let _ = w.set_size(LogicalSize::new(WIDTH, height.clamp(60.0, tallest)));
    }
}

pub fn setup(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    if let Ok(hwnd) = w.hwnd() {
        platform::round_corners(hwnd.0 as isize);
    }
    let handle = app.clone();
    w.on_window_event(move |ev| {
        if let tauri::WindowEvent::Focused(false) = ev {
            // Clicking away closes the palette and cancels any form. Focus
            // belongs to whatever was clicked, so don't take it back.
            let state = handle.state::<Arc<AppState>>().inner().clone();
            engine::dismiss_palette(&state);
            hide(&handle);
        }
    });
}
