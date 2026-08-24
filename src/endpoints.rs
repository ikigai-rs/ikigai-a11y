//! The `urn:a11y:*` endpoints.
//!
//! ## Three actions, one capability between them
//!
//! `urn:a11y:config` requires — and enforces — [`CAP_READ`]. It is tempting to
//! call theme preferences public, but this config also states whether the person
//! at the keyboard needs reduced motion and larger text, and that is
//! assistive-technology information about a human being, not decoration. It is
//! gated for the same reason calendar detail is.
//!
//! `urn:a11y:presentation` requires **nothing**, and serves the rendering half
//! of the same config: themes, contrast floors, link underlining — the
//! deployment's posture about how a page is drawn, with everything about the
//! reader withheld. It is this module's `urn:personal:availability`: a separate
//! IRI serving the minimized view at the lower authority, so the caller that
//! only needs to draw something never has to be handed the rest.
//!
//! `urn:a11y:contrast` requires **nothing**, because it computes a WCAG ratio
//! from two colours the caller already holds. Declaring a capability it does not
//! need would make the manifold under-offer — an agent holding no a11y grant
//! would be told it cannot do arithmetic.
//!
//! ## Why the minimized view is a second IRI, not a face
//!
//! `as=` cannot carry it. An action whose capability requirement depends on
//! WHICH arguments arrive cannot be described: `requires` is per-action, so the
//! manifold would have to declare the union (over-demanding for the ungated
//! call) or the intersection (a lie for the gated one). The same constraint is
//! why `urn:a11y:contrast` takes an explicit `min` rather than reading the
//! configured floor itself — a caller wanting the deployment's floor sources it
//! and passes it in.
//!
//! So authority that differs becomes an action that differs. The alternative —
//! one IRI declaring no capability and silently redacting for callers who hold
//! none — trades a name a caller can see for a difference they cannot, on a
//! resource whose entire job is to be authoritative.
//!
//! ## What the capability protects
//!
//! The **resource**, not the files. A capability is checked when the kernel
//! resolves an IRI; nothing checks one when a linked library reads `a11y.toml`
//! with `std::fs`, and nothing could. That is the shape of the thing rather than
//! a hole: the gate is the only fence that exists for an agent's manifold, a peer
//! across a transport, or an MCP projection, and in-process the host is already
//! reading this person's home directory.
//!
//! Which is why [`crate::load`] offers the split too — `presentation` for a
//! consumer deriving an artifact, `complete` for whatever is entitled to the
//! whole thing. The gate stays meaningful because the ungated path leads
//! somewhere harmless, not because it has been closed.
//!
//! ## Cacheability
//!
//! Both are `.cacheable()`. The config is a pure function of files whose changes
//! it declares as golden threads, so a cut recomputes it and nothing else has to
//! poll. Marking it uncacheable — the lazy reading of "it touches a file" —
//! would be the expensive mistake: expiry propagates, so every stylesheet that
//! joined it would silently stop being cached too.

#[cfg(not(target_family = "wasm"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_family = "wasm"))]
use std::sync::Arc;

#[cfg(not(target_family = "wasm"))]
use ikigai_core::UriTemplate;
use ikigai_core::{
    ArgSpec, Description, EndpointSpace, Error, Exact, FnEndpoint, Invocation, ReprType,
    Representation, Result, Verb,
};

use crate::color::{ratio, Rgba};
use crate::config::theme_ids;
#[cfg(not(target_family = "wasm"))]
use crate::config::{A11y, ConfigError, Presentation};

/// The capability `urn:a11y:config` requires and enforces.
pub const CAP_READ: &str = "urn:cap:a11y:read";

// Gated with the endpoints they name: on wasm there is no config resource to
// point at, and a constant naming one would be an offer the host cannot keep.
/// The shared effective config.
#[cfg(not(target_family = "wasm"))]
pub const CONFIG_IRI: &str = "urn:a11y:config";
/// The per-application effective config.
#[cfg(not(target_family = "wasm"))]
pub const CONFIG_TEMPLATE: &str = "urn:a11y:config:{app}";
/// The shared rendering half — ungated.
#[cfg(not(target_family = "wasm"))]
pub const PRESENTATION_IRI: &str = "urn:a11y:presentation";
/// The per-application rendering half — ungated.
#[cfg(not(target_family = "wasm"))]
pub const PRESENTATION_TEMPLATE: &str = "urn:a11y:presentation:{app}";
/// The WCAG contrast calculator.
pub const CONTRAST_IRI: &str = "urn:a11y:contrast";

const TEXT_PLAIN: &str = "text/plain;charset=utf-8";
const JSON: &str = "application/json";
const TURTLE: &str = "text/turtle";
const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";

fn plain() -> ReprType {
    ReprType::new("text/plain").with_param("charset", "utf-8")
}

/// A config failure as a kernel error. Every one of these is **permanent** — a
/// misspelled theme does not become correct on retry — so none of them are
/// `Unavailable`.
#[cfg(not(target_family = "wasm"))]
fn config_error(e: ConfigError) -> Error {
    Error::Endpoint(e.to_string())
}

