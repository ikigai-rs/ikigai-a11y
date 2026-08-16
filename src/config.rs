//! The accessibility config: schema, **key-wise** layering, and the three faces
//! the effective config is served as.
//!
//! Pure — parsing, merging and rendering only. Nothing here touches a filesystem
//! (that is [`crate::load`], native-only), so this half compiles to
//! `wasm32-unknown-unknown` and a browser host can merge a config it fetched by
//! any means it likes.
//!
//! ## Layering
//!
//! Lowest precedence first: **built-in defaults → `a11y.toml` → `{app}.a11y.toml`
//! → explicit args**. `ikigai_core::layered_paths("a11y.toml", Some(app))` names
//! the files; [`Patch`] is one file's contents and [`A11y::apply`] folds one in.
//!
//! **The merge is key-wise, never wholesale.** A layer states only what it
//! differs on, and every key it stays silent about survives from below. The
//! wholesale alternative — "the last file that exists wins" — is the easy
//! accidental implementation and it silently drops the operator's shared floor
//! the moment one front end wants a different theme. `merge_preserves_a_floor_the_
//! upper_layer_never_mentions` pins it.
//!
//! ## Loud, not lenient
//!
//! An unknown theme name, an out-of-range number and an **unknown key** are all
//! hard errors at load. A misspelled `[contast]` section that silently does
//! nothing is the same defect as a misspelled theme that silently falls back to
//! the default: the operator asked for something and got something else without
//! being told.

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The embedded syntax themes, as `(identifier, syntect name)`.
///
/// The identifier is `two_face::theme::EmbeddedThemeName`'s variant spelling —
/// the one an operator writes in `a11y.toml` — and the second is the name the
/// theme carries inside syntect (what `bat --list-themes` prints). **Both are
/// accepted**; both canonicalize to the identifier, so a config copied from a
/// bat invocation works and the stored value stays one spelling.
///
/// The table is hand-maintained rather than derived, because `two-face` exposes
/// no iteration over its theme enum outside its own tests — and it is exactly
/// what makes the crate's default build wasm-clean: validating a name needs the
/// list, not the 2MB of theme data. `the_theme_table_matches_two_face` (under
/// `--features themes`) fails if the two ever drift.
pub const THEMES: &[(&str, &str)] = &[
    ("Ansi", "ansi"),
    ("Base16", "base16"),
    ("Base16EightiesDark", "base16-eighties.dark"),
    ("Base16MochaDark", "base16-mocha.dark"),
    ("Base16OceanDark", "base16-ocean.dark"),
    ("Base16OceanLight", "base16-ocean.light"),
    ("Base16_256", "base16-256"),
    ("CatppuccinFrappe", "Catppuccin Frappe"),
    ("CatppuccinLatte", "Catppuccin Latte"),
    ("CatppuccinMacchiato", "Catppuccin Macchiato"),
    ("CatppuccinMocha", "Catppuccin Mocha"),
    ("ColdarkCold", "Coldark-Cold"),
    ("ColdarkDark", "Coldark-Dark"),
    ("DarkNeon", "DarkNeon"),
    ("Dracula", "Dracula"),
    ("Github", "GitHub"),
    ("GruvboxDark", "gruvbox-dark"),
    ("GruvboxLight", "gruvbox-light"),
    ("InspiredGithub", "InspiredGitHub"),
    ("Leet", "1337"),
    ("MonokaiExtended", "Monokai Extended"),
    ("MonokaiExtendedBright", "Monokai Extended Bright"),
    ("MonokaiExtendedLight", "Monokai Extended Light"),
    ("MonokaiExtendedOrigin", "Monokai Extended Origin"),
    ("Nord", "Nord"),
    ("OneHalfDark", "OneHalfDark"),
    ("OneHalfLight", "OneHalfLight"),
    ("SolarizedDark", "Solarized (dark)"),
    ("SolarizedLight", "Solarized (light)"),
    ("SublimeSnazzy", "Sublime Snazzy"),
    ("TwoDark", "TwoDark"),
    ("Zenburn", "zenburn"),
];

