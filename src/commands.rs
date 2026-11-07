//! Commands the palette and settings windows call from JavaScript.

use crate::config::{self, Config};
use crate::snippets::{self, SnippetEdit};
use crate::state::AppState;
use crate::template::{self, FieldSpec, Part};
use crate::variables::{self, Content, Variable};
use crate::{engine, hotkey, matcher, palette, platform, tray, usage};
use serde::{Deserialize, Serialize};
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
    /// `{{var:name}}` names in the body that no variable has.
    missing: Vec<String>,
}

#[tauri::command]
pub fn list_snippets(state: S) -> Vec<SnippetView> {
    let store = state.store.read().unwrap();
    let variables = state.variables.read().unwrap();
    // Every body is parsed once, and the clipboard is read at most once rather
    // than once per snippet that uses {{clipboard}}.
    let parsed: Vec<Vec<template::Seg>> = store
        .snippets
        .iter()
        .map(|s| template::parse_resolved(&s.body, &variables))
        .collect();
    let clipboard = parsed
        .iter()
        .any(|segs| template::uses_clipboard(segs))
        .then(template::read_clipboard)
        .flatten();
    store
        .snippets
        .iter()
        .zip(parsed)
        .map(|(s, segs)| SnippetView {
            id: s.id.clone(),
            file: s.file.clone(),
            index: s.index,
            group: s.group.clone(),
            trigger: s.trigger.clone(),
            name: s.name.clone(),
            body: s.body.clone(),
            word: s.word,
            preview: template::preview(&segs, clipboard.as_deref()),
            fields: template::fields(&segs),
            missing: template::missing(&segs),
        })
        .collect()
}

#[derive(Serialize)]
pub struct BodyPreview {
    preview: Vec<Part>,
    fields: Vec<FieldSpec>,
    missing: Vec<String>,
}

#[tauri::command]
pub fn preview_body(state: S, body: String) -> BodyPreview {
    let segs = template::parse_resolved(&body, &state.variables.read().unwrap());
    let clipboard = template::uses_clipboard(&segs)
        .then(template::read_clipboard)
        .flatten();
    BodyPreview {
        preview: template::preview(&segs, clipboard.as_deref()),
        fields: template::fields(&segs),
        missing: template::missing(&segs),
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
    let segs = template::parse_resolved(&snippet.body, &state.variables.read().unwrap());
    let fields = template::fields(&segs);
    if !fields.is_empty() && values.is_none() {
        return Ok(InsertResult::NeedsForm { fields });
    }

    // A form that a typed trigger opened remembers what was typed. One opened
    // from the search hotkey has no trigger, so there is nothing to type back
    // and the insertion cannot be taken back with Backspace.
    let form = state
        .pending_form
        .lock()
        .unwrap()
        .take()
        .filter(|f| f.id == id);
    let (trailing, typed) = match form {
        Some(f) => (f.trailing, Some(f.restore)),
        None => (String::new(), None),
    };
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
        usage::record_use(&st, &snippet);
        if let Some(typed) = typed {
            engine::remember_undo(
                &st,
                &rendered.text,
                &trailing,
                rendered.cursor_back,
                typed,
                usage::key_of(&snippet),
            );
        }
    });
    Ok(InsertResult::Inserted)
}

