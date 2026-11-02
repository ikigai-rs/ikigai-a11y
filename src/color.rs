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
            "expected a hex color (#rgb, #rgba, #rrggbb or #rrggbbaa) or a CSS rgb()/rgba() \
             color, got {:?}",
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

    /// Parse a CSS color in one of two notations:
    ///
    /// - **hex**: `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. The leading `#` is
    ///   optional and case does not matter.
    /// - **`rgb()`/`rgba()`**, as CSS Color 4 §5.1 defines them (the two names
    ///   are aliases): the legacy comma form, whose three channels are all
    ///   numbers or all percentages, with an optional fourth alpha; and the
    ///   modern space form, which may mix them, accepts `none` (zero), and takes
    ///   alpha after a `/`. Alpha is a number (0–1) or a percentage. Out-of-range
    ///   values clamp, as CSS says they do, and fractional channels round to the
    ///   nearest 8-bit step.
    ///
    /// ```
    /// use ikigai_a11y::Rgba;
    /// let tint = Rgba { r: 255, g: 255, b: 255, alpha: 0x80 };
    /// assert_eq!(Rgba::parse("#ffffff80"), Ok(tint));
    /// assert_eq!(Rgba::parse("rgba(255, 255, 255, 0.5)"), Ok(tint));
    /// assert_eq!(Rgba::parse("rgb(100% 100% 100% / 50%)"), Ok(tint));
    /// assert!(Rgba::parse("red").is_err());
    /// ```
    ///
    /// Named CSS colors (`red`, `transparent`) are deliberately NOT accepted, nor
    /// `hsl()`, `var()`, `calc()` or relative colors: silently accepting a subset
    /// of the named palette would make the failure mode "some names work" rather
    /// than "names don't".
    pub fn parse(text: &str) -> Result<Self, ParseColorError> {
        let fail = || ParseColorError {
            input: text.to_string(),
        };
        let trimmed = text.trim();
        if let Some(args) = rgb_arguments(trimmed) {
            return rgb_function(args).ok_or_else(fail);
        }
        let hex = trimmed.trim_start_matches('#');
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

// CSS Color 4 `rgb()`/`rgba()`, implemented here rather than borrowed: no crate in
// this dependency graph parses CSS colors (syntect reads tmTheme hex only), and
// adding a CSS parser (cssparser, csscolorparser) would pull a tokenizer and the
// whole named palette into a crate that compiles to wasm and deliberately refuses
// names. The grammar is small; the vectors in the tests below pin it.

/// The text between the parentheses of `rgb(…)` or `rgba(…)` (the function name
/// in any case), or `None` when `text` is not such a call.
fn rgb_arguments(text: &str) -> Option<&str> {
    let body = text.strip_suffix(')')?;
    ["rgba(", "rgb("].into_iter().find_map(|name| {
        let head = body.get(..name.len())?;
        head.eq_ignore_ascii_case(name).then(|| &body[name.len()..])
    })
}

/// One `rgb()` argument as written.
#[derive(Clone, Copy)]
enum Component {
    Number(f64),
    Percent(f64),
    /// The modern form's missing component, which resolves to zero.
    Missing,
}

impl Component {
    fn read(token: &str) -> Option<Self> {
        if token.eq_ignore_ascii_case("none") {
            return Some(Component::Missing);
        }
        match token.strip_suffix('%') {
            Some(number) => css_number(number).map(Component::Percent),
            None => css_number(token).map(Component::Number),
        }
    }

    /// A color channel: a number on the 0–255 scale, or a percentage of it.
    fn channel(self) -> u8 {
        let value = match self {
            Component::Number(n) => n,
            Component::Percent(p) => p / 100.0 * 255.0,
            Component::Missing => 0.0,
        };
        value.clamp(0.0, 255.0).round() as u8
    }

    /// An alpha: a number on the 0–1 scale, or a percentage of it.
    fn alpha(self) -> u8 {
        let value = match self {
            Component::Number(n) => n,
            Component::Percent(p) => p / 100.0,
            Component::Missing => 0.0,
        };
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

/// A CSS `<number-token>`: an optional sign, digits with an optional fraction
/// (`.5` is a number, `5.` is not), and an optional exponent. Stricter than
/// `f64::from_str`, which also takes `5.`, `inf` and `NaN`.
fn css_number(token: &str) -> Option<f64> {
    let bytes = token.as_bytes();
    let digits_from = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_end = digits_from(i);
    let mut seen_digits = integer_end > i;
    i = integer_end;
    if bytes.get(i) == Some(&b'.') {
        let fraction_end = digits_from(i + 1);
        if fraction_end == i + 1 {
            return None;
        }
        seen_digits = true;
        i = fraction_end;
    }
    if !seen_digits {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        i += usize::from(matches!(bytes.get(i), Some(b'+' | b'-')));
        let exponent_end = digits_from(i);
        if exponent_end == i {
            return None;
        }
        i = exponent_end;
    }
    // A huge exponent reads as infinity, which clamps like any out-of-range value.
    (i == bytes.len()).then(|| token.parse().ok()).flatten()
}

/// The arguments of `rgb()`/`rgba()`: the comma form when there is a comma,
/// otherwise the space form.
fn rgb_function(args: &str) -> Option<Rgba> {
    let (channels, alpha) = if args.contains(',') {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        if !(3..=4).contains(&parts.len()) {
            return None;
        }
        let channels = [parts[0], parts[1], parts[2]].map(Component::read);
        let channels = [channels[0]?, channels[1]?, channels[2]?];
        // The legacy form takes all numbers or all percentages, and no `none`.
        let all_numbers = channels.iter().all(|c| matches!(c, Component::Number(_)));
        let all_percents = channels.iter().all(|c| matches!(c, Component::Percent(_)));
        if !(all_numbers || all_percents) {
            return None;
        }
        let alpha = match parts.get(3) {
            None => None,
            Some(token) => match Component::read(token)? {
                Component::Missing => return None,
                alpha => Some(alpha),
            },
        };
        (channels, alpha)
    } else {
        let (channels, alpha) = match args.split_once('/') {
            Some((channels, alpha)) => (channels, Some(Component::read(alpha.trim())?)),
            None => (args, None),
        };
        let parts: Vec<&str> = channels.split_ascii_whitespace().collect();
        let [r, g, b] = parts[..] else {
            return None;
        };
        let channels = [
            Component::read(r)?,
            Component::read(g)?,
            Component::read(b)?,
        ];
        (channels, alpha)
    };
    Some(Rgba {
        r: channels[0].channel(),
        g: channels[1].channel(),
        b: channels[2].channel(),
        alpha: alpha.map_or(0xff, Component::alpha),
    })
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
    fn a_color_this_crate_does_not_read_is_an_error_not_a_guess() {
        for bad in [
            "red",
            "#12345",
            "",
            "#gggggg",
            "transparent",
            "hsl(0 0% 0%)",
        ] {
            assert!(Rgba::parse(bad).is_err(), "{bad:?} must not parse");
        }
    }

    /// CSS Color 4 §5.1 `rgb()`/`rgba()`, as `(input, r, g, b, alpha)`. The two
    /// names are aliases; the legacy form is comma-separated with channels all
    /// numbers or all percentages, the modern form is space-separated with an
    /// optional `/ alpha` and may mix them. Out-of-range values clamp, fractional
    /// ones round to the nearest 8-bit step, and alpha is a number or a percentage.
    const RGB_VECTORS: &[(&str, u8, u8, u8, u8)] = &[
        // Legacy (comma) syntax.
        ("rgb(255, 0, 0)", 255, 0, 0, 255),
        ("rgb(1,2,3)", 1, 2, 3, 255),
        ("rgba(255, 255, 255, 0.5)", 255, 255, 255, 128),
        ("rgba(0, 0, 0, .25)", 0, 0, 0, 64),
        ("rgba(0, 0, 0, 25%)", 0, 0, 0, 64),
        ("rgb(1, 2, 3, 0.5)", 1, 2, 3, 128),
        ("rgba(1, 2, 3)", 1, 2, 3, 255),
        ("rgb(100%, 50%, 0%)", 255, 128, 0, 255),
        ("rgb( 10 , 20 , 30 )", 10, 20, 30, 255),
        // Modern (space) syntax.
        ("rgb(255 0 0)", 255, 0, 0, 255),
        ("rgb(255 0 0 / 50%)", 255, 0, 0, 128),
        ("rgb(255 0 0/0.25)", 255, 0, 0, 64),
        ("rgba(1 2 3)", 1, 2, 3, 255),
        ("rgba(1 2 3 / 1)", 1, 2, 3, 255),
        ("rgb(100% 0 0)", 255, 0, 0, 255),
        ("rgb(none 2 3)", 0, 2, 3, 255),
        ("rgb(1 2 3 / none)", 1, 2, 3, 0),
        // Case, surrounding space, number syntax.
        ("RGB(1, 2, 3)", 1, 2, 3, 255),
        ("RgBa(1 2 3 / 0)", 1, 2, 3, 0),
        ("  rgb(1 2 3)  ", 1, 2, 3, 255),
        ("rgb(1e2 +5 0.4)", 100, 5, 0, 255),
        ("rgb(127.5 0 0)", 128, 0, 0, 255),
        // Clamping, not refusal.
        ("rgb(300, -20, 0)", 255, 0, 0, 255),
        ("rgb(150% -5% 0%)", 255, 0, 0, 255),
        ("rgb(0 0 0 / 2)", 0, 0, 0, 255),
        ("rgb(0 0 0 / -1)", 0, 0, 0, 0),
        ("rgba(0, 0, 0, 120%)", 0, 0, 0, 255),
    ];

    #[test]
    fn css_color_4_rgb_and_rgba_parse_to_their_vectors() {
        for &(input, r, g, b, alpha) in RGB_VECTORS {
            let want = Rgba { r, g, b, alpha };
            assert_eq!(Rgba::parse(input), Ok(want), "{input:?}");
        }
    }

    /// What the grammar refuses: a malformed function is an error, never a
    /// guess, for the same reason a malformed hex color is.
    #[test]
    fn a_malformed_rgb_function_is_an_error() {
        for bad in [
            "rgb()",
            "rgb(1, 2)",
            "rgb(1 2)",
            "rgb(1, 2, 3, 4, 5)",
            "rgb(1 2 3 4)",
            "rgb(1, 2 3)",
            "rgb(1 2, 3)",
            "rgb(1, 2, 3 / 0.5)",
            "rgb(1 2 3 / )",
            "rgb(1 2 3 / 0.5 / 1)",
            "rgb(1 2 3, 0.5)",
            "rgb(1,,2,3)",
            "rgb(1, 2, 3,)",
            "rgb(1,2,3",
            "rgb 1,2,3",
            "rgb(1 2 3)x",
            "rgbx(1 2 3)",
            "rgb(none, 0, 0)",
            "rgb(100%, 0, 0)",
            "rgb(1px 2 3)",
            "rgb(5. 0 0)",
            "rgb(. 0 0)",
            "rgb(1e 0 0)",
            "rgb(calc(1) 2 3)",
            "rgb(from red r g b)",
            "rgb(1 2 3 / 50%%)",
        ] {
            assert!(Rgba::parse(bad).is_err(), "{bad:?} must not parse");
        }
    }

    /// A refusal names the forms that would have worked, so an operator who
    /// mistyped a color in `a11y.toml` can fix it from the message alone.
    #[test]
    fn the_refusal_names_every_accepted_form() {
        let message = Rgba::parse("red").unwrap_err().to_string();
        for form in ["#rrggbb", "rgb(", "rgba("] {
            assert!(message.contains(form), "{message}");
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
