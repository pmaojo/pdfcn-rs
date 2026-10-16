//! JPEG photos go into the PDF as the bytes they came in, behind the
//! `jpeg-passthrough` cargo feature (docs/spikes/005).
//!
//! PDF's `/DCTDecode` filter *is* JPEG: a baseline or progressive JPEG file is
//! a valid image stream as it stands, which is how Typst (through krilla)
//! embeds every photo. printpdf instead decodes each `<img>` to raw pixels
//! when it lays the page out and encodes those pixels as a new JPEG when it
//! saves -- per photo, one full decode, one full encode and one more
//! generation of JPEG loss, for bytes nobody changed. On a catalog of 100
//! photos that was a third of the render.
//!
//! So, the same in-place trick `vector_pdf` uses for SVG:
//!
//! - before layout, [`take`] moves each eligible JPEG out of the image map
//!   and lays it out by size alone (printpdf's `image_placeholders`,
//!   docs/spikes/006): no pixel is decoded, and printpdf writes a one-pixel
//!   stand-in that declares the photo's size;
//! - after saving, `pdf_xobjects::finish` finds that stand-in's XObject by the
//!   name printpdf derives from the `src`, and replaces the object in place
//!   with a `/DCTDecode` image stream carrying the original file. Same object
//!   number, same unit square, so every resource dictionary and `Do` stays as
//!   it is.
//!
//! Never makes a document worse: a file this module cannot vouch for (CMYK,
//! 12-bit, arithmetic-coded, lossless, an EXIF rotation, a caller asking for
//! greyscale or another encoding) keeps the old path, and a document lopdf
//! can't load or save comes back byte-for-byte unchanged -- with the
//! stand-in in it, which is why [`take`] only runs when the pass will.

use std::borrow::Cow;
use std::collections::BTreeMap;

use lopdf::{dictionary, Stream};

use crate::assets::Images;
use crate::{ImageFormat, ImageOptimization};

/// One JPEG held back from the layout engine: the caller's own bytes when
/// nothing changed them.
pub(crate) struct Jpeg<'a> {
    bytes: Cow<'a, [u8]>,
    info: JpegInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JpegInfo {
    pub width: u32,
    pub height: u32,
    /// 1 (greyscale) or 3 (YCbCr/RGB): the only kinds passed through.
    pub components: u8,
}

/// Whether the caller's image options leave a JPEG's bytes alone. Any option
/// that changes pixels or encoding (a quality, a size budget, greyscale, a
/// lossless or raw format) is a request for printpdf's re-encode, honoured
/// by keeping the old path.
pub(crate) fn allowed(opts: Option<&ImageOptimization>) -> bool {
    let Some(opts) = opts else { return true };
    opts.quality.is_none()
        && opts.max_size.is_none()
        && opts.greyscale != Some(true)
        && matches!(
            opts.format,
            None | Some(ImageFormat::Auto) | Some(ImageFormat::Jpeg)
        )
}

/// Moves every eligible JPEG out of `images`, recording its size in
/// `placeholders` (what the layout engine gets instead), and returns the
/// originals by `src` for `pdf_xobjects::finish` to put back.
pub(crate) fn take<'a>(
    images: &mut Images<'a>,
    placeholders: &mut BTreeMap<String, (u32, u32)>,
) -> BTreeMap<String, Jpeg<'a>> {
    let entries: Vec<(&String, &Cow<'a, [u8]>)> = images.iter().collect();
    // Reading a header is independent per photo: on several threads.
    let infos = crate::parallel::map(&entries, |(_, bytes)| {
        sniff(bytes).filter(|_| !has_orientation(bytes))
    });
    let eligible: Vec<(String, JpegInfo)> = entries
        .iter()
        .zip(infos)
        .filter_map(|((src, _), info)| Some(((*src).clone(), info?)))
        .collect();
    let mut taken = BTreeMap::new();
    for (src, info) in eligible {
        let Some(bytes) = images.remove(&src) else {
            continue;
        };
        placeholders.insert(src.clone(), (info.width, info.height));
        taken.insert(src, Jpeg { bytes, info });
    }
    taken
}