#[tauri::command]
pub fn hide_palette(app: AppHandle, state: S) {
    palette::hide(&app);
    engine::close_palette(&state.inner().clone());
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
    errors.extend(state.variables.read().unwrap().errors.iter().cloned());
    {
        let variables = state.variables.read().unwrap();
        for snippet in &store.snippets {
            let segs = template::parse_resolved(&snippet.body, &variables);
            let missing = template::missing(&segs);
            if !missing.is_empty() {
                let names: Vec<String> =
                    missing.iter().map(|n| format!("{{{{var:{n}}}}}")).collect();
                errors.push(format!(
                    "“{}” uses {}, which no variable defines. Add it on the Variables page.",
                    snippet.name,
                    names.join(", ")
                ));
            }
        }
    }
    if let Some(e) = state.config_error.read().unwrap().clone() {
        errors.insert(0, e);
    }
    // Windows refuses input aimed at elevated windows, and there is nowhere
    // better than here to say so.
    if platform::take_input_blocked() {
        errors.insert(
            0,
            "Windows refused Bloom's last text insertion. The window you were typing in probably runs as administrator, which blocks input from ordinary apps.".into(),
        );
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
    let new_hotkey = config.hotkey != old.hotkey;
    if new_hotkey {
        hotkey::register(&app, &config.hotkey, Some(&old.hotkey))?;
    }
    // Write before adopting: if the file can't be written (a syntax error in
    // bloom.toml, a read-only folder), the running app must not behave like the
    // new settings were saved.
    if let Err(e) = config::save(&state.paths, &config) {
        if new_hotkey {
            let _ = hotkey::register(&app, &old.hotkey, None);
        }
        return Err(e.to_string());
    }
    *state.config.write().unwrap() = config.clone();
    *state.config_error.write().unwrap() = None;
    tray::set_hotkey(&app, &config.hotkey);
    Ok(())
}

/// One variable as the Variables page edits it. Both kinds carry both fields,
/// so switching a row between text and choices does not lose what was typed.
#[derive(Serialize)]
pub struct VariableView {
    name: String,
    /// "text" or "choices".
    kind: &'static str,
    text: String,
    options: Vec<String>,
    /// How many snippets use `{{var:name}}`.
    used_in: usize,
}

/// A name that snippets use but no variable defines.
#[derive(Serialize)]
pub struct UndefinedView {
    name: String,
    used_in: usize,
}

#[derive(Serialize)]
pub struct VariablesView {
    variables: Vec<VariableView>,
    undefined: Vec<UndefinedView>,
}

#[tauri::command]
pub fn list_variables(state: S) -> VariablesView {
    let store = state.store.read().unwrap();
    let variables = state.variables.read().unwrap();

    // Lower-cased name -> (name as first typed, snippets using it).
    let mut uses: HashMap<String, (String, usize)> = HashMap::new();
    for snippet in &store.snippets {
        for name in template::variable_names(&template::parse(&snippet.body)) {
            uses.entry(name.to_lowercase()).or_insert((name, 0)).1 += 1;
        }
    }
    let used_in = |name: &str| uses.get(&name.to_lowercase()).map_or(0, |(_, n)| *n);

    let listed = variables
        .list
        .iter()
        .map(|variable| {
            let (kind, text, options) = match &variable.content {
                Content::Text(text) => ("text", text.clone(), Vec::new()),
                Content::Choices(options) => ("choices", String::new(), options.clone()),
            };
            VariableView {
                name: variable.name.clone(),
                kind,
                text,
                options,
                used_in: used_in(&variable.name),
            }
        })
        .collect();

    let mut undefined: Vec<UndefinedView> = uses
        .iter()
        .filter(|(key, _)| variables.get(key).is_none())
        .map(|(_, (name, count))| UndefinedView {
            name: name.clone(),
            used_in: *count,
        })
        .collect();
    undefined.sort_by_key(|u| u.name.to_lowercase());

    VariablesView {
        variables: listed,
        undefined,
    }
}

/// A variable as sent back from the Variables page.
#[derive(Deserialize)]
pub struct VariableEdit {
    name: String,
    kind: String,
    text: String,
    options: Vec<String>,
}

fn to_variable(edit: VariableEdit) -> Result<Variable, String> {
    let name = edit.name.trim().to_string();
    let content = match edit.kind.as_str() {
        "text" => Content::Text(edit.text),
        "choices" => Content::Choices(
            edit.options
                .into_iter()
                .map(|option| option.trim().to_string())
                .filter(|option| !option.is_empty())
                .collect(),
        ),
        other => return Err(format!("Unknown kind “{other}” for “{name}”.")),
    };
    Ok(Variable { name, content })
}

#[tauri::command]
pub fn save_variables(state: S, variables: Vec<VariableEdit>) -> Result<(), String> {
    let list = variables
        .into_iter()
        .map(to_variable)
        .collect::<Result<Vec<_>, _>>()?;
    variables::save(&state.paths.variables_file, &list).map_err(|e| e.to_string())?;
    // The folder watcher would pick this up too; reloading now means the next
    // snippet expansion already sees the change.
    state.reload_variables();
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
    if trigger.chars().count() > matcher::MAX_TRIGGER {
        return Err(format!(
            "Triggers can be at most {} characters.",
            matcher::MAX_TRIGGER
        ));
    }
    if trigger.contains(['\r', '\n', '\t']) {
        return Err("Triggers can't contain line breaks or tabs.".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(name: &str, kind: &str, text: &str, options: &[&str]) -> VariableEdit {
        VariableEdit {
            name: name.into(),
            kind: kind.into(),
            text: text.into(),
            options: options.iter().map(|o| o.to_string()).collect(),
        }
    }

    #[test]
    fn a_text_row_keeps_its_text_exactly_and_ignores_its_options() {
        let variable = to_variable(edit(" email ", "text", " a@b.c \n", &["stale"])).unwrap();
        assert_eq!(variable.name, "email");
        assert_eq!(variable.content, Content::Text(" a@b.c \n".into()));
    }

    #[test]
    fn a_choices_row_trims_and_drops_blank_options_and_ignores_its_text() {
        let variable = to_variable(edit(
            "signoff",
            "choices",
            "stale",
            &[" Best ", "", "  ", "Cheers"],
        ))
        .unwrap();
        assert_eq!(
            variable.content,
            Content::Choices(vec!["Best".into(), "Cheers".into()])
        );
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        assert!(to_variable(edit("x", "number", "", &[])).is_err());
    }
}