/// This mount's accessibility config: the **config home** it layers within, and
/// the application whose override layer applies when the IRI names none.
///
/// ## Why the endpoints need one
///
/// `config_home()` reads `$XDG_CONFIG_HOME` and `$HOME`, and both are
/// process-global. An endpoint body that called it read the *developer's* real
/// `~/.config/ikigai/a11y.toml` under `cargo test`, could not be handed a
/// different one (`set_var` races the harness's own threads), and gave two tests
/// in one binary no way to disagree — `cargo test` shares a process across a
/// crate's tests. The measurable consequence was a suite that passed identically
/// against three different configs including a **corrupt** one, because not one
/// endpoint test owned the file its values came from.
///
/// So the home is taken here, once, by whoever mounts this space — a fact about
/// the mount rather than a hidden input to a resolution. See
/// `ikigai-core/docs/design/hermetic-endpoint-tests.md`; `ikigai-log`'s
/// `LogHandle` is the reference implementation.
///
/// ## ★ It holds the home, NOT a parsed config
///
/// This is the one place the shape deliberately differs from `LogHandle`, which
/// carries a parsed `LogConfig`. `urn:a11y:config` is `.cacheable()` with a
/// golden thread on every candidate file, and the contract of that thread is
/// that cutting it **recomputes** the answer. A handle that cached an [`A11y`]
/// at construction would serve the config the process started with forever: the
/// watcher would cut, the kernel would re-resolve, and the endpoint would hand
/// back the same stale struct. The layering is cheap and the cache is the thing
/// that makes it cheap to repeat, so the read stays per-resolution and the
/// *home* is what gets held.
///
/// The log's config is process state that a Sink mutates in place, which is why
/// holding it parsed is right there and wrong here.
#[cfg(not(target_family = "wasm"))]
pub struct A11yHandle {
    home: Option<PathBuf>,
    app: Option<String>,
}

#[cfg(not(target_family = "wasm"))]
impl A11yHandle {
    /// A handle over a stated config home.
    ///
    /// `Option<PathBuf>`, not a fallible constructor: a process with no config
    /// home is under-configured, not broken, and that is `config_home()`'s own
    /// `None`-rather-than-a-guess contract carried up one level. The absence
    /// becomes an error only when someone actually resolves a config IRI, where
    /// it is `ConfigError::NoConfigHome` and says exactly what is missing.
    ///
    /// `app` is this mount's default application layer, per
    /// `ambient-app-name.md`: optional, never guessed, and taken at mount time.
    /// A `{app}` binding in the IRI still wins over it — see [`Self::layer`].
    pub fn new(home: Option<PathBuf>, app: Option<String>) -> A11yHandle {
        A11yHandle {
            home,
            app: app.filter(|a| !a.is_empty()),
        }
    }

    /// A handle over **this machine's** config home — the sugar for a host
    /// configuring itself from the environment it is running in.
    ///
    /// Sugar over [`new`](Self::new), never a second code path: the ambient read
    /// happens here, once, and everything after it is the injected form with a
    /// different argument.
    ///
    /// Infallible, unlike `LogHandle::ambient`, because nothing is parsed at
    /// construction — an unreadable layer file surfaces at the resolution that
    /// reads it, not at mount time.
    pub fn ambient(app: Option<String>) -> A11yHandle {
        A11yHandle::new(ikigai_core::config::config_home(), app)
    }

    /// The config home this handle layers within, or `None` if this process has
    /// one that could not be determined.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// This mount's default application layer.
    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    /// The application layer that applies to a request: the `{app}` captured
    /// from the IRI if there was one, else this mount's own.
    ///
    /// The binding wins because it names a different RESOURCE —
    /// `urn:a11y:config:cms-web` is not `urn:a11y:config` under a default, and a
    /// mount whose default silently overrode the name in the IRI would serve one
    /// resource's config under another's identity. This is the "explicit
    /// argument wins over the ambient value" rule of `ambient-app-name.md`, with
    /// the mount as the ambient side.
    pub fn layer<'a>(&'a self, bound: Option<&'a str>) -> Option<&'a str> {
        bound.filter(|a| !a.is_empty()).or_else(|| self.app())
    }

    fn rooted(&self) -> std::result::Result<&Path, ConfigError> {
        self.home.as_deref().ok_or(ConfigError::NoConfigHome)
    }

    /// The whole effective config for `bound` (or this mount's app), read from
    /// the held home. Person-facts included — see [`crate::load::complete`].
    pub fn complete(&self, bound: Option<&str>) -> std::result::Result<A11y, ConfigError> {
        crate::load::complete_in(self.rooted()?, self.layer(bound))
    }

    /// The rendering half of the same read — what a consumer deriving an
    /// artifact wants.
    pub fn presentation(
        &self,
        bound: Option<&str>,
    ) -> std::result::Result<Presentation, ConfigError> {
        crate::load::presentation_in(self.rooted()?, self.layer(bound))
    }

    /// The golden threads that read depends on: one per candidate file, whether
    /// or not it exists.
    ///
    /// **Thread NAMES, not paths** — `urn:file:/abs/path/a11y.toml`, as
    /// [`crate::load::threads_in`] spells out. A host wiring a watcher wants
    /// exactly these strings (a cut is keyed on the name); a host wanting the
    /// files to watch wants [`crate::load::paths_in`] over [`Self::home`].
    pub fn threads(&self, bound: Option<&str>) -> std::result::Result<Vec<String>, ConfigError> {
        Ok(crate::load::threads_in(self.rooted()?, self.layer(bound)))
    }
}

