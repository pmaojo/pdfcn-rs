//! One-off image preparation: the step to run on a folder of photos *before*
//! they go into a template.
//!
//! pdfcn already shrinks and crops images while it renders (see `assets`), but
//! that work is repeated on every build, and a 12-megapixel phone photo costs
//! seconds to decode each time. Preparing the photos once is faster, keeps a
//! repository small, and is the only way to be sure what is in the files you
//! commit: re-encoding drops every piece of metadata, including the GPS
//! location phones write into photos.
//!
//! What it does, in order:
//!
//! 1. decodes the picture and applies its EXIF orientation, so it is stored the
//!    way up it is displayed;
//! 2. shrinks it, with a Lanczos filter, if its longest side is over
//!    `max_edge` (never enlarges);
//! 3. writes it again as a JPEG (at `quality`) or a PNG, carrying no EXIF,
//!    GPS, XMP or ICC data.
//!
//! The ICC profile is dropped, not converted: the result is plain sRGB data, so
//! a photo in a wide gamut (Display P3, Adobe RGB) comes out slightly less
//! saturated. [`Prepared::removed`] says when that happened.

use crate::assets::decode_oriented;

/// What kind of file to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareFormat {
    /// The same kind as the input: JPEG stays JPEG, PNG stays PNG.
    Keep,
    /// A JPEG. A picture with transparency is flattened onto white.
    Jpeg,
    /// A PNG.
    Png,
}

#[derive(Debug, Clone)]
pub struct PrepareOptions {
    /// Longest side of the result, in pixels.
    pub max_edge: u32,
    /// JPEG quality, 1 to 100.
    pub quality: u8,
    pub format: PrepareFormat,
}

impl Default for PrepareOptions {
    /// 2400 px is a full-width A4 photo at 300 dpi (2480) to within a hair, and
    /// 85 is the usual point where a photo stops improving visibly.
    fn default() -> Self {
        Self {
            max_edge: 2400,
            quality: 85,
            format: PrepareFormat::Keep,
        }
    }
}

/// The result of [`prepare_image`].
#[derive(Debug, Clone)]
pub struct Prepared {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    /// What was written: `Jpeg` or `Png`, never `Keep`.
    pub format: PrepareFormat,
    /// The EXIF orientation was not "as stored" and the pixels were turned.
    pub rotated: bool,
    /// The picture was made smaller.
    pub resized: bool,
    /// Metadata that was in the input and is not in the output, by name.
    pub removed: Vec<&'static str>,
}

impl Prepared {
    /// The file extension for what was written.
    pub fn extension(&self) -> &'static str {
        match self.format {
            PrepareFormat::Png => "png",
            _ => "jpg",
        }
    }
}

/// Whether an EXIF block carries a GPS position: the GPSInfo pointer
/// (tag 0x8825) in its first directory. Bounds-checked: a damaged block is
/// "no GPS", never a panic.
fn exif_has_gps(chunk: &[u8]) -> bool {
    let tiff = chunk.strip_prefix(b"Exif\0\0").unwrap_or(chunk);
    let little = match tiff.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return false,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = tiff.get(at..at + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    if u16_at(2) != Some(42) {
        return false;
    }
    let Some(ifd) = u32_at(4).map(|o| o as usize) else {
        return false;
    };
    let Some(count) = u16_at(ifd) else {
        return false;
    };
    (0..usize::from(count)).any(|i| u16_at(ifd + 2 + i * 12) == Some(0x8825))
}

/// What metadata `bytes` carries that re-encoding will drop.
fn metadata_in(bytes: &[u8]) -> Vec<&'static str> {
    use image::ImageDecoder;
    let mut found = Vec::new();
    let Some(mut decoder) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
    else {
        return found;
    };
    if let Ok(Some(exif)) = decoder.exif_metadata() {
        found.push("EXIF");
        if exif_has_gps(&exif) {
            found.push("GPS location");
        }
    }
    if decoder.icc_profile().ok().flatten().is_some() {
        found.push("ICC colour profile");
    }
    if decoder.xmp_metadata().ok().flatten().is_some() {
        found.push("XMP");
    }
    found
}

