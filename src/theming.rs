use std::collections::HashMap;
use std::path::PathBuf;

/// Colors the wheel draws with.
///
/// The base material is deliberately neutral — Apple's glass is charcoal, not a
/// tint of the wallpaper — and the *only* wallpaper-derived value is `accent`,
/// which lights the hover ring, the progress arc and the page dots. Everything
/// else is a near-grey with a fixed alpha so the wheel stays legible over any
/// backdrop (and over Hyprland's blur when the layer rule is installed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Glass gradient, top edge.
    pub glass_top: Rgba,
    /// Glass gradient, bottom edge.
    pub glass_bottom: Rgba,
    /// Sector wash at rest.
    pub wedge: Rgba,
    /// Specular border (1px).
    pub hairline: Rgba,
    /// Bright rim light along the top edge.
    pub highlight: Rgba,
    /// Primary label text.
    pub text: Rgba,
    /// Secondary / hint text.
    pub text_dim: Rgba,
    /// Center well wash.
    pub center: Rgba,
    /// The one accent (matugen `primary`).
    pub accent: Rgba,
    /// Per-layer drop shadow alpha.
    pub shadow: Rgba,
}

pub type Rgba = (f64, f64, f64, f64);

impl Default for Theme {
    fn default() -> Self {
        FALLBACK
    }
}

/// Apple blue — used when matugen has no palette to offer.
const APPLE_BLUE: Rgba = (0.039, 0.518, 1.0, 1.0);

const FALLBACK: Theme = Theme {
    // Light enters the top edge and the base sits denser — that vertical ramp
    // is what makes a flat fill read as a slab of glass rather than a plate.
    // Translucent enough that the compositor's blur is visible through it: an
    // opaque slab throws away the one thing a layer surface gives us.
    glass_top: (0.250, 0.260, 0.290, 0.68),
    glass_bottom: (0.078, 0.080, 0.095, 0.80),
    wedge: (1.0, 1.0, 1.0, 0.09),
    hairline: (1.0, 1.0, 1.0, 0.18),
    highlight: (1.0, 1.0, 1.0, 0.55),
    text: (0.965, 0.965, 0.973, 1.0),
    text_dim: (0.706, 0.706, 0.741, 1.0),
    // A dark inset: the well is glass *over* glass, so the labels never sit on
    // raw desktop content that could be any brightness.
    center: (0.0, 0.0, 0.0, 0.30),
    accent: APPLE_BLUE,
    shadow: (0.0, 0.0, 0.0, 0.014),
};

/// Push a theme colour into a range where it can carry a solid fill.
///
/// `matugen` picks `primary` for tinting text on a dark surface, so it comes out
/// pale — this machine's palette is `#99ccfa`, lightness ≈ 0.79. Filled into a
/// whole sector and read under white type, a pale accent collapses into "light
/// grey": the hover stops looking like a decision. Raising saturation and
/// clamping lightness keeps the hue the wallpaper produced while making the
/// fill actually commit.
pub fn vivid(color: Rgba) -> Rgba {
    let max = color.0.max(color.1).max(color.2);
    let min = color.0.min(color.1).min(color.2);
    let l = (max + min) / 2.0;
    let (h, s) = if (max - min).abs() < f64::EPSILON {
        (0.0, 0.0)
    } else {
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == color.0 {
            ((color.1 - color.2) / d + if color.1 < color.2 { 6.0 } else { 0.0 }) / 6.0
        } else if max == color.1 {
            ((color.2 - color.0) / d + 2.0) / 6.0
        } else {
            ((color.0 - color.1) / d + 4.0) / 6.0
        };
        (h, s)
    };
    // Filled sectors want mid lightness: dark enough for white type, light
    // enough to still read as the wallpaper's hue rather than mud.
    let l = l.clamp(0.44, 0.56);
    let s = s.max(0.55);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |t: f64| {
        let t = if t < 0.0 { t + 1.0 } else if t > 1.0 { t - 1.0 } else { t };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0), color.3)
}

/// Read matugen's ML4W palette for the accent; fall back to Apple blue.
pub fn load() -> Theme {
    let accent = read_matugen()
        .and_then(|m| m.get("primary").and_then(|v| parse_hex(v)))
        .map(|c| (c.0, c.1, c.2, 1.0))
        .or_else(|| read_pywal_accent())
        .or_else(|| crate::platform::accent_hex().and_then(|h| parse_hex(&h)))
        .unwrap_or(APPLE_BLUE);
    Theme { accent, ..FALLBACK }
}

/// Read Pywal's accent color from `~/.cache/wal/colors.json`.
/// Falls back to `None` if Pywal is not configured.
fn read_pywal_accent() -> Option<Rgba> {
    let wal_path = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".cache/wal/colors.json"));
    let Some(path) = wal_path else { return None };
    let text = std::fs::read_to_string(&path).ok()?;
    let colors: serde_json::Value = serde_json::from_str(&text).ok()?;
    let hex = colors.get("colors")?
        .get("color1")? // Pywal's primary accent is usually color1
        .as_str()?;
    parse_hex(hex).map(|c| (c.0, c.1, c.2, 1.0))
}

/// Load Pywal GTK CSS from `~/.cache/wal/colors-gtk.css`, falling back to
/// `~/.cache/wal/colors.css`. Returns the raw CSS string for application as
/// a GTK4 CssProvider.
pub fn load_pywal_css() -> Option<String> {
    for candidate in [
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/wal/colors-gtk.css")),
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/wal/colors.css")),
    ] {
        let Some(path) = candidate else { continue };
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn matugen_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("ml4w/colors/colors.json")
}

fn read_matugen() -> Option<HashMap<String, String>> {
    let text = std::fs::read_to_string(matugen_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// Parse "#rrggbb" or "#rrggbbaa" to normalized RGBA.
pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    let (r, g, b, a) = match s.len() {
        6 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
            u8::from_str_radix(&s[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, a as f64 / 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#99ccfa"), Some((0.6, 0.8, 0.9803921568627451, 1.0)));
        assert_eq!(parse_hex("FFB4AB"), Some((1.0, 0.7058823529411765, 0.6705882352941176, 1.0)));
        assert_eq!(parse_hex("#10141880").map(|c| (c.0, c.3)), Some((0.06274509803921569, 0.5019607843137255)));
        assert_eq!(parse_hex("nope"), None);
    }

    #[test]
    fn fallback_when_no_matugen_in_test_env() {
        // Only asserts the function returns *something* sane without a palette.
        let t = load();
        assert!(t.text.3 > 0.0);
        assert!(t.accent.3 > 0.0);
    }

    #[test]
    fn glass_is_translucent_and_layered() {
        // The body must let the blurred desktop through, and the bottom rim must
        // be denser than the top — that is what reads as thickness.
        let t = Theme::default();
        assert!(t.glass_top.3 > 0.4 && t.glass_top.3 < 0.9);
        assert!(t.glass_bottom.3 > t.glass_top.3);
        let lum = |c: Rgba| 0.2126 * c.0 + 0.7152 * c.1 + 0.0722 * c.2;
        assert!(
            lum(t.glass_top) - lum(t.glass_bottom) > 0.10,
            "the top-to-bottom ramp has to be visible, not a flat fill"
        );
        assert!(t.wedge.3 < t.glass_top.3, "sector wash must stay quieter than the body");
        assert!(t.hairline.3 < t.highlight.3);
        assert!(t.center.3 > 0.0 && t.center.0 < 0.1, "the well must be a dark inset");
    }
}
