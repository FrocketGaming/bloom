//! Moving Bloom to another computer: settings, snippets, variables and the
//! usage record, all in one `.bloom` file.
//!
//! The file is TOML that holds each of Bloom's files as text, exactly as it is
//! on disk, so comments and formatting come across too. Importing replaces
//! what is there, after saving the current setup as a `.bloom` file of its own
//! in the `backups` folder, which can be imported to undo it.

use crate::config::{Config, Paths};
use crate::files;
use crate::snippets;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The extension of an export file.
pub const EXTENSION: &str = "bloom";

/// The version of the file format. A file with a higher number was made by a
/// newer Bloom that may store things this one would drop.
const FORMAT: u32 = 1;

const HEADER: &str = "# A Bloom export: settings, snippets, variables and the usage record.\n\
# Import it from Settings in Bloom on another computer.\n\n";

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    /// Marks the file as a Bloom export, and says which format it uses.
    bloom_export: u32,
    #[serde(default)]
    bloom_version: String,
    /// `bloom.toml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    settings: Option<String>,
    /// `variables.toml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variables: Option<String>,
    /// `usage.toml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<String>,
    /// Every `.toml` file in the snippets folder.
    #[serde(default, rename = "snippet_file")]
    snippet_files: Vec<SnippetFile>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SnippetFile {
    name: String,
    text: String,
}

impl Bundle {
    /// The settings the file carries, if it carries any. They were checked
    /// when the file was read.
    pub fn config(&self) -> Option<Config> {
        self.settings
            .as_deref()
            .and_then(|text| toml::from_str(text).ok())
    }

    pub fn snippet_file_count(&self) -> usize {
        self.snippet_files.len()
    }
}

/// A file's text, or `None` if it doesn't exist.
fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// The `.toml` files in the snippets folder, sorted.
fn snippet_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Gathers everything Bloom keeps.
pub fn collect(paths: &Paths) -> Result<Bundle> {
    let mut snippet_files = Vec::new();
    for path in snippet_paths(&paths.snippets_dir)? {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        snippet_files.push(SnippetFile { name, text });
    }
    Ok(Bundle {
        bloom_export: FORMAT,
        bloom_version: env!("CARGO_PKG_VERSION").into(),
        settings: read_optional(&paths.config_file)?,
        variables: read_optional(&paths.variables_file)?,
        usage: read_optional(&paths.usage_file)?,
        snippet_files,
    })
}

pub fn to_text(bundle: &Bundle) -> Result<String> {
    Ok(format!("{HEADER}{}", toml::to_string(bundle)?))
}

/// Reads an export file and checks it can be imported, before anything on
/// disk is touched.
pub fn parse(text: &str) -> Result<Bundle> {
    let bundle: Bundle = toml::from_str(text).map_err(|e| {
        if text.contains("bloom_export") {
            anyhow!("This export file is damaged: {e}")
        } else {
            anyhow!("This isn't a Bloom export file.")
        }
    })?;
    if bundle.bloom_export == 0 {
        bail!("This isn't a Bloom export file.");
    }
    if bundle.bloom_export > FORMAT {
        bail!(
            "This file was exported by a newer version of Bloom ({}). Update Bloom, then import it again.",
            bundle.bloom_version
        );
    }
    if let Some(settings) = &bundle.settings {
        toml::from_str::<Config>(settings)
            .map_err(|e| anyhow!("The settings in this file have an error: {e}"))?;
    }
    let mut seen = HashSet::new();
    for file in &bundle.snippet_files {
        // A name that changes when made safe could point outside the folder.
        let safe = snippets::safe_file_name(&file.name).ok();
        if safe.as_deref() != Some(file.name.as_str()) {
            bail!("The snippet file name “{}” isn't allowed.", file.name);
        }
        // Windows file names ignore case, so two of these would be one file.
        if !seen.insert(file.name.to_lowercase()) {
            bail!("The file lists “{}” twice.", file.name);
        }
    }
    Ok(bundle)
}

/// Saves the current setup into the `backups` folder. Returns the file's path.
pub fn backup(paths: &Paths) -> Result<PathBuf> {
    let dir = paths.root.join("backups");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M%S");
    let path = dir.join(format!("before import {stamp}.{EXTENSION}"));
    files::write_atomic(&path, &to_text(&collect(paths)?)?)?;
    Ok(path)
}

/// Writes `text` to `path`, or removes the file when there is no text.
fn replace(path: &Path, text: Option<&str>) -> Result<()> {
    match text {
        Some(text) => {
            files::write_atomic(path, text).with_context(|| format!("writing {}", path.display()))
        }
        None => match fs::remove_file(path) {
            Err(e) if e.kind() != ErrorKind::NotFound => {
                Err(e).with_context(|| format!("removing {}", path.display()))
            }
            _ => Ok(()),
        },
    }
}