impl Jpeg<'_> {
    /// Size of the file, which lands in the output as it is.
    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }
}

/// The `/DCTDecode` image XObject for `jpeg`: its own file as the stream.
pub(crate) fn dct_stream(jpeg: Jpeg<'_>) -> Stream {
    let colour_space = if jpeg.info.components == 1 {
        "DeviceGray"
    } else {
        "DeviceRGB"
    };
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(jpeg.info.width),
            "Height" => i64::from(jpeg.info.height),
            "ColorSpace" => colour_space,
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        },
        jpeg.bytes.into_owned(),
    )
    // Already compressed; a Flate pass on top only costs time.
    .with_compression(false)
}

/// Whether `bytes` carry an EXIF orientation other than "as stored". The
/// asset pass already turned such photos upright (re-encoding them without
/// the tag); this is the belt to that pass's braces, since a PDF viewer never
/// reads the tag.
fn has_orientation(bytes: &[u8]) -> bool {
    crate::jpeg_header::read(bytes)
        .is_some_and(|h| h.orientation() != image::metadata::Orientation::NoTransforms)
}

/// Reads a JPEG's frame header: `Some` only for an 8-bit, 1- or 3-component,
/// baseline (SOF0/SOF1) or progressive (SOF2) Huffman-coded file -- what every
/// PDF reader's `/DCTDecode` handles. Bounds-checked throughout: arbitrary
/// bytes return `None`, never panic.
pub(crate) fn sniff(bytes: &[u8]) -> Option<JpegInfo> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    loop {
        // Markers may be padded with any number of 0xFF fill bytes.
        if *bytes.get(i)? != 0xFF {
            return None;
        }
        while *bytes.get(i)? == 0xFF {
            i += 1;
        }
        let marker = *bytes.get(i)?;
        i += 1;
        match marker {
            // Standalone markers: no length field.
            0x01 | 0xD0..=0xD7 => continue,
            // End of image or start of scan before any frame header.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]));
        if len < 2 {
            return None;
        }
        let segment = bytes.get(i + 2..i + len)?;
        match marker {
            0xC0..=0xC2 => {
                let [precision, h0, h1, w0, w1, components, ..] = *segment else {
                    return None;
                };
                let height = u32::from(u16::from_be_bytes([h0, h1]));
                let width = u32::from(u16::from_be_bytes([w0, w1]));
                return (precision == 8 && width > 0 && height > 0 && matches!(components, 1 | 3))
                    .then_some(JpegInfo {
                        width,
                        height,
                        components,
                    });
            }
            // Every other SOF: lossless, hierarchical, arithmetic-coded.
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
            // An Adobe APP14 segment saying the components are CMYK/YCCK is
            // caught by the component count; anything else is skipped.
            _ => i += len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    }

    fn grey_jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(
            width,
            height,
            image::Luma([90]),
        ))
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        out
    }

    #[test]
    fn sniff_reads_the_frame_header() {
        assert_eq!(
            sniff(&jpeg(320, 200)),
            Some(JpegInfo {
                width: 320,
                height: 200,
                components: 3
            })
        );
        assert_eq!(sniff(&grey_jpeg(8, 9)).map(|i| i.components), Some(1));
    }

    #[test]
    fn sniff_never_panics_on_arbitrary_or_truncated_bytes() {
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(b"\x89PNG\r\n"), None);
        let full = jpeg(64, 64);
        for cut in 0..full.len().min(700) {
            let _ = sniff(&full[..cut]);
        }
        // A frame header claiming 4 components (CMYK) is not passed through.
        let mut cmyk = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x14, 8, 0, 4, 0, 4, 4];
        cmyk.extend([0u8; 12]);
        assert_eq!(sniff(&cmyk), None);
        // Nor a 12-bit one, nor an arithmetic-coded (SOF9) one.
        let twelve = [
            0xFF, 0xD8, 0xFF, 0xC1, 0x00, 0x0B, 12, 0, 4, 0, 4, 1, 1, 0x11, 0,
        ];
        assert_eq!(sniff(&twelve), None);
        let arithmetic = [
            0xFF, 0xD8, 0xFF, 0xC9, 0x00, 0x0B, 8, 0, 4, 0, 4, 1, 1, 0x11, 0,
        ];
        assert_eq!(sniff(&arithmetic), None);
    }

    #[test]
    fn options_that_ask_for_a_re_encode_keep_the_old_path() {
        assert!(allowed(None));
        assert!(allowed(Some(&ImageOptimization::default())));
        let jpeg_format = ImageOptimization {
            format: Some(ImageFormat::Jpeg),
            ..Default::default()
        };
        assert!(allowed(Some(&jpeg_format)));
        for opts in [
            ImageOptimization {
                quality: Some(0.5),
                ..Default::default()
            },
            ImageOptimization {
                max_size: Some("300kb".into()),
                ..Default::default()
            },
            ImageOptimization {
                greyscale: Some(true),
                ..Default::default()
            },
            ImageOptimization {
                format: Some(ImageFormat::Lossless),
                ..Default::default()
            },
        ] {
            assert!(!allowed(Some(&opts)));
        }
    }

    fn render(src: &str, bytes: Vec<u8>, style: &str) -> Vec<u8> {
        let images = BTreeMap::from([(src.to_string(), bytes)]);
        crate::render_pdf_with_assets(
            &format!(r#"<html><body><img src="{src}" style="{style}"></body></html>"#),
            &BTreeMap::new(),
            &images,
            &crate::RenderOptions::default(),
        )
        .expect("renders")
    }

    fn image_streams(pdf: &[u8]) -> Vec<Stream> {
        use lopdf::{Document, Object};
        let doc = Document::load_mem(pdf).expect("valid PDF");
        doc.objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| {
                s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Image"[..])
            })
            .cloned()
            .collect()
    }

    #[test]
    fn a_jpeg_lands_in_the_pdf_byte_for_byte() {
        let photo = jpeg(400, 300);
        let pdf = render("photo.jpg", photo.clone(), "width:400px;height:300px");
        let images = image_streams(&pdf);
        assert_eq!(images.len(), 1, "the placeholder must not survive");
        let image = &images[0];
        assert_eq!(
            image.dict.get(b"Filter").unwrap().as_name().unwrap(),
            b"DCTDecode"
        );
        assert_eq!(image.dict.get(b"Width").unwrap().as_i64().unwrap(), 400);
        assert_eq!(image.dict.get(b"Height").unwrap().as_i64().unwrap(), 300);
        assert_eq!(
            image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
            b"DeviceRGB"
        );
        assert_eq!(image.content, photo);
    }

    #[test]
    fn a_greyscale_jpeg_keeps_its_one_channel() {
        let photo = grey_jpeg(40, 30);
        let pdf = render("grey.jpg", photo.clone(), "width:40px;height:30px");
        let images = image_streams(&pdf);
        assert_eq!(
            images[0]
                .dict
                .get(b"ColorSpace")
                .unwrap()
                .as_name()
                .unwrap(),
            b"DeviceGray"
        );
        assert_eq!(images[0].content, photo);
    }

    #[test]
    fn a_png_is_left_to_printpdf() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            20,
            10,
            image::Rgb([200, 10, 10]),
        ))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
        let pdf = render("logo.png", png, "width:20px;height:10px");
        let images = image_streams(&pdf);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].dict.get(b"Width").unwrap().as_i64().unwrap(), 20);
    }

    #[test]
    fn take_moves_eligible_jpegs_out_and_records_their_size() {
        let photo = jpeg(40, 30);
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut images: Images<'_> = BTreeMap::from([
            ("a.jpg".to_string(), Cow::Borrowed(photo.as_slice())),
            ("b.png".to_string(), Cow::Owned(png)),
        ]);
        let mut placeholders = BTreeMap::new();
        let taken = take(&mut images, &mut placeholders);
        assert_eq!(images.keys().collect::<Vec<_>>(), ["b.png"]);
        assert_eq!(
            placeholders,
            BTreeMap::from([("a.jpg".to_string(), (40, 30))])
        );
        // Still the caller's bytes: nothing was copied on the way.
        assert!(matches!(&taken["a.jpg"].bytes, Cow::Borrowed(b) if b.as_ptr() == photo.as_ptr()));
    }
}
