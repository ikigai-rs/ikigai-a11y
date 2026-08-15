//! The `urn:a11y:*` endpoints.
//!
//! ## Two actions, one capability between them
//!
//! `urn:a11y:config` requires — and enforces — [`CAP_READ`]. It is tempting to
//! call theme preferences public, but this config also states whether the person
//! at the keyboard needs reduced motion and larger text, and that is
//! assistive-technology information about a human being, not decoration. It is
//! gated for the same reason calendar detail is.
//!
//! `urn:a11y:contrast` requires **nothing**, because it computes a WCAG ratio
//! from two colours the caller already holds. Declaring a capability it does not
//! need would make the manifold under-offer — an agent holding no a11y grant
//! would be told it cannot do arithmetic.
//!
//! That split is why `urn:a11y:contrast` takes an explicit `min` (defaulting to
//! the WCAG AA constant) rather than reading the configured floor itself. An
//! action whose capability requirement depends on WHICH arguments arrive cannot
//! be described: `requires` is per-action, so the manifold would have to declare
//! the union (over-demanding for the pure call) or the intersection (a lie for
//! the config-reading one). A caller wanting the configured floor sources
//! `urn:a11y:config` — one cached read — and passes it in.
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
    // Declared = enforced. The manifold says this action needs CAP_READ; this is
    // where that stops being a claim.
    if !inv.capability.allows(CAP_READ) {
        return Err(Error::Denied(format!(
            "reading the accessibility config requires `{CAP_READ}`"
        )));
    }
    let app = inv.bindings.get("app");
    let effective = crate::load::load(app).map_err(config_error)?;
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
    let space = space.bind(Exact::new(CONFIG_IRI), config()).bind(
        UriTemplate::parse(CONFIG_TEMPLATE).expect("CONFIG_TEMPLATE is a valid template"),
        config(),
    );
    space
}

/// The theme identifiers an `a11y.toml` may name — exposed so a host building a
/// picker does not re-derive the list.
pub fn configurable_themes() -> Vec<&'static str> {
    theme_ids().into_iter().collect()
}

/// The effective config as a value, for a host that wants the struct rather than
/// a representation. Kept here beside the endpoint so both agree on which layers
/// are consulted.
#[cfg(not(target_family = "wasm"))]
pub fn effective(app: Option<&str>) -> Result<A11y> {
    crate::load::load(app).map_err(config_error)
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

    /// The space binds what a host will actually resolve — both config spellings
    /// and the calculator.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn the_space_binds_all_three_identifiers() {
        use ikigai_core::Space;
        let entries = space().entries().expect("an EndpointSpace enumerates");
        let patterns: Vec<&str> = entries.iter().map(|e| e.pattern.as_str()).collect();
        assert!(patterns.contains(&CONTRAST_IRI), "{patterns:?}");
        assert!(patterns.contains(&CONFIG_IRI), "{patterns:?}");
        assert!(patterns.contains(&CONFIG_TEMPLATE), "{patterns:?}");
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
