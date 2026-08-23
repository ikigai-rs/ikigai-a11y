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
use ikigai_core::UriTemplate;
use ikigai_core::{
    ArgSpec, Description, EndpointSpace, Error, Exact, FnEndpoint, Invocation, ReprType,
    Representation, Result, Verb,
};

use crate::color::{ratio, Rgba};
use crate::config::theme_ids;
#[cfg(not(target_family = "wasm"))]
use crate::config::{A11y, ConfigError};

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
fn config_impl(inv: &Invocation<'_>) -> Result<Representation> {
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
    let effective = crate::load::complete(app).map_err(config_error)?;
    let subject = match app {
        Some(app) => format!("{CONFIG_IRI}:{app}"),
        None => CONFIG_IRI.to_string(),
    };
    let (repr_type, body) = match face(inv)? {
        JSON => (ReprType::new(JSON), effective.to_json()),
        TURTLE => (
            ReprType::new("text/turtle").with_param("charset", "utf-8"),
            effective.to_turtle(&subject, app),
        ),
        _ => (plain(), effective.to_toml()),
    };
    // Cacheable, with a thread on every CANDIDATE file — including the ones that
    // do not exist, so creating an override invalidates this too.
    let mut repr = Representation::new(repr_type, body.into_bytes()).cacheable();
    for thread in crate::load::threads(app).map_err(config_error)? {
        repr = repr.depends_on(thread);
    }
    Ok(repr)
}