/// The requested face, defaulting to `text/plain`. An unrecognised `as` is an
/// error rather than a silent fallback: a caller that asked for JSON and got
/// prose would notice much later than the caller who asked wrong.
fn face(inv: &Invocation<'_>) -> Result<&'static str> {
    match inv.inline_str("as").map(str::trim) {
        Err(_) | Ok("") => Ok(TEXT_PLAIN),
        Ok(t) if t == JSON => Ok(JSON),
        Ok(t) if t == TURTLE => Ok(TURTLE),
        Ok(t) if t.starts_with("text/plain") => Ok(TEXT_PLAIN),
        Ok(other) => Err(Error::InvalidArgument {
            name: "as".to_string(),
            detail: format!("expected one of {TEXT_PLAIN}|{JSON}|{TURTLE}, got {other:?}"),
        }),
    }
}

#[cfg(not(target_family = "wasm"))]
fn config_impl(handle: &A11yHandle, inv: &Invocation<'_>) -> Result<Representation> {
    // Declared = enforced — but the kernel is what makes it so: it refuses a caller
    // without CAP_READ before dispatch and before any cache-serve (core 0.1.49
    // onward), so under a kernel this check never fires. It is the second line, and
    // it earns its keep on the paths where no kernel gate ran — a detached
    // invocation, a module shim, or (see `crate::load`) a linked library calling in
    // as plain Rust, where nothing resolves an IRI and nothing checks a capability.
    if !inv.capability.allows(CAP_READ) {
        return Err(Error::Denied(format!(
            "reading the accessibility config requires `{CAP_READ}`"
        )));
    }
    let app = inv.bindings.get("app");
    let effective = handle.complete(app).map_err(config_error)?;
    let subject = match app {
        Some(app) => format!("{CONFIG_IRI}:{app}"),
        None => CONFIG_IRI.to_string(),
    };
    let (repr_type, body) = match face(inv)? {
        JSON => (ReprType::new(JSON), effective.to_json()),
        TURTLE => (
            ReprType::new("text/turtle").with_param("charset", "utf-8"),
            effective.to_turtle(&subject, handle.layer(app)),
        ),
        _ => (plain(), effective.to_toml()),
    };
    // Cacheable, with a thread on every CANDIDATE file — including the ones that
    // do not exist, so creating an override invalidates this too.
    let mut repr = Representation::new(repr_type, body.into_bytes()).cacheable();
    for thread in handle.threads(app).map_err(config_error)? {
        repr = repr.depends_on(thread);
    }
    Ok(repr)
}

/// `urn:a11y:config` / `urn:a11y:config:{app}` over **this machine's** config
/// home.
///
/// Sugar for [`config_with`] over [`A11yHandle::ambient`], and the entry point
/// a host that is configuring itself from its own environment wants. A host
/// serving someone else's config home — or a test that owns the files it is
/// asserting about — builds the handle itself.
#[cfg(not(target_family = "wasm"))]
pub fn config() -> FnEndpoint {
    config_with(Arc::new(A11yHandle::ambient(None)))
}

/// `urn:a11y:config` / `urn:a11y:config:{app}` — the EFFECTIVE merged config,
/// layered within the handle's config home.
#[cfg(not(target_family = "wasm"))]
pub fn config_with(handle: Arc<A11yHandle>) -> FnEndpoint {
    FnEndpoint::new("a11yConfig", move |inv| config_impl(&handle, inv)).with_description(
        Description::new("a11yConfig")
            .title("Effective accessibility config")
            .summary(
                "The merged accessibility settings for this machine and, with `{app}`, for one \
                 application: built-in defaults ⊕ a11y.toml ⊕ {app}.a11y.toml, merged key-wise \
                 so a shared contrast floor survives an app that overrides only its theme. \
                 Serves the EFFECTIVE config, never a raw file. as=application/json or \
                 as=text/turtle for the machine faces; the Turtle is skolemized and links the \
                 files that contributed.",
            )
            .verb(Verb::Source)
            .verb(Verb::Meta)
            .requires(CAP_READ)
            .input(
                ArgSpec::new("app")
                    .summary(
                        "the application whose override layer applies (the PROCESS's name: \
                         cms-web, dev-server, web), captured from the IRI",
                    )
                    .class(XSD_STRING)
                    .binding()
                    .optional(),
            )
            .input(
                ArgSpec::new("as")
                    .summary("the face: TOML by default, or JSON, or the skolemized graph")
                    .class(XSD_STRING)
                    .one_of([TEXT_PLAIN, JSON, TURTLE])
                    .default_value(TEXT_PLAIN),
            )
            .output(TEXT_PLAIN)
            .output(JSON)
            .output(TURTLE),
    )
}

#[cfg(not(target_family = "wasm"))]
fn presentation_impl(handle: &A11yHandle, inv: &Invocation<'_>) -> Result<Representation> {
    // No capability check, and that is the contract: this action declares none,
    // and what it can reach is bounded by the projection rather than by a gate.
    let app = inv.bindings.get("app");
    let rendering = handle.presentation(app).map_err(config_error)?;
    let subject = match app {
        Some(app) => format!("{PRESENTATION_IRI}:{app}"),
        None => PRESENTATION_IRI.to_string(),
    };
    let (repr_type, body) = match face(inv)? {
        JSON => (ReprType::new(JSON), rendering.to_json()),
        TURTLE => (
            ReprType::new("text/turtle").with_param("charset", "utf-8"),
            rendering.to_turtle(&subject, handle.layer(app)),
        ),
        _ => (plain(), rendering.to_toml()),
    };
    // Same files, same threads as the gated resource: the two views are cached
    // separately and invalidated together.
    let mut repr = Representation::new(repr_type, body.into_bytes()).cacheable();
    for thread in handle.threads(app).map_err(config_error)? {
        repr = repr.depends_on(thread);
    }
    Ok(repr)
}

/// `urn:a11y:presentation` / `urn:a11y:presentation:{app}` over **this
/// machine's** config home — sugar for [`presentation_with`] over
/// [`A11yHandle::ambient`].
#[cfg(not(target_family = "wasm"))]
pub fn presentation() -> FnEndpoint {
    presentation_with(Arc::new(A11yHandle::ambient(None)))
}

/// `urn:a11y:presentation` / `urn:a11y:presentation:{app}` — the rendering half
/// of the effective config, ungated, layered within the handle's config home.
#[cfg(not(target_family = "wasm"))]
pub fn presentation_with(handle: Arc<A11yHandle>) -> FnEndpoint {
    FnEndpoint::new("a11yPresentation", move |inv| {
        presentation_impl(&handle, inv)
    })
    .with_description(
        Description::new("a11yPresentation")
            .title("Effective accessibility config: the rendering half")
            .summary(
                "How this deployment draws a page — the configured light and dark themes, both \
                 WCAG contrast floors, and whether links are underlined — merged over the same \
                 layers as urn:a11y:config (defaults ⊕ a11y.toml ⊕ {app}.a11y.toml). Requires no \
                 capability, and states NOTHING about the person reading: reduced-motion and \
                 text-scale preferences are assistive-technology facts about a human being and \
                 live on urn:a11y:config, behind urn:cap:a11y:read. This is what a consumer \
                 deriving a stylesheet or a palette should read. as=application/json or \
                 as=text/turtle for the machine faces.",
            )
            .verb(Verb::Source)
            .verb(Verb::Meta)
            .input(
                ArgSpec::new("app")
                    .summary(
                        "the application whose override layer applies (the PROCESS's name: \
                         cms-web, dev-server, web), captured from the IRI",
                    )
                    .class(XSD_STRING)
                    .binding()
                    .optional(),
            )
            .input(
                ArgSpec::new("as")
                    .summary("the face: TOML by default, or JSON, or the skolemized graph")
                    .class(XSD_STRING)
                    .one_of([TEXT_PLAIN, JSON, TURTLE])
                    .default_value(TEXT_PLAIN),
            )
            .output(TEXT_PLAIN)
            .output(JSON)
            .output(TURTLE),
    )
}

fn contrast_impl(inv: &Invocation<'_>) -> Result<Representation> {
    let colour = |name: &str| -> Result<Rgba> {
        let raw = inv.inline_str(name)?;
        Rgba::parse(raw).map_err(|e| Error::InvalidArgument {
            name: name.to_string(),
            detail: e.to_string(),
        })
    };
    let from = colour("from")?;
    let on = colour("on")?;
    let min = match inv.inline_str("min").map(str::trim) {
        Err(_) | Ok("") => crate::config::DEFAULT_MIN,
        Ok(raw) => raw.parse::<f64>().map_err(|_| Error::InvalidArgument {
            name: "min".to_string(),
            detail: format!("expected a contrast ratio, got {raw:?}"),
        })?,
    };
    let achieved = ratio(from, on);
    let passes = achieved >= min;
    let (repr_type, body) = match face(inv)? {
        JSON => (
            ReprType::new(JSON),
            serde_json::json!({
                "from": from.to_css(),
                "on": on.to_css(),
                "ratio": (achieved * 100.0).round() / 100.0,
                "min": min,
                "passes": passes,
            })
            .to_string(),
        ),
        TURTLE => {
            return Err(Error::InvalidArgument {
                name: "as".to_string(),
                detail: format!("{CONTRAST_IRI} has no graph face; use {JSON}"),
            })
        }
        // Ratio first so the value pipes straight into `urn:text:*`, verdict
        // second so a human reading the REPL does not have to do the comparison.
        _ => (
            plain(),
            format!("{:.2} {}", achieved, if passes { "pass" } else { "fail" }),
        ),
    };
    // A pure function of its arguments: permanently cacheable, no threads, and
    // no capability — see the module docs.
    Ok(Representation::new(repr_type, body.into_bytes()).cacheable())
}

/// `urn:a11y:contrast` — the WCAG ratio between two colours and whether it
/// clears a floor.
pub fn contrast() -> FnEndpoint {
    FnEndpoint::new("a11yContrast", contrast_impl).with_description(
        Description::new("a11yContrast")
            .title("WCAG contrast ratio")
            .summary(
                "The WCAG 2.x contrast ratio between two colours (1.0–21.0) and whether it \
                 clears a floor: `<ratio> <pass|fail>`, or as=application/json for the fields. \
                 A translucent `from` is composited over `on` first, because apparent contrast \
                 is what a reader gets. `min` defaults to 4.5 (AA body text); pass the \
                 deployment's own floor from urn:a11y:config to judge against that instead.",
            )
            .verb(Verb::Source)
            .verb(Verb::Meta)
            .input(
                ArgSpec::new("from")
                    .summary("the foreground colour, as #rgb, #rgba, #rrggbb or #rrggbbaa")
                    .class(XSD_STRING),
            )
            .input(
                ArgSpec::new("on")
                    .summary("the background colour it sits on")
                    .class(XSD_STRING),
            )
            .input(
                ArgSpec::new("min")
                    .summary("the floor to judge against (4.5 = AA body text, 3.0 = AA large, 7.0 = AAA)")
                    .class(XSD_DECIMAL)
                    .default_value("4.5"),
            )
            .input(
                ArgSpec::new("as")
                    .summary("the face: `<ratio> <pass|fail>` by default, or JSON")
                    .class(XSD_STRING)
                    .one_of([TEXT_PLAIN, JSON])
                    .default_value(TEXT_PLAIN),
            )
            .output(TEXT_PLAIN)
            .output(JSON),
    )
}

/// The module's space over **this machine's** config home: `urn:a11y:contrast`
/// everywhere, plus the config endpoints on hosts that have a filesystem.
///
/// On wasm the config endpoints are absent rather than present-and-failing: an
/// action in the manifold that cannot succeed is worse than one that is not
/// offered, since an agent will select it.
pub fn space() -> EndpointSpace {
    #[cfg(target_family = "wasm")]
    {
        EndpointSpace::new().bind(Exact::new(CONTRAST_IRI), contrast())
    }
    #[cfg(not(target_family = "wasm"))]
    {
        space_with(Arc::new(A11yHandle::ambient(None)))
    }
}

/// The module's space over a config home the caller states.
///
/// This is what a host serving a config home other than its own environment's
/// mounts — and what a test mounts, so that what the endpoints read is a
/// directory the test wrote rather than the developer's `~/.config/ikigai`.
///
/// One handle for all four bindings: the two views of the same files are
/// separate resources, but they must never be able to disagree about which
/// files those are.
#[cfg(not(target_family = "wasm"))]
pub fn space_with(handle: Arc<A11yHandle>) -> EndpointSpace {
    EndpointSpace::new()
        .bind(Exact::new(CONTRAST_IRI), contrast())
        .bind(Exact::new(CONFIG_IRI), config_with(handle.clone()))
        .bind(
            UriTemplate::parse(CONFIG_TEMPLATE).expect("CONFIG_TEMPLATE is a valid template"),
            config_with(handle.clone()),
        )
        .bind(
            Exact::new(PRESENTATION_IRI),
            presentation_with(handle.clone()),
        )
        .bind(
            UriTemplate::parse(PRESENTATION_TEMPLATE)
                .expect("PRESENTATION_TEMPLATE is a valid template"),
            presentation_with(handle),
        )
}

/// The theme identifiers an `a11y.toml` may name — exposed so a host building a
/// picker does not re-derive the list.
pub fn configurable_themes() -> Vec<&'static str> {
    theme_ids().into_iter().collect()
}

