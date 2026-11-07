//! User-defined variables, kept in `variables.toml`.
//!
//! A variable is either a piece of text that snippets insert as written, or a
//! list that snippets offer as a pick-one choice. Snippets refer to both as
//! `{{var:name}}`.
//!
//! The file lives beside `bloom.toml`, not in the snippets folder, because that
//! folder treats every `.toml` file in it as snippets.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

/// What a new `variables.toml` starts with.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once snippets and the editor read and write variables"
    )
)]
const STARTER: &str = "# Your variables. Use one in a snippet as {{var:name}}.\n\
#   name = \"text\"          is inserted exactly as written\n\
#   name = [\"A\", \"B\"]      is offered as a pick-one choice\n\
\n\
[variables]\n";

/// Longest variable name accepted.
const MAX_NAME: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Content {
    /// Inserted exactly as written. Placeholders inside it are not expanded.
    Text(String),
    /// Offered as a pick-one choice when the snippet is used.
    Choices(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    #[serde(flatten)]
    pub content: Content,
}

#[derive(Debug, Default)]
pub struct Variables {
    pub list: Vec<Variable>,
    /// Problems found in the file, for the warnings banner.
    pub errors: Vec<String>,
}

/// Names use letters, digits, `-` and `_`, so they read cleanly inside
/// `{{var:name}}` and stay valid as bare TOML keys.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

impl Variables {
    /// Reads `variables.toml`. A missing file means no variables. A broken one
    /// means none either, plus an error saying why, and it is never rewritten.
    pub fn load(path: &Path) -> Variables {
        let mut variables = Variables::default();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return variables,
            Err(e) => {
                variables
                    .errors
                    .push(format!("couldn't read variables.toml: {e}"));
                return variables;
            }
        };
        let doc: DocumentMut = match text.parse() {
            Ok(doc) => doc,
            Err(e) => {
                variables.errors.push(format!("variables.toml: {e}"));
                return variables;
            }
        };
        match doc.get("variables") {
            None => {}
            Some(item) => match item.as_table_like() {
                Some(table) => {
                    let (list, errors) = read_table(table);
                    variables.list = list;
                    variables.errors = errors;
                }
                None => variables
                    .errors
                    .push("variables.toml: `variables` must be a table, [variables]".into()),
            },
        }
        variables
    }

    /// Finds a variable by name. Names ignore case, like the built-in placeholders.
    pub fn get(&self, name: &str) -> Option<&Variable> {
        let wanted = name.trim().to_lowercase();
        self.list.iter().find(|v| v.name.to_lowercase() == wanted)
    }
}

/// Reads every entry of a `[variables]` table, keeping the good ones and
/// describing each bad one.
fn read_table(table: &dyn TableLike) -> (Vec<Variable>, Vec<String>) {
    let mut list = Vec::new();
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    for (name, item) in table.iter() {
        if !valid_name(name) {
            errors.push(format!(
                "Variable “{name}” has a name Bloom can't use. Names are letters, digits, - and _, up to {MAX_NAME} characters."
            ));
            continue;
        }
        if !seen.insert(name.to_lowercase()) {
            errors.push(format!(
                "“{name}” is defined twice (names ignore case); only the first is used."
            ));
            continue;
        }
        match read_content(name, item) {
            Ok(content) => list.push(Variable {
                name: name.to_string(),
                content,
            }),
            Err(message) => errors.push(message),
        }
    }
    (list, errors)
}

fn read_content(name: &str, item: &Item) -> std::result::Result<Content, String> {
    let value = item
        .as_value()
        .ok_or_else(|| format!("Variable “{name}” must be text or a list of text, not a table."))?;
    if let Some(text) = value.as_str() {
        return Ok(Content::Text(text.replace("\r\n", "\n")));
    }
    if let Some(array) = value.as_array() {
        let mut options = Vec::new();
        for entry in array.iter() {
            match entry.as_str() {
                Some(option) if !option.trim().is_empty() => options.push(option.to_string()),
                Some(_) => {
                    return Err(format!(
                        "Variable “{name}” has an empty option in its list."
                    ));
                }
                None => {
                    return Err(format!(
                        "Variable “{name}” is a list, so every option must be text."
                    ));
                }
            }
        }
        if options.is_empty() {
            return Err(format!(
                "Variable “{name}” is an empty list; give it at least one option."
            ));
        }
        return Ok(Content::Choices(options));
    }
    Err(format!("Variable “{name}” must be text or a list of text."))
}