/// The canonical identifier for a theme written either way, or `None` if no
/// embedded theme goes by that name.
pub fn canonical_theme(name: &str) -> Option<&'static str> {
    THEMES
        .iter()
        .find(|(id, syntect)| *id == name || *syntect == name)
        .map(|(id, _)| *id)
}

/// The config file stem every ikigai front end layers: `a11y.toml` shared, and
/// `{app}.a11y.toml` as one application's override.
///
/// It lives in this (pure, wasm-clean) module rather than beside the loader
/// because [`A11y::to_turtle`] needs it to tell a shared layer from an app one,
/// and [`crate::load`] does not exist on wasm. The loader re-exports it, so
/// `load::STEM` still resolves for native callers.
pub const STEM: &str = "a11y.toml";

/// The default light theme — what `ikigai-browse` hard-codes today, so adopting
/// a config file changes nothing until an operator writes one.
pub const DEFAULT_LIGHT: &str = "InspiredGithub";
/// The default dark theme (likewise `ikigai-browse`'s current constant).
pub const DEFAULT_DARK: &str = "Base16OceanDark";
/// WCAG AA for body text.
pub const DEFAULT_MIN: f64 = 4.5;
/// WCAG AA for large text (≥18pt, or ≥14pt bold) and for UI components.
pub const DEFAULT_MIN_LARGE: f64 = 3.0;
/// The widest text scale a layout can be asked to survive.
const SCALE_RANGE: (f64, f64) = (0.5, 4.0);
/// The full span of possible contrast ratios.
const RATIO_RANGE: (f64, f64) = (1.0, 21.0);

/// What went wrong reading or merging a config. Every variant names the field it
/// blames, because the operator's next move is to edit that line.
//
// `PartialEq` but not `Eq`: `OutOfRange` carries the offending `f64`, which may
// be the NaN that got it rejected.
#[derive(Clone, PartialEq, Debug)]
pub enum ConfigError {
    /// The TOML did not parse, or carried a key the schema does not define.
    Parse {
        /// The file it came from, when it came from one.
        path: Option<PathBuf>,
        /// The parser's own message.
        message: String,
    },
    /// A theme name that no embedded theme answers to.
    UnknownTheme {
        /// `theme.light` or `theme.dark`.
        field: &'static str,
        /// What was written.
        name: String,
    },
    /// A number outside the range its field can mean.
    OutOfRange {
        /// The dotted field path, e.g. `contrast.min`.
        field: &'static str,
        /// What was written.
        value: f64,
        /// Lowest accepted value.
        min: f64,
        /// Highest accepted value.
        max: f64,
    },
    /// A config file exists but could not be read.
    Unreadable {
        /// The file.
        path: PathBuf,
        /// The OS error.
        message: String,
    },
    /// Neither `XDG_CONFIG_HOME` nor `HOME` is set, so there is no config home to
    /// layer within. Not the same as "no config file": an absent file means the
    /// defaults, an absent config HOME means the process cannot tell.
    NoConfigHome,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Parse { path, message } => match path {
                Some(path) => write!(f, "{}: {message}", path.display()),
                None => write!(f, "{message}"),
            },
            ConfigError::UnknownTheme { field, name } => write!(
                f,
                "{field}: no embedded theme is called {name:?} — expected one of: {}",
                THEMES
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            ConfigError::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "{field}: {value} is outside {min}..={max}"),
            ConfigError::Unreadable { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            ConfigError::NoConfigHome => {
                f.write_str("no ikigai config home: neither XDG_CONFIG_HOME nor HOME is set")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// The two syntax themes a front end offers — one per colour scheme, so a page
/// can serve both and let `prefers-color-scheme` choose without a round trip.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Theme {
    /// Theme for the light scheme, as a [`THEMES`] identifier.
    pub light: String,
    /// Theme for the dark scheme, as a [`THEMES`] identifier.
    pub dark: String,
}

/// The contrast floors this deployment holds itself to.
///
/// **Two floors, not one** — the sketch this crate started from had a single
/// `min`, but WCAG does not: body text is 4.5:1 while large text and non-text UI
/// components are 3.0:1. Collapsing them to one number either over-constrains
/// headings (forcing a palette flatter than the standard asks for) or, if the
/// single number is set to 3.0, quietly drops body text below AA.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct Contrast {
    /// Floor for body text. 4.5 is WCAG AA; 7.0 is AAA.
    pub min: f64,
    /// Floor for large text and UI components. 3.0 is WCAG AA.
    pub min_large: f64,
}

/// Motion preferences.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Motion {
    /// Whether to suppress non-essential animation.
    ///
    /// **Three-valued, not a boolean** — the sketch defaulted it to `false`,
    /// which is a claim ("this user does not need reduced motion") that the
    /// config home is not entitled to make. Absent means *unstated*: the front
    /// end should emit `@media (prefers-reduced-motion: reduce)` and let the OS
    /// answer, which is the setting the user already configured once. A present
    /// value overrides the OS in either direction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce: Option<bool>,
}