/// The effective config as a value, for a host that wants the struct rather than
/// a representation.
#[cfg(not(target_family = "wasm"))]
#[deprecated(
    since = "0.2.0",
    note = "one door per view, and each named for what it hands over: \
            `load::presentation` for the rendering half, `load::complete` for the whole config"
)]
pub fn effective(app: Option<&str>) -> Result<A11y> {
    crate::load::complete(app).map_err(config_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use ikigai_core::{ArgRef, Bindings, Capability, Endpoint, Iri, Request};

    fn iri(s: &str) -> Iri {
        Iri::parse(s).expect("a valid IRI")
    }

    fn invoke(ep: &FnEndpoint, request: Request, cap: &Capability) -> Result<Representation> {
        let bindings = Bindings::default();
        let inv = Invocation::detached(&request, &bindings, cap);
        block_on(ep.invoke(&inv))
    }

    fn contrast_request(from: &str, on: &str) -> Request {
        Request::new(Verb::Source, iri(CONTRAST_IRI))
            .with_arg("from", ArgRef::Inline(from.as_bytes().to_vec()))
            .with_arg("on", ArgRef::Inline(on.as_bytes().to_vec()))
    }

    #[test]
    fn contrast_reports_the_ratio_and_the_verdict() {
        let cap = Capability::root();
        let rep = invoke(&contrast(), contrast_request("#000000", "#ffffff"), &cap).unwrap();
        assert_eq!(String::from_utf8_lossy(&rep.bytes), "21.00 pass");

        let rep = invoke(&contrast(), contrast_request("#323232", "#2b303b"), &cap).unwrap();
        assert_eq!(String::from_utf8_lossy(&rep.bytes), "1.03 fail");
    }

    /// Pure arithmetic needs no authority — an agent holding nothing at all can
    /// still ask.
    #[test]
    fn contrast_needs_no_capability() {
        let nothing = Capability::scoped(Vec::<String>::new());
        let rep = invoke(
            &contrast(),
            contrast_request("#000000", "#ffffff"),
            &nothing,
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&rep.bytes).starts_with("21.00"));
        assert!(
            contrast().describe().requires.is_empty(),
            "and does not claim to"
        );
    }

    #[test]
    fn contrast_honours_an_explicit_floor_and_the_json_face() {
        let cap = Capability::root();
        let request = contrast_request("#767676", "#ffffff")
            .with_arg("min", ArgRef::Inline(b"3.0".to_vec()))
            .with_arg("as", ArgRef::Inline(JSON.as_bytes().to_vec()));
        let rep = invoke(&contrast(), request, &cap).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&rep.bytes).expect("the JSON face is JSON");
        assert_eq!(json["min"], 3.0);
        assert_eq!(json["passes"], true);
        assert_eq!(json["from"], "#767676");
        assert!(json["ratio"].as_f64().unwrap() > 4.5);
    }

    #[test]
    fn a_malformed_colour_or_floor_is_an_invalid_argument() {
        let cap = Capability::root();
        for request in [
            contrast_request("chartreuse", "#ffffff"),
            contrast_request("#000000", "not-a-colour"),
            contrast_request("#000000", "#ffffff")
                .with_arg("min", ArgRef::Inline(b"very".to_vec())),
        ] {
            assert!(
                matches!(
                    invoke(&contrast(), request, &cap),
                    Err(Error::InvalidArgument { .. })
                ),
                "a bad argument must not be guessed at"
            );
        }
        // A missing required colour is missing, not defaulted.
        let bare = Request::new(Verb::Source, iri(CONTRAST_IRI));
        assert!(matches!(
            invoke(&contrast(), bare, &cap),
            Err(Error::MissingArgument(_))
        ));
    }

    #[test]
    fn an_unknown_face_is_refused_rather_than_silently_downgraded() {
        let cap = Capability::root();
        let request = contrast_request("#000000", "#ffffff")
            .with_arg("as", ArgRef::Inline(b"text/html".to_vec()));
        assert!(matches!(
            invoke(&contrast(), request, &cap),
            Err(Error::InvalidArgument { .. })
        ));
    }

    /// Declared = enforced, in both directions: the description names the cap and
    /// an invocation without it is refused with the typed, permanent `Denied`.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_config_endpoint_declares_and_enforces_its_capability() {
        assert_eq!(config().describe().requires, vec![CAP_READ.to_string()]);
        let nothing = Capability::scoped(Vec::<String>::new());
        let request = Request::new(Verb::Source, iri(CONFIG_IRI));
        match invoke(&config(), request, &nothing) {
            Err(Error::Denied(message)) => assert!(message.contains(CAP_READ), "{message}"),
            other => panic!("{other:?}"),
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_manifold_declares_every_face_and_a_typed_theme_enum() {
        let d = config().describe();
        assert!(d.outputs.contains(&TURTLE.to_string()));
        assert!(d.outputs.contains(&JSON.to_string()));
        let app = d.inputs.iter().find(|i| i.name == "app").expect("app");
        assert_eq!(app.source, ikigai_core::InputSource::Binding);
        assert!(!app.required);
        let as_arg = d.inputs.iter().find(|i| i.name == "as").expect("as");
        assert_eq!(as_arg.default.as_deref(), Some(TEXT_PLAIN));
        assert_eq!(as_arg.one_of.len(), 3);
        // The theme list is offered whole, so a picker has one source.
        assert_eq!(configurable_themes().len(), crate::config::THEMES.len());
        assert!(configurable_themes().contains(&"Base16OceanDark"));
    }

    /// The space binds what a host will actually resolve — both spellings of
    /// both views, and the calculator.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_space_binds_every_identifier() {
        use ikigai_core::Space;
        for space in [
            space(),
            space_with(std::sync::Arc::new(A11yHandle::new(None, None))),
        ] {
            let entries = space.entries().expect("an EndpointSpace enumerates");
            let patterns: Vec<&str> = entries.iter().map(|e| e.pattern.as_str()).collect();
            for expected in [
                CONTRAST_IRI,
                CONFIG_IRI,
                CONFIG_TEMPLATE,
                PRESENTATION_IRI,
                PRESENTATION_TEMPLATE,
            ] {
                assert!(patterns.contains(&expected), "{expected}: {patterns:?}");
            }
        }
    }

    /// The config endpoints, against config homes these tests WRITE.
    ///
    /// Every test in here used to end in `else { return; }` or assert only that
    /// a field had the right type, because the endpoint read `$HOME` and no test
    /// owned the file its values came from. Measured on 2026-08-23, the same
    /// suite passed 58/58 against three different sandbox configs — including
    /// one whose `a11y.toml` did not parse. These assert values, against layers
    /// written three lines above them, and there is no machine that can skip
    /// them.
    #[cfg(not(target_family = "wasm"))]
    mod config_home {
        use super::*;
        use crate::config::file_iri;
        use crate::load::STEM;
        use crate::scratch::Scratch;
        use std::sync::Arc;

        /// The shared layer every fixture below starts from: a floor and a theme
        /// that are NOT the defaults (so a test asserting them cannot be passing
        /// on a default), plus both person-facts (so the ungated view has
        /// something real to withhold).
        const SHARED: &str = "[contrast]\nmin = 7.0\n\n[theme]\ndark = \"Nord\"\n\n\
                              [motion]\nreduce = true\n\n\
                              [text]\nscale = 1.75\nunderline_links = false\n";
        /// An app layer that overrides exactly one key.
        const APP_LAYER: &str = "[theme]\ndark = \"Dracula\"\n";

        fn seeded(tag: &str) -> Scratch {
            let home = Scratch::new(tag);
            home.write(STEM, SHARED);
            home
        }

        fn over(home: &Scratch) -> Arc<A11yHandle> {
            Arc::new(A11yHandle::new(Some(home.path().to_path_buf()), None))
        }

        fn json_face(ep: &FnEndpoint, resource: &str, cap: &Capability) -> serde_json::Value {
            let request = Request::new(Verb::Source, iri(resource))
                .with_arg("as", ArgRef::Inline(JSON.as_bytes().to_vec()));
            let rep = invoke(ep, request, cap).expect("a stated config home resolves");
            serde_json::from_slice(&rep.bytes).expect("the JSON face is JSON")
        }

        fn json_through(
            kernel: &ikigai_core::Kernel,
            resource: &str,
            cap: &Capability,
        ) -> serde_json::Value {
            let request = Request::new(Verb::Source, iri(resource))
                .with_arg("as", ArgRef::Inline(JSON.as_bytes().to_vec()));
            let rep = block_on(kernel.issue(request, cap)).expect("a stated config home resolves");
            serde_json::from_slice(&rep.bytes).expect("the JSON face is JSON")
        }

        /// The values the endpoint serves are the values in the handle's home —
        /// asserted as numbers and names, not as `is_number()`.
        #[test]
        fn the_endpoint_serves_the_config_home_it_was_handed() {
            let home = seeded("served");
            let json = json_face(&config_with(over(&home)), CONFIG_IRI, &Capability::root());
            assert_eq!(json["contrast"]["min"], 7.0);
            assert_eq!(json["theme"]["dark"], "Nord");
            assert_eq!(json["motion"]["reduce"], true);
            assert_eq!(json["text"]["scale"], 1.75);
            assert_eq!(
                json["layers"][0],
                home.path().join(STEM).display().to_string(),
                "the provenance names the fixture, so a wrong home would be visible"
            );
        }

        /// ★ Two mounts, two config homes, ONE process. This test could not be
        /// written at all while the read was ambient: `cargo test` shares a
        /// process across a crate's tests, so there was one `$HOME` and one
        /// possible answer.
        #[test]
        fn two_handles_in_one_process_serve_different_configs() {
            let strict = Scratch::new("strict");
            strict.write(STEM, "[contrast]\nmin = 21.0\n");
            let lenient = Scratch::new("lenient");
            lenient.write(STEM, "[contrast]\nmin = 3.0\n");
            let root = Capability::root();
            assert_eq!(
                json_face(&config_with(over(&strict)), CONFIG_IRI, &root)["contrast"]["min"],
                21.0
            );
            assert_eq!(
                json_face(&config_with(over(&lenient)), CONFIG_IRI, &root)["contrast"]["min"],
                3.0
            );
        }

        /// The layering the operator is promised, through a real kernel: an app
        /// file overrides one key and the shared floor stands.
        #[test]
        fn an_app_layer_overrides_one_key_and_the_shared_floor_stands() {
            let home = seeded("layered");
            home.write("cms-web.a11y.toml", APP_LAYER);
            let kernel = ikigai_core::Kernel::new(Arc::new(space_with(over(&home))));
            let cap = Capability::scoped([CAP_READ]);

            let app = json_through(&kernel, "urn:a11y:config:cms-web", &cap);
            assert_eq!(app["theme"]["dark"], "Dracula");
            assert_eq!(app["contrast"]["min"], 7.0, "the shared floor SURVIVES");

            let shared = json_through(&kernel, CONFIG_IRI, &cap);
            assert_eq!(
                shared["theme"]["dark"], "Nord",
                "and the app file is not read"
            );

            // An app with no file of its own sees the shared layer only.
            let other = json_through(&kernel, "urn:a11y:config:dev-server", &cap);
            assert_eq!(other["theme"]["dark"], "Nord");
        }

        /// An absent file is a layer that states nothing — the defaults, and not
        /// an error.
        #[test]
        fn a_missing_layer_states_nothing_rather_than_failing() {
            let home = Scratch::new("empty");
            let json = json_face(&config_with(over(&home)), CONFIG_IRI, &Capability::root());
            assert_eq!(json["contrast"]["min"], crate::config::DEFAULT_MIN);
            assert_eq!(json["theme"]["dark"], crate::config::DEFAULT_DARK);
            assert!(
                json["layers"]
                    .as_array()
                    .expect("layers is an array")
                    .is_empty(),
                "{json}"
            );
        }

        /// ★ A config that does not parse is an ERROR, on both views. This is
        /// the row of the measured table that mattered most: a corrupt
        /// `a11y.toml` used to leave the suite at 58 passed, because nothing
        /// read it.
        #[test]
        fn a_corrupt_layer_fails_loudly_and_names_the_file() {
            let home = Scratch::new("corrupt");
            home.write(STEM, "[contrast\nmin = 21.0\nthis is not toml\n");
            let nothing = Capability::scoped(Vec::<String>::new());
            for (ep, resource, cap) in [
                (config_with(over(&home)), CONFIG_IRI, Capability::root()),
                (presentation_with(over(&home)), PRESENTATION_IRI, nothing),
            ] {
                let request = Request::new(Verb::Source, iri(resource));
                match invoke(&ep, request, &cap) {
                    // Permanent, not `Unavailable`: bad TOML does not parse on
                    // retry.
                    Err(Error::Endpoint(message)) => assert!(
                        message.contains(&home.path().join(STEM).display().to_string()),
                        "the failure names the file that failed: {message}"
                    ),
                    other => panic!("a config that does not parse must not read as one: {other:?}"),
                }
            }
        }

        /// A misspelled theme is refused at the resource too, not only in the
        /// loader — an operator who typed it must not be told everything is fine.
        #[test]
        fn an_unknown_theme_is_refused_by_the_endpoint() {
            let home = Scratch::new("misspelled");
            home.write(STEM, "[theme]\ndark = \"Base16OceanDrak\"\n");
            let request = Request::new(Verb::Source, iri(CONFIG_IRI));
            match invoke(&config_with(over(&home)), request, &Capability::root()) {
                Err(Error::Endpoint(message)) => {
                    assert!(message.contains("Base16OceanDrak"), "{message}")
                }
                other => panic!("{other:?}"),
            }
        }

        /// No config home at all is a legal under-configured state: the handle
        /// is CONSTRUCTED (no fallible constructor, no guessed directory), and
        /// the absence surfaces at the resolution that needed it, saying what is
        /// missing.
        #[test]
        fn no_config_home_is_a_legal_state_that_fails_at_resolution() {
            let handle = Arc::new(A11yHandle::new(None, None));
            assert!(handle.home().is_none());
            let request = Request::new(Verb::Source, iri(CONFIG_IRI));
            match invoke(&config_with(handle), request, &Capability::root()) {
                Err(Error::Endpoint(message)) => {
                    assert!(message.contains("no ikigai config home"), "{message}")
                }
                other => panic!("{other:?}"),
            }
        }

        /// The ungated view reads the very same files and stops at the rendering
        /// half. Both halves are asserted as VALUES now: the floor the fixture
        /// states comes back, and the person-facts that are demonstrably in the
        /// file do not.
        #[test]
        fn the_ungated_view_serves_the_same_files_and_withholds_the_person_facts() {
            let home = seeded("halves");
            assert!(
                presentation().describe().requires.is_empty(),
                "an ungated action must not claim a capability it never checks"
            );
            let nothing = Capability::scoped(Vec::<String>::new());
            let open = json_face(&presentation_with(over(&home)), PRESENTATION_IRI, &nothing);
            assert_eq!(open["theme"]["dark"], "Nord");
            assert_eq!(open["contrast"]["min"], 7.0);
            assert_eq!(open["text"]["underline_links"], false);
            assert!(open["motion"].is_null(), "{open}");
            assert!(open["text"].get("scale").is_none(), "{open}");

            // The same files state them; the gated resource is where they live.
            let gated = json_face(&config_with(over(&home)), CONFIG_IRI, &Capability::root());
            assert_eq!(gated["motion"]["reduce"], true);
            assert_eq!(gated["text"]["scale"], 1.75);
        }

        /// The two views are separate resources with separate subjects, and the
        /// ungated graph names no host paths.
        #[test]
        fn the_two_views_are_distinct_subjects_under_distinct_capabilities() {
            let home = seeded("subjects");
            let cap = Capability::scoped([CAP_READ]);
            let turtle = |ep: &FnEndpoint, subject: &str| {
                let request = Request::new(Verb::Source, iri(subject))
                    .with_arg("as", ArgRef::Inline(TURTLE.as_bytes().to_vec()));
                invoke(ep, request, &cap)
                    .map(|rep| String::from_utf8_lossy(&rep.bytes).to_string())
                    .expect("a stated config home resolves")
            };
            let gated = turtle(&config_with(over(&home)), CONFIG_IRI);
            let open = turtle(&presentation_with(over(&home)), PRESENTATION_IRI);
            assert!(gated.contains(&format!("<{CONFIG_IRI}> a ik:AccessibilityConfig")));
            assert!(open.contains(&format!("<{PRESENTATION_IRI}> a ik:AccessibilityConfig")));
            assert!(gated.contains("ik:contrastMin 7.0"), "{gated}");
            assert!(open.contains("ik:contrastMin 7.0"), "{open}");
            assert!(
                gated.contains(&file_iri(&home.path().join(STEM))),
                "the gated graph names the file it was derived from: {gated}"
            );
            assert!(
                !open.contains("prov:wasDerivedFrom"),
                "the ungated graph names no host paths: {open}"
            );
        }

        /// Cacheable, with a thread on every CANDIDATE file — named after the
        /// fixture, so a handle reading the wrong home would fail here rather
        /// than serve plausible values.
        #[test]
        fn the_config_is_cacheable_and_declares_the_fixture_files() {
            let home = seeded("threads");
            let cap = Capability::scoped([CAP_READ]);
            let threads_of = |resource: &str| {
                let request = Request::new(Verb::Source, iri(resource));
                let rep = invoke(&config_with(over(&home)), request, &cap)
                    .expect("a stated config home resolves");
                assert_eq!(rep.expiry, ikigai_core::Expiry::Never, "cacheable");
                rep.threads()
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<String>>()
            };
            assert_eq!(
                threads_of(CONFIG_IRI),
                vec![file_iri(&home.path().join(STEM))]
            );

            // The per-app resource declares the override too, whether or not it
            // exists — creating it must invalidate this answer. Through a kernel,
            // because `{app}` is captured by the template match: a detached
            // invocation of the same endpoint carries no bindings.
            let kernel = ikigai_core::Kernel::new(Arc::new(space_with(over(&home))));
            let request = Request::new(Verb::Source, iri("urn:a11y:config:cms-web"));
            let rep = block_on(kernel.issue(request, &cap)).expect("the fixture resolves");
            let bound: Vec<String> = rep.threads().iter().map(|t| t.to_string()).collect();
            assert_eq!(
                bound,
                vec![
                    file_iri(&home.path().join(STEM)),
                    file_iri(&home.path().join("cms-web.a11y.toml")),
                ]
            );
        }

        /// The `{app}` binding reaches both views through a real kernel, and
        /// carries the app layer's VALUES with it.
        #[test]
        fn the_app_binding_reaches_both_views_through_the_kernel() {
            let home = seeded("bound");
            home.write("cms-web.a11y.toml", APP_LAYER);
            let kernel = ikigai_core::Kernel::new(Arc::new(space_with(over(&home))));
            let ttl = |resource: &str, cap: &Capability| {
                let request = Request::new(Verb::Source, iri(resource))
                    .with_arg("as", ArgRef::Inline(TURTLE.as_bytes().to_vec()));
                let rep = block_on(kernel.issue(request, cap)).expect("the fixture resolves");
                assert_eq!(rep.threads().len(), 2, "{:?}", rep.threads());
                String::from_utf8_lossy(&rep.bytes).to_string()
            };
            let open = ttl(
                "urn:a11y:presentation:cms-web",
                &Capability::scoped(Vec::<String>::new()),
            );
            assert!(open.contains("<urn:a11y:presentation:cms-web>"), "{open}");
            assert!(open.contains("ik:app \"cms-web\""), "{open}");
            assert!(open.contains("ik:themeDark \"Dracula\""), "{open}");

            let gated = ttl("urn:a11y:config:cms-web", &Capability::scoped([CAP_READ]));
            assert!(gated.contains("<urn:a11y:config:cms-web>"), "{gated}");
            assert!(gated.contains("ik:app \"cms-web\""), "{gated}");
            assert!(gated.contains("ik:themeDark \"Dracula\""), "{gated}");
        }

        /// The mount's own app is the DEFAULT for the bare IRI; a `{app}` in the
        /// IRI names its own resource and wins. A mount whose default overrode
        /// the name in the IRI would serve one resource's config under another's
        /// identity.
        #[test]
        fn the_iri_binding_wins_over_the_mounts_own_app() {
            let home = Scratch::new("mounted");
            home.write(STEM, "[contrast]\nmin = 7.0\n");
            home.write("dev-server.a11y.toml", "[theme]\ndark = \"Nord\"\n");
            home.write("cms-web.a11y.toml", APP_LAYER);
            let handle = Arc::new(A11yHandle::new(
                Some(home.path().to_path_buf()),
                Some("dev-server".to_string()),
            ));
            let kernel = ikigai_core::Kernel::new(Arc::new(space_with(handle.clone())));
            let cap = Capability::scoped([CAP_READ]);

            assert_eq!(
                json_through(&kernel, CONFIG_IRI, &cap)["theme"]["dark"],
                "Nord",
                "the mount's own layer applies where the IRI names none"
            );
            assert_eq!(
                json_through(&kernel, "urn:a11y:config:cms-web", &cap)["theme"]["dark"],
                "Dracula",
                "and the bound name wins where there is one"
            );

            assert_eq!(handle.layer(None), Some("dev-server"));
            assert_eq!(handle.layer(Some("cms-web")), Some("cms-web"));
            assert_eq!(
                A11yHandle::new(None, Some(String::new())).app(),
                None,
                "an empty name counts as absent, per layered_paths_in"
            );
        }

        /// The ambient sugar is [`A11yHandle::new`] over the machine's own home,
        /// asserted against the RULE rather than a fixed path — the harness's
        /// environment is not this test's to pin, which is the whole reason the
        /// endpoints stopped reading it.
        #[test]
        fn the_ambient_handle_matches_the_rule() {
            assert_eq!(
                A11yHandle::ambient(None).home().map(Path::to_path_buf),
                ikigai_core::config::config_home()
            );
            assert_eq!(
                A11yHandle::ambient(Some("cms-web".to_string())).app(),
                Some("cms-web")
            );
        }
    }
}