/// Prepares one image; see the module documentation.
pub fn prepare_image(bytes: &[u8], options: &PrepareOptions) -> Result<Prepared, String> {
    if options.max_edge == 0 {
        return Err("max_edge must be at least 1".into());
    }
    let quality = options.quality.clamp(1, 100);
    let removed = metadata_in(bytes);
    let source_is_jpeg = bytes.starts_with(&[0xFF, 0xD8]);
    let (original_width, original_height) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map_err(|e| format!("not an image pdfcn can read (JPEG and PNG are supported): {e}"))?;
    let img = decode_oriented(bytes).ok_or("the image could not be decoded")?;
    // Oriented dimensions differ from the stored ones exactly when it was turned
    // by a quarter turn; a half turn or a mirror keeps them, so ask the tag.
    let rotated = {
        use image::ImageDecoder;
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_decoder().ok())
            .and_then(|mut d| d.orientation().ok())
            .is_some_and(|o| o != image::metadata::Orientation::NoTransforms)
    };

    let longest = img.width().max(img.height());
    let (img, resized) = if longest > options.max_edge {
        let scale = f64::from(options.max_edge) / f64::from(longest);
        let w = ((f64::from(img.width()) * scale).round() as u32).max(1);
        let h = ((f64::from(img.height()) * scale).round() as u32).max(1);
        (
            img.resize_exact(w, h, image::imageops::FilterType::Lanczos3),
            true,
        )
    } else {
        (img, false)
    };

    let format = match options.format {
        PrepareFormat::Keep if source_is_jpeg => PrepareFormat::Jpeg,
        PrepareFormat::Keep => PrepareFormat::Png,
        other => other,
    };
    let (width, height) = (img.width(), img.height());
    let mut out = Vec::new();
    match format {
        PrepareFormat::Png => img
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?,
        _ => {
            // JPEG has no alpha: put transparent areas on white, not black.
            let rgb = if img.color().has_alpha() {
                let rgba = img.to_rgba8();
                let mut flat = image::RgbImage::new(rgba.width(), rgba.height());
                for (x, y, p) in rgba.enumerate_pixels() {
                    let a = u32::from(p[3]);
                    let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
                    flat.put_pixel(x, y, image::Rgb([over(p[0]), over(p[1]), over(p[2])]));
                }
                flat
            } else {
                img.to_rgb8()
            };
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
                .encode_image(&image::DynamicImage::ImageRgb8(rgb))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(Prepared {
        bytes: out,
        width,
        height,
        original_width,
        original_height,
        format,
        rotated,
        resized,
        removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A solid JPEG with optional EXIF: an orientation and/or a GPS pointer.
    fn jpeg(width: u32, height: u32, orientation: Option<u16>, gps: bool) -> Vec<u8> {
        let img = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([
                (x * 255 / width.max(1)) as u8,
                (y * 255 / height.max(1)) as u8,
                90,
            ])
        });
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 92)
            .encode_image(&image::DynamicImage::ImageRgb8(img))
            .unwrap();
        let mut entries: Vec<(u16, u16, u32)> = Vec::new();
        if let Some(o) = orientation {
            entries.push((0x0112, 3, u32::from(o)));
        }
        if gps {
            // A pointer to a GPS directory; the directory itself does not matter here.
            entries.push((0x8825, 4, 0));
        }
        if entries.is_empty() {
            return data;
        }
        let mut tiff = vec![b'I', b'I', 0x2A, 0, 8, 0, 0, 0];
        tiff.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, value) in &entries {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&kind.to_le_bytes());
            tiff.extend_from_slice(&1u32.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
        }
        tiff.extend_from_slice(&[0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&data[2..]);
        out
    }

    fn opts(max_edge: u32) -> PrepareOptions {
        PrepareOptions {
            max_edge,
            ..PrepareOptions::default()
        }
    }

    #[test]
    fn a_phone_photo_is_turned_shrunk_and_stripped() {
        let input = jpeg(4000, 3000, Some(6), true);
        let out = prepare_image(&input, &opts(1000)).unwrap();
        // Orientation 6 makes it portrait; the long side is now the height.
        assert_eq!((out.width, out.height), (750, 1000));
        assert_eq!((out.original_width, out.original_height), (4000, 3000));
        assert!(out.rotated && out.resized);
        assert_eq!(out.removed, vec!["EXIF", "GPS location"]);
        // What was written has no metadata, and is already upright.
        assert!(!out.bytes.windows(4).any(|w| w == b"Exif"));
        let back = image::load_from_memory(&out.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (750, 1000));
    }

    #[test]
    fn it_never_enlarges_and_keeps_the_aspect_ratio() {
        let small = prepare_image(&jpeg(300, 200, None, false), &opts(2400)).unwrap();
        assert_eq!((small.width, small.height), (300, 200));
        assert!(!small.resized && !small.rotated);
        assert!(small.removed.is_empty());
        let big = prepare_image(&jpeg(3000, 2000, None, false), &opts(1500)).unwrap();
        assert_eq!((big.width, big.height), (1500, 1000));
    }

    #[test]
    fn exif_without_a_gps_block_is_reported_as_exif_only() {
        let out = prepare_image(&jpeg(200, 100, Some(1), false), &opts(2400)).unwrap();
        assert_eq!(out.removed, vec!["EXIF"]);
        assert!(!out.rotated, "orientation 1 is as stored");
    }

    #[test]
    fn quality_changes_the_size_of_a_jpeg() {
        let input = jpeg(800, 600, None, false);
        let high = prepare_image(
            &input,
            &PrepareOptions {
                quality: 95,
                ..opts(2400)
            },
        )
        .unwrap();
        let low = prepare_image(
            &input,
            &PrepareOptions {
                quality: 40,
                ..opts(2400)
            },
        )
        .unwrap();
        assert!(low.bytes.len() < high.bytes.len());
        assert_eq!(low.extension(), "jpg");
    }

    #[test]
    fn a_png_stays_a_png_with_its_transparency() {
        let mut img = image::RgbaImage::from_pixel(40, 40, image::Rgba([10, 20, 30, 255]));
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let out = prepare_image(&png, &opts(2400)).unwrap();
        assert_eq!(out.format, PrepareFormat::Png);
        assert_eq!(
            image::load_from_memory(&out.bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)[3],
            0
        );
        // Asked for a JPEG, the transparent pixel goes onto white, not black.
        let flat = prepare_image(
            &png,
            &PrepareOptions {
                format: PrepareFormat::Jpeg,
                ..opts(2400)
            },
        )
        .unwrap();
        let back = image::load_from_memory(&flat.bytes).unwrap().to_rgb8();
        assert!(back.get_pixel(0, 0)[0] > 200, "{:?}", back.get_pixel(0, 0));
    }

    #[test]
    fn garbage_and_bad_options_are_errors() {
        assert!(prepare_image(b"not an image", &opts(100)).is_err());
        assert!(prepare_image(&jpeg(10, 10, None, false), &opts(0)).is_err());
    }

    #[test]
    fn a_damaged_exif_block_is_not_gps_and_does_not_panic() {
        for chunk in [
            &b""[..],
            b"II",
            b"II*\0",
            b"MM\0*\xFF\xFF\xFF\xFF",
            b"Exif\0\0II*\0\x08\0\0\0\xFF\xFF",
        ] {
            assert!(!exif_has_gps(chunk), "{chunk:?}");
        }
    }
}
