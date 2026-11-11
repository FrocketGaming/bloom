//! OS-specific pieces: listening to keys, typing text, and window focus.
//! Only Windows is implemented. macOS/Linux go in sibling modules exposing the same functions.

#[cfg(windows)]
mod caret;
#[cfg(windows)]
mod password;
#[cfg(windows)]
mod recycle;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::caret::{CaretPosition, caret_position_bounded, start_caret_probe};
#[cfg(windows)]
pub use self::password::{password_field_focused, start_password_watch};
#[cfg(windows)]
pub use self::recycle::move_to_recycle_bin;
#[cfg(all(windows, test))]
pub use self::startup::delete_key as delete_startup_key;
#[cfg(windows)]
pub use self::startup::{
    RUN_KEY, delete_value as delete_startup_value, read_value as read_startup_value,
    write_value as write_startup_value,
};
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
compile_error!("Bloom currently supports Windows only.");