/// Replaces what is on disk with the bundle. Settings are left alone when the
/// bundle has none; the snippets folder ends up holding exactly the bundle's
/// files.
pub fn write(paths: &Paths, bundle: &Bundle) -> Result<()> {
    if let Some(settings) = &bundle.settings {
        replace(&paths.config_file, Some(settings))?;
    }
    replace(&paths.variables_file, bundle.variables.as_deref())?;
    replace(&paths.usage_file, bundle.usage.as_deref())?;

    fs::create_dir_all(&paths.snippets_dir)
        .with_context(|| format!("creating {}", paths.snippets_dir.display()))?;
    let keep: HashSet<String> = bundle
        .snippet_files
        .iter()
        .map(|f| f.name.to_lowercase())
        .collect();
    for path in snippet_paths(&paths.snippets_dir)? {
        let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
        if !keep.contains(&name) {
            replace(&path, None)?;
        }
    }
    for file in &bundle.snippet_files {
        replace(&paths.snippets_dir.join(&file.name), Some(&file.text))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("bloom-transfer-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = Paths {
            config_file: root.join("bloom.toml"),
            snippets_dir: root.join("snippets"),
            variables_file: root.join("variables.toml"),
            usage_file: root.join("usage.toml"),
            root,
        };
        fs::create_dir_all(&paths.snippets_dir).unwrap();
        paths
    }

    #[test]
    fn a_round_trip_brings_every_file_back_exactly() {
        let from = temp_paths("from");
        fs::write(&from.config_file, "# mine\nhotkey = \"Ctrl+Alt+K\"\n").unwrap();
        fs::write(
            &from.variables_file,
            "[[variable]]\nname = \"me\"\ntext = \"Sam\"\n",
        )
        .unwrap();
        let multi_line =
            "# work\n[[snippet]]\ntrigger = \";sig\"\nbody = \"\"\"\nBest,\n¯\\_(ツ)_/¯\"\"\"\n";
        fs::write(from.snippets_dir.join("work.toml"), multi_line).unwrap();
        fs::write(from.snippets_dir.join("b.toml"), "").unwrap();

        let text = to_text(&collect(&from).unwrap()).unwrap();
        let bundle = parse(&text).unwrap();
        assert_eq!(bundle, collect(&from).unwrap());
        assert_eq!(bundle.usage, None, "a missing file stays missing");

        let to = temp_paths("to");
        fs::write(to.snippets_dir.join("old.toml"), "").unwrap();
        fs::write(&to.usage_file, "stale").unwrap();
        write(&to, &bundle).unwrap();

        assert_eq!(
            fs::read_to_string(&to.config_file).unwrap(),
            fs::read_to_string(&from.config_file).unwrap()
        );
        assert_eq!(
            fs::read_to_string(to.snippets_dir.join("work.toml")).unwrap(),
            multi_line
        );
        assert!(
            !to.snippets_dir.join("old.toml").exists(),
            "files not in the export are removed"
        );
        assert!(!to.usage_file.exists());
        assert_eq!(collect(&to).unwrap(), bundle);
        assert_eq!(bundle.config().unwrap().hotkey, "Ctrl+Alt+K");
        fs::remove_dir_all(&from.root).ok();
        fs::remove_dir_all(&to.root).ok();
    }

    #[test]
    fn other_files_are_refused() {
        for text in ["", "hotkey = \"Ctrl+K\"\n", "not toml at all {"] {
            let err = parse(text).unwrap_err().to_string();
            assert!(err.contains("isn't a Bloom export"), "{text}: {err}");
        }
        let err = parse("bloom_export = 99\nbloom_version = \"9.0.0\"\n").unwrap_err();
        assert!(err.to_string().contains("newer version"), "{err}");
    }

    #[test]
    fn broken_settings_are_refused_before_anything_is_written() {
        let err = parse("bloom_export = 1\nsettings = \"theme = 'sepia'\"\n").unwrap_err();
        assert!(err.to_string().contains("settings"), "{err}");
    }

    #[test]
    fn snippet_file_names_cannot_leave_the_folder() {
        for name in [
            "../bloom.toml",
            "..\\x.toml",
            "C:/x.toml",
            "CON.toml",
            "x",
            "a.toml.exe",
        ] {
            let text =
                format!("bloom_export = 1\n[[snippet_file]]\nname = {name:?}\ntext = \"\"\n");
            assert!(parse(&text).is_err(), "{name} was accepted");
        }
        let twice = "bloom_export = 1\n[[snippet_file]]\nname = \"a.toml\"\ntext = \"\"\n\
                     [[snippet_file]]\nname = \"A.toml\"\ntext = \"\"\n";
        assert!(parse(twice).is_err());
    }

    #[test]
    fn a_backup_can_be_imported_again() {
        let paths = temp_paths("backup");
        fs::write(&paths.config_file, "hotkey = \"Ctrl+Shift+Space\"\n").unwrap();
        fs::write(paths.snippets_dir.join("a.toml"), "").unwrap();
        let path = backup(&paths).unwrap();
        assert!(path.starts_with(paths.root.join("backups")));
        let bundle = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(bundle, collect(&paths).unwrap());
        fs::remove_dir_all(&paths.root).ok();
    }
}
