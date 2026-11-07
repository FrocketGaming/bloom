//! Moving a file to the Recycle Bin, so deleting it can be undone.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use ::windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    SHFILEOPSTRUCTW, SHFileOperationW,
};
use ::windows::core::PCWSTR;

/// Sends `path` to the Recycle Bin. Windows asks the user before it would
/// delete the file for good, which it does when the Recycle Bin cannot be used
/// for it (a network folder, say), rather than doing so silently. A refusal, or
/// an answer of no, comes back as an error and the file is left where it is.
pub fn move_to_recycle_bin(path: &Path) -> Result<(), String> {
    // The call needs a full path, and its list of files ends in two nulls.
    if !path.is_absolute() {
        return Err("Bloom can only move a file to the Recycle Bin by its full path.".into());
    }
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let flags = FOF_ALLOWUNDO.0
        | FOF_NOCONFIRMATION.0
        | FOF_NOERRORUI.0
        | FOF_SILENT.0
        | FOF_WANTNUKEWARNING.0;
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: flags as u16,
        ..Default::default()
    };
    // SAFETY: `from` outlives the call, and `operation` is a valid structure.
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 {
        return Err(format!(
            "Windows could not move the file to the Recycle Bin (error {code:#x}), so it was left where it is."
        ));
    }
    if operation.fAnyOperationsAborted.as_bool() {
        return Err(
            "The file was not moved to the Recycle Bin, so it was left where it is.".into(),
        );
    }
    Ok(())
}