/// Text presentation preferences.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct Text {
    /// Multiplier on the base font size. 1.0 is the design size.
    pub scale: f64,
    /// Whether links carry an underline rather than relying on colour alone
    /// (WCAG 1.4.1: colour must not be the only visual means of conveying
    /// information).
    pub underline_links: bool,
}

/// The **effective** accessibility config: every key resolved, nothing optional
/// except the deliberately three-valued [`Motion::reduce`].
///
/// This is what `urn:a11y:config` serves. It is never the contents of one file.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct A11y {
    /// Syntax themes per colour scheme.
    pub theme: Theme,
    /// Contrast floors.
    pub contrast: Contrast,
    /// Motion preferences.
    pub motion: Motion,
    /// Text presentation preferences.
    pub text: Text,
    /// The files that contributed, lowest precedence first — provenance, not
    /// configuration. Skipped in the TOML face so that face round-trips as a
    /// config file; present in JSON and Turtle, where it answers "why is the
    /// floor 7.0?" without a second lookup.
    #[serde(skip_serializing)]
    pub layers: Vec<PathBuf>,
}

impl Default for A11y {
    fn default() -> Self {
        A11y {
            theme: Theme {
                light: DEFAULT_LIGHT.to_string(),
                dark: DEFAULT_DARK.to_string(),
            },
            contrast: Contrast {
                min: DEFAULT_MIN,
                min_large: DEFAULT_MIN_LARGE,
            },
            motion: Motion { reduce: None },
            text: Text {
                scale: 1.0,
                underline_links: true,
            },
            layers: Vec::new(),
        }
    }
}

/// One layer's contents: every key optional, because a layer states only its
/// differences. `deny_unknown_fields` is what makes a typo loud.
#[derive(Clone, Default, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// Theme overrides.
    #[serde(default)]
    pub theme: Option<ThemePatch>,
    /// Contrast overrides.
    #[serde(default)]
    pub contrast: Option<ContrastPatch>,
    /// Motion overrides.
    #[serde(default)]
    pub motion: Option<MotionPatch>,
    /// Text overrides.
    #[serde(default)]
    pub text: Option<TextPatch>,
}

/// `[theme]` overrides.
#[derive(Clone, Default, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePatch {
    /// Light-scheme theme.
    pub light: Option<String>,
    /// Dark-scheme theme.
    pub dark: Option<String>,
}

/// `[contrast]` overrides.
#[derive(Clone, Default, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContrastPatch {
    /// Body-text floor.
    pub min: Option<f64>,
    /// Large-text / UI-component floor.
    pub min_large: Option<f64>,
}

/// `[motion]` overrides.
#[derive(Clone, Default, PartialEq, Eq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionPatch {
    /// Whether to suppress non-essential animation.
    pub reduce: Option<bool>,
}

