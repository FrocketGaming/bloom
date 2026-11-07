//! The look of Bloom's windows: light, dark or the system's, and an accent
//! colour.
//!
//! The accent colours buttons, toggles, highlights and links. A colour is used
//! as text as well as a fill, and on a light and a dark background, so a colour
//! that is fine on one is often unreadable on the other. Every accent is
//! therefore adjusted for each mode: its lightness is moved until text in that
//! colour keeps enough contrast against the lightest or darkest surface it sits
//! on, and the label on a filled button switches between light and dark text.
//! The default pink is left alone, so the look of an existing install does not
//! change until a choice is made.

use crate::config::ThemeMode;
use serde::Serialize;

/// The accent Bloom has always had. Choosing it overrides nothing.
pub const DEFAULT_ACCENT: &str = "pink";

/// Ready-made accents: id, label and the colour they are built from.
pub const PRESETS: [(&str, &str, &str); 5] = [
    ("blue", "Blue", "#2f6fe4"),
    ("green", "Green", "#2f9e44"),
    ("purple", "Purple", "#7048e8"),
    ("orange", "Orange", "#e8590c"),
    ("teal", "Teal", "#0c8599"),
];

/// The colour the pink swatch shows.
const PINK_SWATCH: &str = "#c2255c";

/// The darkest surface of the light theme and the lightest of the dark theme.
/// Text in the accent is checked against these, the hardest cases.
const LIGHT_SURFACE: Rgb = Rgb(0xf3, 0xf0, 0xf2);
const DARK_SURFACE: Rgb = Rgb(0x2b, 0x26, 0x2a);

/// The contrast text in the accent must keep. WCAG asks for 4.5 for ordinary
/// text; the margin covers the accent's own tint behind it.
const MIN_CONTRAST: f64 = 5.0;

/// Label colours on a filled accent: white, and the near-black the dark theme has
/// always used.
const INK_LIGHT: Rgb = Rgb(0xff, 0xff, 0xff);
const INK_DARK: Rgb = Rgb(0x1a, 0x0b, 0x11);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` or `#rgb`, with or without the `#`.
    pub fn parse(text: &str) -> Option<Rgb> {
        let digits = text.trim().trim_start_matches('#');
        if !digits.is_ascii() {
            return None;
        }
        let byte = |s: &str| u8::from_str_radix(s, 16).ok();
        match digits.len() {
            6 => Some(Rgb(
                byte(&digits[0..2])?,
                byte(&digits[2..4])?,
                byte(&digits[4..6])?,
            )),
            3 => {
                let double = |c: &str| byte(&format!("{c}{c}"));
                Some(Rgb(
                    double(&digits[0..1])?,
                    double(&digits[1..2])?,
                    double(&digits[2..3])?,
                ))
            }
            _ => None,
        }
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Relative luminance, as the WCAG contrast formula defines it.
    fn luminance(self) -> f64 {
        let channel = |value: u8| {
            let c = f64::from(value) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }

    fn to_hsl(self) -> (f64, f64, f64) {
        let (r, g, b) = (
            f64::from(self.0) / 255.0,
            f64::from(self.1) / 255.0,
            f64::from(self.2) / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lightness = (max + min) / 2.0;
        let delta = max - min;
        if delta == 0.0 {
            return (0.0, 0.0, lightness);
        }
        let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
        let hue = if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        } * 60.0;
        (hue, saturation, lightness)
    }

    fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> Rgb {
        let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let x = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
        let m = lightness - chroma / 2.0;
        let (r, g, b) = match (hue / 60.0) as u32 % 6 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let to_byte = |c: f64| ((c + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb(to_byte(r), to_byte(g), to_byte(b))
    }
}

/// The WCAG contrast ratio between two colours, from 1 (identical) to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Moves `color` lighter or darker, a step at a time, until it keeps
/// `MIN_CONTRAST` against `surface`. Hue and saturation are kept, so it stays
/// the colour that was picked, only lighter or darker.
fn made_readable(color: Rgb, surface: Rgb, lighter: bool) -> Rgb {
    let (hue, saturation, mut lightness) = color.to_hsl();
    let mut current = color;
    while contrast(current, surface) < MIN_CONTRAST {
        lightness += if lighter { 0.01 } else { -0.01 };
        if !(0.0..=1.0).contains(&lightness) {
            break;
        }
        current = Rgb::from_hsl(hue, saturation, lightness);
    }
    current
}

/// The label colour for text on a filled `accent`: whichever of white and the
/// dark ink reads better on it.
fn ink_for(accent: Rgb) -> Rgb {
    if contrast(INK_LIGHT, accent) >= contrast(INK_DARK, accent) {
        INK_LIGHT
    } else {
        INK_DARK
    }
}

/// The colours that depend on the accent, for one mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Palette {
    pub accent: String,
    /// Text on a filled accent.
    pub ink: String,
    /// A faint wash of the accent, behind highlighted text.
    pub soft: String,
    /// The highlight of the selected row.
    pub selected: String,
    /// The highlight under the pointer.
    pub hover: String,
}

fn wash(color: Rgb, alpha: f64) -> String {
    format!("rgba({}, {}, {}, {alpha})", color.0, color.1, color.2)
}

/// The palette of `base` for the light (`dark` false) or dark theme.
pub fn palette_for(base: Rgb, dark: bool) -> Palette {
    let accent = if dark {
        made_readable(base, DARK_SURFACE, true)
    } else {
        made_readable(base, LIGHT_SURFACE, false)
    };
    let (soft, selected, hover) = if dark {
        (0.15, 0.16, 0.08)
    } else {
        (0.12, 0.12, 0.07)
    };
    Palette {
        accent: accent.hex(),
        ink: ink_for(accent).hex(),
        soft: wash(accent, soft),
        selected: wash(accent, selected),
        hover: wash(accent, hover),
    }
}

/// What the windows need to draw themselves: the mode, and the accent's two
/// palettes (`None` for the default pink, which the stylesheets already have).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Theme {
    pub mode: ThemeMode,
    pub light: Option<Palette>,
    pub dark: Option<Palette>,
}

/// One of the ready-made accents, for the Settings page.
#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub swatch: &'static str,
}

