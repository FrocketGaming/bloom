//! Starting Bloom with Windows.
//!
//! The setting `start_with_windows` has three states: on, off, and not decided
//! yet (a `bloom.toml` from before the setting existed, which the Bloom window
//! asks about once). Bloom keeps one entry in the current user's `Run` key
//! to match it, pointing at the running program, so moving or reinstalling Bloom
//! never leaves a stale path behind.

use crate::config::Config;
use crate::platform::{self, RUN_KEY};
use std::path::Path;

/// The name of Bloom's entry in the `Run` key.
const ENTRY_NAME: &str = "Bloom";

/// What has to change in the startup entry.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Nothing,
    Set(String),
    Remove,
}

/// The command Windows runs: the program's full path, quoted because paths have
/// spaces in them.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// What to do about the entry. Undecided leaves it alone; on makes sure it
/// points at `command`; off removes it.
pub fn plan(wanted: Option<bool>, current: Option<&str>, command: &str) -> Change {
    match wanted {
        None => Change::Nothing,
        Some(true) if current == Some(command) => Change::Nothing,
        Some(true) => Change::Set(command.to_string()),
        Some(false) if current.is_some() => Change::Remove,
        Some(false) => Change::Nothing,
    }
}

/// Brings the entry `name` under `key` in line with `wanted`.
fn apply_to(key: &str, name: &str, wanted: Option<bool>, command: &str) -> Result<(), String> {
    let current = platform::read_startup_value(key, name);
    match plan(wanted, current.as_deref(), command) {
        Change::Nothing => Ok(()),
        Change::Set(command) => platform::write_startup_value(key, name, &command),
        Change::Remove => platform::delete_startup_value(key, name),
    }
}

/// Brings Windows' startup entry in line with the setting.
pub fn apply(config: &Config) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Bloom could not find out where it is installed: {e}"))?;
    apply_to(
        RUN_KEY,
        ENTRY_NAME,
        config.start_with_windows,
        &command_for(&exe),
    )
}

/// Runs when Bloom starts. A development build (`cargo run`) leaves the entry
/// alone: pointing Windows at a build folder is never wanted.
pub fn sync_at_startup(config: &Config) {
    if cfg!(debug_assertions) {
        return;
    }
    if let Err(e) = apply(config) {
        eprintln!("bloom: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const COMMAND: &str = "\"C:\\Users\\Sam\\AppData\\Local\\Bloom\\bloom.exe\"";

    #[test]
    fn the_command_is_the_full_path_in_quotes() {
        let exe = PathBuf::from(r"C:\Program Files\Bloom\bloom.exe");
        assert_eq!(command_for(&exe), "\"C:\\Program Files\\Bloom\\bloom.exe\"");
    }

    #[test]
    fn undecided_never_touches_the_entry() {
        assert_eq!(plan(None, None, COMMAND), Change::Nothing);
        assert_eq!(
            plan(None, Some("\"D:\\old\\bloom.exe\""), COMMAND),
            Change::Nothing
        );
        assert_eq!(plan(None, Some(COMMAND), COMMAND), Change::Nothing);
    }

    #[test]
    fn on_adds_the_entry_and_keeps_it_pointing_at_this_program() {
        assert_eq!(plan(Some(true), None, COMMAND), Change::Set(COMMAND.into()));
        assert_eq!(plan(Some(true), Some(COMMAND), COMMAND), Change::Nothing);
        // Reinstalled somewhere else: the old path is replaced.
        assert_eq!(
            plan(Some(true), Some("\"D:\\old\\bloom.exe\""), COMMAND),
            Change::Set(COMMAND.into())
        );
    }

    #[test]
    fn off_removes_the_entry_if_there_is_one() {
        assert_eq!(plan(Some(false), Some(COMMAND), COMMAND), Change::Remove);
        assert_eq!(plan(Some(false), None, COMMAND), Change::Nothing);
    }

    /// A registry key of its own, so the test never touches the real `Run` key.
    fn test_key(tag: &str) -> String {
        format!("Software\\BloomUnitTest-{tag}-{}", std::process::id())
    }

    #[test]
    fn values_can_be_written_read_replaced_and_removed() {
        let key = test_key("values");
        assert_eq!(platform::read_startup_value(&key, "Bloom"), None);
        platform::write_startup_value(&key, "Bloom", "\"C:\\a b\\bloom.exe\"").unwrap();
        assert_eq!(
            platform::read_startup_value(&key, "Bloom").as_deref(),
            Some("\"C:\\a b\\bloom.exe\"")
        );
        platform::write_startup_value(&key, "Bloom", "\"D:\\bloom.exe\"").unwrap();
        assert_eq!(
            platform::read_startup_value(&key, "Bloom").as_deref(),
            Some("\"D:\\bloom.exe\"")
        );
        platform::delete_startup_value(&key, "Bloom").unwrap();
        assert_eq!(platform::read_startup_value(&key, "Bloom"), None);
        // Removing what is not there is not an error.
        platform::delete_startup_value(&key, "Bloom").unwrap();
        platform::delete_startup_key(&key);
    }

    #[test]
    fn long_paths_and_non_ascii_survive_the_round_trip() {
        let key = test_key("long");
        let command = format!(
            "\"C:\\Users\\Zoë\\{}\\bloom.exe\"",
            "very-long-folder-name/".repeat(20)
        );
        platform::write_startup_value(&key, "Bloom", &command).unwrap();
        assert_eq!(platform::read_startup_value(&key, "Bloom"), Some(command));
        platform::delete_startup_key(&key);
    }

    #[test]
    fn applying_a_setting_changes_the_entry_to_match() {
        let key = test_key("apply");
        let read = || platform::read_startup_value(&key, "Bloom");
        apply_to(&key, "Bloom", None, COMMAND).unwrap();
        assert_eq!(read(), None, "undecided adds nothing");
        apply_to(&key, "Bloom", Some(true), COMMAND).unwrap();
        assert_eq!(read().as_deref(), Some(COMMAND));
        apply_to(&key, "Bloom", None, "\"other\"").unwrap();
        assert_eq!(
            read().as_deref(),
            Some(COMMAND),
            "undecided leaves it alone"
        );
        apply_to(&key, "Bloom", Some(true), "\"D:\\moved\\bloom.exe\"").unwrap();
        assert_eq!(read().as_deref(), Some("\"D:\\moved\\bloom.exe\""));
        apply_to(&key, "Bloom", Some(false), COMMAND).unwrap();
        assert_eq!(read(), None);
        platform::delete_startup_key(&key);
    }
}
