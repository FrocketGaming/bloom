//! Loading, saving and watching the snippet files in the snippets folder.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

#[derive(Deserialize, Default)]
struct SnippetFile {
    #[serde(default)]
    snippet: Vec<RawSnippet>,
}

#[derive(Deserialize)]
struct RawSnippet {
    trigger: Option<String>,
    name: Option<String>,
    body: String,
    /// Only expand at the start of a word. On unless a snippet sets `word = false`.
    #[serde(default = "default_word")]
    word: bool,
}

fn default_word() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct Snippet {
    /// Stable within one load: "<file name>#<index>".
    pub id: String,
    pub file: String,
    pub index: usize,
    pub group: String,
    pub trigger: Option<String>,
    pub name: String,
    pub body: String,
    pub word: bool,
}

/// A trigger prepared for the matcher.
#[derive(Debug, Clone)]
pub struct Trigger {
    pub text: Vec<char>,
    pub word: bool,
    pub id: String,
}

#[derive(Default)]
pub struct Store {
    pub snippets: Vec<Snippet>,
    pub triggers: Arc<Vec<Trigger>>,
    pub errors: Vec<String>,
    pub files: Vec<String>,
}

impl Store {
    pub fn load(dir: &Path) -> Store {
        let mut store = Store::default();
        let mut paths: Vec<_> = fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                    .collect()
            })
            .unwrap_or_default();
        paths.sort();

        for path in paths {
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            store.files.push(file.clone());
            let parsed = fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|t| toml::from_str::<SnippetFile>(&t).map_err(anyhow::Error::from));
            let parsed = match parsed {
                Ok(p) => p,
                Err(e) => {
                    store.errors.push(format!("{file}: {e}"));
                    continue;
                }
            };
            let group = group_name(&path);
            for (index, raw) in parsed.snippet.into_iter().enumerate() {
                let trigger = raw.trigger.filter(|t| !t.trim().is_empty());
                let name = raw
                    .name
                    .filter(|n| !n.trim().is_empty())
                    .or_else(|| trigger.clone())
                    .unwrap_or_else(|| first_line(&raw.body));
                store.snippets.push(Snippet {
                    id: format!("{file}#{index}"),
                    file: file.clone(),
                    index,
                    group: group.clone(),
                    trigger,
                    name,
                    body: raw.body.replace("\r\n", "\n"),
                    word: raw.word,
                });
            }
        }

        store.triggers = Arc::new(
            store
                .snippets
                .iter()
                .filter_map(|s| {
                    s.trigger.as_ref().map(|t| Trigger {
                        text: t.chars().collect(),
                        word: s.word,
                        id: s.id.clone(),
                    })
                })
                .collect(),
        );
        store
    }

    pub fn get(&self, id: &str) -> Option<&Snippet> {
        self.snippets.iter().find(|s| s.id == id)
    }
}

fn group_name(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let words: Vec<String> = stem
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    words.join(" ")
}

fn first_line(body: &str) -> String {
    let line = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Untitled");
    let mut s: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        s.push('…');
    }
    s
}

#[derive(Debug, Deserialize)]
pub struct SnippetEdit {
    /// File name inside the snippets folder, e.g. "personal.toml".
    pub file: String,
    /// None creates a new snippet at the end of the file.
    pub index: Option<usize>,
    pub trigger: Option<String>,
    pub name: Option<String>,
    pub body: String,
    pub word: bool,
}

fn safe_file_name(file: &str) -> Result<String> {
    let mut f = file.trim().to_string();
    if f.is_empty() {
        bail!("Choose a file for the snippet.");
    }
    if !f.ends_with(".toml") {
        f.push_str(".toml");
    }
    if f.contains(['/', '\\', ':']) || f.starts_with('.') {
        bail!("File names can't contain / \\ or :");
    }
    Ok(f)
}

fn load_doc(path: &Path) -> Result<DocumentMut> {
    let text = fs::read_to_string(path).unwrap_or_default();
    text.parse::<DocumentMut>().map_err(|e| {
        anyhow!(
            "{} can't be edited until its error is fixed: {e}",
            path.display()
        )
    })
}

