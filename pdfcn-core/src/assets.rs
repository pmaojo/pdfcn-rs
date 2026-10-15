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
//! 3. **Resolution normalization** (opt-in, `RenderOptions::shrink_images`;
//!    `pdfcn images` is the tool for preparing photos) — a source image far larger than its
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
/// The Wave 1 vector-substrate schemes (`%Vector`, Charts v2, `%Barcode`).
/// Charts and barcodes carry their (small, self-describing) spec hex-encoded
/// in the src -- the exact convention `%QRCode` proved; arbitrary caller
/// SVG travels through `RenderOptions::svg_assets` keyed by the id instead,
/// because an XML document doesn't belong URL-encoded inside an attribute.
#[cfg(feature = "vector")]
const VECTOR_SCHEME: &str = "pdfcn-vector:";
#[cfg(feature = "vector")]
const CHART_SCHEME: &str = "pdfcn-chart:";
#[cfg(feature = "vector")]
const BARCODE_SCHEME: &str = "pdfcn-barcode:";
/// `%Icon`: the placeholder names an icon, colour and stroke width; the SVG is
/// built here from the icon set (see `pdfcn_components::icon_svg`).
#[cfg(feature = "vector")]
const ICON_SCHEME: &str = "pdfcn-icon:";
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
    object_position: Option<String>,
}

fn parse_style(style: &str) -> BoxStyle {
    let mut out = BoxStyle {
        width_px: None,
        height_px: None,
        object_fit: None,
        object_position: None,
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
            "object-position" => out.object_position = Some(value.to_ascii_lowercase()),
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

/// Splits what follows `pdfcn-qrcode:` into the error-correction level and
/// the hex payload. A bare payload is level M (what `%QRCode` always meant);
/// `L.`, `Q.` or `H.` in front selects another level, for the tax authorities
/// whose QR specification asks for one. Uppercase can never be mistaken for the
/// lowercase hex digits that follow.
fn split_qrcode_src(rest: &str) -> (qrcodegen::QrCodeEcc, &str) {
    use qrcodegen::QrCodeEcc;
    match rest.split_once('.') {
        Some(("L", hex)) => (QrCodeEcc::Low, hex),
        Some(("Q", hex)) => (QrCodeEcc::Quartile, hex),
        Some(("H", hex)) => (QrCodeEcc::High, hex),
        _ => (QrCodeEcc::Medium, rest),
    }
}

/// Generates PNG bytes for one QR payload: a black-on-white symbol with a
/// quiet zone, at [`QR_MODULE_SCALE`] px per module.
fn qrcode_png(payload: &[u8], ecc: qrcodegen::QrCodeEcc) -> Option<Vec<u8>> {
    use qrcodegen::QrCode;

    if payload.is_empty() || payload.len() > MAX_QRCODE_PAYLOAD_BYTES {
        return None;
    }
    let qr = QrCode::encode_binary(payload, ecc).ok()?;
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

/// The same symbol as [`qrcode_png`], as SVG: one path, one unit square per
/// dark module, the quiet zone kept in the viewBox. It lets the QR land in the
/// PDF as vectors (see `vector_pdf`) while the PNG stays the layout's input.
fn qrcode_svg(payload: &[u8], ecc: qrcodegen::QrCodeEcc) -> Option<String> {
    use qrcodegen::QrCode;
    use std::fmt::Write as _;

    if payload.is_empty() || payload.len() > MAX_QRCODE_PAYLOAD_BYTES {
        return None;
    }
    let qr = QrCode::encode_binary(payload, ecc).ok()?;
    let size = qr.size();
    let dim = size + 2 * QR_QUIET_ZONE;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                let _ = write!(path, "M{} {}h1v1h-1z", x + QR_QUIET_ZONE, y + QR_QUIET_ZONE);
            }
        }
    }
    Some(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {dim} {dim}" shape-rendering="crispEdges"><rect width="{dim}" height="{dim}" fill="#fff"/><path d="{path}" fill="#000"/></svg>"##
    ))
}

/// Pass 1: for every `<img src="pdfcn-qrcode:…">`, generate the PNG and
/// register it under that exact src (no HTML rewrite needed — the layout
/// engine resolves it like any caller-supplied image). Its SVG goes into
/// `sources` under the same src, for the vector pass.
fn generate_qrcodes(
    html: &str,
    images: &mut BTreeMap<String, Vec<u8>>,
    sources: &mut BTreeMap<String, String>,
) {
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
        let (ecc, hex) = split_qrcode_src(&src[QRCODE_SCHEME.len()..]);
        let Some(payload) = hex_decode(hex) else {
            continue;
        };
        if let Some(png) = qrcode_png(&payload, ecc) {
            if let Some(svg) = qrcode_svg(&payload, ecc) {
                sources.insert(src.clone(), svg);
            }
            images.insert(src, png);
        }
    }
}

