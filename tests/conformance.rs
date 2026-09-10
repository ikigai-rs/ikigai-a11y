//! The module recipe as one test: `ikigai-conformance` walks every endpoint
//! [`ikigai_a11y::space_with`] binds and reports every violation at once.
//!
//! The kernel under test is a fixture: a fresh temporary config home this file
//! seeds with a shared `a11y.toml` and one app override, so the config
//! endpoints read layers the test wrote rather than the developer's
//! `~/.config/ikigai` (which is what `space()` would read). Nothing here
//! mutates anything outside that directory, and it is removed afterwards.
//!
//! ## Declarations, and why each
//!
//! - `a11yContrast` is `pure` AND `cacheable`: WCAG arithmetic over two colours
//!   the caller supplies, no file, clock or platform read, so its cacheable
//!   result rightly carries an empty golden-thread set. It also needs a
//!   [`Fixture`]: its inputs are `xsd:string`s with a lexical form (`#rrggbb`)
//!   the suite's minimal `x` does not satisfy.
//! - `a11yConfig` and `a11yPresentation` are `cacheable` and deliberately NOT
//!   `pure`: both read the layered files, and both declare a thread per
//!   candidate file (`urn:file:/abs/…/a11y.toml`, existing or not). Declared
//!   cacheable, the suite holds them to a cache hit on the second resolution
//!   and a non-empty thread set — so a future dependency that silently
//!   downgraded the effective expiry, or a refactor that dropped `depends_on`,
//!   is a red test rather than a ~40× slowdown (README, "Cacheability") or a
//!   config served forever.
//! - `NAMES` is skipped suite-wide, not opted out per id: the three ids
//!   (`a11yConfig`, `a11yPresentation`, `a11yContrast`) are live MCP tool names
//!   and are renamed in one coordinated pass (wave two, owned by
//!   `ikigai-core-PENDING.md` §1). `Suite::opt_out` cannot carry this — it
//!   excludes an endpoint from the INVOKING checks and leaves NAMES running —
//!   so dropping the check is the only spelling, and the report prints it as
//!   skipped.
//!
//! No opt-outs, no module namespace: every term the Turtle face emits is
//! defined in `ikigai-vocab` (the suite's VOCABULARY check says so).
//!
//! ## What the suite cannot see
//!
//! A thread set that is non-empty is not a thread set anything CUTS. This module
//! declares the threads and cuts none of them — it has no file watcher, and a
//! host that wants an edit to `a11y.toml` to invalidate must watch the config
//! home and cut the names from [`ikigai_a11y::load::threads_in`] itself
//! (README, "Cacheability"). [`a_cut_on_the_declared_thread_recomputes`] pins
//! both halves of that: the cached answer IS stale after an edit with no cut,
//! and the name this module declares IS the name a cut is keyed on.

use ikigai_a11y::{A11yHandle, CONFIG_IRI};
use ikigai_conformance::{Check, Checks, Fixture, Report, Suite};
use ikigai_core::{ArgRef, Capability, Iri, Kernel, Request, Verb};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// The three endpoints `space_with` binds, by description id.
const CONTRAST: &str = "a11yContrast";
const CONFIG: &str = "a11yConfig";
const PRESENTATION: &str = "a11yPresentation";

/// The application whose override layer the seeded home carries, and what the
/// `{app}` template variable is bound to on every fired action — so the graph
/// face is exercised with BOTH layer roles (`ik:sharedLayer`, `ik:appLayer`)
/// and `ik:app` present, not just the shared defaults.
const APP: &str = "conformance";

/// The shared layer: a floor and a theme that are NOT the defaults, plus a
/// person-fact, so the gated face has something the ungated one withholds.
const SHARED: &str =
    "[contrast]\nmin = 7.0\n\n[theme]\ndark = \"Nord\"\n\n[motion]\nreduce = true\n";

/// The app override: one key, so the key-wise merge is what the faces show.
const OVERRIDE: &str = "[theme]\ndark = \"Dracula\"\n";

/// A fresh config home under the platform temp dir, unique per test, seeded
/// with both layers.
fn seeded_home() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ikigai-a11y-conformance-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a11y.toml"), SHARED).unwrap();
    std::fs::write(dir.join(format!("{APP}.a11y.toml")), OVERRIDE).unwrap();
    dir
}

