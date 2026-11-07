use crate::palette;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                palette::toggle(app);
            }
        })
        .build()
}

fn parse(hotkey: &str) -> Result<Shortcut, String> {
    hotkey
        .parse()
        .map_err(|_| format!("“{hotkey}” isn't a hotkey Bloom understands."))
}

/// Switches the palette hotkey. If the new one can't be registered
/// (usually because another app owns it), the old one is restored.
pub fn register(app: &AppHandle, hotkey: &str, previous: Option<&str>) -> Result<(), String> {
    let new = parse(hotkey)?;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if gs.register(new).is_err() {
        if let Some(old) = previous.and_then(|p| parse(p).ok()) {
            let _ = gs.register(old);
        }
        return Err(format!(
            "{hotkey} is already used by another app. Pick a different combination."
        ));
    }
    Ok(())
}
