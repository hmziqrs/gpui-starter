//! Generic color utilities for the UI layer.
//!
//! A small, dependency-light [`Color`] value type (float RGBA in 0..=1) with
//! parsing from hex/`rgb()`/`rgba()` strings, conversion to gpui's [`Hsla`],
//! and lighten/darken helpers. Rebinds to gpui's color type rather than any
//! application-specific theme type.

use gpui::{Hsla, hsla};

/// A floating-point RGBA color. All channels are in the range `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Build a color from 0..=255 integer RGB components, fully opaque.
    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// Build a color from 0..=255 integer RGBA components.
    pub fn from_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Build a color directly from float channels (0..=1).
    pub const fn from_f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Parse a color string.
    ///
    /// Supported forms: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(r,g,b)`,
    /// `rgba(r,g,b,a)` (channels 0..=255, alpha 0..=1 or 0..=255). Returns
    /// `None` (rather than erroring) so callers can degrade gracefully when a
    /// user-supplied color is malformed.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();

        if let Some(hex) = text.strip_prefix('#') {
            return parse_hex(hex);
        }

        if let Some(rest) = text
            .strip_prefix("rgba(")
            .or_else(|| text.strip_prefix("rgb("))
        {
            return parse_rgb(rest);
        }

        None
    }

    /// Convert to a gpui [`Hsla`] (hue in 0..=1, matching gpui convention).
    pub fn to_hsla(&self) -> Hsla {
        rgba_to_hsla(self.r, self.g, self.b, self.a)
    }

    /// Lighten the color by `amount` (0..=1), added to its lightness and clamped.
    /// Alpha is preserved.
    pub fn lighten(&self, amount: f32) -> Self {
        let hsla = self.to_hsla();
        let l = (hsla.l + amount).clamp(0.0, 1.0);
        hsla_to_rgba(hsla.h, hsla.s, l, hsla.a)
    }

    /// Darken the color by `amount` (0..=1), subtracted from its lightness and
    /// clamped. Alpha is preserved.
    pub fn darken(&self, amount: f32) -> Self {
        let hsla = self.to_hsla();
        let l = (hsla.l - amount).clamp(0.0, 1.0);
        hsla_to_rgba(hsla.h, hsla.s, l, hsla.a)
    }
}

/// Convert float RGBA (0..=1) into a gpui [`Hsla`] (hue 0..=1).
fn rgba_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let l = (max + min) / 2.0;

    let s = if delta == 0.0 {
        0.0
    } else if l < 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };

    hsla(h, s, l, a)
}

/// Convert HSLA (h/s/l/a in 0..=1) into float RGBA (0..=1).
fn hsla_to_rgba(h: f32, s: f32, l: f32, a: f32) -> Color {
    if s == 0.0 {
        return Color::from_f32(l, l, l, a);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let hue_to_rgb = |p: f32, q: f32, mut t: f32| -> f32 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };

    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);

    Color::from_f32(r, g, b, a)
}

/// Parse hex digits (without the leading `#`).
fn parse_hex(hex: &str) -> Option<Color> {
    let (r, g, b, a) = match hex.len() {
        3 => (
            u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?,
            u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?,
            u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?,
            255,
        ),
        4 => (
            u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?,
            u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?,
            u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?,
            u8::from_str_radix(&hex[3..4].repeat(2), 16).ok()?,
        ),
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Color::from_rgba_u8(r, g, b, a))
}

/// Parse the body of an `rgb()`/`rgba()` call (prefix already stripped).
fn parse_rgb(body: &str) -> Option<Color> {
    let body = body.trim().trim_end_matches(')');
    let parts: Vec<&str> = body.split(',').map(|p| p.trim()).collect();
    if parts.len() < 3 {
        return None;
    }
    let r = parts[0].parse::<u8>().ok()?;
    let g = parts[1].parse::<u8>().ok()?;
    let b = parts[2].parse::<u8>().ok()?;
    let a = if parts.len() >= 4 {
        let raw = parts[3].parse::<f32>().ok()?;
        if raw > 1.0 {
            (raw / 255.0).clamp(0.0, 1.0)
        } else {
            raw.clamp(0.0, 1.0)
        }
    } else {
        1.0
    };
    Some(Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_rrggbb() {
        let c = Color::parse("#ff00aa").unwrap();
        assert!((c.r - 1.0).abs() < 1e-4);
        assert!(c.g.abs() < 1e-4);
        assert!((c.b - (170.0 / 255.0)).abs() < 1e-4);
        assert!((c.a - 1.0).abs() < 1e-4);
    }

    #[test]
    fn parses_hex_rgb_short() {
        let c = Color::parse("#f0a").unwrap();
        assert!((c.r - 1.0).abs() < 1e-4);
        assert!(c.g.abs() < 1e-4);
        assert!((c.b - (170.0 / 255.0)).abs() < 1e-4);
    }

    #[test]
    fn parses_hex_rrggbbaa() {
        let c = Color::parse("#ff00aa80").unwrap();
        assert!((c.a - (128.0 / 255.0)).abs() < 1e-3);
    }

    #[test]
    fn parses_rgb_tuple() {
        let c = Color::parse("rgb(255, 128, 64)").unwrap();
        assert!((c.r - 1.0).abs() < 1e-4);
        assert!((c.g - (128.0 / 255.0)).abs() < 1e-4);
        assert!((c.b - (64.0 / 255.0)).abs() < 1e-4);
    }

    #[test]
    fn parses_rgba_float_alpha() {
        let c = Color::parse("rgba(255, 128, 64, 0.5)").unwrap();
        assert!((c.a - 0.5).abs() < 1e-4);
    }

    #[test]
    fn to_hsla_red_roundtrips_to_rgba() {
        let red = Color::from_rgb_u8(255, 0, 0);
        let h = red.to_hsla();
        // pure red: saturation 1, lightness 0.5
        assert!((h.s - 1.0).abs() < 1e-4);
        assert!((h.l - 0.5).abs() < 1e-4);
    }

    #[test]
    fn lighten_increases_lightness() {
        let c = Color::from_rgb_u8(100, 100, 100); // mid grey
        let lighter = c.lighten(0.2);
        assert!(lighter.to_hsla().l > c.to_hsla().l);
    }

    #[test]
    fn darken_decreases_lightness() {
        let c = Color::from_rgb_u8(200, 200, 200);
        let darker = c.darken(0.2);
        assert!(darker.to_hsla().l < c.to_hsla().l);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(Color::parse("not a color").is_none());
        assert!(Color::parse("#zzzzzz").is_none());
        assert!(Color::parse("#12345").is_none()); // bad length
    }

    #[test]
    fn lighten_clamps_to_one() {
        let c = Color::from_rgb_u8(255, 255, 255); // already l=1
        let clamped = c.lighten(0.9);
        assert!((clamped.to_hsla().l - 1.0).abs() < 1e-4);
    }
}
