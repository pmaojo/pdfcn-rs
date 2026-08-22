//! Post-render asset preparation: everything that must happen to the HTML +
//! image-map pair just before `printpdf` lays it out, because the layout
//! engine can't do it itself.
//!
//! Three passes today, all driven by scanning `<img>` tags in the rendered
//! markup:
//!
//! 1. **QR codes** — a `%QRCode(value="…")` component (see
//!    `pdfcn-components`) emits `<img src="pdfcn-qrcode:<hex payload>">`.
//!    This pass decodes the payload, renders the QR matrix into a real PNG
//!    (quiet zone included) and registers it under that exact `src`, so the
//!    layout engine just sees an ordinary embedded image.
//! 2. **`object-fit: cover`** — the engine scales an image to fill its box
//!    on *both* axes and ignores `object-fit`, stretching mismatched
//!    aspect ratios. For an `<img>` that declares a px width, a px height
//!    and `object-fit: cover`, and whose bytes we have, this pass
//!    center-crops the source to the box's aspect ratio first — so the
//!    engine's "scale to fill" then produces exactly what a browser would
//!    show. The `<img src>` is rewritten to the cropped variant, leaving
//!    the original bytes untouched for any other reference.
//! 3. **Resolution normalization** — a source image far larger than its
//!    layout box can ever paint is dead weight in the output PDF (and in
//!    layout time). Any image whose box is known in px is downscaled to
//!    ~300dpi of that box (3x, see `MAX_PRINT_SCALE`); any image is capped
//!    at `MAX_IMAGE_DIMENSION_PX` on a side. Shrink-only, format-preserving.
//!
//! Like `img_srcs`, the scanner relies on our own `maud` output being
//! well-formed: double-quoted attributes, no `<`/`>` inside values.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

const QRCODE_SCHEME: &str = "pdfcn-qrcode:";
/// Payload cap before the QR encoder would need the largest symbol
/// versions; anything bigger degrades to a broken-image placeholder rather
/// than risking an oversized encode.
const MAX_QRCODE_PAYLOAD_BYTES: usize = 1000;
/// Modules of white border around the symbol (spec-recommended minimum 4).
const QR_QUIET_ZONE: i32 = 4;
/// Output pixels per QR module.
const QR_MODULE_SCALE: i32 = 8;

/// One `<img ...>` occurrence: the tag's span and its attribute text.
pub(crate) struct ImgTag {
    span: Range<usize>,
    pub(crate) attrs: String,
}

/// Scans `html` for `<img ...>` tags (open-tag span only; our renderer may
/// emit a matching `</img>` which this deliberately ignores).
pub(crate) fn scan_img_tags(html: &str) -> Vec<ImgTag> {
    let mut tags = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = html[from..].find("<img") {
        let start = from + rel;
        let Some(gt_rel) = html[start..].find('>') else {
            break;
        };
        let end = start + gt_rel + 1;
        tags.push(ImgTag {
            span: start..end,
            attrs: html[start..end].to_string(),
        });
        from = end;
        if from >= html.len() {
            break;
        }
    }
    tags
}

/// Value of `name="…"` within an `<img>` attribute string.
pub(crate) fn attr_value<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=\"");
    let start = attrs.find(&marker)? + marker.len();
    let end = start + attrs[start..].find('"')?;
    Some(&attrs[start..end])
}

/// A handful of parsed `style="…"` declarations we care about.
struct BoxStyle {
    width_px: Option<f64>,
    height_px: Option<f64>,
    object_fit: Option<String>,
}

fn parse_style(style: &str) -> BoxStyle {
    let mut out = BoxStyle {
        width_px: None,
        height_px: None,
        object_fit: None,
    };
    for decl in style.split(';') {
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        match prop.as_str() {
            "width" => out.width_px = px_value(value),
            "height" => out.height_px = px_value(value),
            "object-fit" => out.object_fit = Some(value.to_ascii_lowercase()),
            _ => {}
        }
    }
    out
}

/// Parses `120px` (and plain `120`, as the engine treats unitless as px)
/// into a pixel count; percentages and other units return None.
fn px_value(value: &str) -> Option<f64> {
    let numeric = value.strip_suffix("px").unwrap_or(value);
    numeric.trim().parse::<f64>().ok()
}

