//! OS-specific pieces: listening to keys, typing text, and window focus.
//! Only Windows is implemented. macOS/Linux go in sibling modules exposing the same functions.

#[cfg(windows)]
mod caret;
#[cfg(windows)]
mod password;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
#[allow(unused_imports)] // the dropdown window will use these in the next step
pub use self::caret::{CaretPosition, CaretSource, caret_position, start_caret_probe};
#[cfg(windows)]
pub use self::password::{password_field_focused, start_password_watch};
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
compile_error!("Bloom currently supports Windows only.");
