//! Where the time actually goes.
//!
//! Before this file the workspace had no benchmarks, no timing
//! instrumentation, and not a single `Instant` -- so every statement about
//! what pdfcn-rs costs was a guess, and any optimisation would have been
//! optimising by intuition. These benchmarks exist to make the next wave of
//! performance work answerable to measurement rather than to plausibility.
//!
//! They are split along the pipeline's real seam. `render_html` covers
//! parsing, template evaluation, component expansion, the gap rewrite and the
//! stylesheet build; `render_files` covers all of that plus asset preparation
//! and the layout engine. Benchmarking both separately is what makes the
//! difference legible: if the full pipeline is dominated by the part
//! `render_html` does not include, the layout engine is the cost and the
//! Rust-side caches barely matter.
//!
//! The `repeated_*` benchmarks render the *same* template many times, which
//! is the batch endpoint's shape -- a run of invoices off one template. They
//! are where the missing caches should show up: today the stylesheet is
//! rebuilt and re-minified per document, and a parse-cache hit still deep
//! clones the whole AST out from under a global mutex.
//!
//! Record a baseline before changing anything:
//!
//! ```sh
//! cargo bench -p pdfcn-core -- --save-baseline before
//! # ... optimise ...
//! cargo bench -p pdfcn-core -- --baseline before
//! ```

use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};
use pdfcn_core::{render_files, render_html, NoPartials, RenderOptions};

/// One render of a typical invoice, repeated this many times, standing in for
/// the batch endpoint's "one template, many rows" workload.
const BATCH_SIZE: usize = 20;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("examples")
}

fn read(example: &str, extension: &str) -> String {
    let path = examples_dir().join(format!("{example}.{extension}"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"))
}

/// Template + parsed data for one example, loaded once outside the timed
/// section so file I/O and JSON parsing do not pollute the measurement.
struct Fixture {
    name: &'static str,
    source: String,
    data: serde_json::Value,
}

fn fixtures() -> Vec<Fixture> {
    ["invoice", "catalog", "showcase"]
        .into_iter()
        .map(|name| {
            let raw = read(name, "json");
            let data =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {name}.json: {e}"));
            Fixture {
                name,
                source: read(name, "haml"),
                data,
            }
        })
        .collect()
}

/// Everything up to the layout engine: parse, evaluate, expand components,
/// rewrite gaps, build and minify the stylesheet.
fn bench_html(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_html");
    for fixture in fixtures() {
        group.bench_function(fixture.name, |b| {
            b.iter(|| {
                let html = render_html(
                    black_box(&fixture.source),
                    black_box(&fixture.data),
                    &NoPartials,
                );
                black_box(html.is_ok())
            })
        });
    }
    group.finish();
}

/// The whole thing, ending in PDF bytes. Uses `render_files` so that local
/// `<img src>` resolution and the asset passes are included -- the catalog
/// example carries four real photographs, which is the point of having it
/// here.
fn bench_pdf(c: &mut Criterion) {
    let dir = examples_dir();
    let options = RenderOptions::default();
    let mut group = c.benchmark_group("render_pdf");
    // The layout engine dominates here, so a handful of samples is enough to
    // see a regression without making `cargo bench` unusable.
    group.sample_size(20);
    for name in ["invoice", "catalog", "showcase"] {
        let template = dir.join(format!("{name}.haml"));
        let data = dir.join(format!("{name}.json"));
        group.bench_function(name, |b| {
            b.iter(|| {
                let bytes = render_files(black_box(&template), black_box(&data), &options);
                black_box(bytes.is_ok())
            })
        });
    }
    group.finish();
}

/// The batch shape: the same template rendered repeatedly. A parse cache that
/// deep clones on hit, and a stylesheet rebuilt from scratch per document,
/// both cost linearly here while an effective cache would not.
fn bench_repeated(c: &mut Criterion) {
    let fixture = fixtures()
        .into_iter()
        .find(|f| f.name == "invoice")
        .unwrap_or_else(|| panic!("the invoice fixture is missing"));

    let mut group = c.benchmark_group("repeated_same_template");
    group.sample_size(20);
    group.bench_function(format!("render_html x{BATCH_SIZE}"), |b| {
        b.iter(|| {
            for _ in 0..BATCH_SIZE {
                let html = render_html(
                    black_box(&fixture.source),
                    black_box(&fixture.data),
                    &NoPartials,
                );
                black_box(html.is_ok());
            }
        })
    });
    group.finish();
}

criterion_group!(benches, bench_html, bench_pdf, bench_repeated);
criterion_main!(benches);