/// Hex-encodes `bytes` (lowercase) for a `pdfcn-qrcode:` src.
#[cfg(test)]
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Generates PNG bytes for one QR payload: a black-on-white symbol with a
/// quiet zone, at [`QR_MODULE_SCALE`] px per module.
fn qrcode_png(payload: &[u8]) -> Option<Vec<u8>> {
    use qrcodegen::{QrCode, QrCodeEcc};

    if payload.is_empty() || payload.len() > MAX_QRCODE_PAYLOAD_BYTES {
        return None;
    }
    let qr = QrCode::encode_binary(payload, QrCodeEcc::Medium).ok()?;
    let size = qr.size();
    let dim = (size + 2 * QR_QUIET_ZONE) as u32 * QR_MODULE_SCALE as u32;
    let mut img = image::RgbImage::from_pixel(dim, dim, image::Rgb([255, 255, 255]));
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                let x0 = ((x + QR_QUIET_ZONE) as u32) * QR_MODULE_SCALE as u32;
                let y0 = ((y + QR_QUIET_ZONE) as u32) * QR_MODULE_SCALE as u32;
                for dy in 0..QR_MODULE_SCALE as u32 {
                    for dx in 0..QR_MODULE_SCALE as u32 {
                        img.put_pixel(x0 + dx, y0 + dy, image::Rgb([0, 0, 0]));
                    }
                }
            }
        }
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// Pass 1: for every `<img src="pdfcn-qrcode:…">`, generate the PNG and
/// register it under that exact src (no HTML rewrite needed — the layout
/// engine resolves it like any caller-supplied image).
fn generate_qrcodes(html: &str, images: &mut BTreeMap<String, Vec<u8>>) {
    let srcs: Vec<String> = scan_img_tags(html)
        .iter()
        .filter_map(|tag| attr_value(&tag.attrs, "src").map(str::to_string))
        .filter(|src| src.starts_with(QRCODE_SCHEME))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    for src in srcs {
        if images.contains_key(&src) {
            continue;
        }
        let Some(payload) = hex_decode(&src[QRCODE_SCHEME.len()..]) else {
            continue;
        };
        if let Some(png) = qrcode_png(&payload) {
            images.insert(src, png);
        }
    }
}

/// Pass 2: center-crops source bytes to an `<img>` box's aspect ratio when
/// the markup asks for `object-fit: cover` with a px box, rewriting the
/// `src` to the cropped variant. Returns the rewritten HTML.
fn apply_cover_crops(html: &str, images: &mut BTreeMap<String, Vec<u8>>) -> String {
    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
    let mut cropped: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for tag in scan_img_tags(html) {
        let Some(src) = attr_value(&tag.attrs, "src").map(str::to_string) else {
            continue;
        };
        let Some(style) = attr_value(&tag.attrs, "style") else {
            continue;
        };
        let style = parse_style(style);
        let (Some(width), Some(height)) = (style.width_px, style.height_px) else {
            continue;
        };
        if style.object_fit.as_deref() != Some("cover") || width <= 0.0 || height <= 0.0 {
            continue;
        }
        let Some(source_bytes) = images.get(&src) else {
            continue;
        };
        let key = format!("{src}#pdfcn-cover-{width}x{height}");
        if !cropped.contains_key(&key) && !images.contains_key(&key) {
            let Some(bytes) = center_crop_to_aspect(source_bytes, width / height) else {
                continue;
            };
            cropped.insert(key.clone(), bytes);
        }
        // The attr value's span inside the full tag: find it again in the
        // tag text to rewrite just the src.
        let marker = format!("src=\"{src}\"");
        if let Some(rel) = tag.attrs.find(&marker) {
            let val_start = tag.span.start + rel + "src=\"".len();
            replacements.push((
                val_start..val_start + src.len(),
                key,
            ));
        }
    }
    for (key, bytes) in cropped {
        images.insert(key, bytes);
    }
    let mut out = html.to_string();
    // Back to front so earlier spans stay valid.
    replacements.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, key) in replacements {
        out.replace_range(range, &key);
    }
    out
}

