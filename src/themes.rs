//! The `themes` face (feature `themes`): the bridge from a configured theme
//! **name** to a real `syntect` theme, and from a theme to its own CSS with the
//! contrast floor already applied.
//!
//! Optional and off by default. `two-face` carries ~2MB of embedded theme data
//! and pulls in `syntect`; a consumer that only wants the contrast maths, the
//! layered merge or the CSS floor pass — the browser demo, say — needs none of
//! it, and the default build stays wasm-clean without it.
//!
//! `two-face` exposes no public iteration over its theme enum, so
//! [`crate::config::THEMES`] is a hand-maintained table. [`ALL`] below is the
//! matching list; [`identifier`] is an exhaustive `match`, which turns a theme
//! added upstream into a compile error rather than a silent gap in what an
//! operator may configure.

use syntect::highlighting::{Color, Theme};
use syntect::html::{css_for_theme_with_class_style, ClassStyle};
use two_face::theme::EmbeddedThemeName;

use crate::color::Rgba;
use crate::config::canonical_theme;
use crate::css::{apply_floor, FloorPass};

/// Every embedded theme, in [`crate::config::THEMES`] order.
pub const ALL: &[EmbeddedThemeName] = &[
    EmbeddedThemeName::Ansi,
    EmbeddedThemeName::Base16,
    EmbeddedThemeName::Base16EightiesDark,
    EmbeddedThemeName::Base16MochaDark,
    EmbeddedThemeName::Base16OceanDark,
    EmbeddedThemeName::Base16OceanLight,
    EmbeddedThemeName::Base16_256,
    EmbeddedThemeName::CatppuccinFrappe,
    EmbeddedThemeName::CatppuccinLatte,
    EmbeddedThemeName::CatppuccinMacchiato,
    EmbeddedThemeName::CatppuccinMocha,
    EmbeddedThemeName::ColdarkCold,
    EmbeddedThemeName::ColdarkDark,
    EmbeddedThemeName::DarkNeon,
    EmbeddedThemeName::Dracula,
    EmbeddedThemeName::Github,
    EmbeddedThemeName::GruvboxDark,
    EmbeddedThemeName::GruvboxLight,
    EmbeddedThemeName::InspiredGithub,
    EmbeddedThemeName::Leet,
    EmbeddedThemeName::MonokaiExtended,
    EmbeddedThemeName::MonokaiExtendedBright,
    EmbeddedThemeName::MonokaiExtendedLight,
    EmbeddedThemeName::MonokaiExtendedOrigin,
    EmbeddedThemeName::Nord,
    EmbeddedThemeName::OneHalfDark,
    EmbeddedThemeName::OneHalfLight,
    EmbeddedThemeName::SolarizedDark,
    EmbeddedThemeName::SolarizedLight,
    EmbeddedThemeName::SublimeSnazzy,
    EmbeddedThemeName::TwoDark,
    EmbeddedThemeName::Zenburn,
];

/// The identifier an operator writes in `a11y.toml` for a theme.
///
/// Exhaustive by construction: a theme added to `two-face` stops this crate
/// compiling until the table, `ALL` and this `match` agree again — which is the
/// only mechanism available, since the enum is `#[non_exhaustive]`-free but
/// un-iterable outside its own tests.
pub fn identifier(theme: EmbeddedThemeName) -> &'static str {
    match theme {
        EmbeddedThemeName::Ansi => "Ansi",
        EmbeddedThemeName::Base16 => "Base16",
        EmbeddedThemeName::Base16EightiesDark => "Base16EightiesDark",
        EmbeddedThemeName::Base16MochaDark => "Base16MochaDark",
        EmbeddedThemeName::Base16OceanDark => "Base16OceanDark",
        EmbeddedThemeName::Base16OceanLight => "Base16OceanLight",
        EmbeddedThemeName::Base16_256 => "Base16_256",
        EmbeddedThemeName::CatppuccinFrappe => "CatppuccinFrappe",
        EmbeddedThemeName::CatppuccinLatte => "CatppuccinLatte",
        EmbeddedThemeName::CatppuccinMacchiato => "CatppuccinMacchiato",
        EmbeddedThemeName::CatppuccinMocha => "CatppuccinMocha",
        EmbeddedThemeName::ColdarkCold => "ColdarkCold",
        EmbeddedThemeName::ColdarkDark => "ColdarkDark",
        EmbeddedThemeName::DarkNeon => "DarkNeon",
        EmbeddedThemeName::Dracula => "Dracula",
        EmbeddedThemeName::Github => "Github",
        EmbeddedThemeName::GruvboxDark => "GruvboxDark",
        EmbeddedThemeName::GruvboxLight => "GruvboxLight",
        EmbeddedThemeName::InspiredGithub => "InspiredGithub",
        EmbeddedThemeName::Leet => "Leet",
        EmbeddedThemeName::MonokaiExtended => "MonokaiExtended",
        EmbeddedThemeName::MonokaiExtendedBright => "MonokaiExtendedBright",
        EmbeddedThemeName::MonokaiExtendedLight => "MonokaiExtendedLight",
        EmbeddedThemeName::MonokaiExtendedOrigin => "MonokaiExtendedOrigin",
        EmbeddedThemeName::Nord => "Nord",
        EmbeddedThemeName::OneHalfDark => "OneHalfDark",
        EmbeddedThemeName::OneHalfLight => "OneHalfLight",
        EmbeddedThemeName::SolarizedDark => "SolarizedDark",
        EmbeddedThemeName::SolarizedLight => "SolarizedLight",
        EmbeddedThemeName::SublimeSnazzy => "SublimeSnazzy",
        EmbeddedThemeName::TwoDark => "TwoDark",
        EmbeddedThemeName::Zenburn => "Zenburn",
    }
}

