//! WCAG colour maths — relative luminance and contrast ratio.
//!
//! Pure, allocation-light, and free of every ikigai type, so it compiles to
//! `wasm32-unknown-unknown` and can be unit-tested against the published WCAG
//! worked examples. Everything else in this crate that reasons about legibility
//! reduces to [`ratio`].
//!
//! The definitions are WCAG 2.x §1.4.3, verbatim:
//!
//! - a channel is gamma-decoded `c/12.92` below the 0.03928 knee and
//!   `((c + 0.055)/1.055)^2.4` above it,
//! - relative luminance is `0.2126·R + 0.7152·G + 0.0722·B` of the decoded
//!   channels,
//! - the ratio of two colours is `(L_lighter + 0.05) / (L_darker + 0.05)`.
//!
//! Alpha is composited, not ignored. syntect emits `#rrggbbaa` whenever a theme
//! gives a scope a translucent colour, and a translucent foreground's *apparent*
//! contrast is the contrast of what the eye actually sees — the colour composited
//! over its ground. Treating `#ffffff00` as white would report 21:1 for an
//! invisible glyph.

use std::fmt;

/// An 8-bit-per-channel sRGB colour with alpha.
///
/// `alpha` is 255 for the opaque `#rrggbb` form. It is not part of luminance:
/// composite with [`over`](Rgba::over) first, which is what the eye does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgba {
    /// Red channel, 0–255.
    pub r: u8,
    /// Green channel, 0–255.
    pub g: u8,
    /// Blue channel, 0–255.
    pub b: u8,
    /// Alpha, 0 (transparent) – 255 (opaque).
    pub alpha: u8,
}

/// Why a colour string could not be read — carried rather than swallowed, because
/// a mistyped colour in an operator's config must be loud.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParseColorError {
    /// The offending text, as written.
    pub input: String,
}

impl fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "expected a hex colour (#rgb, #rgba, #rrggbb or #rrggbbaa), got {:?}",
            self.input
        )
    }
}

impl std::error::Error for ParseColorError {}

impl Rgba {
    /// An opaque colour from its three channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba {
            r,
            g,
            b,
            alpha: 0xff,
        }
    }

    /// Parse a CSS hex colour: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. The
    /// leading `#` is optional and case does not matter.
    ///
    /// Named CSS colours (`red`, `transparent`) are deliberately NOT accepted: the
    /// producers this crate reads — syntect's CSS generator and an operator's
    /// `a11y.toml` — emit hex, and silently accepting a subset of the named
    /// palette would make the failure mode "some names work" rather than "names
    /// don't".
    pub fn parse(text: &str) -> Result<Self, ParseColorError> {
        let fail = || ParseColorError {
            input: text.to_string(),
        };
        let hex = text.trim().trim_start_matches('#');
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(fail());
        }
        // The short forms duplicate each digit — `#abc` is `#aabbcc`, per CSS.
        let expand = |c: char| {
            let d = c.to_digit(16).expect("checked ascii hexdigit") as u8;
            d << 4 | d
        };
        let byte = |s: &str| u8::from_str_radix(s, 16).expect("checked ascii hexdigit");
        let digits: Vec<char> = hex.chars().collect();
        match digits.len() {
            3 => Ok(Rgba::rgb(
                expand(digits[0]),
                expand(digits[1]),
                expand(digits[2]),
            )),
            4 => Ok(Rgba {
                r: expand(digits[0]),
                g: expand(digits[1]),
                b: expand(digits[2]),
                alpha: expand(digits[3]),
            }),
            6 => Ok(Rgba::rgb(
                byte(&hex[0..2]),
                byte(&hex[2..4]),
                byte(&hex[4..6]),
            )),
            8 => Ok(Rgba {
                r: byte(&hex[0..2]),
                g: byte(&hex[2..4]),
                b: byte(&hex[4..6]),
                alpha: byte(&hex[6..8]),
            }),
            _ => Err(fail()),
        }
    }

    /// Whether this colour is fully opaque.
    pub fn is_opaque(self) -> bool {
        self.alpha == 0xff
    }

    /// This colour composited over `ground` (source-over alpha blending),
    /// yielding the opaque colour an eye actually sees.
    pub fn over(self, ground: Rgba) -> Rgba {
        if self.is_opaque() {
            return self;
        }
        let a = f64::from(self.alpha) / 255.0;
        let mix = |top: u8, bottom: u8| {
            (f64::from(top) * a + f64::from(bottom) * (1.0 - a)).round() as u8
        };
        Rgba::rgb(
            mix(self.r, ground.r),
            mix(self.g, ground.g),
            mix(self.b, ground.b),
        )
    }

    /// The canonical CSS spelling: `#rrggbb`, or `#rrggbbaa` when translucent —
    /// the same shape syntect writes, so a rewritten stylesheet stays uniform.
    pub fn to_css(self) -> String {
        if self.is_opaque() {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.alpha
            )
        }
    }

    /// WCAG relative luminance of this colour, **ignoring alpha** — composite
    /// with [`over`](Self::over) first if it may be translucent.
    pub fn luminance(self) -> f64 {
        fn decode(channel: u8) -> f64 {
            let c = f64::from(channel) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * decode(self.r) + 0.7152 * decode(self.g) + 0.0722 * decode(self.b)
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_css())
    }
}

