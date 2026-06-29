//! Small HSLA math helpers used by theming code.
//!
//! Operate directly on [`gpui::Hsla`] (hue in 0..=1, matching gpui's
//! convention) so they can be applied to colors pulled from `cx.theme()` without
//! any intermediate conversion. Pure functions: no app state, no I/O.

use gpui::Hsla;

/// Return a copy of `color` with its lightness increased by `amount`
/// (0..=1), clamped to the valid range. Alpha is preserved.
///
/// Useful for building hover / highlight variants of a theme color, e.g.
/// `lighten_color(cx.theme().accent, 0.08)`.
pub fn lighten_color(color: Hsla, amount: f32) -> Hsla {
    Hsla {
        h: color.h,
        s: color.s,
        l: (color.l + amount).clamp(0.0, 1.0),
        a: color.a,
    }
}

/// Return a copy of `color` with its lightness decreased by `amount`
/// (0..=1), clamped to the valid range. Alpha is preserved.
///
/// Useful for building pressed / disabled variants of a theme color, e.g.
/// `darken_color(cx.theme().accent, 0.12)`.
pub fn darken_color(color: Hsla, amount: f32) -> Hsla {
    Hsla {
        h: color.h,
        s: color.s,
        l: (color.l - amount).clamp(0.0, 1.0),
        a: color.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    #[test]
    fn lighten_increases_lightness() {
        let c = hsla(0.5, 0.6, 0.4, 1.0);
        assert!((lighten_color(c, 0.2).l - 0.6).abs() < 1e-4);
    }

    #[test]
    fn darken_decreases_lightness() {
        let c = hsla(0.5, 0.6, 0.4, 1.0);
        assert!((darken_color(c, 0.2).l - 0.2).abs() < 1e-4);
    }

    #[test]
    fn lighten_clamps_to_one() {
        let c = hsla(0.0, 1.0, 0.9, 1.0);
        assert!((lighten_color(c, 0.5).l - 1.0).abs() < 1e-4);
    }

    #[test]
    fn darken_clamps_to_zero() {
        let c = hsla(0.0, 1.0, 0.1, 1.0);
        assert!((darken_color(c, 0.5).l - 0.0).abs() < 1e-4);
    }

    #[test]
    fn alpha_is_preserved() {
        let c = hsla(0.2, 0.3, 0.4, 0.5);
        assert!((lighten_color(c, 0.1).a - 0.5).abs() < 1e-4);
        assert!((darken_color(c, 0.1).a - 0.5).abs() < 1e-4);
    }
}