/// Decodes `bytes` (JPEG/PNG/…), center-crops to `aspect` (w/h), and
/// re-encodes as PNG. Returns None for anything undecodable — the caller
/// leaves the original in place, degrading to the engine's stretch-fill
/// rather than failing the render.
fn center_crop_to_aspect(bytes: &[u8], aspect: f64) -> Option<Vec<u8>> {
    if !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (iw, ih) = (img.width(), img.height());
    if iw == 0 || ih == 0 {
        return None;
    }
    let source_aspect = iw as f64 / ih as f64;
    if (source_aspect - aspect).abs() < 1e-3 {
        // Already the right shape: re-encoding would only lose quality.
        return None;
    }
    let (cw, ch) = if source_aspect > aspect {
        let cw = ((ih as f64 * aspect).round() as u32).clamp(1, iw);
        (cw, ih)
    } else {
        let ch = ((iw as f64 / aspect).round() as u32).clamp(1, ih);
        (iw, ch)
    };
    let x = (iw - cw) / 2;
    let y = (ih - ch) / 2;
    let mut png = Vec::new();
    img.crop_imm(x, y, cw, ch)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// How many device pixels of source resolution one CSS px of layout box
/// may carry. PDF pixels map to points at 96/inch; print wants ~300dpi,
/// so 3x keeps every image above print quality while capping waste -- a
/// 4000px logo dropped into an 80px-tall card slot ships ~48x more pixels
/// than it can ever paint.
const MAX_PRINT_SCALE: f64 = 3.0;

/// Absolute cap on either dimension of any embedded image, even one with
/// no known box: beyond this, no plausible page size can use the extra
/// detail and decode/encode cost grows quadratically.
const MAX_IMAGE_DIMENSION_PX: u32 = 4096;

/// Pass 3: downscales over-resolved sources to what their layout boxes can
/// actually paint (`box px * [`MAX_PRINT_SCALE`]`), and caps any image at
/// [`MAX_IMAGE_DIMENSION_PX`] on a side regardless. Shrinks only -- never
/// upscales, never touches an image already within limits -- and re-encodes
/// in the source's format (JPEG stays JPEG, everything else PNG), so the
/// caller's `<img>` markup needs no rewriting. An undecodable source is
/// left alone, degrading exactly as before.
fn normalize_resolutions(html: &str, images: &mut BTreeMap<String, Vec<u8>>) {
    // The widest/tallest box each src paints into governs its limit.
    let mut limits: BTreeMap<String, (Option<f64>, Option<f64>)> = BTreeMap::new();
    for tag in scan_img_tags(html) {
        let Some(src) = attr_value(&tag.attrs, "src").map(str::to_string) else {
            continue;
        };
        let (w, h) = attr_value(&tag.attrs, "style")
            .map(parse_style)
            .map(|s| (s.width_px, s.height_px))
            .unwrap_or((None, None));
        let entry = limits.entry(src).or_insert((None, None));
        // Only a tag that actually declares a dimension raises that
        // dimension's limit; an absent style never shrinks it.
        if let Some(w) = w {
            entry.0 = Some(entry.0.map_or(w, |prev| prev.max(w)));
        }
        if let Some(h) = h {
            entry.1 = Some(entry.1.map_or(h, |prev| prev.max(h)));
        }
    }
    let replacements: Vec<(String, Vec<u8>)> = limits
        .into_iter()
        .filter_map(|(src, (box_w, box_h))| {
            let bytes = images.get(&src)?;
            let shrunk = downscale_to_limits(bytes, box_w, box_h)?;
            Some((src, shrunk))
        })
        .collect();
    for (src, bytes) in replacements {
        images.insert(src, bytes);
    }
}

/// The shrunken re-encode of `bytes`, or `None` when it's already within
/// its limits (or undecodable) and should be left exactly as-is.
fn downscale_to_limits(
    bytes: &[u8],
    box_w_px: Option<f64>,
    box_h_px: Option<f64>,
) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let (iw, ih) = (img.width(), img.height());
    if iw == 0 || ih == 0 {
        return None;
    }
    let mut max_scale = f64::INFINITY;
    if let Some(w) = box_w_px.filter(|w| *w > 0.0) {
        max_scale = max_scale.min((w * MAX_PRINT_SCALE) / iw as f64);
    }
    if let Some(h) = box_h_px.filter(|h| *h > 0.0) {
        max_scale = max_scale.min((h * MAX_PRINT_SCALE) / ih as f64);
    }
    let cap = MAX_IMAGE_DIMENSION_PX as f64;
    max_scale = max_scale.min(cap / iw as f64).min(cap / ih as f64);
    if !max_scale.is_finite() || max_scale >= 1.0 {
        return None;
    }
    let nw = (((iw as f64) * max_scale).round() as u32).max(1);
    let nh = (((ih as f64) * max_scale).round() as u32).max(1);
    let resized = img.resize(nw, nh, image::imageops::FilterType::Triangle);
    let mut out = Vec::new();
    if bytes.starts_with(&[0xFF, 0xD8]) {
        image::DynamicImage::ImageRgb8(resized.into_rgb8())
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
            .ok()?;
    } else {
        resized
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
    }
    Some(out)
}

