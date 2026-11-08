//! The settings window. It is built when asked for and closed for good when the
//! user is done, because a web view that sits hidden still holds tens of
//! megabytes. The search window and the suggestion list stay loaded instead:
//! they must appear instantly.

use crate::state::AppState;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "settings";

/// WebView2 needs every window to use the same browser arguments, so this must
/// match `additionalBrowserArgs` in tauri.conf.json. Software rendering is
/// enough for Bloom's plain panels, and it spares the GPU process, which
/// otherwise held about 100 MB.
const BROWSER_ARGS: &str =
    "--disable-gpu --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Shows the settings window, building it first if it isn't open.
pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    match build(app) {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("bloom: couldn't open the settings window: {e}"),
    }
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("settings.html".into()))
        .title("Bloom")
        .additional_browser_args(BROWSER_ARGS)
        .inner_size(1000.0, 680.0)
        .min_inner_size(780.0, 500.0)
        .center()
        .build()?;

    let state = app.state::<Arc<AppState>>().inner().clone();
    let win = w.clone();
    w.on_window_event(move |ev| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
            // Unsaved edits live in the page, so hide the window rather than
            // lose them. Anything else closes, and its memory goes with it.
            if state.settings_unsaved.load(Ordering::Relaxed) {
                api.prevent_close();
                let _ = win.hide();
            }
        }
    });
    Ok(w)
}
