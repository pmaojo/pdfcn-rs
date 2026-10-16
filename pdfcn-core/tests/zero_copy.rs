//! A photo is not copied on its way through a render (docs/spikes/006).
//!
//! A counting global allocator records the peak of live heap bytes during a
//! render. Rendering the same document with a large photo and with a tiny one
//! cancels out everything but what the photo itself costs: with the JPEG
//! passed through and laid out by size alone, that is the stream lopdf writes
//! it into and the output PDF that carries it, plus the JPEG decoder's own
//! buffer while it reads the EXIF orientation -- 2.7 copies, measured. Before
//! (three byte-for-byte copies on the way in, the saved PDF loaded twice by
//! lopdf) it was 4.4, and a smooth photo cost its full-size placeholder's
//! decoded pixels on top.
//!
//! One test in its own binary: the counters are process-wide, and the test
//! harness would run several tests on parallel threads.
#![cfg(feature = "jpeg-passthrough")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size > layout.size() {
                let grown = new_size - layout.size();
                let live = LIVE.fetch_add(grown, Ordering::Relaxed) + grown;
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A JPEG of noise: incompressible, so its file is large for its size.
fn noisy_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut state = 0x2545_f491u32;
    let img = image::RgbImage::from_fn(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        image::Rgb([state as u8, (state >> 8) as u8, (state >> 16) as u8])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&img)
        .expect("encodes");
    out
}

/// Peak live bytes above the starting point while rendering `photo`.
fn render_peak(photo: &[u8]) -> usize {
    let images = BTreeMap::from([("photo.jpg".to_string(), photo.to_vec())]);
    let html =
        r#"<html><body><img src="photo.jpg" style="width:300px;height:300px"></body></html>"#;
    let start = LIVE.load(Ordering::Relaxed);
    PEAK.store(start, Ordering::Relaxed);
    let pdf = pdfcn_core::render_pdf_with_assets(
        html,
        &BTreeMap::new(),
        &images,
        &pdfcn_core::RenderOptions::default(),
    )
    .expect("renders");
    let peak = PEAK.load(Ordering::Relaxed) - start;
    assert!(pdf
        .windows(photo.len().min(64))
        .any(|w| w == &photo[..photo.len().min(64)]));
    drop(pdf);
    peak
}

#[test]
fn a_photo_costs_about_two_copies_of_itself_and_no_pixels() {
    let tiny = noisy_jpeg(300, 300);
    let small = smooth_jpeg(300, 300);
    let big = noisy_jpeg(2000, 2000);
    // Warm-up: the process-wide font pool and parse caches fill on first use.
    render_peak(&tiny);
    let cost = render_peak(&big).saturating_sub(render_peak(&tiny));
    let photo = big.len() - tiny.len();
    let copies = cost as f64 / photo as f64;
    eprintln!("a {photo} byte photo cost {cost} bytes = {copies:.2} copies");
    assert!(
        copies < 3.0,
        "the photo cost {cost} bytes = {copies:.2} copies of its {photo} bytes"
    );

    // And its pixels never exist: a smooth 3000 x 3000 photo is a small file
    // of 27 MB of pixels, and it costs a small fraction of that.
    let smooth = smooth_jpeg(3000, 3000);
    let cost = render_peak(&smooth).saturating_sub(render_peak(&small));
    eprintln!("a {} byte 3000x3000 photo cost {cost} bytes", smooth.len());
    assert!(
        cost < 3000 * 3000 / 4,
        "a {} byte photo of 3000x3000 px cost {cost} bytes",
        smooth.len()
    );
}

fn smooth_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x / 12) as u8, (y / 12) as u8, 128])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&img)
        .expect("encodes");
    out
}
