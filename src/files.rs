//! File writing shared by the settings and snippet files.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Writes a file through a temporary neighbour, so a crash, a full disk or an
/// interrupted save can't leave a half-written file behind. A truncated snippet
/// file matters more than it sounds: one parse error hides every snippet in it.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let tmp = sibling(path);
    let written = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        // Flush before the rename, or a crash can leave an empty file under the
        // real name.
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if written.is_ok() {
        return Ok(());
    }
    let _ = fs::remove_file(&tmp);
    // Renaming over a file that another program (an editor, an antivirus
    // scanner) holds open fails. Writing in place still saves the edit.
    fs::write(path, contents)
}

/// `dir/name.toml` -> `dir/name.toml.bloom-tmp`. The suffix keeps the temporary
/// file out of the snippets folder's `*.toml` glob.
fn sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bloom-tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_without_leaving_debris() {
        let dir = std::env::temp_dir().join(format!("bloom-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.toml");
        write_atomic(&path, "one").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        write_atomic(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!sibling(&path).exists(), "the temporary file is cleaned up");
        fs::remove_dir_all(&dir).ok();
    }
}
