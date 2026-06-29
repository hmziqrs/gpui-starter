//! Serde (de)serialization helpers for gpui color and dimension types.
//!
//! Two modules intended for use with `#[serde(with = "...")]`:
//!
//! * [`hsla_serde`] — serialize/deserialize [`gpui::Hsla`] from hex strings
//!   (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`), `rgb()`/`rgba()` strings, or an
//!   `{h,s,l,a}` / `{r,g,b(,a)}` object. Serializes back to the `{h,s,l,a}`
//!   object form.
//! * [`pixels_serde`] — serialize/deserialize [`gpui::Pixels`] as a bare float
//!   (the pixel count), accepting either a number or a `"Pixels(12.0)"` string.
//!
//! Generic boilerplate: contains no application-specific names.

// ---------------------------------------------------------------------------
// Hsla serde
// ---------------------------------------------------------------------------

/// Serde module for [`gpui::Hsla`]. Use with `#[serde(with = "hsla_serde")]`.
pub mod hsla_serde {
    use gpui::{Hsla, hsla};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serialize an [`Hsla`] as `{ h, s, l, a }` (all `f32`, range 0..=1).
    pub fn serialize<S>(color: &Hsla, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct HslaHelper {
            h: f32,
            s: f32,
            l: f32,
            a: f32,
        }

        let helper = HslaHelper {
            h: color.h,
            s: color.s,
            l: color.l,
            a: color.a,
        };
        helper.serialize(serializer)
    }