/// `[text]` overrides.
#[derive(Clone, Default, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatch {
    /// Font-size multiplier.
    pub scale: Option<f64>,
    /// Whether links are underlined.
    pub underline_links: Option<bool>,
}

impl Patch {
    /// Parse one layer from TOML text. `path` is carried only for the error
    /// message; parsing does not read it.
    pub fn parse(toml_text: &str, path: Option<PathBuf>) -> Result<Self, ConfigError> {
        let patch: Patch = toml::from_str(toml_text).map_err(|e| ConfigError::Parse {
            path: path.clone(),
            message: e.message().to_string(),
        })?;
        patch.validate()?;
        Ok(patch)
    }

    /// Check every value this layer *states*, before it is merged.
    ///
    /// Validating each layer rather than only the merged result is deliberate: a
    /// misspelled theme in the shared `a11y.toml` is a real defect even when one
    /// app's override happens to hide it, and hiding it is precisely how it would
    /// reach every OTHER app unnoticed.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Some(theme) = &self.theme {
            check_theme("theme.light", theme.light.as_deref())?;
            check_theme("theme.dark", theme.dark.as_deref())?;
        }
        if let Some(contrast) = &self.contrast {
            check_range("contrast.min", contrast.min, RATIO_RANGE)?;
            check_range("contrast.min_large", contrast.min_large, RATIO_RANGE)?;
        }
        if let Some(text) = &self.text {
            check_range("text.scale", text.scale, SCALE_RANGE)?;
        }
        Ok(())
    }
}

fn check_theme(field: &'static str, name: Option<&str>) -> Result<(), ConfigError> {
    match name {
        None => Ok(()),
        Some(name) if canonical_theme(name).is_some() => Ok(()),
        Some(name) => Err(ConfigError::UnknownTheme {
            field,
            name: name.to_string(),
        }),
    }
}

fn check_range(
    field: &'static str,
    value: Option<f64>,
    range: (f64, f64),
) -> Result<(), ConfigError> {
    match value {
        // NaN fails the comparison and lands here too, which is correct: a floor
        // nothing can be compared against is not a floor.
        Some(v) if !(v >= range.0 && v <= range.1) => Err(ConfigError::OutOfRange {
            field,
            value: v,
            min: range.0,
            max: range.1,
        }),
        _ => Ok(()),
    }
}

impl A11y {
    /// Fold one layer in, **key-wise**: each key the layer states replaces the
    /// value below it, and each key it omits is left exactly as it was.
    pub fn apply(&mut self, patch: &Patch) {
        if let Some(theme) = &patch.theme {
            if let Some(light) = &theme.light {
                self.theme.light = canonical(light);
            }
            if let Some(dark) = &theme.dark {
                self.theme.dark = canonical(dark);
            }
        }
        if let Some(contrast) = &patch.contrast {
            if let Some(min) = contrast.min {
                self.contrast.min = min;
            }
            if let Some(min_large) = contrast.min_large {
                self.contrast.min_large = min_large;
            }
        }
        if let Some(motion) = &patch.motion {
            if let Some(reduce) = motion.reduce {
                self.motion.reduce = Some(reduce);
            }
        }
        if let Some(text) = &patch.text {
            if let Some(scale) = text.scale {
                self.text.scale = scale;
            }
            if let Some(underline) = text.underline_links {
                self.text.underline_links = underline;
            }
        }
    }