/// Checks a list of variables before anything is written.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once snippets and the editor read and write variables"
    )
)]
fn check(variables: &[Variable]) -> Result<()> {
    let mut seen = HashSet::new();
    for variable in variables {
        if !valid_name(&variable.name) {
            bail!(
                "“{}” isn't a usable name. Use letters, digits, - and _ (up to {MAX_NAME} characters).",
                variable.name
            );
        }
        if !seen.insert(variable.name.to_lowercase()) {
            bail!(
                "“{}” is used for more than one variable. Names ignore case.",
                variable.name
            );
        }
        if let Content::Choices(options) = &variable.content {
            if options.is_empty() || options.iter().any(|o| o.trim().is_empty()) {
                bail!(
                    "“{}” needs at least one option, and none of them can be empty.",
                    variable.name
                );
            }
        }
    }
    Ok(())
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once snippets and the editor read and write variables"
    )
)]
fn to_value(content: &Content) -> Result<Value> {
    Ok(match content {
        Content::Text(text) => {
            let text = text.replace("\r\n", "\n");
            if text.contains('\n') {
                // Multi-line text is written as a `"""` block so the file stays readable.
                crate::snippets::body_value(&text)?
            } else {
                Value::from(text)
            }
        }
        Content::Choices(options) => {
            let mut array = Array::new();
            for option in options {
                array.push(option.as_str());
            }
            Value::Array(array)
        }
    })
}