/// Charts v2 specs (series + labels + colors, hex-encoded whole) can make
/// `<img src>` far longer than `%Vector`'s caller-chosen id or `%Barcode`'s
/// value ever get. printpdf turns a `src` straight into a PDF Name for the
/// image's XObject resource (`HtmlImg_<src>`, one-to-one, no truncation or
/// hashing on its side) -- verified empirically against printpdf itself,
/// isolated from the rest of this pipeline: a ~130-byte src round-trips
/// fine, but a ~240+ byte one is registered in `/Resources/XObject` under
/// its full name and *also* referenced by that exact name in the content
/// stream's `Do` operator (both sides agree, printpdf isn't at fault for a
/// mismatch), yet real PDF readers (MuPDF, and whatever renders it in a
/// browser) refuse to resolve a name that long and the image comes out
/// blank. Rewriting any placeholder src past this length to a short
/// hash-based one keeps the pipeline self-describing -- the hash is
/// computed and resolved within this same pass, nothing external needed --
/// while staying well under whatever limit real readers enforce.
#[cfg(feature = "vector")]
const MAX_SAFE_PLACEHOLDER_SRC_LEN: usize = 100;

#[cfg(feature = "vector")]
fn short_placeholder_src(scheme: &str, src: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut hasher);
    format!("{scheme}h{:016x}", hasher.finish())
}

