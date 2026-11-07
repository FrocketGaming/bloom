//! Commands the palette and settings windows call from JavaScript.

use crate::config::{self, Config};
use crate::snippets::{self, SnippetEdit};
use crate::state::AppState;
use crate::template::{self, FieldSpec, Part};
use crate::{engine, hotkey, palette, platform, tray};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

type S<'a> = State<'a, Arc<AppState>>;

#[derive(Serialize)]
pub struct SnippetView {
    id: String,
    file: String,
    index: usize,
    group: String,
    trigger: Option<String>,
    name: String,
    body: String,
    word: bool,
    preview: Vec<Part>,
    fields: Vec<FieldSpec>,
}

#[tauri::command]
pub fn list_snippets(state: S) -> Vec<SnippetView> {
    let store = state.store.read().unwrap();
    store
        .snippets
        .iter()
        .map(|s| {
            let segs = template::parse(&s.body);
            SnippetView {
                id: s.id.clone(),
                file: s.file.clone(),
                index: s.index,
                group: s.group.clone(),
                trigger: s.trigger.clone(),
                name: s.name.clone(),
                body: s.body.clone(),
                word: s.word,
                preview: template::preview(&segs),
                fields: template::fields(&segs),
            }
        })
        .collect()
}

#[derive(Serialize)]
pub struct BodyPreview {
    preview: Vec<Part>,
    fields: Vec<FieldSpec>,
}

#[tauri::command]
pub fn preview_body(body: String) -> BodyPreview {
    let segs = template::parse(&body);
    BodyPreview {
        preview: template::preview(&segs),
        fields: template::fields(&segs),
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum InsertResult {
    Inserted,
    NeedsForm { fields: Vec<FieldSpec> },
}

#[tauri::command]
pub fn insert_snippet(
    app: AppHandle,
    state: S,
    id: String,
    values: Option<HashMap<String, String>>,
) -> Result<InsertResult, String> {
    let snippet = state
        .store
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("This snippet was changed or removed. Search again.")?;
    let segs = template::parse(&snippet.body);
    let fields = template::fields(&segs);
    if !fields.is_empty() && values.is_none() {
        return Ok(InsertResult::NeedsForm { fields });
    }

    let trailing = state
        .pending_form
        .lock()
        .unwrap()
        .take()
        .filter(|f| f.id == id)
        .map(|f| f.trailing)
        .unwrap_or_default();
    let rendered = template::render(&segs, &values.unwrap_or_default());
    let prev = state.prev_window.lock().unwrap().take();
    palette::hide(&app);

    let st = state.inner().clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        if let Some(h) = prev {
            platform::focus_window(h);
        }
        thread::sleep(Duration::from_millis(90));
        engine::deliver(&st, &rendered.text, &trailing, rendered.cursor_back);
    });
    Ok(InsertResult::Inserted)
}

#[tauri::command]
pub fn hide_palette(app: AppHandle, state: S) {
    state.pending_form.lock().unwrap().take();
    let prev = state.prev_window.lock().unwrap().take();
    palette::hide(&app);
    if let Some(h) = prev {
        platform::focus_window(h);
    }
}

#[tauri::command]
pub fn resize_palette(app: AppHandle, height: f64) {
    palette::resize(&app, height);
}

#[derive(Serialize)]
pub struct SettingsView {
    config: Config,
    paused: bool,
    snippets_dir: String,
    files: Vec<String>,
    errors: Vec<String>,
}

#[tauri::command]
pub fn get_settings(state: S) -> SettingsView {
    let store = state.store.read().unwrap();
    let mut errors = store.errors.clone();
    if let Some(e) = state.config_error.read().unwrap().clone() {
        errors.insert(0, e);
    }
    SettingsView {
        config: state.config.read().unwrap().clone(),
        paused: state.paused.load(Ordering::Relaxed),
        snippets_dir: state.paths.snippets_dir.display().to_string(),
        files: store.files.clone(),
        errors,
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: S, config: Config) -> Result<(), String> {
    let old = state.config.read().unwrap().clone();
    if config.hotkey != old.hotkey {
        hotkey::register(&app, &config.hotkey, Some(&old.hotkey))?;
    }
    config::save(&state.paths, &config).map_err(|e| e.to_string())?;
    *state.config.write().unwrap() = config;
    *state.config_error.write().unwrap() = None;
    Ok(())
}

#[tauri::command]
pub fn set_paused(app: AppHandle, state: S, paused: bool) {
    state.paused.store(paused, Ordering::Relaxed);
    tray::sync_paused(&app, paused);
}

fn after_change(app: &AppHandle, state: &AppState) {
    state.reload_snippets();
    let _ = app.emit("snippets-changed", ());
}

#[tauri::command]
pub fn save_snippet(app: AppHandle, state: S, edit: SnippetEdit) -> Result<String, String> {
    let trigger = edit.trigger.as_deref().map(str::trim).unwrap_or("");
    if edit.body.is_empty() {
        return Err("Add some text for the snippet to insert.".into());
    }
    if trigger.chars().count() == 1 {
        return Err(
            "Use at least two characters for a trigger, or it will fire constantly.".into(),
        );
    }
    if !trigger.is_empty() {
        let editing = edit
            .index
            .map(|i| format!("{}#{i}", ensure_toml(&edit.file)));
        let store = state.store.read().unwrap();
        if let Some(other) = store
            .snippets
            .iter()
            .find(|s| s.trigger.as_deref() == Some(trigger) && Some(&s.id) != editing.as_ref())
        {
            return Err(format!(
                "“{trigger}” is already the trigger for “{}”.",
                other.name
            ));
        }
    }
    let id = snippets::save(&state.paths.snippets_dir, edit).map_err(|e| e.to_string())?;
    after_change(&app, &state);
    Ok(id)
}

fn ensure_toml(f: &str) -> String {
    if f.ends_with(".toml") {
        f.to_string()
    } else {
        format!("{f}.toml")
    }
}

#[tauri::command]
pub fn delete_snippet(app: AppHandle, state: S, file: String, index: usize) -> Result<(), String> {
    snippets::delete(&state.paths.snippets_dir, &file, index).map_err(|e| e.to_string())?;
    after_change(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn open_snippets_folder(state: S) {
    tray::open_folder(&state.paths.snippets_dir);
}
