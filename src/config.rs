//! Where Bloom keeps its files, and the `bloom.toml` settings file.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InjectMode {
    /// Type short single-line text as keystrokes, paste anything longer.
    Auto,
    /// Always type as keystrokes.
    Keys,
    /// Always paste through the clipboard.
    Clipboard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Hotkey that opens the search palette, e.g. "Ctrl+Shift+Space".
    pub hotkey: String,
    pub inject: InjectMode,
    /// In `auto` mode, expansions longer than this are pasted.
    pub clipboard_threshold: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            inject: InjectMode::Auto,
            clipboard_threshold: 80,
        }
    }
}

const DEFAULT_CONFIG: &str = r#"# Bloom settings. You can also change these from the Bloom window.

# Hotkey that opens snippet search.
hotkey = "Ctrl+Shift+Space"

# How Bloom inserts text:
#   "auto"      types short text as keystrokes and pastes long or multi-line text
#   "keys"      always types
#   "clipboard" always pastes (your clipboard is restored afterwards)
inject = "auto"

# In "auto" mode, text longer than this many characters is pasted.
clipboard_threshold = 80
"#;

const DEFAULT_SNIPPETS: &str = r#"# Your snippets. Bloom reloads this folder as soon as you save a file.
#
# Each [[snippet]] needs a body. The trigger is optional: snippets without
# one are still available from search (Ctrl+Shift+Space by default).
#
# Inside a body you can use:
#   {{date}}                today's date, e.g. Nov 7, 2026
#   {{date:%d/%m/%Y}}       a custom format (chrono strftime codes)
#   {{time}}                the current time
#   {{clipboard}}           whatever text is on your clipboard
#   {{cursor}}              where the text cursor ends up after inserting
#   {{field:Name}}          ask for a value before inserting
#   {{field:Tone|Warm|Formal}}  ask you to pick one of these options
#
# Triggers start with ; because a semicolon is almost never followed directly
# by a letter, so they don't fire by accident. They also only expand at the
# start of a word: after a space, punctuation or a new line. Set word = false
# on a snippet to let it expand in the middle of a word too.

[[snippet]]
trigger = ";sig"
name = "Email signature"
body = """
Best,
Your Name"""

[[snippet]]
trigger = ";date"
name = "Today's date"
body = "{{date}}"

[[snippet]]
trigger = ";time"
name = "Current time"
body = "{{time}}"

[[snippet]]
trigger = ";shrug"
name = "Shrug"
body = '¯\_(ツ)_/¯'

[[snippet]]
trigger = ";quote"
name = "Quote clipboard"
body = """
> {{clipboard}}

{{cursor}}"""

[[snippet]]
trigger = ";hi"
name = "Greeting"
body = """
Hi {{field:Name}},

{{cursor}}

Thanks,
Your Name"""

[[snippet]]
trigger = ";fu"
name = "Meeting follow-up"
body = """
Hi {{field:Name}},

Thanks for meeting on {{date}}. As promised, here are the next steps:

- {{cursor}}

{{field:Sign-off|Best|Cheers|Kind regards}},
Your Name"""

[[snippet]]
name = "Office address (search only)"
body = """
221 Harbor Street, Suite 4
Portland, OR 97209"""
"#;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub snippets_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Self {
        let root = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Bloom");
        Self {
            config_file: root.join("bloom.toml"),
            snippets_dir: root.join("snippets"),
            root,
        }
    }

    /// Creates the folders and starter files. Returns true on the very first run.
    pub fn ensure(&self) -> Result<bool> {
        let first_run = !self.config_file.exists();
        fs::create_dir_all(&self.snippets_dir)
            .with_context(|| format!("creating {}", self.snippets_dir.display()))?;
        if first_run {
            fs::write(&self.config_file, DEFAULT_CONFIG)?;
            let has_snippets = fs::read_dir(&self.snippets_dir)?
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "toml"));
            if !has_snippets {
                fs::write(self.snippets_dir.join("personal.toml"), DEFAULT_SNIPPETS)?;
            }
        }
        Ok(first_run)
    }
}

/// Loads `bloom.toml`. A broken file falls back to defaults and reports why.
pub fn load(paths: &Paths) -> (Config, Option<String>) {
    match fs::read_to_string(&paths.config_file) {
        Err(_) => (Config::default(), None),
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(c) => (c, None),
            Err(e) => (
                Config::default(),
                Some(format!(
                    "bloom.toml has an error, so defaults are in use: {e}"
                )),
            ),
        },
    }
}

/// Writes settings back, keeping the comments already in the file.
pub fn save(paths: &Paths, config: &Config) -> Result<()> {
    let text = fs::read_to_string(&paths.config_file).unwrap_or_else(|_| DEFAULT_CONFIG.into());
    let mut doc: toml_edit::DocumentMut = text.parse().unwrap_or_default();
    doc["hotkey"] = toml_edit::value(config.hotkey.clone());
    doc["inject"] = toml_edit::value(match config.inject {
        InjectMode::Auto => "auto",
        InjectMode::Keys => "keys",
        InjectMode::Clipboard => "clipboard",
    });
    doc["clipboard_threshold"] = toml_edit::value(config.clipboard_threshold as i64);
    fs::write(&paths.config_file, doc.to_string())?;
    Ok(())
}