/// Runs every pass over the rendered HTML + image map, in place for the
/// map and by value for the HTML. Call this right before handing both to
/// `printpdf`. Cover cropping runs first, slicing from each source's full
/// resolution; resolution normalization runs last, capping everything at
/// print scale without disturbing sources that already have a crop.
pub fn prepare_assets(html: &str, images: &mut BTreeMap<String, Vec<u8>>) -> String {
    // Cover-crop before normalizing: a crop slices from the source's full
    // resolution, so it must run while those bytes are intact --
    // normalizing first would shrink the pool it samples and bake that
    // loss into every variant (a 400x100 source into a 100px square box
    // would come out 75x75 instead of 100x100).
    let rewritten = apply_cover_crops(html, images);
    generate_qrcodes(html, images);
    // The original stays registered under its own src untouched, so
    // normalization -- last, capping everything at box * MAX_PRINT_SCALE --
    // parks any src that has a cropped variant aside: its visual role is
    // carried by the variant alone.
    let cropped_bases: BTreeSet<String> = images
        .keys()
        .filter_map(|key| key.split_once("#pdfcn-cover-").map(|(base, _)| base.to_string()))
        .collect();
    let parked: Vec<(String, Vec<u8>)> = cropped_bases
        .into_iter()
        .filter_map(|base| images.remove(&base).map(|bytes| (base, bytes)))
        .collect();
    normalize_resolutions(html, images);
    for (base, bytes) in parked {
        images.insert(base, bytes);
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb(color));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn generates_and_registers_a_qrcode_for_the_placeholder_src() {
        let value = "https://example.com/pay/INV-1042";
        let html = format!(r#"<img src="{QRCODE_SCHEME}{}" style="width:96px">"#, hex_encode(value.as_bytes()));
        let mut images = BTreeMap::new();
        let out = prepare_assets(&html, &mut images);
        let src = out
            .split("src=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        let bytes = images.get(src).expect("qr png registered");
        let decoded = image::load_from_memory(bytes).expect("valid png");
        // Square symbol, quiet-zone-included dimension is a multiple of the
        // module scale.
        let w = decoded.width();
        let h = decoded.height();
        assert_eq!(w, h);
        assert_eq!(w % QR_MODULE_SCALE as u32, 0);
    }

    #[test]
    fn an_undecodable_qrcode_payload_degrades_to_a_broken_image() {
        let html = format!(r#"<img src="{QRCODE_SCHEME}zzzz">"#);
        let mut images = BTreeMap::new();
        prepare_assets(&html, &mut images);
        assert!(images.is_empty());
    }

    #[test]
    fn qrcode_payloads_over_the_cap_are_skipped() {
        let payload = vec![b'x'; MAX_QRCODE_PAYLOAD_BYTES + 1];
        assert!(qrcode_png(&payload).is_none());
        assert!(qrcode_png(b"ok").is_some());
    }

    #[test]
    fn cover_crops_the_source_to_the_box_aspect_and_rewrites_src() {
        // 400x100 source into a 100x100 box: cover must crop to 100x100.
        let src = "photo.png";
        let html = format!(
            r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover">"#
        );
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(400, 100, [10, 20, 30]))]);
        let out = prepare_assets(&html, &mut images);
        assert!(out.contains(&format!("src=\"{src}#pdfcn-cover-")), "{out}");
        let cropped_key = out
            .split("src=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        let decoded = image::load_from_memory(&images[cropped_key]).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (100, 100));
        // The original bytes stay registered under their own src.
        assert_eq!(images[src].len(), png_bytes(400, 100, [10, 20, 30]).len());
    }

    #[test]
    fn matching_aspect_is_not_reencoded() {
        let src = "photo.png";
        let html = format!(
            r#"<img src="{src}" style="width:200px;height:100px;object-fit:cover">"#
        );
        let original = png_bytes(400, 200, [1, 2, 3]);
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        let out = prepare_assets(&html, &mut images);
        assert!(!out.contains("#pdfcn-cover-"), "{out}");
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn non_cover_object_fit_and_missing_dimensions_are_ignored() {
        let src = "photo.png";
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(400, 100, [1, 2, 3]))]);
        for style in [
            "width:100px;height:100px;object-fit:contain",
            "width:100px;height:100px",
            "width:100%;height:100px;object-fit:cover",
        ] {
            let html = format!(r#"<img src="{src}" style="{style}">"#);
            let out = prepare_assets(&html, &mut images);
            assert_eq!(out, html, "style {style} must be untouched");
        }
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn undecodable_source_bytes_are_left_alone() {
        let src = "broken.png";
        let html = format!(
            r#"<img src="{src}" style="width:100px;height:50px;object-fit:cover">"#
        );
        let mut images = BTreeMap::from([(src.to_string(), b"not-an-image".to_vec())]);
        let out = prepare_assets(&html, &mut images);
        assert_eq!(out, html);
    }

    #[test]
    fn two_boxes_over_one_source_get_two_crops() {
        let src = "photo.png";
        // 800x200 source (aspect 4.0) differs from both box aspects (1.0
        // and 2.0), so both imgs get their own cropped variant.
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(800, 200, [1, 2, 3]))]);
        let html = format!(
            r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover">"#
        ) + &format!(
            r#"<img src="{src}" style="width:200px;height:100px;object-fit:cover">"#
        );
        let out = prepare_assets(&html, &mut images);
        assert_eq!(out.matches("#pdfcn-cover-").count(), 2, "{out}");
        assert_eq!(images.len(), 3); // original + two crops
    }

    /// A source far beyond its box's print resolution is downscaled to
    /// box_px * 3 (~300dpi), shrinking the embedded bytes.
    #[test]
    fn an_oversized_source_is_downscaled_to_its_boxs_print_resolution() {
        let src = "photo.png";
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(2000, 1000, [1, 2, 3]))]);
        let html = format!(r#"<img src="{src}" style="width:100px;height:50px">"#);
        prepare_assets(&html, &mut images);
        let decoded = image::load_from_memory(&images[src]).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (300, 150));
    }

    #[test]
    fn a_source_within_its_boxs_resolution_is_untouched() {
        let src = "photo.png";
        let original = png_bytes(300, 150, [1, 2, 3]);
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        let html = format!(r#"<img src="{src}" style="width:100px;height:50px">"#);
        prepare_assets(&html, &mut images);
        assert_eq!(images[src].len(), original.len());
    }

    /// Even with no known box, a monster image is capped at
    /// MAX_IMAGE_DIMENSION_PX on its longest side.
    #[test]
    fn a_boxless_monster_image_is_capped_at_the_absolute_dimension_limit() {
        let src = "photo.png";
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(5000, 2500, [1, 2, 3]))]);
        let html = format!(r#"<img src="{src}">"#);
        prepare_assets(&html, &mut images);
        let decoded = image::load_from_memory(&images[src]).unwrap();
        assert_eq!(decoded.width(), MAX_IMAGE_DIMENSION_PX);
    }

    /// JPEG sources stay JPEG (magic bytes) after normalization, so the
    /// engine's decoder sees the same format it would have.
    #[test]
    fn jpeg_sources_stay_jpeg_after_downscaling() {
        let src = "photo.jpg";
        let jpeg = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2000,
            1000,
            image::Rgb([10, 20, 30]),
        ));
        let mut bytes = Vec::new();
        jpeg.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        let mut images = BTreeMap::from([(src.to_string(), bytes)]);
        let html = format!(r#"<img src="{src}" style="width:100px;height:50px">"#);
        prepare_assets(&html, &mut images);
        let shrunk = &images[src];
        assert!(shrunk.starts_with(&[0xFF, 0xD8]), "must stay JPEG");
        let decoded = image::load_from_memory(shrunk).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (300, 150));
    }

    #[test]
    fn an_undecodable_source_survives_normalization_untouched() {
        let src = "broken.png";
        let original = b"not-an-image".to_vec();
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        let html = format!(r#"<img src="{src}" style="width:100px">"#);
        prepare_assets(&html, &mut images);
        assert_eq!(images[src], original);
    }
}