/// The WCAG contrast ratio between two colours: `1.0` (identical) to `21.0`
/// (black on white). Symmetric — the lighter colour is found, not assumed.
///
/// A translucent `foreground` is composited over `background` first; a
/// translucent *background* is taken at face value, because what lies behind it
/// is not knowable from here (a caller that knows should composite it itself).
pub fn ratio(foreground: Rgba, background: Rgba) -> f64 {
    let fg = foreground.over(background).luminance();
    let bg = background.luminance();
    let (light, dark) = if fg >= bg { (fg, bg) } else { (bg, fg) };
    (light + 0.05) / (dark + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two anchors of the scale, exactly as WCAG defines them.
    #[test]
    fn black_on_white_is_twenty_one_to_one_and_a_colour_on_itself_is_one() {
        let black = Rgba::rgb(0, 0, 0);
        let white = Rgba::rgb(0xff, 0xff, 0xff);
        assert!((ratio(black, white) - 21.0).abs() < 1e-9);
        assert!((ratio(white, black) - 21.0).abs() < 1e-9, "symmetric");
        assert!((ratio(white, white) - 1.0).abs() < 1e-9);
    }

    /// The pair that motivated this crate: InspiredGitHub's base foreground
    /// `#323232` landing on base16-ocean.dark's ground `#2b303b` when a light
    /// theme's rule escaped into the dark block. 1.03:1 — text on text.
    #[test]
    fn the_pair_that_motivated_this_crate_is_one_point_oh_three() {
        let r = ratio(
            Rgba::parse("#323232").unwrap(),
            Rgba::parse("#2b303b").unwrap(),
        );
        assert!((r - 1.03).abs() < 0.005, "{r}");
    }

    /// The parameter colour base16-ocean.dark actually means (its own
    /// foreground) clears AA on its own ground; the red it falls through to
    /// does not. Both numbers are quoted in ikigai-browse's DARK_PARAMETER.
    #[test]
    fn the_theme_s_own_foreground_clears_aa_where_the_fallthrough_does_not() {
        let ground = Rgba::parse("#2b303b").unwrap();
        let intended = ratio(Rgba::parse("#c0c5ce").unwrap(), ground);
        assert!((intended - 7.63).abs() < 0.01, "{intended}");
        assert!(intended >= 4.5);
        let fallthrough = ratio(Rgba::parse("#bf616a").unwrap(), ground);
        assert!((fallthrough - 3.23).abs() < 0.01, "{fallthrough}");
        assert!(fallthrough < 4.5);
    }

    #[test]
    fn every_hex_form_parses_and_the_short_ones_duplicate_digits() {
        assert_eq!(Rgba::parse("#fff").unwrap(), Rgba::rgb(255, 255, 255));
        assert_eq!(Rgba::parse("abc").unwrap(), Rgba::parse("#aabbcc").unwrap());
        assert_eq!(Rgba::parse("#C0C5CE").unwrap(), Rgba::rgb(0xc0, 0xc5, 0xce));
        let translucent = Rgba::parse("#11223380").unwrap();
        assert_eq!(translucent.alpha, 0x80);
        assert_eq!(Rgba::parse("#1238").unwrap().alpha, 0x88);
        assert_eq!(translucent.to_css(), "#11223380");
        assert_eq!(Rgba::rgb(1, 2, 3).to_css(), "#010203");
    }

    #[test]
    fn a_colour_that_is_not_hex_is_an_error_not_a_guess() {
        for bad in ["red", "#12345", "", "#gggggg", "rgb(1,2,3)"] {
            assert!(Rgba::parse(bad).is_err(), "{bad:?} must not parse");
        }
    }

    /// Alpha is composited, not ignored: fully transparent text has the contrast
    /// of its ground (1:1 — invisible), not of the colour it nominally names.
    #[test]
    fn a_translucent_foreground_is_composited_before_measuring() {
        let ground = Rgba::parse("#000000").unwrap();
        let invisible = Rgba::parse("#ffffff00").unwrap();
        assert!((ratio(invisible, ground) - 1.0).abs() < 1e-9);
        // Half-opacity white over black composites to mid grey, so the ratio
        // sits between the two extremes rather than at 21:1.
        let half = Rgba::parse("#ffffff80").unwrap();
        let r = ratio(half, ground);
        assert!(r > 1.0 && r < 21.0, "{r}");
        assert_eq!(half.over(ground), Rgba::rgb(128, 128, 128));
    }
}
