//! Canonical venue theme identifiers and RGB palettes shared across B.O.S.S.,
//! `astro-shell`, `astro-daemon` OSDs, and frontend clients.

/// Default venue theme applied when none is specified or an unknown theme is given.
pub const DEFAULT_THEME: &str = "taproom";

/// Exhaustive list of supported venue theme identifiers.
pub const AVAILABLE_THEMES: &[&str] = &["taproom", "vector", "bigsky", "pinball", "classic"];

/// Returns `true` when `theme` matches one of [`AVAILABLE_THEMES`].
pub fn is_known_theme(theme: &str) -> bool {
    AVAILABLE_THEMES.contains(&theme)
}

/// 24-bit RGB color triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Decodes a packed `0xRRGGBB` color literal into an [`Rgb`].
    pub const fn from_hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Returns the raw `(r, g, b)` byte tuple.
    pub const fn as_tuple(self) -> (u8, u8, u8) {
        (self.0, self.1, self.2)
    }
}

/// Complete 7-color palette for a venue theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg: Rgb,
    pub panel: Rgb,
    pub accent: Rgb,
    pub accent2: Rgb,
    pub warn: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
}

/// Resolves the [`ThemePalette`] for `theme`, falling back to [`DEFAULT_THEME`] (`taproom`).
pub fn palette(theme: &str) -> ThemePalette {
    let (bg, panel, accent, accent2, warn, text, dim) = match theme {
        "vector" => (
            0x04060A, 0x0A0E14, 0x5CF2FF, 0xFF4FD8, 0xFFE45C, 0xDDF8FF, 0x6F8A96,
        ),
        "bigsky" => (
            0x0B1F3A, 0x12305A, 0x4FA3FF, 0xFFD23F, 0xFF7A59, 0xF4F8FF, 0x8FA6C4,
        ),
        "pinball" => (
            0x14060A, 0x2A0C12, 0xFFC94A, 0xFF3B5C, 0xFFE9A8, 0xFFF4DD, 0xB08A70,
        ),
        "classic" => (
            0x05070D, 0x0D1220, 0x00F0FF, 0xFF00C8, 0xFFE600, 0xE8F6FF, 0x6C7A92,
        ),
        _ => (
            0x1A1108, 0x2A1C0E, 0xF2A900, 0xD9531E, 0xFFE0A3, 0xFFF3DC, 0xA88B66,
        ),
    };
    ThemePalette {
        bg: Rgb::from_hex(bg),
        panel: Rgb::from_hex(panel),
        accent: Rgb::from_hex(accent),
        accent2: Rgb::from_hex(accent2),
        warn: Rgb::from_hex(warn),
        text: Rgb::from_hex(text),
        dim: Rgb::from_hex(dim),
    }
}

/// Returns the primary accent RGB color for `theme` (matches `astro-daemon` OSD colors).
pub fn accent_rgb(theme: &str) -> Rgb {
    palette(theme).accent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_themes_and_default() {
        assert_eq!(DEFAULT_THEME, "taproom");
        assert!(is_known_theme(DEFAULT_THEME));
        for theme in AVAILABLE_THEMES {
            assert!(is_known_theme(theme));
        }
        assert!(!is_known_theme("neon_unknown"));
        assert!(!is_known_theme(""));
    }

    #[test]
    fn test_palettes_and_accents_match_spec() {
        assert_eq!(accent_rgb("vector"), Rgb(0x5c, 0xf2, 0xff));
        assert_eq!(accent_rgb("bigsky"), Rgb(0x4f, 0xa3, 0xff));
        assert_eq!(accent_rgb("pinball"), Rgb(0xff, 0xc9, 0x4a));
        assert_eq!(accent_rgb("classic"), Rgb(0x00, 0xf0, 0xff));
        assert_eq!(accent_rgb("taproom"), Rgb(0xf2, 0xa9, 0x00));
        assert_eq!(palette("unknown"), palette("taproom"));

        let v = palette("vector");
        assert_eq!(v.bg, Rgb(0x04, 0x06, 0x0A));
        assert_eq!(v.panel, Rgb(0x0A, 0x0E, 0x14));
        assert_eq!(v.accent2, Rgb(0xFF, 0x4F, 0xD8));
        assert_eq!(v.warn, Rgb(0xFF, 0xE4, 0x5C));
        assert_eq!(v.text, Rgb(0xDD, 0xF8, 0xFF));
        assert_eq!(v.dim, Rgb(0x6F, 0x8A, 0x96));
    }
}
