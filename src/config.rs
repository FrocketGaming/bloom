//! Where Bloom keeps its files, and the `bloom.toml` settings file.

use anyhow::{Context, Result, anyhow};
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
    /// Show a short list of matching snippets at the text cursor while a
    /// trigger is being typed.
    pub suggestions: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            inject: InjectMode::Auto,
            clipboard_threshold: 80,
            suggestions: true,
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

# Show a short list of matching snippets at your text cursor after you type the
# first few characters of a trigger. Up/Down move, Enter or Tab inserts, Esc closes.
suggestions = true
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
    // Borrowing the user's file to edit it only works while it parses. Falling
    // back to an empty document would rewrite it with three keys and no
    // comments, silently throwing away everything else in there.
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| {
        anyhow!("bloom.toml has a syntax error, so Bloom won't overwrite it. Fix the file, then save again: {e}")
    })?;
    doc["hotkey"] = toml_edit::value(config.hotkey.clone());
    doc["inject"] = toml_edit::value(match config.inject {
        InjectMode::Auto => "auto",
        InjectMode::Keys => "keys",
        InjectMode::Clipboard => "clipboard",
    });
    doc["clipboard_threshold"] = toml_edit::value(config.clipboard_threshold as i64);
    doc["suggestions"] = toml_edit::value(config.suggestions);
    crate::files::write_atomic(&paths.config_file, &doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("bloom-config-{tag}-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Paths {
            config_file: root.join("bloom.toml"),
            snippets_dir: root.join("snippets"),
            root,
        }
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        let paths = temp_paths("broken");
        let broken = "hotkey = \"Ctrl+Shift+Space\"\nthis line is not toml\n";
        fs::write(&paths.config_file, broken).unwrap();
        assert!(save(&paths, &Config::default()).is_err());
        assert_eq!(fs::read_to_string(&paths.config_file).unwrap(), broken);
        fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn saving_keeps_comments() {
        let paths = temp_paths("comments");
        fs::write(
            &paths.config_file,
            "# keep me\nhotkey = \"Ctrl+Shift+Space\"\n",
        )
        .unwrap();
        save(&paths, &Config::default()).unwrap();
        let text = fs::read_to_string(&paths.config_file).unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("clipboard_threshold"));
        fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn suggestions_are_on_by_default_and_for_older_files() {
        assert!(Config::default().suggestions);
        let paths = temp_paths("older");
        // A bloom.toml written before the setting existed has no such key.
        fs::write(&paths.config_file, "hotkey = \"Ctrl+Shift+Space\"\n").unwrap();
        let (config, error) = load(&paths);
        assert!(error.is_none());
        assert!(config.suggestions);
        fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn turning_suggestions_off_is_saved_and_read_back() {
        let paths = temp_paths("off");
        let config = Config {
            suggestions: false,
            ..Config::default()
        };
        save(&paths, &config).unwrap();
        assert!(!load(&paths).0.suggestions);
        fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn the_starter_file_documents_suggestions() {
        let config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert!(config.suggestions);
    }
}