/// Pass 1b (Ola 1.2): fills every vector-substrate placeholder -- `%Vector`
/// side-channel lookups, Charts v2 specs and `%Barcode` payloads -- with real
/// PNG bytes registered under the placeholder's own src (or a short
/// hash-based stand-in for an overlong one, see
/// [`MAX_SAFE_PLACEHOLDER_SRC_LEN`]), rewriting the HTML to match. A
/// placeholder whose SVG can't be produced (unknown id, malformed spec,
/// unencodable payload) is left unresolved and degrades exactly like any
/// missing image; nothing here fails or panics the render.
/// `sources` receives each placeholder's SVG text under the same key as its
/// PNG, so the PNG can be swapped for real vectors once the PDF is saved
/// (see `vector_pdf`); the PNG stays the layout's input and the fallback.
/// No-op (and compiles to nothing, returning `html` unchanged) without the
/// `vector` cargo feature.
pub(crate) fn generate_vectors(
    html: &str,
    images: &mut BTreeMap<String, Vec<u8>>,
    svg_assets: &BTreeMap<String, String>,
    sources: &mut BTreeMap<String, String>,
) -> String {
    #[cfg(feature = "vector")]
    {
        use crate::{barcode, charts_svg, raster};
        use serde_json::Value as JsonValue;

        let mut boxes: BTreeMap<String, (Option<f64>, Option<f64>)> = BTreeMap::new();
        for tag in scan_img_tags(html) {
            let Some(src) = attr_value(&tag.attrs, "src").map(str::to_string) else {
                continue;
            };
            if !(src.starts_with(VECTOR_SCHEME)
                || src.starts_with(CHART_SCHEME)
                || src.starts_with(BARCODE_SCHEME)
                || src.starts_with(ICON_SCHEME))
            {
                continue;
            }
            let (w, h) = attr_value(&tag.attrs, "style")
                .map(parse_style)
                .map(|s| (s.width_px, s.height_px))
                .unwrap_or((None, None));
            let entry = boxes.entry(src).or_insert((None, None));
            if let Some(w) = w {
                entry.0 = Some(entry.0.map_or(w, |prev| prev.max(w)));
            }
            if let Some(h) = h {
                entry.1 = Some(entry.1.map_or(h, |prev| prev.max(h)));
            }
        }

        let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
        for (src, (box_w, box_h)) in boxes {
            let short_src = if src.len() > MAX_SAFE_PLACEHOLDER_SRC_LEN {
                let scheme = [VECTOR_SCHEME, CHART_SCHEME, BARCODE_SCHEME, ICON_SCHEME]
                    .into_iter()
                    .find(|s| src.starts_with(s))
                    .unwrap_or("");
                Some(short_placeholder_src(scheme, &src))
            } else {
                None
            };
            let registered_key = short_src.as_ref().unwrap_or(&src);
            if !images.contains_key(registered_key) {
                // One closure so any step can bail out with `?`: an
                // unfillable placeholder is simply left unresolved. Yields
                // the SVG and the box to rasterize it at.
                let svg = (|| {
                    if let Some(id) = src.strip_prefix(VECTOR_SCHEME) {
                        return Some((svg_assets.get(id)?.clone(), box_w, box_h));
                    }
                    if let Some(payload) = src.strip_prefix(ICON_SCHEME) {
                        let (name, color, stroke) = pdfcn_components::decode_icon_payload(payload)?;
                        let svg = pdfcn_components::icon_svg(&name, &color, stroke.as_deref())?;
                        return Some((svg, box_w, box_h));
                    }
                    if let Some(spec_hex) = src.strip_prefix(CHART_SCHEME) {
                        let bytes = hex_decode(spec_hex)?;
                        let spec: JsonValue = serde_json::from_slice(&bytes).ok()?;
                        return Some((charts_svg::chart_svg(&spec)?, box_w, box_h));
                    }
                    if let Some(rest) = src.strip_prefix(BARCODE_SCHEME) {
                        let (scheme, hex_value) = rest.split_once(':')?;
                        let bytes = hex_decode(hex_value)?;
                        let value = String::from_utf8(bytes).ok()?;
                        let w = box_w.unwrap_or(240.0);
                        let h = box_h.unwrap_or(60.0);
                        let svg = barcode::barcode_svg(scheme, &value, w, h)?;
                        return Some((svg, Some(w), Some(h)));
                    }
                    None
                })();
                let png = svg.and_then(|(svg, w, h)| {
                    let png = raster::svg_to_png(&svg, w, h)?;
                    sources.insert(registered_key.clone(), svg);
                    Some(png)
                });
                if let Some(png) = png {
                    images.insert(registered_key.clone(), png);
                } else if short_src.is_some() {
                    // Nothing to embed, so no rewrite either -- leave the
                    // original (unresolvable) src in place, same as any
                    // other missing image.
                    continue;
                }
            }
            if let Some(short) = short_src {
                // The same src can appear in more than one `<img>` tag (the
                // same chart placed twice, say) -- every occurrence needs
                // the rewrite, not just the first.
                let marker = format!("src=\"{src}\"");
                for (rel, _) in html.match_indices(&marker) {
                    let val_start = rel + "src=\"".len();
                    replacements.push((val_start..val_start + src.len(), short.clone()));
                }
            }
        }

        let mut out = html.to_string();
        replacements.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        for (range, key) in replacements {
            out.replace_range(range, &key);
        }
        out
    }
    #[cfg(not(feature = "vector"))]
    {
        let _ = (images, svg_assets, sources);
        html.to_string()
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
        // This pass can only crop around the center. An `object-position`
        // other than the center is left to the layout engine, which crops
        // from the right anchor (it fits and clips the full image itself).
        if style
            .object_position
            .as_deref()
            .is_some_and(|p| !matches!(p, "center" | "center center" | "50% 50%"))
        {
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
            replacements.push((val_start..val_start + src.len(), key));
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

/// Decodes `bytes` and applies the image's EXIF orientation, so what comes out
/// is the picture as a viewer shows it. A phone held upright writes landscape
/// pixels plus an orientation tag; everything downstream of here (the layout
/// engine, printpdf) ignores the tag, so without this step such a photo lands
/// in the PDF sideways, or squeezed into a portrait box. Returns `None` for
/// anything undecodable.
pub(crate) fn decode_oriented(bytes: &[u8]) -> Option<image::DynamicImage> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).ok()?;
    img.apply_orientation(orientation);
    Some(img)
}

/// The EXIF orientation recorded in `bytes`, without decoding any pixels.
fn exif_orientation(bytes: &[u8]) -> image::metadata::Orientation {
    use image::ImageDecoder;
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
        .and_then(|mut d| d.orientation().ok())
        .unwrap_or(image::metadata::Orientation::NoTransforms)
}

/// Pass 0: turns every image whose EXIF orientation is not "as stored" into
/// the picture as displayed (rotated or mirrored pixels), re-encoded as the
/// same kind of file. Runs first and on every referenced image, not only on
/// those another pass would re-encode: an untouched photo would otherwise keep
/// its sideways pixels. Images without an orientation tag are left byte for
/// byte as they were.
fn apply_exif_orientation(html: &str, images: &mut BTreeMap<String, Vec<u8>>) {
    let srcs: BTreeSet<String> = scan_img_tags(html)
        .iter()
        .filter_map(|tag| attr_value(&tag.attrs, "src").map(str::to_string))
        .collect();
    for src in srcs {
        let Some(bytes) = images.get(&src) else {
            continue;
        };
        if exif_orientation(bytes) == image::metadata::Orientation::NoTransforms {
            continue;
        }
        let Some(img) = decode_oriented(bytes) else {
            continue;
        };
        let is_jpeg = bytes.starts_with(&[0xFF, 0xD8]);
        let mut out = Vec::new();
        let written = if is_jpeg {
            encode_jpeg(&img, &mut out).is_some()
        } else {
            img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .is_ok()
        };
        if written {
            images.insert(src, out);
        }
    }
}

/// Decodes `bytes` (JPEG/PNG/…), center-crops to `aspect` (w/h), and
/// re-encodes it: a JPEG stays a JPEG (so it can still go into the PDF
/// without another re-encode), anything else becomes a PNG. Returns None for
/// anything undecodable — the caller leaves the original in place, degrading
/// to the engine's stretch-fill rather than failing the render.
fn center_crop_to_aspect(bytes: &[u8], aspect: f64) -> Option<Vec<u8>> {
    if !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    // The shape comes from the header: a source that already fits its box
    // (the common case) costs no decode at all.
    let (iw, ih) = oriented_dimensions(bytes)?;
    if iw == 0 || ih == 0 {
        return None;
    }
    let source_aspect = iw as f64 / ih as f64;
    if (source_aspect - aspect).abs() < 1e-3 {
        // Already the right shape: re-encoding would only lose quality.
        return None;
    }
    let img = decode_oriented(bytes)?;
    let (iw, ih) = (img.width(), img.height());
    let (cw, ch) = if source_aspect > aspect {
        let cw = ((ih as f64 * aspect).round() as u32).clamp(1, iw);
        (cw, ih)
    } else {
        let ch = ((iw as f64 / aspect).round() as u32).clamp(1, ih);
        (iw, ch)
    };
    let x = (iw - cw) / 2;
    let y = (ih - ch) / 2;
    let cropped = img.crop_imm(x, y, cw, ch);
    let mut out = Vec::new();
    if bytes.starts_with(&[0xFF, 0xD8]) {
        encode_jpeg(&cropped, &mut out)?;
    } else {
        cropped
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
    }
    Some(out)
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
/// The widest and tallest box a page can offer, in CSS px: a percentage or
/// `auto` dimension can never exceed it, so it is a safe stand-in for a size
/// only layout knows.
pub(crate) type PageBound = (f64, f64);

fn normalize_resolutions(
    html: &str,
    images: &mut BTreeMap<String, Vec<u8>>,
    page: Option<PageBound>,
) {
    // The widest/tallest box each src paints into governs its limit.
    struct Limit {
        w: Option<f64>,
        h: Option<f64>,
        cover: bool,
    }
    let mut limits: BTreeMap<String, Limit> = BTreeMap::new();
    for tag in scan_img_tags(html) {
        let Some(src) = attr_value(&tag.attrs, "src").map(str::to_string) else {
            continue;
        };
        let style = attr_value(&tag.attrs, "style").map(parse_style);
        let (w, h, cover) = match &style {
            Some(s) => (
                s.width_px,
                s.height_px,
                s.object_fit.as_deref() == Some("cover"),
            ),
            None => (None, None, false),
        };
        let entry = limits.entry(src).or_insert(Limit {
            w: None,
            h: None,
            cover: false,
        });
        // Only a tag that actually declares a dimension raises that
        // dimension's limit; an absent style never shrinks it.
        if let Some(w) = w {
            entry.w = Some(entry.w.map_or(w, |prev| prev.max(w)));
        }
        if let Some(h) = h {
            entry.h = Some(entry.h.map_or(h, |prev| prev.max(h)));
        }
        entry.cover |= cover;
    }
    let replacements: Vec<(String, Vec<u8>)> = limits
        .into_iter()
        .filter_map(|(src, limit)| {
            let bytes = images.get(&src)?;
            let shrunk = downscale_to_limits(bytes, limit.w, limit.h, limit.cover, page)?;
            Some((src, shrunk))
        })
        .collect();
    for (src, bytes) in replacements {
        images.insert(src, bytes);
    }
}

/// Below this fraction of a source's size, shrinking it is worth a decode, a
/// resample and a re-encode (plus one more generation of JPEG loss). Above it
/// the source is embedded as it is: at 0.75 the saving is under half the
/// pixels, and the source can still travel into the PDF untouched (see
/// `jpeg_passthrough`), which is how Typst treats every photo. The absolute
/// [`MAX_IMAGE_DIMENSION_PX`] cap applies regardless.
const MIN_WORTHWHILE_SHRINK: f64 = 0.5;

/// Width and height of `bytes` as a viewer shows them (EXIF orientation
/// applied), read from the header alone: no pixel is decoded.
pub(crate) fn oriented_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    use image::metadata::Orientation;
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (w, h) = decoder.dimensions();
    let swapped = matches!(
        decoder.orientation().unwrap_or(Orientation::NoTransforms),
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    Some(if swapped { (h, w) } else { (w, h) })
}

/// The shrunken re-encode of `bytes`, or `None` when it's already within
/// its limits (or undecodable) and should be left exactly as-is. Decides
/// from the header alone, so a source that stays as it is never pays for a
/// decode.
fn downscale_to_limits(
    bytes: &[u8],
    box_w_px: Option<f64>,
    box_h_px: Option<f64>,
    cover: bool,
    page: Option<PageBound>,
) -> Option<Vec<u8>> {
    let (iw, ih) = oriented_dimensions(bytes)?;
    if iw == 0 || ih == 0 {
        return None;
    }
    let mut max_scale = f64::INFINITY;
    if cover && (box_w_px.is_none() || box_h_px.is_none()) {
        // `object-fit: cover` fills the box on whichever axis needs the larger
        // scale, so one known side says nothing about the resolution needed:
        // a `width:100%; height:300px` banner of a portrait photo is limited
        // by its width, not by its 300px. Bound the unknown side by the page
        // (a box cannot be larger) and keep enough pixels for the larger
        // scale; with no page bound, do not shrink at all.
        if let Some((page_w, page_h)) = page {
            let w = box_w_px.unwrap_or(page_w);
            let h = box_h_px.unwrap_or(page_h);
            let display_scale = (w / iw as f64).max(h / ih as f64);
            max_scale = MAX_PRINT_SCALE * display_scale;
        }
    } else if cover {
        // Both sides known: the cover pass already cropped to this aspect,
        // so both axes agree.
        let (w, h) = (box_w_px.unwrap_or(0.0), box_h_px.unwrap_or(0.0));
        if w > 0.0 && h > 0.0 {
            max_scale = MAX_PRINT_SCALE * (w / iw as f64).max(h / ih as f64);
        }
    } else {
        if let Some(w) = box_w_px.filter(|w| *w > 0.0) {
            max_scale = max_scale.min((w * MAX_PRINT_SCALE) / iw as f64);
        }
        if let Some(h) = box_h_px.filter(|h| *h > 0.0) {
            max_scale = max_scale.min((h * MAX_PRINT_SCALE) / ih as f64);
        }
    }
    if max_scale >= MIN_WORTHWHILE_SHRINK {
        max_scale = f64::INFINITY;
    }
    let cap = MAX_IMAGE_DIMENSION_PX as f64;
    max_scale = max_scale.min(cap / iw as f64).min(cap / ih as f64);
    if !max_scale.is_finite() || max_scale >= 1.0 {
        return None;
    }
    let nw = (((iw as f64) * max_scale).round() as u32).max(1);
    let nh = (((ih as f64) * max_scale).round() as u32).max(1);
    let img = decode_oriented(bytes)?;
    // Area averaging: at a 2x-or-more reduction it is as sharp as Lanczos3
    // and a fraction of its cost (Lanczos3 was half of an image-heavy render).
    let resized = img.thumbnail_exact(nw, nh);
    let mut out = Vec::new();
    if bytes.starts_with(&[0xFF, 0xD8]) {
        encode_jpeg(&resized, &mut out)?;
    } else {
        resized
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
    }
    Some(out)
}

/// JPEG quality for every photo this module re-encodes. High enough that the
/// re-encode is the last generation: the result goes into the PDF as it is.
const JPEG_QUALITY: u8 = 92;

/// Encodes `img` as a baseline RGB JPEG at [`JPEG_QUALITY`].
fn encode_jpeg(img: &image::DynamicImage, out: &mut Vec<u8>) -> Option<()> {
    image::codecs::jpeg::JpegEncoder::new_with_quality(out, JPEG_QUALITY)
        .encode_image(&image::DynamicImage::ImageRgb8(img.to_rgb8()))
        .ok()
}

/// The pre-vector entry point, kept for the asset pass's own test suite;
/// production code always goes through [`prepare_assets_with_vectors`] so
/// the side channel reaches the substrate.
#[cfg(test)]
pub fn prepare_assets(html: &str, images: &mut BTreeMap<String, Vec<u8>>) -> String {
    prepare_assets_with_vectors(
        html,
        images,
        &BTreeMap::new(),
        None,
        true,
        &mut BTreeMap::new(),
    )
}

/// Like [`prepare_assets`], with the vector substrate's side channel:
/// `id -> SVG source` backing `<img src="pdfcn-vector:{id}">` placeholders.
/// `shrink` runs pass 3 (resolution normalization); without it every source
/// keeps its own resolution, and preparing photos is `pdfcn images`' job.
pub(crate) fn prepare_assets_with_vectors(
    html: &str,
    images: &mut BTreeMap<String, Vec<u8>>,
    svg_assets: &BTreeMap<String, String>,
    page: Option<PageBound>,
    shrink: bool,
    vector_sources: &mut BTreeMap<String, String>,
) -> String {
    // Orientation first, on the full-resolution bytes: the crops and the
    // normalisation below must all see the picture the right way up.
    apply_exif_orientation(html, images);
    // Cover-crop before normalizing: a crop slices from the source's full
    // resolution, so it must run while those bytes are intact --
    // normalizing first would shrink the pool it samples and bake that
    // loss into every variant (a 400x100 source into a 100px square box
    // would come out 75x75 instead of 100x100).
    let rewritten = apply_cover_crops(html, images);
    generate_qrcodes(&rewritten, images, vector_sources);
    // May further rewrite `<img src>` for any Charts v2/vector/barcode
    // placeholder too long to survive as a PDF Name once printpdf turns it
    // into an XObject resource id (see `MAX_SAFE_PLACEHOLDER_SRC_LEN`).
    let rewritten = generate_vectors(&rewritten, images, svg_assets, vector_sources);
    // The original stays registered under its own src untouched, so
    // normalization -- last, capping everything at box * MAX_PRINT_SCALE --
    // parks any src that has a cropped variant aside: its visual role is
    // carried by the variant alone.
    let cropped_bases: BTreeSet<String> = images
        .keys()
        .filter_map(|key| {
            key.split_once("#pdfcn-cover-")
                .map(|(base, _)| base.to_string())
        })
        .collect();
    let parked: Vec<(String, Vec<u8>)> = cropped_bases
        .into_iter()
        .filter_map(|base| images.remove(&base).map(|bytes| (base, bytes)))
        .collect();
    if shrink {
        normalize_resolutions(&rewritten, images, page);
    }
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
        let html = format!(
            r#"<img src="{QRCODE_SCHEME}{}" style="width:96px">"#,
            hex_encode(value.as_bytes())
        );
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

    #[cfg(feature = "vector")]
    #[test]
    fn an_icon_placeholder_becomes_a_png_in_its_colour() {
        let payload: String = "receipt|#ff0000|2"
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        let html = format!(r#"<img src="pdfcn-icon:{payload}" style="width:24px;height:24px;">"#);
        let mut images = BTreeMap::new();
        generate_vectors(&html, &mut images, &BTreeMap::new(), &mut BTreeMap::new());
        let png = images
            .get(&format!("pdfcn-icon:{payload}"))
            .expect("the icon was rasterised");
        let img = image::load_from_memory(png).unwrap().to_rgba8();
        // 24 CSS px at 3x print density.
        assert_eq!((img.width(), img.height()), (72, 72));
        // Drawn in red, and transparent elsewhere.
        assert!(img.pixels().any(|p| p[0] > 200 && p[1] < 40 && p[3] > 200));
        assert!(img.pixels().any(|p| p[3] == 0));
    }

    #[cfg(feature = "vector")]
    #[test]
    fn an_unknown_icon_is_left_unresolved() {
        let payload: String = "no-such-icon|#000000|"
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        let html = format!(r#"<img src="pdfcn-icon:{payload}" style="width:24px;height:24px;">"#);
        let mut images = BTreeMap::new();
        generate_vectors(&html, &mut images, &BTreeMap::new(), &mut BTreeMap::new());
        assert!(images.is_empty());
    }

    #[test]
    fn qrcode_payloads_over_the_cap_are_skipped() {
        let payload = vec![b'x'; MAX_QRCODE_PAYLOAD_BYTES + 1];
        assert!(qrcode_png(&payload, qrcodegen::QrCodeEcc::Medium).is_none());
        assert!(qrcode_png(b"ok", qrcodegen::QrCodeEcc::Medium).is_some());
    }

    #[test]
    fn the_error_correction_level_is_chosen_by_a_prefix() {
        use qrcodegen::QrCodeEcc::{High, Low, Medium, Quartile};
        let level = |rest: &str| split_qrcode_src(rest).0 as usize;
        assert_eq!(level("6162"), Medium as usize, "bare payload is level M");
        assert_eq!(level("L.6162"), Low as usize);
        assert_eq!(level("Q.6162"), Quartile as usize);
        assert_eq!(level("H.6162"), High as usize);
        assert_eq!(split_qrcode_src("H.6162").1, "6162");
        // Higher levels need a bigger symbol for the same payload.
        let low = qrcode_png(b"https://example.com/a/fairly/long/path?x=1", Low).unwrap();
        let high = qrcode_png(b"https://example.com/a/fairly/long/path?x=1", High).unwrap();
        assert_ne!(low, high);
    }

    #[test]
    fn cover_crops_the_source_to_the_box_aspect_and_rewrites_src() {
        // 400x100 source into a 100x100 box: cover must crop to 100x100.
        let src = "photo.png";
        let html =
            format!(r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover">"#);
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
        let html =
            format!(r#"<img src="{src}" style="width:200px;height:100px;object-fit:cover">"#);
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
    fn a_non_center_object_position_is_left_to_the_engine() {
        let src = "photo.png";
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(400, 100, [1, 2, 3]))]);
        let html = format!(
            r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover;object-position:left">"#
        );
        assert_eq!(prepare_assets(&html, &mut images), html);
        assert_eq!(images.len(), 1);
        // The center position is what the crop does anyway, so it still applies.
        let html = format!(
            r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover;object-position:center">"#
        );
        assert_ne!(prepare_assets(&html, &mut images), html);
    }

    #[test]
    fn undecodable_source_bytes_are_left_alone() {
        let src = "broken.png";
        let html = format!(r#"<img src="{src}" style="width:100px;height:50px;object-fit:cover">"#);
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
        let html =
            format!(r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover">"#)
                + &format!(
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

    /// A box with only a height (its width is `w-full`, a class, unknowable
    /// pre-layout) and no `object-fit`: the height alone still bounds the
    /// embedded resolution rather than falling through to the boxless
    /// `MAX_IMAGE_DIMENSION_PX` cap. A 4000x3000 photo in a 192px-tall slot
    /// ships around 768x576 (192 * MAX_PRINT_SCALE), not 4096px wide.
    #[test]
    fn a_height_only_box_still_bounds_the_embedded_resolution() {
        let src = "photo.png";
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(4000, 3000, [1, 2, 3]))]);
        let html = format!(r#"<img src="{src}" style="height:192px">"#);
        prepare_assets(&html, &mut images);
        let decoded = image::load_from_memory(&images[src]).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (768, 576));
    }

    /// The same box with `object-fit: cover` is a different matter: cover fills
    /// the box on whichever axis needs the larger scale, so the height says
    /// nothing about the pixels needed. A `width:100%; height:300px` banner of a
    /// portrait photo used to be cut to 675px wide (about 100 ppi across a
    /// page); it must keep enough for the widest box the page offers.
    #[test]
    fn a_cover_banner_with_an_unknown_width_keeps_the_pixels_the_page_width_needs() {
        let src = "photo.png";
        let mut images = BTreeMap::from([(src.to_string(), png_bytes(6300, 8400, [1, 2, 3]))]);
        let html = format!(r#"<img src="{src}" style="width:100%;height:300px;object-fit:cover">"#);
        prepare_assets_with_vectors(
            &html,
            &mut images,
            &BTreeMap::new(),
            Some((700.0, 1000.0)),
            true,
            &mut BTreeMap::new(),
        );
        let decoded = image::load_from_memory(&images[src]).unwrap();
        // 700 CSS px across the page at 3x.
        assert_eq!(decoded.width(), 2100);
        assert_eq!(decoded.height(), 2800);
    }

    /// Shrinking is opt-in: the render path embeds a source at its own
    /// resolution unless asked (`pdfcn images` prepares photos beforehand).
    #[test]
    fn without_shrink_an_oversized_source_keeps_its_resolution() {
        let src = "photo.png";
        let original = png_bytes(2000, 1000, [1, 2, 3]);
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        let html = format!(r#"<img src="{src}" style="width:100px;height:50px">"#);
        prepare_assets_with_vectors(
            &html,
            &mut images,
            &BTreeMap::new(),
            Some((700.0, 1000.0)),
            false,
            &mut BTreeMap::new(),
        );
        assert_eq!(images[src], original);
    }

    /// A source less than twice what its box can use is embedded as it is:
    /// the saving would not pay for a decode, a resample and a re-encode.
    #[test]
    fn a_marginally_oversized_source_is_left_as_it_is() {
        let src = "photo.png";
        let original = png_bytes(400, 200, [1, 2, 3]);
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        let html = format!(r#"<img src="{src}" style="width:100px;height:50px">"#);
        prepare_assets(&html, &mut images);
        assert_eq!(images[src], original);
    }

    #[test]
    fn a_cover_image_with_an_unknown_side_and_no_page_bound_is_not_shrunk() {
        let src = "photo.png";
        let original = png_bytes(3060, 4080, [1, 2, 3]);
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        let html = format!(r#"<img src="{src}" style="height:192px;object-fit:cover">"#);
        prepare_assets(&html, &mut images);
        assert_eq!(images[src], original);
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
        jpeg.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
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

    /// A JPEG whose EXIF says "rotate 90 degrees clockwise to display" (a phone
    /// held upright): landscape pixels, orientation 6.
    fn jpeg_with_orientation(width: u32, height: u32, orientation: u16) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&image::DynamicImage::ImageRgb8(img))
            .unwrap();
        // APP1 "Exif" with one IFD0 entry: Orientation (0x0112), SHORT, 1 value.
        let mut tiff = vec![b'I', b'I', 0x2A, 0, 8, 0, 0, 0, 1, 0];
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn the_test_helper_really_writes_an_orientation_tag() {
        let bytes = jpeg_with_orientation(40, 20, 6);
        assert_eq!(
            exif_orientation(&bytes),
            image::metadata::Orientation::Rotate90
        );
        let plain = jpeg_with_orientation(40, 20, 1);
        assert_eq!(
            exif_orientation(&plain),
            image::metadata::Orientation::NoTransforms
        );
    }

    /// The phone-photo bug: landscape pixels plus orientation 6 went into the
    /// PDF as landscape pixels, squeezed into a portrait box.
    #[test]
    fn a_photo_with_an_orientation_tag_is_rotated_to_how_it_is_displayed() {
        let src = "phone.jpg";
        let mut images = BTreeMap::from([(src.to_string(), jpeg_with_orientation(80, 60, 6))]);
        let html = format!(r#"<img src="{src}" style="width:30px;height:40px">"#);
        prepare_assets(&html, &mut images);
        let decoded = image::load_from_memory(&images[src]).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (60, 80),
            "now portrait"
        );
        assert_eq!(
            exif_orientation(&images[src]),
            image::metadata::Orientation::NoTransforms,
            "and not rotated a second time by a viewer"
        );
        assert!(images[src].starts_with(&[0xFF, 0xD8]), "still a JPEG");
    }

    #[test]
    fn every_orientation_swaps_or_keeps_the_dimensions_as_it_should() {
        for (tag, swaps) in [
            (1, false),
            (2, false),
            (3, false),
            (4, false),
            (5, true),
            (6, true),
            (7, true),
            (8, true),
        ] {
            let mut images =
                BTreeMap::from([("a.jpg".to_string(), jpeg_with_orientation(80, 60, tag))]);
            prepare_assets(r#"<img src="a.jpg" style="width:80px">"#, &mut images);
            let d = image::load_from_memory(&images["a.jpg"]).unwrap();
            let expected = if swaps { (60, 80) } else { (80, 60) };
            assert_eq!((d.width(), d.height()), expected, "orientation {tag}");
        }
    }

    #[test]
    fn an_orientation_is_applied_before_the_cover_crop() {
        // Landscape pixels, displayed portrait: a portrait cover box must crop
        // the *displayed* picture, giving a portrait crop.
        let src = "phone.jpg";
        let mut images = BTreeMap::from([(src.to_string(), jpeg_with_orientation(400, 300, 6))]);
        let html =
            format!(r#"<img src="{src}" style="width:100px;height:100px;object-fit:cover">"#);
        let out = prepare_assets(&html, &mut images);
        let key = out
            .split("src=\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .unwrap();
        let d = image::load_from_memory(&images[key]).unwrap();
        assert_eq!(d.width(), d.height(), "square crop of the rotated picture");
        assert!(d.width() <= 300, "from the 300px short side, not 400");
    }

    #[test]
    fn an_image_without_an_orientation_tag_is_left_byte_for_byte() {
        let src = "plain.jpg";
        let original = jpeg_with_orientation(80, 60, 1);
        let mut images = BTreeMap::from([(src.to_string(), original.clone())]);
        prepare_assets(
            &format!(r#"<img src="{src}" style="width:80px;height:60px">"#),
            &mut images,
        );
        assert_eq!(images[src], original);
    }
}