/// The embedded theme a configured name refers to, accepting either spelling
/// (`Base16OceanDark` or `base16-ocean.dark`). `None` for a name no theme
/// answers to — the same verdict [`crate::config`] reaches without this feature.
pub fn embedded(name: &str) -> Option<EmbeddedThemeName> {
    let canonical = canonical_theme(name)?;
    ALL.iter().copied().find(|t| identifier(*t) == canonical)
}

fn convert(color: Color) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        alpha: color.a,
    }
}

/// A theme's own ground and default foreground — the two colours the floor pass
/// measures against and repairs to.
///
/// A theme that declares neither is read as black on white, which is what a
/// browser would show for the same sheet.
pub fn ground_and_foreground(theme: &Theme) -> (Rgba, Rgba) {
    let ground = theme
        .settings
        .background
        .map_or(Rgba::rgb(0xff, 0xff, 0xff), convert);
    let foreground = theme
        .settings
        .foreground
        .map_or(Rgba::rgb(0, 0, 0), convert)
        // A translucent default foreground is what the eye sees over the ground.
        .over(ground);
    (ground, foreground)
}

/// A theme's CSS with the contrast floor applied — the turnkey call for a front
/// end that wants "this theme, but legible".
///
/// `prefix` is the class prefix (`ikigai-browse` uses `hl-`). The returned
/// [`FloorPass`] carries both the stylesheet and the list of what it changed, so
/// a caller can log the repair rather than wonder whether one happened.
pub fn theme_css(theme: EmbeddedThemeName, prefix: &'static str, min: f64) -> FloorPass {
    let themes = two_face::theme::extra();
    let theme = themes.get(theme);
    let (ground, foreground) = ground_and_foreground(theme);
    let css = css_for_theme_with_class_style(theme, ClassStyle::SpacedPrefixed { prefix })
        .expect("an embedded theme generates CSS");
    apply_floor(&css, ground, foreground, min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::THEMES;

    /// The hand-maintained table IS the embedded set — both spellings, same
    /// order, nothing missing in either direction. This is the drift guard the
    /// wasm-clean default build buys at the cost of a duplicated list.
    #[test]
    fn the_theme_table_matches_two_face() {
        assert_eq!(ALL.len(), THEMES.len());
        for (theme, (id, syntect_name)) in ALL.iter().zip(THEMES) {
            assert_eq!(identifier(*theme), *id);
            assert_eq!(theme.as_name(), *syntect_name, "{id}");
        }
    }

    #[test]
    fn every_configurable_name_resolves_to_a_theme_both_ways() {
        for (id, syntect_name) in THEMES {
            assert_eq!(embedded(id).map(identifier), Some(*id), "{id}");
            assert_eq!(embedded(syntect_name).map(identifier), Some(*id), "{id}");
        }
        assert_eq!(embedded("NotATheme"), None);
    }

    /// The numbers ikigai-browse hard-codes today, read back off the themes
    /// themselves — so a theme-data update that moved them would be caught here
    /// rather than in a page.
    #[test]
    fn the_two_default_themes_report_the_grounds_browse_hard_codes() {
        let themes = two_face::theme::extra();
        let (ground, foreground) =
            ground_and_foreground(themes.get(EmbeddedThemeName::InspiredGithub));
        assert_eq!(ground.to_css(), "#ffffff");
        assert_eq!(foreground.to_css(), "#323232");
        let (ground, foreground) =
            ground_and_foreground(themes.get(EmbeddedThemeName::Base16OceanDark));
        assert_eq!(ground.to_css(), "#2b303b");
        assert_eq!(foreground.to_css(), "#c0c5ce");
    }

    /// The generalization claim, checked on the exact case that made
    /// ikigai-browse hand-write DARK_PARAMETER: the pass lifts `.hl-variable`'s
    /// red to `#c0c5ce` — the same colour, from the theme, without the constant.
    #[test]
    fn the_pass_reaches_the_rule_dark_parameter_was_written_for() {
        let pass = theme_css(EmbeddedThemeName::Base16OceanDark, "hl-", 4.5);
        assert!(pass.repairable(4.5));
        // syntect emits the scope as one multi-selector rule
        // (`.hl-variable, .hl-variable.hl-other.hl-dollar.hl-only.hl-js`), so
        // match on the selector containing the bare class rather than equalling
        // it — a `.hl-variable` a page carries is styled by exactly this rule.
        let variable = pass
            .lifted
            .iter()
            .find(|l| l.selector.starts_with(".hl-variable,"))
            .expect("the .hl-variable rule is below the floor");
        assert_eq!(variable.from.to_css(), "#bf616a");
        assert_eq!(variable.to.to_css(), "#c0c5ce");
        assert!((variable.ratio - 3.23).abs() < 0.01, "{}", variable.ratio);
        // Running the pass again changes nothing: what could be lifted has been,
        // and what could not is reported rather than churned.
        let second = apply_floor(
            &pass.css,
            Rgba::parse("#2b303b").unwrap(),
            Rgba::parse("#c0c5ce").unwrap(),
            4.5,
        );
        assert_eq!(second.css, pass.css, "the pass is idempotent");
        assert!(second.lifted.is_empty(), "{:?}", second.lifted);
    }
}