/// The module's space over `home`, as a host serving a stated config home
/// mounts it. No mount-level default app: the IRI's `{app}` binding (or its
/// absence) is what selects the layer.
fn kernel(home: &Path) -> Kernel {
    Kernel::new(Arc::new(ikigai_a11y::space_with(Arc::new(
        A11yHandle::new(Some(home.to_path_buf()), None),
    ))))
}

/// The suite, configured for this module (see the file docs for why each line).
fn suite() -> Suite {
    Suite::new()
        // Wave two: ids are live MCP tool names, renamed in one coordinated pass.
        .checks(Checks::all() - Checks::NAMES)
        .fixture(
            Fixture::new(CONTRAST, Verb::Source)
                .arg("from", "#000000")
                .arg("on", "#ffffff"),
        )
        .fixture(Fixture::new(CONFIG, Verb::Source).binding("app", APP))
        .fixture(Fixture::new(PRESENTATION, Verb::Source).binding("app", APP))
        .pure(CONTRAST)
        .cacheable(CONTRAST)
        .cacheable(CONFIG)
        .cacheable(PRESENTATION)
}

/// Run `suite` over a fresh seeded home, clean up, and hand back the report.
fn run(suite: Suite) -> Report {
    let home = seeded_home();
    let report = suite.run_blocking(&kernel(&home));
    std::fs::remove_dir_all(&home).ok();
    report
}

#[test]
fn conforms() {
    let report = run(suite());
    // Printed even when clean (`--nocapture`): the report is the record.
    eprintln!("{report}");
    assert!(report.is_clean(), "{report}");
    // The walk saw exactly the three endpoints declared above, and five
    // actions: one Source per bound entry — `urn:a11y:config` and
    // `urn:a11y:config:{app}` are two entries sharing one description, and
    // likewise for presentation. A fourth endpoint bound without a line here
    // would be held to a weaker standard (the suite cannot know which ones it
    // was not told about); a declared id that binds nothing is a stale list.
    assert_eq!(
        report.endpoints, 3,
        "contrast, config, presentation: {report}"
    );
    assert_eq!(
        report.actions, 5,
        "one Source per bound entry (two spellings of each config view): {report}"
    );
    // The ONLY skipped check is NAMES; anything else dropped here is a gate
    // silently covering less than it looks like it does.
    assert_eq!(
        report.checks.skipped().collect::<Vec<_>>(),
        vec![Check::Names],
        "{report}"
    );
}

/// The half the suite cannot see: the threads this module declares are names
/// a HOST must cut. It has no watcher of its own, so after an edit with no cut
/// the cached answer is stale — and the name the endpoint declared is exactly
/// the one a cut is keyed on, so a host wiring `load::threads_in` into its
/// watcher gets the fresh answer with no poll.
#[test]
fn a_cut_on_the_declared_thread_recomputes() {
    let home = seeded_home();
    let kernel = kernel(&home);
    let root = Capability::root();
    let request = || {
        Request::new(Verb::Source, Iri::parse(CONFIG_IRI).unwrap())
            .with_arg("as", ArgRef::Inline(b"application/json".to_vec()))
    };
    let read = || {
        futures::executor::block_on(kernel.issue(request(), &root))
            .map(|r| String::from_utf8(r.bytes).unwrap())
            .unwrap()
    };

    let first = read();
    assert!(first.contains("7.0"), "the seeded floor: {first}");
    assert!(
        kernel.is_cached(&request(), &root),
        "declared cacheable, so the first read is now cached"
    );

    // The operator lowers the floor. Nothing in this module notices.
    std::fs::write(home.join("a11y.toml"), SHARED.replace("7.0", "3.0")).unwrap();
    let stale = read();
    assert_eq!(
        stale, first,
        "no watcher here: an edit with no cut is served from the cache"
    );

    // A host that watches the config home cuts the name this module declared.
    let threads = ikigai_a11y::load::threads_in(&home, None);
    assert_eq!(
        threads.len(),
        1,
        "one candidate file with no app: {threads:?}"
    );
    kernel.cut(threads[0].clone());
    let fresh = read();
    assert!(fresh.contains("3.0"), "recomputed after the cut: {fresh}");
    assert!(!fresh.contains("7.0"), "{fresh}");

    std::fs::remove_dir_all(&home).ok();
}