    /// Deserialize an [`Hsla`] from any supported color representation:
    /// hex string, `rgb()`/`rgba()` string, or an RGB/HSL object.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Hsla, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ColorFormat {
            // HSLA object: { h = 0.5, s = 0.8, l = 0.6, a = 1.0 }
            Hsla { h: f32, s: f32, l: f32, a: f32 },
            // RGBA object (alpha optional): { r = 255, g = 128, b = 64 } or { ..., a = 128 }
            Rgba { r: u8, g: u8, b: u8, a: Option<u8> },
            // Anything else (hex / rgb()/rgba()/hsla() strings).
            Str(String),
        }

        match ColorFormat::deserialize(deserializer)? {
            ColorFormat::Hsla { h, s, l, a } => Ok(hsla(h, s, l, a)),
            ColorFormat::Rgba { r, g, b, a } => Ok(rgba_to_hsla(r, g, b, a.unwrap_or(255))),
            ColorFormat::Str(s) => parse_color_string::<D::Error>(&s),
        }
    }

    /// Convert 0..=255 RGBA components into a gpui [`Hsla`] (hue in 0..=1).
    fn rgba_to_hsla(r: u8, g: u8, b: u8, a: u8) -> Hsla {
        let r = r as f32 / 255.0;
        let g = g as f32 / 255.0;
        let b = b as f32 / 255.0;
        let a = a as f32 / 255.0;

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

    /// Parse a color string (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
    /// `rgb(r,g,b)`, `rgba(r,g,b,a)`). Returns the matching [`Hsla`].
    fn parse_color_string<E: serde::de::Error>(s: &str) -> Result<Hsla, E> {
        let s = s.trim();

        if let Some(hex) = s.strip_prefix('#') {
            return parse_hex_color::<E>(hex);
        }

        if let Some(rest) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")) {
            return parse_rgb_tuple::<E>(rest, /* allow_alpha = */ true);
        }

        if let Some(rest) = s.strip_prefix("hsla(").or_else(|| s.strip_prefix("hsl(")) {
            return parse_hsl_tuple::<E>(rest);
        }

        Err(E::custom(format!("unsupported color string: {s:?}")))
    }

    /// Parse the hex digits (without the leading `#`).
    fn parse_hex_color<E: serde::de::Error>(hex: &str) -> Result<Hsla, E> {
        let bad = |e: core::num::ParseIntError| E::custom(format!("invalid hex color: {e}"));

        let (r, g, b, a) = match hex.len() {
            3 => (
                u8::from_str_radix(&hex[0..1].repeat(2), 16).map_err(bad)?,
                u8::from_str_radix(&hex[1..2].repeat(2), 16).map_err(bad)?,
                u8::from_str_radix(&hex[2..3].repeat(2), 16).map_err(bad)?,
                255,
            ),
            4 => (
                u8::from_str_radix(&hex[0..1].repeat(2), 16).map_err(bad)?,
                u8::from_str_radix(&hex[1..2].repeat(2), 16).map_err(bad)?,
                u8::from_str_radix(&hex[2..3].repeat(2), 16).map_err(bad)?,
                u8::from_str_radix(&hex[3..4].repeat(2), 16).map_err(bad)?,
            ),
            6 => (
                u8::from_str_radix(&hex[0..2], 16).map_err(bad)?,
                u8::from_str_radix(&hex[2..4], 16).map_err(bad)?,
                u8::from_str_radix(&hex[4..6], 16).map_err(bad)?,
                255,
            ),
            8 => (
                u8::from_str_radix(&hex[0..2], 16).map_err(bad)?,
                u8::from_str_radix(&hex[2..4], 16).map_err(bad)?,
                u8::from_str_radix(&hex[4..6], 16).map_err(bad)?,
                u8::from_str_radix(&hex[6..8], 16).map_err(bad)?,
            ),
            _ => {
                return Err(E::custom(format!(
                    "invalid hex color length: {}",
                    hex.len()
                )));
            }
        };

        Ok(rgba_to_hsla(r, g, b, a))
    }

    /// Parse the `r,g,b[,a]` body of an `rgb()` / `rgba()` call (already
    /// stripped of the prefix). Strips a trailing `)`.
    fn parse_rgb_tuple<E: serde::de::Error>(body: &str, allow_alpha: bool) -> Result<Hsla, E> {
        let body = body.trim().trim_end_matches(')');
        let parts: Vec<&str> = body.split(',').map(|p| p.trim()).collect();

        if parts.len() < 3 || (!allow_alpha && parts.len() != 3) {
            return Err(E::custom(format!("invalid rgb()/rgba() tuple: {body:?}")));
        }

        let r = parts[0]
            .parse::<u8>()
            .map_err(|e| E::custom(format!("invalid red channel: {e}")))?;
        let g = parts[1]
            .parse::<u8>()
            .map_err(|e| E::custom(format!("invalid green channel: {e}")))?;
        let b = parts[2]
            .parse::<u8>()
            .map_err(|e| E::custom(format!("invalid blue channel: {e}")))?;

        let a = if parts.len() >= 4 {
            // Accept 0..=255 integer or 0..=1 float alpha.
            let raw = parts[3];
            if let Ok(v) = raw.parse::<f32>() {
                (if v > 1.0 { v } else { v * 255.0 }) as u8
            } else {
                255
            }
        } else {
            255
        };

        Ok(rgba_to_hsla(r, g, b, a))
    }

    /// Parse the `h,s,l[,a]` body of an `hsl()` / `hsla()` call. Hue is in
    /// degrees (0..=360); s/l/a are percentages (0..=100) scaled into 0..=1.
    fn parse_hsl_tuple<E: serde::de::Error>(body: &str) -> Result<Hsla, E> {
        let body = body.trim().trim_end_matches(')');
        let parts: Vec<&str> = body.split(',').map(|p| p.trim()).collect();

        if parts.len() < 3 {
            return Err(E::custom(format!("invalid hsl()/hsla() tuple: {body:?}")));
        }

        let h_deg = parts[0]
            .trim_end_matches("deg")
            .parse::<f32>()
            .map_err(|e| E::custom(format!("invalid hue: {e}")))?;
        let s_pct = parse_pct::<E>(parts[1])?;
        let l_pct = parse_pct::<E>(parts[2])?;
        let a = if parts.len() >= 4 {
            parts[3].parse::<f32>().unwrap_or(1.0)
        } else {
            1.0
        };

        Ok(hsla(h_deg / 360.0, s_pct, l_pct, a))
    }

    /// Parse a percentage like `"80%"` or `"0.8"` into a 0..=1 factor.
    fn parse_pct<E: serde::de::Error>(raw: &str) -> Result<f32, E> {
        let raw = raw.trim();
        if let Some(rest) = raw.strip_suffix('%') {
            rest.parse::<f32>()
                .map(|v| v / 100.0)
                .map_err(|e| E::custom(format!("invalid percentage: {e}")))
        } else {
            raw.parse::<f32>()
                .map_err(|e| E::custom(format!("invalid percentage: {e}")))
        }
    }
}

