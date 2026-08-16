//! What the golden thread is worth — measured, not asserted.
//!
//!     cargo run --release --example cache_measure
//!
//! `urn:a11y:config` reads a file, and the lazy conclusion from that is
//! "uncacheable". This example is the reason not to. It resolves the SAME
//! effective config three ways through a real kernel:
//!
//! 1. **uncacheable** — the lazy shape: every read re-opens and re-parses the
//!    layered files,
//! 2. **cacheable + golden thread** — this crate's shape,
//! 3. **a consumer that joins it** — a stylesheet-shaped resource that sources
//!    the config and runs the contrast-floor pass. This is where the difference
//!    stops being academic: effective expiry propagates from dependencies, so
//!    joining an uncacheable config makes the CONSUMER uncacheable too, however
//!    expensive the consumer is.
//!
//! It then cuts the thread and shows the cached resource recomputing — the
//! payoff the golden thread buys: edit `a11y.toml`, derived stylesheets refresh,
//! and nothing polls.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ikigai_a11y::config::file_iri;
use ikigai_a11y::{apply_floor, Rgba};
use ikigai_core::{
    Capability, EndpointSpace, Exact, FnEndpoint, Invocation, Iri, Kernel, ReprType,
    Representation, Request, Result, Verb,
};

const READS: u32 = 200;

/// A stand-in for the real stylesheet work a consumer does per read.
fn stylesheet(min: f64) -> String {
    let css = std::iter::repeat_n(
        ".hl-variable {\n color: #bf616a;\n}\n.hl-string {\n color: #a3be8c;\n}\n",
        400,
    )
    .collect::<String>();
    apply_floor(
        &css,
        Rgba::parse("#2b303b").expect("a valid colour"),
        Rgba::parse("#c0c5ce").expect("a valid colour"),
        min,
    )
    .css
}

/// A scratch config home with real files, so the measurement times real parsing
/// rather than two failed `stat` calls on a machine that has no `a11y.toml`.
fn scratch_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ikigai-a11y-measure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    std::fs::write(
        dir.join("a11y.toml"),
        "[contrast]\nmin = 7.0\nmin_large = 4.5\n\n[theme]\ndark = \"Nord\"\n",
    )
    .expect("scratch write");
    std::fs::write(
        dir.join("cms-web.a11y.toml"),
        "[theme]\ndark = \"Dracula\"\n",
    )
    .expect("scratch write");
    dir
}

fn threads(home: &std::path::Path) -> Vec<String> {
    ikigai_core::config::layered_paths_in(home, ikigai_a11y::load::STEM, Some("cms-web"))
        .iter()
        .map(|p| file_iri(p))
        .collect()
}

fn config_endpoint(home: PathBuf, cacheable: bool) -> FnEndpoint {
    FnEndpoint::new(
        "config",
        move |_inv: &Invocation<'_>| -> Result<Representation> {
            let effective = ikigai_a11y::load::complete_in(&home, Some("cms-web"))
                .map_err(|e| ikigai_core::Error::Endpoint(e.to_string()))?;
            let repr = Representation::new(
                ReprType::new("text/plain"),
                effective.to_toml().into_bytes(),
            );
            if !cacheable {
                return Ok(repr);
            }
            let mut repr = repr.cacheable();
            for thread in threads(&home) {
                repr = repr.depends_on(thread);
            }
            Ok(repr)
        },
    )
}

/// The consumer: sources the config, then does the expensive derived work.
fn styles_endpoint() -> ikigai_core::AsyncFnEndpoint {
    ikigai_core::AsyncFnEndpoint::new("styles", |inv| {
        Box::pin(async move {
            let config = inv
                .source(&Iri::parse("urn:demo:config").expect("a valid IRI"))
                .await?;
            let min = String::from_utf8_lossy(&config.bytes)
                .lines()
                .find_map(|l| l.strip_prefix("min = ")?.trim().parse::<f64>().ok())
                .unwrap_or(4.5);
            Ok(
                Representation::new(ReprType::new("text/css"), stylesheet(min).into_bytes())
                    .cacheable(),
            )
        })
    })
}

fn time(kernel: &Kernel, target: &str, cap: &Capability) -> f64 {
    let iri = Iri::parse(target).expect("a valid IRI");
    // One warm-up so the first read's cost is not attributed to the loop.
    futures::executor::block_on(kernel.issue(Request::new(Verb::Source, iri.clone()), cap))
        .expect("resolves");
    let start = Instant::now();
    for _ in 0..READS {
        futures::executor::block_on(kernel.issue(Request::new(Verb::Source, iri.clone()), cap))
            .expect("resolves");
    }
    start.elapsed().as_secs_f64() * 1e6 / f64::from(READS)
}

fn kernel_with(home: &std::path::Path, cacheable: bool) -> Kernel {
    Kernel::new(Arc::new(
        EndpointSpace::new()
            .bind(
                Exact::new("urn:demo:config"),
                config_endpoint(home.to_path_buf(), cacheable),
            )
            .bind(Exact::new("urn:demo:styles"), styles_endpoint()),
    ))
}

fn main() {
    let cap = Capability::root();
    let home = scratch_home();

    let lazy = kernel_with(&home, false);
    let config_uncached = time(&lazy, "urn:demo:config", &cap);
    let styles_uncached = time(&lazy, "urn:demo:styles", &cap);

    let good = kernel_with(&home, true);
    let config_cached = time(&good, "urn:demo:config", &cap);
    let styles_cached = time(&good, "urn:demo:styles", &cap);

    println!("mean of {READS} reads, µs\n");
    println!("{:<34}{:>12}{:>12}", "", "uncacheable", "cacheable");
    println!(
        "{:<34}{:>12.1}{:>12.1}   {:.0}×",
        "urn:a11y:config (reads 1-2 files)",
        config_uncached,
        config_cached,
        config_uncached / config_cached
    );
    println!(
        "{:<34}{:>12.1}{:>12.1}   {:.0}×",
        "a consumer joining it",
        styles_uncached,
        styles_cached,
        styles_uncached / styles_cached
    );
    println!(
        "\nThe second row is the one that matters: the consumer marks itself .cacheable(), and \
         is cached\nonly in the right-hand column — expiry propagates from the config it joined."
    );

    // And the payoff: a cut recomputes rather than a poll.
    let before = time(&good, "urn:demo:styles", &cap);
    for thread in threads(&home) {
        good.cut(thread);
    }
    let iri = Iri::parse("urn:demo:styles").expect("a valid IRI");
    let start = Instant::now();
    futures::executor::block_on(good.issue(Request::new(Verb::Source, iri), &cap))
        .expect("resolves");
    let after_cut = start.elapsed().as_secs_f64() * 1e6;
    println!(
        "\ncached read {before:.1}µs → first read after cutting the a11y.toml thread \
         {after_cut:.1}µs (recomputed)"
    );
    let _ = std::fs::remove_dir_all(&home);
}
