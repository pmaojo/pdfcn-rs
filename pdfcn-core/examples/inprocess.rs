//! In-process timings that a fresh-process benchmark (scripts/bench/run.py)
//! cannot show: a warm re-render in a long-lived process (`pdfcn dev`, a
//! server) and how a batch scales with threads. See scripts/bench/README.md.
//!
//! ```sh
//! python3 -I scripts/bench/run.py catalog -n 1     # creates target/bench/catalog
//! cargo run --release -p pdfcn-core --example inprocess
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use pdfcn_core::{render_files, render_files_html, with_threads, RenderOptions, Theme};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Median of `runs` timings of `f`, in ms.
fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn catalog_options(dir: &Path) -> RenderOptions {
    let mut options = RenderOptions::default();
    for i in 0..100 {
        let id = format!("s{i:03}");
        if let Ok(svg) = std::fs::read_to_string(dir.join(format!("img/{id}.svg"))) {
            options.svg_assets.insert(id, svg);
        }
    }
    options
}

fn main() {
    let root = repo();
    let catalog = root.join("target/bench/catalog");
    let (template, data) = (catalog.join("catalog.haml"), catalog.join("catalog.json"));
    if template.exists() {
        let options = catalog_options(&catalog);
        let first = median_ms(1, || {
            let _ = render_files(&template, &data, &options);
        });
        let warm = median_ms(9, || {
            let _ = render_files(&template, &data, &options);
        });
        let html = median_ms(9, || {
            let _ = render_files_html(&template, &data, &Theme::light());
        });
        println!("catalog: first render {first:.1} ms, warm PDF {warm:.1} ms, warm HTML (pdfcn dev) {html:.1} ms");
    } else {
        println!("catalog: run `python3 -I scripts/bench/run.py catalog -n 1` first");
    }

    // A batch of 8 invoices, rendered on 1..=N threads at once.
    let (template, data) = (
        root.join("examples/invoice.haml"),
        root.join("examples/invoice.json"),
    );
    let options = RenderOptions::default();
    let _ = render_files(&template, &data, &options); // warm the process-wide caches
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let batch: usize = 8;
    let serial = median_ms(5, || {
        for _ in 0..batch {
            let _ = render_files(&template, &data, &options);
        }
    });
    println!("batch of {batch} invoices: 1 thread {serial:.1} ms");
    for threads in [2, cores] {
        if threads < 2 {
            continue;
        }
        let parallel = median_ms(5, || {
            std::thread::scope(|scope| {
                let per = batch.div_ceil(threads);
                for chunk in 0..threads {
                    let (template, data, options) = (&template, &data, &options);
                    // As the batch endpoint does: the documents are spread
                    // over threads, so each renders on its own thread.
                    scope.spawn(move || {
                        with_threads(1, || {
                            for _ in (chunk * per)..((chunk + 1) * per).min(batch) {
                                let _ = render_files(template, data, options);
                            }
                        })
                    });
                }
            });
        });
        println!(
            "batch of {batch} invoices: {threads} threads {parallel:.1} ms ({:.2}x)",
            serial / parallel
        );
    }
}