/// Writes the variables to `variables.toml`, keeping its comments and anything
/// else in the file. `variables` is the full list: a variable the file has but
/// the list lacks is removed. Entries that Bloom could not read (a hand-written
/// mistake, say) are left alone, so saving can never delete work that the
/// editor was not able to show.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once snippets and the editor read and write variables"
    )
)]
pub fn save(path: &Path, variables: &[Variable]) -> Result<()> {
    check(variables)?;
    let mut doc: DocumentMut = match fs::read_to_string(path) {
        Ok(text) => text.parse().map_err(|e| {
            anyhow!(
                "variables.toml has a syntax error, so Bloom won't overwrite it. Fix the file, then save again: {e}"
            )
        })?,
        Err(e) if e.kind() == ErrorKind::NotFound => STARTER
            .parse()
            .map_err(|e| anyhow!("internal error in the starter file: {e}"))?,
        Err(e) => bail!("couldn't read variables.toml: {e}"),
    };
    if !doc.contains_key("variables") {
        doc["variables"] = Item::Table(Table::new());
    }
    let table = doc["variables"]
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("`variables` in variables.toml isn't a table, [variables]"))?;

    let wanted: HashSet<String> = variables.iter().map(|v| v.name.to_lowercase()).collect();
    let (existing, _) = read_table(table);
    for old in existing {
        if !wanted.contains(&old.name.to_lowercase()) {
            table.remove(&old.name);
        }
    }

    for variable in variables {
        let value = to_value(&variable.content)?;
        // The key as the file spells it. A different capitalisation is a rename.
        let spelled = table
            .iter()
            .map(|(key, _)| key.to_string())
            .find(|key| key.to_lowercase() == variable.name.to_lowercase());
        match spelled {
            Some(key) if key == variable.name => {
                if let Some(item) = table.get_mut(&key) {
                    let mut value = value;
                    // Keep a trailing comment on the line.
                    if let Some(old) = item.as_value() {
                        *value.decor_mut() = old.decor().clone();
                    }
                    *item = Item::Value(value);
                }
            }
            Some(old_key) => {
                table.remove(&old_key);
                table.insert(&variable.name, Item::Value(value));
            }
            None => {
                table.insert(&variable.name, Item::Value(value));
            }
        }
    }
    crate::files::write_atomic(path, &doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bloom-vars-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("variables.toml")
    }

    fn cleanup(path: &Path) {
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    fn text(name: &str, value: &str) -> Variable {
        Variable {
            name: name.into(),
            content: Content::Text(value.into()),
        }
    }

    fn choices(name: &str, options: &[&str]) -> Variable {
        Variable {
            name: name.into(),
            content: Content::Choices(options.iter().map(|o| o.to_string()).collect()),
        }
    }

    #[test]
    fn a_missing_file_means_no_variables_and_no_errors() {
        let path = temp_file("missing");
        let variables = Variables::load(&path);
        assert!(variables.list.is_empty());
        assert!(variables.errors.is_empty());
        cleanup(&path);
    }

    #[test]
    fn reads_text_lists_and_multiline_text_in_file_order() {
        let path = temp_file("read");
        fs::write(
            &path,
            "[variables]\nemail = \"sam@example.com\"\nsignoff = [\"Best\", \"Cheers\"]\naddress = \"\"\"\n1 Main St\nTown\"\"\"\n",
        )
        .unwrap();
        let variables = Variables::load(&path);
        assert!(variables.errors.is_empty(), "{:?}", variables.errors);
        assert_eq!(
            variables.list,
            vec![
                text("email", "sam@example.com"),
                choices("signoff", &["Best", "Cheers"]),
                text("address", "1 Main St\nTown"),
            ]
        );
        cleanup(&path);
    }

    #[test]
    fn lookup_ignores_case() {
        let path = temp_file("case");
        fs::write(&path, "[variables]\nEmail = \"a@b.c\"\n").unwrap();
        let variables = Variables::load(&path);
        assert_eq!(variables.get("email"), Some(&text("Email", "a@b.c")));
        assert_eq!(variables.get(" EMAIL "), Some(&text("Email", "a@b.c")));
        assert!(variables.get("other").is_none());
        cleanup(&path);
    }

    #[test]
    fn bad_entries_are_reported_and_the_good_ones_still_load() {
        let path = temp_file("bad");
        fs::write(
            &path,
            "[variables]\ngood = \"ok\"\nnumber = 5\nmixed = [\"a\", 2]\nempty = []\nblank = [\"a\", \" \"]\n\"bad name\" = \"x\"\nGood = \"again\"\n",
        )
        .unwrap();
        let variables = Variables::load(&path);
        assert_eq!(variables.list, vec![text("good", "ok")]);
        assert_eq!(variables.errors.len(), 6, "{:?}", variables.errors);
        assert!(variables.errors.iter().any(|e| e.contains("“number”")));
        assert!(variables.errors.iter().any(|e| e.contains("“bad name”")));
        assert!(variables.errors.iter().any(|e| e.contains("defined twice")));
        cleanup(&path);
    }

    #[test]
    fn a_file_with_a_syntax_error_loads_nothing_and_says_so() {
        let path = temp_file("syntax");
        fs::write(&path, "[variables]\nthis is not toml\n").unwrap();
        let variables = Variables::load(&path);
        assert!(variables.list.is_empty());
        assert_eq!(variables.errors.len(), 1);
        cleanup(&path);
    }

    #[test]
    fn names_are_validated() {
        assert!(valid_name("email"));
        assert!(valid_name("sign-off_2"));
        assert!(valid_name("Café"));
        assert!(!valid_name(""));
        assert!(!valid_name("two words"));
        assert!(!valid_name("a:b"));
        assert!(!valid_name("a}"));
        assert!(!valid_name(&"x".repeat(65)));
    }

    #[test]
    fn saving_creates_a_documented_file() {
        let path = temp_file("create");
        save(&path, &[text("email", "sam@example.com")]).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("# Your variables."));
        assert_eq!(
            Variables::load(&path).list,
            vec![text("email", "sam@example.com")]
        );
        cleanup(&path);
    }

    #[test]
    fn saving_round_trips_every_kind_of_value() {
        let path = temp_file("roundtrip");
        let list = vec![
            text("email", "sam@example.com"),
            text("address", "1 Main St\nTown \"North\"\nback\\slash"),
            choices("signoff", &["Best", "Kind regards"]),
            text("braces", "{{date}} stays literal"),
        ];
        save(&path, &list).unwrap();
        let loaded = Variables::load(&path);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        assert_eq!(loaded.list, list);
        cleanup(&path);
    }

    #[test]
    fn saving_keeps_comments_and_other_content_and_updates_in_place() {
        let path = temp_file("keep");
        fs::write(
            &path,
            "# my notes\n[variables]\n# the address\nemail = \"old@example.com\" # work\nkeep = \"me\"\n\n[other]\nx = 1\n",
        )
        .unwrap();
        save(
            &path,
            &[text("email", "new@example.com"), text("keep", "me")],
        )
        .unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("# my notes"));
        assert!(written.contains("# the address"));
        assert!(
            written.contains("# work"),
            "trailing comment kept: {written}"
        );
        assert!(written.contains("[other]\nx = 1"));
        assert!(written.contains("new@example.com"));
        assert!(!written.contains("old@example.com"));
        cleanup(&path);
    }

    #[test]
    fn saving_removes_dropped_variables_and_appends_new_ones() {
        let path = temp_file("remove");
        fs::write(&path, "[variables]\na = \"1\"\nb = \"2\"\n").unwrap();
        save(&path, &[text("b", "2"), text("c", "3")]).unwrap();
        let names: Vec<String> = Variables::load(&path)
            .list
            .into_iter()
            .map(|v| v.name)
            .collect();
        assert_eq!(names, vec!["b", "c"]);
        cleanup(&path);
    }

    #[test]
    fn saving_never_deletes_an_entry_the_editor_could_not_show() {
        let path = temp_file("invalid");
        fs::write(&path, "[variables]\nbroken = 5\nfine = \"x\"\n").unwrap();
        save(&path, &[text("fine", "y")]).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("broken = 5"), "{written}");
        assert!(written.contains("fine = \"y\""));
        cleanup(&path);
    }

    #[test]
    fn renaming_only_the_capitalisation_replaces_the_key() {
        let path = temp_file("rename");
        fs::write(&path, "[variables]\nemail = \"a\"\n").unwrap();
        save(&path, &[text("Email", "a")]).unwrap();
        let loaded = Variables::load(&path);
        assert_eq!(loaded.list, vec![text("Email", "a")]);
        cleanup(&path);
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        let path = temp_file("noclobber");
        let broken = "[variables]\nthis is not toml\n";
        fs::write(&path, broken).unwrap();
        assert!(save(&path, &[text("a", "b")]).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        cleanup(&path);
    }

    #[test]
    fn bad_input_is_refused_before_anything_is_written() {
        let path = temp_file("refuse");
        assert!(save(&path, &[text("two words", "x")]).is_err());
        assert!(save(&path, &[text("a", "1"), text("A", "2")]).is_err());
        assert!(save(&path, &[choices("a", &[])]).is_err());
        assert!(save(&path, &[choices("a", &["ok", " "])]).is_err());
        assert!(!path.exists());
        cleanup(&path);
    }
}