/// Every ready-made accent, the default first.
pub fn presets() -> Vec<Preset> {
    let mut list = vec![Preset {
        id: DEFAULT_ACCENT,
        label: "Pink",
        swatch: PINK_SWATCH,
    }];
    list.extend(
        PRESETS
            .iter()
            .map(|&(id, label, swatch)| Preset { id, label, swatch }),
    );
    list
}

/// The colour an accent choice is built from, or `None` for the default.
fn base_colour(accent: &str, custom: &str) -> Result<Option<Rgb>, String> {
    if accent == DEFAULT_ACCENT {
        return Ok(None);
    }
    if accent == "custom" {
        return match Rgb::parse(custom) {
            Some(rgb) => Ok(Some(rgb)),
            None => Err(format!(
                "“{custom}” isn't a colour. Use a hex colour such as #3b82f6."
            )),
        };
    }
    match PRESETS.iter().find(|(id, _, _)| *id == accent) {
        Some((_, _, hex)) => Ok(Rgb::parse(hex)),
        None => Err(format!("“{accent}” isn't one of the accent colours.")),
    }
}

/// Checks the accent settings before they are saved.
pub fn validate(accent: &str, custom: &str) -> Result<(), String> {
    base_colour(accent, custom).map(|_| ())
}

/// Works out the theme for the settings. A choice that can't be understood, from
/// a hand-edited file, falls back to the default rather than breaking the look.
pub fn resolve(mode: ThemeMode, accent: &str, custom: &str) -> Theme {
    match base_colour(accent, custom) {
        Ok(Some(base)) => Theme {
            mode,
            light: Some(palette_for(base, false)),
            dark: Some(palette_for(base, true)),
        },
        _ => Theme {
            mode,
            light: None,
            dark: None,
        },
    }
}