/// `urn:a11y:config` / `urn:a11y:config:{app}` — the EFFECTIVE merged config.
#[cfg(not(target_family = "wasm"))]
pub fn config() -> FnEndpoint {
    FnEndpoint::new("a11yConfig", config_impl).with_description(
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
fn presentation_impl(inv: &Invocation<'_>) -> Result<Representation> {
    // No capability check, and that is the contract: this action declares none,
    // and what it can reach is bounded by the projection rather than by a gate.
    let app = inv.bindings.get("app");
    let rendering = crate::load::presentation(app).map_err(config_error)?;
    let subject = match app {
        Some(app) => format!("{PRESENTATION_IRI}:{app}"),
        None => PRESENTATION_IRI.to_string(),
    };
    let (repr_type, body) = match face(inv)? {
        JSON => (ReprType::new(JSON), rendering.to_json()),
        TURTLE => (
            ReprType::new("text/turtle").with_param("charset", "utf-8"),
            rendering.to_turtle(&subject, app),
        ),
        _ => (plain(), rendering.to_toml()),
    };
    // Same files, same threads as the gated resource: the two views are cached
    // separately and invalidated together.
    let mut repr = Representation::new(repr_type, body.into_bytes()).cacheable();
    for thread in crate::load::threads(app).map_err(config_error)? {
        repr = repr.depends_on(thread);
    }
    Ok(repr)
}

/// `urn:a11y:presentation` / `urn:a11y:presentation:{app}` — the rendering half
/// of the effective config, ungated.
#[cfg(not(target_family = "wasm"))]
pub fn presentation() -> FnEndpoint {
    FnEndpoint::new("a11yPresentation", presentation_impl).with_description(
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

/// The module's space: `urn:a11y:contrast` everywhere, plus the config
/// endpoints on hosts that have a filesystem.
///
/// On wasm the config endpoints are absent rather than present-and-failing: an
/// action in the manifold that cannot succeed is worse than one that is not
/// offered, since an agent will select it.
#[allow(clippy::let_and_return)] // the config bindings are cfg'd out on wasm
pub fn space() -> EndpointSpace {
    let space = EndpointSpace::new().bind(Exact::new(CONTRAST_IRI), contrast());
    #[cfg(not(target_family = "wasm"))]
    let space = space
        .bind(Exact::new(CONFIG_IRI), config())
        .bind(
            UriTemplate::parse(CONFIG_TEMPLATE).expect("CONFIG_TEMPLATE is a valid template"),
            config(),
        )
        .bind(Exact::new(PRESENTATION_IRI), presentation())
        .bind(
            UriTemplate::parse(PRESENTATION_TEMPLATE)
                .expect("PRESENTATION_TEMPLATE is a valid template"),
            presentation(),
        );
    space
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

    /// The ungated resource is reachable holding nothing at all, and what it
    /// serves is bounded by the projection rather than by a check. Both halves
    /// matter: a caller with no grant gets the themes and the floors, and no
    /// caller — grant or no grant — gets the person-facts from this IRI.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_presentation_endpoint_needs_no_capability_and_withholds_the_person_facts() {
        assert!(
            presentation().describe().requires.is_empty(),
            "an ungated action must not claim a capability it never checks"
        );
        let nothing = Capability::scoped(Vec::<String>::new());
        let request = Request::new(Verb::Source, iri(PRESENTATION_IRI))
            .with_arg("as", ArgRef::Inline(JSON.as_bytes().to_vec()));
        let Ok(rep) = invoke(&presentation(), request, &nothing) else {
            return; // no config home on this machine; the loader said so
        };
        let json: serde_json::Value =
            serde_json::from_slice(&rep.bytes).expect("the JSON face is JSON");
        assert!(json["theme"]["dark"].is_string());
        assert!(json["contrast"]["min"].is_number());
        assert!(json["text"]["underline_links"].is_boolean());
        assert!(json["motion"].is_null(), "{json}");
        assert!(json["text"].get("scale").is_none(), "{json}");
    }

    /// The two views are separate resources with separate names, because
    /// authority that differs cannot ride on an argument — and the graph faces
    /// carry separate subjects, so a reader unioning them never merges a
    /// withheld property with an unstated one.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_two_views_are_distinct_subjects_under_distinct_capabilities() {
        let cap = Capability::scoped([CAP_READ]);
        let turtle = |ep: &FnEndpoint, subject: &str| {
            let request = Request::new(Verb::Source, iri(subject))
                .with_arg("as", ArgRef::Inline(TURTLE.as_bytes().to_vec()));
            invoke(ep, request, &cap).map(|rep| String::from_utf8_lossy(&rep.bytes).to_string())
        };
        let (Ok(gated), Ok(open)) = (
            turtle(&config(), CONFIG_IRI),
            turtle(&presentation(), PRESENTATION_IRI),
        ) else {
            return; // no config home on this machine
        };
        assert!(gated.contains(&format!("<{CONFIG_IRI}> a ik:AccessibilityConfig")));
        assert!(open.contains(&format!("<{PRESENTATION_IRI}> a ik:AccessibilityConfig")));
        assert!(
            !open.contains("prov:wasDerivedFrom"),
            "the ungated graph names no host paths: {open}"
        );
    }

    /// The result is CACHEABLE and carries a thread per candidate file. This is
    /// the property the whole design turns on: an uncacheable config would make
    /// every stylesheet that joined it uncacheable too.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_config_is_cacheable_and_declares_its_files() {
        let cap = Capability::scoped([CAP_READ]);
        let request = Request::new(Verb::Source, iri(CONFIG_IRI));
        let Ok(rep) = invoke(&config(), request, &cap) else {
            // No config home on this machine (no HOME, no XDG) — the loader says
            // so rather than guessing, and there is nothing to assert here.
            return;
        };
        assert_eq!(rep.expiry, ikigai_core::Expiry::Never, "cacheable");
        let threads: Vec<String> = rep.threads().iter().map(|t| t.to_string()).collect();
        assert_eq!(threads.len(), 1, "the shared layer only: {threads:?}");
        assert!(threads[0].starts_with("urn:file:/"), "{threads:?}");
        assert!(threads[0].ends_with("a11y.toml"), "{threads:?}");
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
        let entries = space().entries().expect("an EndpointSpace enumerates");
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

    /// The `{app}` binding reaches the ungated view too — an operator's
    /// `cms-web.a11y.toml` theme override must apply to the resource the
    /// stylesheet actually reads, or the override silently does nothing.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_app_binding_reaches_the_ungated_view_through_the_kernel() {
        use std::sync::Arc;
        let kernel = ikigai_core::Kernel::new(Arc::new(space()));
        let nothing = Capability::scoped(Vec::<String>::new());
        let request = Request::new(Verb::Source, iri("urn:a11y:presentation:cms-web"))
            .with_arg("as", ArgRef::Inline(TURTLE.as_bytes().to_vec()));
        let Ok(rep) = block_on(kernel.issue(request, &nothing)) else {
            return; // no config home on this machine
        };
        let ttl = String::from_utf8_lossy(&rep.bytes);
        assert!(ttl.contains("<urn:a11y:presentation:cms-web>"), "{ttl}");
        assert!(ttl.contains("ik:app \"cms-web\""), "{ttl}");
        assert_eq!(rep.threads().len(), 2, "{:?}", rep.threads());
    }

    /// End to end through a real kernel: the `{app}` binding reaches the
    /// endpoint, which is the whole reason the second grammar exists — the
    /// per-app IRI must serve a per-app graph, not the shared one under a
    /// different name.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_app_binding_reaches_the_graph_face_through_the_kernel() {
        use std::sync::Arc;
        let kernel = ikigai_core::Kernel::new(Arc::new(space()));
        let cap = Capability::scoped([CAP_READ]);
        let request = Request::new(Verb::Source, iri("urn:a11y:config:cms-web"))
            .with_arg("as", ArgRef::Inline(TURTLE.as_bytes().to_vec()));
        let Ok(rep) = block_on(kernel.issue(request, &cap)) else {
            return; // no config home on this machine; the loader said so
        };
        let ttl = String::from_utf8_lossy(&rep.bytes);
        assert!(ttl.contains("<urn:a11y:config:cms-web>"), "{ttl}");
        assert!(ttl.contains("ik:app \"cms-web\""), "{ttl}");
        // Both candidate files are declared, not just the one that exists.
        assert_eq!(rep.threads().len(), 2, "{:?}", rep.threads());
    }
}
