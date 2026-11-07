//! OS-specific pieces: listening to keys, typing text, and window focus.
//! Only Windows is implemented. macOS/Linux go in sibling modules exposing the same functions.

#[cfg(windows)]
mod password;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::password::{password_field_focused, start_password_watch};
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
compile_error!("Bloom currently supports Windows only.");
