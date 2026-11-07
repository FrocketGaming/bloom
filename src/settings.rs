use tauri::{AppHandle, Manager};

pub const LABEL: &str = "settings";

pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Closing the settings window only hides it; Bloom keeps running in the tray.
pub fn setup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let win = w.clone();
        w.on_window_event(move |ev| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                api.prevent_close();
                let _ = win.hide();
            }
        });
    }
}