// ---------------------------------------------------------------------------
// Pixels serde
// ---------------------------------------------------------------------------

/// Serde module for [`gpui::Pixels`]. Use with `#[serde(with = "pixels_serde")]`.
pub mod pixels_serde {
    use gpui::{Pixels, px};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize [`Pixels`] as the bare `f32` pixel count.
    pub fn serialize<S>(pixels: &Pixels, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // `Pixels`' inner field is private, but its Display/Debug output is
        // "<n>px" (e.g. "12px"). Strip the suffix and serialize the bare f32.
        let rendered = format!("{pixels}");
        let num_str = rendered.trim_end_matches("px");
        match num_str.parse::<f32>() {
            Ok(value) => serializer.serialize_f32(value),
            Err(_) => serializer.serialize_str(num_str),
        }
    }

    /// Deserialize [`Pixels`] from a number or a `"Pixels(12.0)"` / `"12"` string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Pixels, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PixelsHelper {
            Float(f32),
            String(String),
        }

        match PixelsHelper::deserialize(deserializer)? {
            PixelsHelper::Float(value) => Ok(px(value)),
            PixelsHelper::String(s) => {
                // Accept "Pixels(12.0)", "12.0px", or bare "12".
                let cleaned = s
                    .trim()
                    .trim_start_matches("Pixels(")
                    .trim_end_matches(')')
                    .trim_end_matches("px");
                cleaned
                    .parse::<f32>()
                    .map(px)
                    .map_err(|e| serde::de::Error::custom(format!("invalid pixels value: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hsla_serde;
    use gpui::Hsla;

    #[derive(serde::Deserialize, serde::Serialize)]
    struct Wrap {
        #[serde(with = "hsla_serde")]
        color: Hsla,
    }

    #[test]
    fn deserializes_hex_rrggbb() {
        let w: Wrap = serde_json::from_str(r##"{"color":"#ff0000"}"##).unwrap();
        // pure red: h=0, s=1, l=0.5, a=1
        assert!((w.color.h).abs() < 1e-4 || (w.color.h - 1.0).abs() < 1e-4);
        assert!((w.color.s - 1.0).abs() < 1e-4);
        assert!((w.color.l - 0.5).abs() < 1e-4);
        assert!((w.color.a - 1.0).abs() < 1e-4);
    }

    #[test]
    fn deserializes_rgb_object() {
        let w: Wrap = serde_json::from_str(r#"{"color":{"r":0,"g":0,"b":255}}" "#).unwrap();
        // pure blue hue in gpui 0..=1 is 2/3.
        assert!((w.color.h - (2.0 / 3.0)).abs() < 1e-3);
    }

    #[test]
    fn roundtrips_through_object() {
        let original = Wrap {
            color: gpui::hsla(0.33, 0.5, 0.25, 1.0),
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Wrap = serde_json::from_str(&json).unwrap();
        assert!((back.color.h - original.color.h).abs() < 1e-4);
        assert!((back.color.s - original.color.s).abs() < 1e-4);
        assert!((back.color.l - original.color.l).abs() < 1e-4);
    }

    #[test]
    fn deserializes_rrggbbaa_with_alpha() {
        let w: Wrap = serde_json::from_str(r##"{"color":"#ff000080"}"##).unwrap();
        assert!((w.color.a - (128.0 / 255.0)).abs() < 1e-3);
    }
}
