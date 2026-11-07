// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod dropdown;
mod engine;
mod files;
mod hotkey;
mod matcher;
mod palette;
mod platform;
mod settings;
mod snippets;
mod state;
mod template;
mod tray;
mod undo;
mod usage;
mod variables;

use state::AppState;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tauri::Emitter;

fn main() {
    let paths = config::Paths::resolve();
    let first_run = paths.ensure().unwrap_or_else(|e| {
        eprintln!("bloom: couldn't create {}: {e}", paths.root.display());
        false
    });
    let (config, config_error) = config::load(&paths);
    let store = snippets::Store::load(&paths.snippets_dir);
    let variables = variables::Variables::load(&paths.variables_file);
    let usage = usage::Usage::load(&paths.usage_file);
    let state = Arc::new(AppState {
        paths,
        config: RwLock::new(config),
        config_error: RwLock::new(config_error),
        store: RwLock::new(store),
        variables: RwLock::new(variables),
        paused: AtomicBool::new(false),
        prev_window: Mutex::new(None),
        pending_form: Mutex::new(None),
        undo: Mutex::new(None),
        usage: Mutex::new(usage),
    });

    tauri::Builder::default()
        // A second launch just opens the existing Bloom.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            settings::show(app)
        }))
        .plugin(hotkey::plugin())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::list_snippets,
            commands::preview_body,
            commands::insert_snippet,
            commands::hide_palette,
            commands::resize_palette,
            commands::get_settings,
            commands::save_settings,
            commands::set_paused,
            commands::save_snippet,
            commands::delete_snippet,
            commands::delete_snippet_file,
            commands::open_snippets_folder,
            commands::list_variables,
            commands::save_variables,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            palette::setup(&handle);
            dropdown::setup(&handle);
            settings::setup(&handle);
            tray::build(&handle)?;

            let hotkey = state.config.read().unwrap().hotkey.clone();
            if let Err(e) = hotkey::register(&handle, &hotkey, None) {
                let mut err = state.config_error.write().unwrap();
                *err = Some(format!("Search hotkey not active: {e}"));
            }

            let watch_handle = handle.clone();
            let watch_state = state.clone();
            if let Err(e) = snippets::watch(&state.paths.snippets_dir, move || {
                watch_state.reload_snippets();
                let _ = watch_handle.emit("snippets-changed", ());
            }) {
                eprintln!("bloom: not watching snippet files: {e}");
            }

            // variables.toml sits in the settings folder, so watch that folder,
            // but only react to that file: usage.toml is rewritten constantly.
            let vars_handle = handle.clone();
            let vars_state = state.clone();
            let vars_file = state.paths.variables_file.clone();
            if let Err(e) = snippets::watch_matching(
                &state.paths.root,
                move |changed| changed == vars_file,
                move || {
                    vars_state.reload_variables();
                    let _ = vars_handle.emit("variables-changed", ());
                },
            ) {
                eprintln!("bloom: not watching variables.toml: {e}");
            }

            usage::start_saver(state.clone());
            engine::start(handle.clone(), state.clone());

            if first_run {
                settings::show(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Bloom")
        .run(|_app, event| {
            // Keep running in the tray when every window is hidden.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event
                && code.is_none()
            {
                api.prevent_exit();
            }
        });
}
