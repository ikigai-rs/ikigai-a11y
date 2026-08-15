//! `ikigai-a11y` — accessibility as a shared, layered, resolvable capability.
//!
//! One crate that answers *"what are this machine's accessibility settings, for
//! this application?"* — and carries the contrast machinery every ikigai server
//! and control plane can reuse rather than re-deriving per front end.
//!
//! ```text
//! source urn:a11y:config:cms-web              # the EFFECTIVE merged config
//! source urn:a11y:config as=text/turtle       # the skolemized graph face
//! source urn:a11y:contrast from=#323232 on=#2b303b
//! → 1.03 fail
//! ```
//!
//! ## The three pieces
//!
//! - [`color`] — WCAG relative luminance and contrast ratio. Pure, wasm-clean,
//!   alpha-composited.
//! - [`config`] — the schema and the **key-wise** layering: built-in defaults ⊕
//!   `a11y.toml` ⊕ `{app}.a11y.toml`. A layer states only its differences, and
//!   every key it stays silent about survives from below.
//! - [`css`] — the contrast-floor pass: lift a generated stylesheet's sub-floor
//!   colours to the theme's own default foreground, repairing in-palette and
//!   inventing nothing.
//!
//! [`load`] (native) reads the layered files from `ikigai_core::config`'s config
//! home; [`themes`] (feature `themes`) bridges a configured theme name to a real
//! `syntect` theme. Everything else compiles to `wasm32-unknown-unknown`.
//!
//! ## `app` is the PROCESS's name
//!
//! `cms-web`, `dev-server`, `web` — the binary, not the module. A module linked
//! into three servers reads three different effective configs, which is the
//! point: the operator's override is about the front end a person is looking at.
//!
//! ## Loud, never lenient
//!
//! An unknown theme name, an out-of-range number and an unknown key are hard
//! errors at load. A misspelled theme that silently rendered the default would
//! leave the operator believing they had changed something.
//!
//! ## Cacheability
//!
//! `urn:a11y:config` is `.cacheable()` with a golden thread on every candidate
//! file — **not** uncacheable-because-it-reads-a-file. Effective expiry
//! propagates from dependencies, so an uncacheable config would silently
//! un-cache every stylesheet that joined it; the same shape was measured at
//! ~2000× on a hot path elsewhere in this ecosystem. A host that watches the
//! config home and cuts the threads from [`load::threads`] gets the good version
//! instead: edit `a11y.toml`, derived stylesheets recompute, and nothing polls.

#![deny(missing_docs)]

pub mod color;
pub mod config;
pub mod css;
mod endpoints;
#[cfg(not(target_family = "wasm"))]
pub mod load;
#[cfg(feature = "themes")]
pub mod themes;

pub use color::{ratio, ParseColorError, Rgba};
pub use config::{
    canonical_theme, A11y, ConfigError, Contrast, Motion, Patch, Text, Theme, THEMES,
};
pub use css::{apply_floor, FloorPass, Lift};
#[cfg(not(target_family = "wasm"))]
pub use endpoints::{config, effective, CONFIG_IRI, CONFIG_TEMPLATE};
pub use endpoints::{configurable_themes, contrast, space, CAP_READ, CONTRAST_IRI};