/// How a colour typed or picked in Settings will really be used, so the page
/// can show the adjusted result.
pub fn preview(custom: &str) -> Result<(Palette, Palette), String> {
    let base = base_colour("custom", custom)?.expect("a custom accent has a colour");
    Ok((palette_for(base, false), palette_for(base, true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_in_the_forms_people_type() {
        assert_eq!(Rgb::parse("#c2255c"), Some(Rgb(0xc2, 0x25, 0x5c)));
        assert_eq!(Rgb::parse("C2255C"), Some(Rgb(0xc2, 0x25, 0x5c)));
        assert_eq!(Rgb::parse("  #fff "), Some(Rgb(255, 255, 255)));
        assert_eq!(Rgb::parse("#07f"), Some(Rgb(0, 0x77, 0xff)));
        for bad in [
            "",
            "#",
            "#12",
            "#12345",
            "#1234567",
            "blue",
            "#gggggg",
            "#12é456",
            "rgb(1,2,3)",
        ] {
            assert_eq!(Rgb::parse(bad), None, "{bad:?}");
        }
        assert_eq!(Rgb(0xc2, 0x25, 0x5c).hex(), "#c2255c");
    }

    #[test]
    fn contrast_matches_the_wcag_definition() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        assert!((contrast(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast(white, white) - 1.0).abs() < 1e-9);
        // Symmetric, and a known mid value: #767676 on white is 4.54.
        assert!((contrast(Rgb(0x76, 0x76, 0x76), white) - 4.54).abs() < 0.01);
        assert_eq!(contrast(black, white), contrast(white, black));
    }

    #[test]
    fn hsl_round_trips() {
        for color in [
            Rgb(194, 37, 92),
            Rgb(47, 111, 228),
            Rgb(0, 0, 0),
            Rgb(255, 255, 255),
            Rgb(128, 128, 128),
            Rgb(255, 0, 0),
        ] {
            let (h, s, l) = color.to_hsl();
            let back = Rgb::from_hsl(h, s, l);
            for (a, b) in [(color.0, back.0), (color.1, back.1), (color.2, back.2)] {
                assert!(a.abs_diff(b) <= 1, "{color:?} became {back:?}");
            }
        }
    }

    #[test]
    fn a_colour_that_is_already_readable_is_left_as_picked() {
        assert_eq!(
            palette_for(Rgb::parse("#a61e4d").unwrap(), false).accent,
            "#a61e4d"
        );
        assert_eq!(
            palette_for(Rgb::parse("#ff8fb3").unwrap(), true).accent,
            "#ff8fb3"
        );
    }

    #[test]
    fn the_original_pink_is_barely_changed_if_someone_picks_it_as_a_custom_colour() {
        // Its light-theme colour is a hair under the margin, so it is nudged
        // darker. The default pink itself is never overridden.
        for (original, dark, surface) in [
            (Rgb(0xc2, 0x25, 0x5c), false, LIGHT_SURFACE),
            (Rgb(0xf0, 0x65, 0x95), true, DARK_SURFACE),
        ] {
            let adjusted = Rgb::parse(&palette_for(original, dark).accent).unwrap();
            for (a, b) in [
                (adjusted.0, original.0),
                (adjusted.1, original.1),
                (adjusted.2, original.2),
            ] {
                assert!(a.abs_diff(b) <= 8, "{adjusted:?} vs {original:?}");
            }
            assert!(contrast(adjusted, surface) >= MIN_CONTRAST);
        }
    }

    #[test]
    fn a_pale_colour_is_darkened_on_light_and_a_dark_one_lightened_on_dark() {
        let pale_yellow = Rgb::parse("#ffee99").unwrap();
        let light = Rgb::parse(&palette_for(pale_yellow, false).accent).unwrap();
        assert!(contrast(light, LIGHT_SURFACE) >= MIN_CONTRAST, "{light:?}");
        assert!(light.luminance() < pale_yellow.luminance(), "darkened");

        let navy = Rgb::parse("#000044").unwrap();
        let dark = Rgb::parse(&palette_for(navy, true).accent).unwrap();
        assert!(contrast(dark, DARK_SURFACE) >= MIN_CONTRAST, "{dark:?}");
        assert!(dark.luminance() > navy.luminance(), "lightened");
    }

    #[test]
    fn the_hue_survives_the_adjustment() {
        let base = Rgb::parse("#e8590c").unwrap();
        let (hue, _, _) = base.to_hsl();
        for dark in [false, true] {
            let adjusted = Rgb::parse(&palette_for(base, dark).accent).unwrap();
            let (adjusted_hue, _, _) = adjusted.to_hsl();
            assert!(
                (hue - adjusted_hue).abs() < 4.0,
                "{dark}: {hue} vs {adjusted_hue}"
            );
        }
    }

    #[test]
    fn every_preset_and_a_wide_spread_of_colours_stay_readable_in_both_modes() {
        let mut colours: Vec<Rgb> = PRESETS.iter().map(|p| Rgb::parse(p.2).unwrap()).collect();
        // A sweep through hue and lightness, plus the extremes.
        for hue in (0..360).step_by(15) {
            for lightness in [0.05, 0.2, 0.35, 0.5, 0.65, 0.8, 0.95] {
                colours.push(Rgb::from_hsl(f64::from(hue), 0.9, lightness));
            }
        }
        colours.extend([
            Rgb(0, 0, 0),
            Rgb(255, 255, 255),
            Rgb(128, 128, 128),
            Rgb(255, 255, 0),
        ]);
        for base in colours {
            for dark in [false, true] {
                let p = palette_for(base, dark);
                let accent = Rgb::parse(&p.accent).unwrap();
                let surface = if dark { DARK_SURFACE } else { LIGHT_SURFACE };
                // Pure grey and extreme colours can run out of room at white or
                // black, but even then the contrast must clear the WCAG minimum.
                assert!(
                    contrast(accent, surface) >= 4.5,
                    "{base:?} dark={dark}: {}",
                    p.accent
                );
                let ink = Rgb::parse(&p.ink).unwrap();
                assert!(
                    contrast(ink, accent) >= 4.5,
                    "{base:?} dark={dark}: ink {} on {}",
                    p.ink,
                    p.accent
                );
            }
        }
    }

    #[test]
    fn the_label_on_a_filled_button_is_white_on_dark_fills_and_dark_on_light_ones() {
        assert_eq!(ink_for(Rgb::parse("#c2255c").unwrap()), INK_LIGHT);
        assert_eq!(ink_for(Rgb::parse("#f06595").unwrap()), INK_DARK);
        assert_eq!(ink_for(Rgb(0, 0, 0)), INK_LIGHT);
        assert_eq!(ink_for(Rgb(255, 255, 255)), INK_DARK);
    }

    #[test]
    fn the_default_pink_overrides_nothing() {
        let theme = resolve(ThemeMode::System, "pink", "#123456");
        assert_eq!((theme.light, theme.dark), (None, None));
        assert_eq!(theme.mode, ThemeMode::System);
    }

    #[test]
    fn a_preset_or_custom_accent_gives_a_palette_for_each_mode() {
        let blue = resolve(ThemeMode::Dark, "blue", "");
        assert_eq!(blue.mode, ThemeMode::Dark);
        assert!(blue.light.is_some() && blue.dark.is_some());
        assert_ne!(blue.light, blue.dark, "each mode has its own colours");

        let custom = resolve(ThemeMode::Light, "custom", "#12ab34");
        assert!(custom.light.is_some() && custom.dark.is_some());
    }

    #[test]
    fn washes_use_the_adjusted_colour_with_fixed_strengths() {
        let p = palette_for(Rgb::parse("#a61e4d").unwrap(), false);
        assert_eq!(p.soft, "rgba(166, 30, 77, 0.12)");
        assert_eq!(p.selected, "rgba(166, 30, 77, 0.12)");
        assert_eq!(p.hover, "rgba(166, 30, 77, 0.07)");
        let p = palette_for(Rgb::parse("#ff8fb3").unwrap(), true);
        assert_eq!(p.soft, "rgba(255, 143, 179, 0.15)");
        assert_eq!(p.selected, "rgba(255, 143, 179, 0.16)");
        assert_eq!(p.hover, "rgba(255, 143, 179, 0.08)");
    }

    #[test]
    fn bad_choices_are_refused_when_saving_and_fall_back_when_loading() {
        assert!(validate("pink", "").is_ok());
        assert!(validate("blue", "").is_ok());
        assert!(validate("custom", "#3b82f6").is_ok());
        assert!(validate("custom", "blue").is_err());
        assert!(validate("custom", "").is_err());
        assert!(validate("magenta", "#3b82f6").is_err());
        // A hand-edited file with nonsense still gives a usable look.
        let theme = resolve(ThemeMode::System, "magenta", "nonsense");
        assert_eq!((theme.light, theme.dark), (None, None));
        let theme = resolve(ThemeMode::System, "custom", "nonsense");
        assert_eq!((theme.light, theme.dark), (None, None));
    }

    #[test]
    fn the_preview_shows_what_a_custom_colour_will_really_look_like() {
        let (light, dark) = preview("#ffee99").unwrap();
        assert_ne!(
            light.accent, "#ffee99",
            "a pale colour is darkened for light mode"
        );
        assert_ne!(light.accent, dark.accent);
        assert!(preview("not a colour").is_err());
    }

    #[test]
    fn the_presets_list_starts_with_the_default() {
        let list = presets();
        assert_eq!(list[0].id, "pink");
        assert_eq!(list.len(), 6);
        assert!(list.iter().all(|p| Rgb::parse(p.swatch).is_some()));
        let ids: Vec<_> = list.iter().map(|p| p.id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "ids are unique");
    }
}