    /// The effective config from the built-in defaults plus `layers`, lowest
    /// precedence first.
    pub fn merged<'a>(layers: impl IntoIterator<Item = &'a Patch>) -> Self {
        let mut effective = A11y::default();
        for patch in layers {
            effective.apply(patch);
        }
        effective
    }

    /// The `text/plain` (and TOML) face: the effective config as a config file.
    /// Round-trips — feeding this back through [`Patch::parse`] yields the same
    /// effective config, which is what makes it a usable starting point for an
    /// operator who has never written one.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("the effective config is representable as TOML")
    }

    /// The `application/json` face.
    pub fn to_json(&self) -> String {
        // The provenance is `skip_serializing` on the struct (so the TOML face
        // stays a config file); JSON re-attaches it alongside.
        let mut value = serde_json::to_value(self).expect("the effective config is JSON");
        let layers: Vec<String> = self
            .layers
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        if let Some(object) = value.as_object_mut() {
            object.insert("layers".to_string(), serde_json::json!(layers));
        }
        serde_json::to_string_pretty(&value).expect("the effective config is JSON")
    }

    /// The `text/turtle` face: a skolemized graph, no blank nodes.
    ///
    /// `subject` is the resource's own IRI (`urn:a11y:config`, or
    /// `urn:a11y:config:{app}`), so the graph is diffable against another host's
    /// and unionable with the rest of a catalog.
    ///
    /// Contributing files are named twice, deliberately:
    ///
    /// - **`prov:wasDerivedFrom`** — the standard fact, one triple per file, so
    ///   any PROV-aware reader gets the provenance without knowing this
    ///   vocabulary at all.
    /// - **`ik:sharedLayer` / `ik:appLayer`** — the same files by ROLE, because
    ///   RDF triples are unordered and the role is what answers "why is the
    ///   floor 7.0?". A bare repeated property would lose precedence exactly
    ///   where it is being asked for: with two layers each stating
    ///   `contrast.min`, an unordered set cannot say which one won.
    ///
    /// The role is taken from the FILE NAME, never from position in
    /// [`A11y::layers`], which holds only the files that exist — when the shared
    /// file is absent, the app override is at index 0.
    ///
    /// Both point at `urn:file:` IRIs: the same ones this crate names its golden
    /// threads after, so the graph shows what would invalidate the answer as
    /// well as what produced it.
    pub fn to_turtle(&self, subject: &str, app: Option<&str>) -> String {
        let mut out = String::new();
        out.push_str("@prefix ik: <https://ikigai-rs.dev/ns#> .\n");
        out.push_str("@prefix prov: <http://www.w3.org/ns/prov#> .\n");
        out.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n");
        out.push_str(&format!("<{subject}> a ik:AccessibilityConfig ;\n"));
        if let Some(app) = app {
            out.push_str(&format!("    ik:app \"{}\" ;\n", escape(app)));
        }
        out.push_str(&format!(
            "    ik:themeLight \"{}\" ;\n",
            escape(&self.theme.light)
        ));
        out.push_str(&format!(
            "    ik:themeDark \"{}\" ;\n",
            escape(&self.theme.dark)
        ));
        out.push_str(&format!(
            "    ik:contrastMin {} ;\n",
            decimal(self.contrast.min)
        ));
        out.push_str(&format!(
            "    ik:contrastMinLarge {} ;\n",
            decimal(self.contrast.min_large)
        ));
        if let Some(reduce) = self.motion.reduce {
            out.push_str(&format!("    ik:reduceMotion {reduce} ;\n"));
        }
        out.push_str(&format!(
            "    ik:textScale {} ;\n",
            decimal(self.text.scale)
        ));
        out.push_str(&format!(
            "    ik:underlineLinks {} ",
            self.text.underline_links
        ));
        for layer in &self.layers {
            let iri = file_iri(layer);
            out.push_str(&format!(";\n    prov:wasDerivedFrom <{iri}> "));
            out.push_str(&format!(";\n    {} <{iri}> ", layer_role(layer)));
        }
        out.push_str(".\n");
        out
    }
}

/// The `urn:file:` IRI naming a config file — also the golden thread cut when it
/// changes (see [`crate::load::threads`]).
pub fn file_iri(path: &std::path::Path) -> String {
    format!("urn:file:{}", path.display())
}

