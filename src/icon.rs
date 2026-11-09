//! Bloom's icon in the accent colour chosen in Settings.
//!
//! The icon is a flat flower in the pink accent on a transparent background,
//! so recolouring it means giving every visible pixel the accent colour and
//! keeping its transparency, which keeps the soft edges. The pink accent keeps
//! the icon exactly as drawn.
//!
//! Only what Bloom shows while it runs can change: the tray, its windows and
//! their taskbar buttons. The exe, the installer and any pinned shortcut take
//! the icon from the file, so they stay pink.

use crate::state::AppState;
use crate::theme::{self, Rgb};
use std::sync::Arc;
use tauri::image::Image;
use tauri::{AppHandle, Manager};

/// The tray's id, as `tray::build` creates it.
pub const TRAY_ID: &str = "bloom";

/// The icon in the current accent, or `None` if the app has no icon.
pub fn current(app: &AppHandle) -> Option<Image<'static>> {
    let icon = app.default_window_icon()?.clone().to_owned();
    let state = app.state::<Arc<AppState>>();
    let colour = {
        let config = state.config.read().unwrap();
        theme::icon_colour(&config.accent, &config.accent_color)
    };
    let Some(colour) = colour else {
        return Some(icon);
    };
    let mut rgba = icon.rgba().to_vec();
    tint(&mut rgba, colour);
    Some(Image::new_owned(rgba, icon.width(), icon.height()))
}

/// Gives the tray and every open window the icon in the current accent.
pub fn apply(app: &AppHandle) {
    let Some(icon) = current(app) else { return };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon.clone()));
    }
    for window in app.webview_windows().values() {
        let _ = window.set_icon(icon.clone());
    }
}

/// Paints every visible pixel of an RGBA image in `colour`, keeping how
/// transparent each one is.
fn tint(rgba: &mut [u8], colour: Rgb) {
    for pixel in rgba.chunks_exact_mut(4) {
        if pixel[3] != 0 {
            pixel[0] = colour.0;
            pixel[1] = colour.1;
            pixel[2] = colour.2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_pixels_take_the_colour_and_keep_their_transparency() {
        let mut rgba = vec![
            0xc2, 0x25, 0x5c, 255, // solid
            0xbf, 0x1f, 0x5f, 8, // a faint edge pixel
            0, 0, 0, 0, // the background
        ];
        tint(&mut rgba, Rgb(0x2f, 0x6f, 0xe4));
        assert_eq!(
            rgba,
            vec![0x2f, 0x6f, 0xe4, 255, 0x2f, 0x6f, 0xe4, 8, 0, 0, 0, 0]
        );
    }
}
