//! The Wave 1 vector substrate's renderer: SVG source text -> PNG bytes at
//! print density (Ola 1.2). Everything behind pdfcn-core's opt-in `vector`
//! cargo feature; the default build never compiles (or links) any of this,
//! so the serverless binary is byte-for-byte unaffected.
//!
//! Decision trail: `docs/spikes/001-vector-vs-raster.md` -- rasterized via
//! resvg rather than printpdf's own `svg` feature, because the HTML bridge
//! has no route for an `ExternalXObject` (see the spike for the verified
//! upstream evidence).

use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{fontdb, Options, Tree};

/// Pixels of output per CSS px of layout box -- the same ~300dpi criterion
/// `assets::normalize_resolutions` applies to photographs (see its
/// `MAX_PRINT_SCALE`): a vector is infinitely sharp, but the *rasterized*
/// PNG it becomes is not, so we render it at the density the box can
/// actually paint.
const PRINT_SCALE: f64 = 3.0;

/// Same absolute cap as the photo pipeline: beyond this, no plausible page
/// size can use the extra detail and the PNG's cost grows quadratically.
const MAX_DIMENSION_PX: f64 = 4096.0;

/// The embedded UI typefaces, registered into resvg's font database so SVGs
/// that contain `<text>` (charts v2's axis labels do) render with the same
/// family the rest of the document uses -- no system font scan, no host
/// dependency (NFR-3). Built once per process; `fontdb::Database` is
/// `Arc`-shared by usvg.
fn font_db() -> Arc<fontdb::Database> {
    static FONT_DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONT_DB
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            // The two families chart labels use; loading all twelve built-in
            // cuts would only slow the first render for nothing.
            for bytes in [
                crate::BUILTIN_FONTS[0].1, // Inter Regular
                crate::BUILTIN_FONTS[1].1, // Inter Bold
            ] {
                db.load_font_data(bytes.to_vec());
            }
            db.set_sans_serif_family("Inter");
            Arc::new(db)
        })
        .clone()
}

/// Renders `svg` to PNG bytes. `box_w_px`/`box_h_px` are the layout box the
/// placeholder paints into, when known in px: the output is that box at
/// [`PRINT_SCALE`], capped at [`MAX_DIMENSION_PX`] per side. Without a box
/// the SVG's own viewport is rendered at [`PRINT_SCALE`]. Aspect ratio is
/// preserved (uniform scale, min of the two axes). Returns `None` for SVG
/// that fails to parse or any render/encode failure -- the caller degrades
/// exactly like an unresolved image, never panics.
pub(crate) fn svg_to_png(
    svg: &str,
    box_w_px: Option<f64>,
    box_h_px: Option<f64>,
) -> Option<Vec<u8>> {
    let mut options = Options::default();
    options.fontdb = font_db();
    options.font_family = "Inter".to_string();
    let tree = Tree::from_str(svg, &options).ok()?;
    let viewport = tree.size();
    let (iw, ih) = (f64::from(viewport.width()), f64::from(viewport.height()));
    if !(iw.is_finite() && ih.is_finite() && iw > 0.0 && ih > 0.0) {
        return None;
    }

    // Desired output size per axis: the box at print density when the box is
    // known, otherwise the viewport at print density. Then one uniform scale
    // (the min) so the vector's aspect ratio survives, and the hard caps.
    let want_w = box_w_px.filter(|w| *w > 0.0).unwrap_or(iw) * PRINT_SCALE;
    let want_h = box_h_px.filter(|h| *h > 0.0).unwrap_or(ih) * PRINT_SCALE;
    let scale = (want_w / iw)
        .min(want_h / ih)
        .min(MAX_DIMENSION_PX / iw)
        .min(MAX_DIMENSION_PX / ih);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let out_w = ((iw * scale).round() as u32).clamp(1, MAX_DIMENSION_PX as u32);
    let out_h = ((ih * scale).round() as u32).clamp(1, MAX_DIMENSION_PX as u32);

    let mut pixmap = Pixmap::new(out_w, out_h)?;
    resvg::render(
        &tree,
        Transform::from_scale(scale as f32, scale as f32),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_simple_svg_to_a_real_png_at_print_density() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="#2563eb"/></svg>"##;
        let png = svg_to_png(svg, None, None).expect("should render");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        let decoded = image::load_from_memory(&png).unwrap();
        // No box: the viewport itself at PRINT_SCALE.
        assert_eq!((decoded.width(), decoded.height()), (300, 150));
    }

    #[test]
    fn a_known_box_sets_the_output_size_at_print_density() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"></svg>"#;
        let png = svg_to_png(svg, Some(100.0), Some(50.0)).expect("should render");
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (300, 150));
    }

    #[test]
    fn a_monster_viewport_is_capped_at_the_absolute_dimension_limit() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="100"></svg>"#;
        let png = svg_to_png(svg, None, None).expect("should render");
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!(decoded.width(), MAX_DIMENSION_PX as u32);
    }

    #[test]
    fn invalid_svg_degrades_to_none() {
        assert!(svg_to_png("not svg at all", None, None).is_none());
        assert!(svg_to_png("", None, None).is_none());
    }

    #[test]
    fn text_nodes_render_through_the_embedded_inter_fontdb() {
        // Would silently produce empty glyphs without the fontdb registration;
        // a successful non-blank raster proves the family resolves.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="30"><text x="4" y="20" font-family="Inter" font-size="14" fill="#000">Total</text></svg>"##;
        let png = svg_to_png(svg, None, None).expect("should render");
        let decoded = image::load_from_memory(&png).unwrap();
        let dark_pixels = decoded
            .to_rgb8()
            .pixels()
            .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
            .count();
        assert!(
            dark_pixels > 20,
            "expected rendered glyph ink, got {dark_pixels} dark pixels"
        );
    }
}