/// Which role a contributing file plays, as the property naming it in the Turtle
/// face: `ik:sharedLayer` for the machine-wide `a11y.toml`, `ik:appLayer` for a
/// `{app}.a11y.toml` override.
///
/// Decided by FILE NAME, not by position. [`A11y::layers`] holds only the files
/// that exist, so when the shared file is absent the app override sits at index
/// 0 and an index-based rule would mislabel it — silently, and in the one field
/// whose entire job is explaining precedence.
fn layer_role(path: &std::path::Path) -> &'static str {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) if name == STEM => "ik:sharedLayer",
        _ => "ik:appLayer",
    }
}

/// A theme name in its canonical spelling; unknown names are left as written,
/// because [`Patch::validate`] has already refused them and leaving the original
/// keeps any error text honest.
fn canonical(name: &str) -> String {
    canonical_theme(name).unwrap_or(name).to_string()
}

/// A float as a Turtle `xsd:decimal` literal — which needs a decimal point, or
/// `3` would be read back as an `xsd:integer`.
fn decimal(value: f64) -> String {
    let s = format!("{value}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Escape a Turtle short-string literal.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The set of theme identifiers, for an `ArgSpec`'s `one_of`.
pub fn theme_ids() -> BTreeSet<&'static str> {
    THEMES.iter().map(|(id, _)| *id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE property the layering exists for. The shared file sets a floor and a
    /// dark theme; the app file overrides only the theme. Wholesale replacement
    /// would silently drop the operator's floor back to the default — which is
    /// the bug this test exists to prevent, not merely to describe.
    #[test]
    fn merge_preserves_a_floor_the_upper_layer_never_mentions() {
        let shared =
            Patch::parse("[contrast]\nmin = 7.0\n\n[theme]\ndark = \"Nord\"\n", None).unwrap();
        let app = Patch::parse("[theme]\ndark = \"Dracula\"\n", None).unwrap();

        let effective = A11y::merged([&shared, &app]);
        assert_eq!(effective.theme.dark, "Dracula", "the upper layer wins");
        assert_eq!(effective.contrast.min, 7.0, "the floor SURVIVES");
        assert_eq!(
            effective.theme.light, DEFAULT_LIGHT,
            "a key no layer states falls through to the default"
        );
        assert_eq!(effective.contrast.min_large, DEFAULT_MIN_LARGE);
    }

    /// Merging is per-KEY inside a section too, not per-section: an app that
    /// overrides `contrast.min` keeps the shared `contrast.min_large`.
    #[test]
    fn merge_is_key_wise_within_a_section_as_well_as_across_them() {
        let shared = Patch::parse("[contrast]\nmin = 7.0\nmin_large = 4.5\n", None).unwrap();
        let app = Patch::parse("[contrast]\nmin = 4.5\n", None).unwrap();
        let effective = A11y::merged([&shared, &app]);
        assert_eq!(effective.contrast.min, 4.5);
        assert_eq!(effective.contrast.min_large, 4.5, "sibling key survives");
    }

    #[test]
    fn an_unknown_theme_name_fails_loudly_and_names_the_field() {
        let err = Patch::parse("[theme]\ndark = \"Base16OceanDrak\"\n", None).unwrap_err();
        assert_eq!(
            err,
            ConfigError::UnknownTheme {
                field: "theme.dark",
                name: "Base16OceanDrak".to_string(),
            }
        );
        // …and the message hands the operator the accepted list.
        let text = err.to_string();
        assert!(text.contains("Base16OceanDark"), "{text}");
    }

    /// A layer's own bad value is refused even when a later layer overrides it —
    /// otherwise the typo silently reaches every OTHER app that reads the shared
    /// file.
    #[test]
    fn a_lower_layer_s_bad_theme_is_refused_even_if_overridden_later() {
        assert!(Patch::parse("[theme]\ndark = \"Nope\"\n", None).is_err());
    }

    #[test]
    fn either_spelling_of_a_theme_name_is_accepted_and_canonicalized() {
        let patch = Patch::parse("[theme]\ndark = \"base16-ocean.dark\"\n", None).unwrap();
        let effective = A11y::merged([&patch]);
        assert_eq!(effective.theme.dark, "Base16OceanDark");
        assert_eq!(canonical_theme("Solarized (dark)"), Some("SolarizedDark"));
        assert_eq!(
            canonical_theme("solarized (dark)"),
            None,
            "exact, not fuzzy"
        );
    }

    /// A misspelled section or key is as loud as a misspelled theme. Silently
    /// ignoring `[contast]` is how an operator ends up believing a floor is set.
    #[test]
    fn an_unknown_key_or_section_fails_loudly() {
        for bad in [
            "[contast]\nmin = 7.0\n",
            "[contrast]\nminimum = 7.0\n",
            "[theme]\nlight = \"Nord\"\nmedium = \"Nord\"\n",
        ] {
            let err = Patch::parse(bad, Some(PathBuf::from("/cfg/a11y.toml"))).unwrap_err();
            assert!(
                matches!(err, ConfigError::Parse { .. }),
                "{bad:?} → {err:?}"
            );
            assert!(err.to_string().contains("/cfg/a11y.toml"), "names the file");
        }
    }

    #[test]
    fn out_of_range_numbers_are_refused() {
        for (text, field) in [
            ("[contrast]\nmin = 0.5\n", "contrast.min"),
            ("[contrast]\nmin = 22.0\n", "contrast.min"),
            ("[contrast]\nmin_large = 0.0\n", "contrast.min_large"),
            ("[text]\nscale = 0.0\n", "text.scale"),
            ("[text]\nscale = 12.0\n", "text.scale"),
            ("[text]\nscale = nan\n", "text.scale"),
        ] {
            match Patch::parse(text, None) {
                Err(ConfigError::OutOfRange { field: f, .. }) => assert_eq!(f, field),
                other => panic!("{text:?} → {other:?}"),
            }
        }
    }

    /// `reduce` absent means UNSTATED — the front end defers to the OS — and is
    /// distinguishable from an explicit `false`.
    #[test]
    fn motion_reduce_distinguishes_unstated_from_explicitly_off() {
        assert_eq!(A11y::default().motion.reduce, None);
        let off = A11y::merged([&Patch::parse("[motion]\nreduce = false\n", None).unwrap()]);
        assert_eq!(off.motion.reduce, Some(false));
        let on = A11y::merged([&Patch::parse("[motion]\nreduce = true\n", None).unwrap()]);
        assert_eq!(on.motion.reduce, Some(true));
        // An empty section states nothing.
        let quiet = A11y::merged([&Patch::parse("[motion]\n", None).unwrap()]);
        assert_eq!(quiet.motion.reduce, None);
    }

    #[test]
    fn the_toml_face_round_trips_through_the_parser() {
        let mut effective = A11y::merged([&Patch::parse(
            "[contrast]\nmin = 7.0\n[motion]\nreduce = true\n[text]\nscale = 1.25\n",
            None,
        )
        .unwrap()]);
        effective.layers = vec![PathBuf::from("/cfg/ikigai/a11y.toml")];
        let text = effective.to_toml();
        let reparsed = A11y::merged([&Patch::parse(&text, None).unwrap()]);
        assert_eq!(reparsed.contrast, effective.contrast);
        assert_eq!(reparsed.motion, effective.motion);
        assert_eq!(reparsed.text, effective.text);
        assert_eq!(reparsed.theme, effective.theme);
        assert!(!text.contains("layers"), "provenance is not config: {text}");
    }

    #[test]
    fn the_json_face_carries_the_provenance_the_toml_face_omits() {
        let effective = A11y {
            layers: vec![PathBuf::from("/cfg/ikigai/a11y.toml")],
            ..A11y::default()
        };
        let json: serde_json::Value = serde_json::from_str(&effective.to_json()).unwrap();
        assert_eq!(json["theme"]["light"], DEFAULT_LIGHT);
        assert_eq!(json["contrast"]["min"], 4.5);
        assert_eq!(json["layers"][0], "/cfg/ikigai/a11y.toml");
        assert!(json.get("motion").is_some());
        assert!(
            json["motion"].get("reduce").is_none(),
            "unstated stays unstated"
        );
    }

    #[test]
    fn the_turtle_face_is_skolemized_and_types_its_literals() {
        let mut effective =
            A11y::merged([
                &Patch::parse("[contrast]\nmin = 3.0\n[motion]\nreduce = true\n", None).unwrap(),
            ]);
        effective.layers = vec![PathBuf::from("/cfg/ikigai/a11y.toml")];
        let ttl = effective.to_turtle("urn:a11y:config:cms-web", Some("cms-web"));
        assert!(!ttl.contains("_:"), "no blank nodes: {ttl}");
        assert!(ttl.contains("<urn:a11y:config:cms-web> a ik:AccessibilityConfig"));
        assert!(ttl.contains("ik:app \"cms-web\""));
        assert!(
            ttl.contains("ik:contrastMin 3.0"),
            "a decimal keeps its point: {ttl}"
        );
        assert!(ttl.contains("ik:reduceMotion true"));
        assert!(ttl.contains("@prefix prov: <http://www.w3.org/ns/prov#> ."));
        // Named twice: the standard fact for any PROV reader, and the role that
        // carries precedence RDF's unordered triples otherwise lose.
        assert!(ttl.contains("prov:wasDerivedFrom <urn:file:/cfg/ikigai/a11y.toml>"));
        assert!(ttl.contains("ik:sharedLayer <urn:file:/cfg/ikigai/a11y.toml>"));
        assert!(ttl.trim_end().ends_with('.'));
        // An unstated preference emits no triple rather than a false one.
        let quiet = A11y::default().to_turtle("urn:a11y:config", None);
        assert!(!quiet.contains("ik:reduceMotion"), "{quiet}");
        assert!(!quiet.contains("ik:app"), "{quiet}");
    }

    /// The layer ROLE comes from the file name, never from position — the whole
    /// reason `layer_role` exists. `layers` holds only the files that EXIST, so
    /// when the machine-wide file is absent the app override sits at index 0,
    /// and an index-based rule would publish it as the shared layer: a wrong
    /// answer in the one field whose entire job is explaining precedence.
    #[test]
    fn the_layer_role_follows_the_file_name_not_the_position() {
        // App override alone, at index 0.
        let only_app = A11y {
            layers: vec![PathBuf::from("/cfg/ikigai/cms-web.a11y.toml")],
            ..Default::default()
        };
        let ttl = only_app.to_turtle("urn:a11y:config:cms-web", Some("cms-web"));
        assert!(
            ttl.contains("ik:appLayer <urn:file:/cfg/ikigai/cms-web.a11y.toml>"),
            "an override at index 0 is still the app layer: {ttl}"
        );
        assert!(
            !ttl.contains("ik:sharedLayer"),
            "there is no shared layer to claim: {ttl}"
        );

        // Both layers: each takes its own role, and each is also stated the
        // standard way for a reader that knows PROV and not this vocabulary.
        let both = A11y {
            layers: vec![
                PathBuf::from("/cfg/ikigai/a11y.toml"),
                PathBuf::from("/cfg/ikigai/cms-web.a11y.toml"),
            ],
            ..Default::default()
        };
        let ttl = both.to_turtle("urn:a11y:config:cms-web", Some("cms-web"));
        assert!(
            ttl.contains("ik:sharedLayer <urn:file:/cfg/ikigai/a11y.toml>"),
            "{ttl}"
        );
        assert!(
            ttl.contains("ik:appLayer <urn:file:/cfg/ikigai/cms-web.a11y.toml>"),
            "{ttl}"
        );
        assert_eq!(
            ttl.matches("prov:wasDerivedFrom").count(),
            2,
            "every contributing file is derived-from, whatever its role: {ttl}"
        );
    }

    #[test]
    fn an_empty_layer_changes_nothing() {
        assert_eq!(
            A11y::merged([&Patch::parse("", None).unwrap()]),
            A11y::default()
        );
    }
}
