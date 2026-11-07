//! Reading and writing text values under a key of the current user's registry.
//! Windows starts the programs listed under the `Run` key when the user signs
//! in, which is how Bloom starts with Windows.

use ::windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use ::windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use ::windows::core::{HSTRING, PCWSTR};

/// The key whose programs Windows starts at sign-in, for the current user only.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The text value `name` under `key`, or `None` if it does not exist.
pub fn read_value(key: &str, name: &str) -> Option<String> {
    let key = HSTRING::from(key);
    let name = HSTRING::from(name);
    // Ask how much room the value needs, then read it.
    let mut bytes = 0u32;
    // SAFETY: the strings outlive the calls, and the pointers are valid.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return None;
    }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
    let mut capacity = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` holds `capacity` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut capacity),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

/// Sets the text value `name` under `key`, creating the key if it is missing.
pub fn write_value(key: &str, name: &str, value: &str) -> Result<(), String> {
    let key = HSTRING::from(key);
    let name = HSTRING::from(name);
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `data` outlives the call and its size in bytes is passed with it.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "Windows refused to save the startup entry (error {:#x}).",
            status.0
        ))
    }
}

/// Removes the value `name` under `key`. A value that is not there is fine.
pub fn delete_value(key: &str, name: &str) -> Result<(), String> {
    let key = HSTRING::from(key);
    let name = HSTRING::from(name);
    // SAFETY: the strings outlive the call.
    let status = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
        )
    };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "Windows refused to remove the startup entry (error {:#x}).",
            status.0
        ))
    }
}

/// Removes a whole key, for tests that made their own.
#[cfg(test)]
pub fn delete_key(key: &str) {
    use ::windows::Win32::System::Registry::RegDeleteKeyW;
    let key = HSTRING::from(key);
    // SAFETY: the string outlives the call.
    let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
}
