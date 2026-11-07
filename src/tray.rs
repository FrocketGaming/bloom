use crate::state::AppState;
use crate::{palette, settings};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub struct PauseItem(pub CheckMenuItem<Wry>);

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let hotkey = app
        .state::<Arc<AppState>>()
        .config
        .read()
        .unwrap()
        .hotkey
        .clone();
    let search = MenuItem::with_id(
        app,
        "search",
        format!("Search snippets\t{hotkey}"),
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(app, "open", "Open Bloom", true, None::<&str>)?;
    let pause =
        CheckMenuItem::with_id(app, "pause", "Pause expansions", true, false, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", "Open snippets folder", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "Reload snippets", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Bloom", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &search, &open, &sep1, &pause, &folder, &reload, &sep2, &quit,
        ],
    )?;
    app.manage(PauseItem(pause));

    TrayIconBuilder::with_id("bloom")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Bloom")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let state = app.state::<Arc<AppState>>();
            match event.id.as_ref() {
                "search" => palette::open(app, None),
                "open" => settings::show(app),
                "pause" => {
                    let paused = !state.paused.load(Ordering::Relaxed);
                    state.paused.store(paused, Ordering::Relaxed);
                    sync_paused(app, paused);
                }
                "folder" => open_folder(&state.paths.snippets_dir),
                "reload" => {
                    state.reload_snippets();
                    let _ = app.emit("snippets-changed", ());
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                settings::show(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Keeps the tray checkbox, tooltip and settings window in step with the paused state.
pub fn sync_paused(app: &AppHandle, paused: bool) {
    if let Some(item) = app.try_state::<PauseItem>() {
        let _ = item.0.set_checked(paused);
    }
    if let Some(tray) = app.tray_by_id("bloom") {
        let _ = tray.set_tooltip(Some(if paused { "Bloom (paused)" } else { "Bloom" }));
    }
    let _ = app.emit("paused-changed", paused);
}

pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}