/// Writes a snippet into its file, keeping the file's comments and formatting.
/// Returns the snippet's id after saving.
pub fn save(dir: &Path, edit: SnippetEdit) -> Result<String> {
    let file = safe_file_name(&edit.file)?;
    let path = dir.join(&file);
    let mut doc = load_doc(&path)?;
    if !doc.contains_key("snippet") {
        doc["snippet"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let arr = doc["snippet"]
        .as_array_of_tables_mut()
        .context("`snippet` in this file isn't a list of [[snippet]] tables")?;
    let index = match edit.index {
        Some(i) if i < arr.len() => i,
        Some(_) => {
            bail!("That snippet no longer exists in {file}. It may have been edited outside Bloom.")
        }
        None => {
            arr.push(Table::new());
            arr.len() - 1
        }
    };
    let tbl = arr.get_mut(index).unwrap();
    set_opt(tbl, "trigger", edit.trigger);
    set_opt(tbl, "name", edit.name);
    tbl["body"] = Item::Value(body_value(&edit.body.replace("\r\n", "\n"))?);
    // Start-of-word is the default, so only the exception is written out.
    if edit.word {
        tbl.remove("word");
    } else {
        tbl["word"] = toml_edit::value(false);
    }
    fs::write(&path, doc.to_string())?;
    Ok(format!("{file}#{index}"))
}

pub fn delete(dir: &Path, file: &str, index: usize) -> Result<()> {
    let file = safe_file_name(file)?;
    let path = dir.join(&file);
    let mut doc = load_doc(&path)?;
    let arr = doc["snippet"]
        .as_array_of_tables_mut()
        .context("no snippets in this file")?;
    if index >= arr.len() {
        bail!("That snippet no longer exists in {file}.");
    }
    arr.remove(index);
    fs::write(&path, doc.to_string())?;
    Ok(())
}

fn set_opt(tbl: &mut Table, key: &str, v: Option<String>) {
    match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(s) => tbl[key] = toml_edit::value(s),
        None => {
            tbl.remove(key);
        }
    }
}

/// Multi-line bodies are written as `"""` blocks so the files stay readable.
fn body_value(body: &str) -> Result<Value> {
    if !body.contains('\n') {
        return Ok(Value::from(body.to_string()));
    }
    let mut esc = String::with_capacity(body.len() + 8);
    let mut quotes = 0;
    for ch in body.chars() {
        match ch {
            '\\' => {
                esc.push_str("\\\\");
                quotes = 0;
            }
            '"' => {
                quotes += 1;
                if quotes == 3 {
                    esc.push_str("\\\"");
                    quotes = 0;
                } else {
                    esc.push('"');
                }
            }
            c if c.is_control() && c != '\n' && c != '\t' => {
                esc.push_str(&format!("\\u{:04X}", c as u32));
                quotes = 0;
            }
            c => {
                esc.push(c);
                quotes = 0;
            }
        }
    }
    if esc.ends_with('"') && !esc.ends_with("\\\"") {
        esc.pop();
        esc.push_str("\\\"");
    }
    Value::from_str(&format!("\"\"\"\n{esc}\"\"\""))
        .map_err(|e| anyhow!("couldn't encode the snippet body: {e}"))
}

/// Calls `on_change` (debounced) whenever a file in the folder changes.
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<()> {
    use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};
    let mut debouncer = new_debouncer(
        Duration::from_millis(250),
        move |res: DebounceEventResult| {
            if res.is_ok() {
                on_change();
            }
        },
    )?;
    debouncer
        .watcher()
        .watch(dir, RecursiveMode::NonRecursive)?;
    // The watcher lives for the whole app run.
    Box::leak(Box::new(debouncer));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_body_round_trips() {
        let body = "Hi \"there\",\nback\\slash \"\"\" end\"";
        let v = body_value(body).unwrap();
        let doc: DocumentMut = format!("body = {v}").parse().unwrap();
        assert_eq!(doc["body"].as_str().unwrap(), body);
    }

    #[test]
    fn save_creates_and_edits() {
        let dir = std::env::temp_dir().join(format!("bloom-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.toml"),
            "# keep me\n[[snippet]]\ntrigger = \";x\"\nbody = \"x\"\n",
        )
        .unwrap();
        let id = save(
            &dir,
            SnippetEdit {
                file: "a".into(),
                index: None,
                trigger: Some(";y".into()),
                name: None,
                body: "line1\nline2".into(),
                word: false,
            },
        )
        .unwrap();
        assert_eq!(id, "a.toml#1");
        let text = fs::read_to_string(dir.join("a.toml")).unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("word = false"));
        let store = Store::load(&dir);
        assert_eq!(store.snippets.len(), 2);
        assert!(store.snippets[0].word, "start-of-word is the default");
        assert_eq!(store.snippets[1].body, "line1\nline2");
        assert!(!store.snippets[1].word);
        delete(&dir, "a.toml", 0).unwrap();
        assert_eq!(Store::load(&dir).snippets[0].trigger.as_deref(), Some(";y"));
        fs::remove_dir_all(&dir).ok();
    }
}
