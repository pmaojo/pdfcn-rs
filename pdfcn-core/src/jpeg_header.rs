//! What the asset passes need from a JPEG's header -- its size and its EXIF
//! orientation -- read by walking the marker segments in place.
//!
//! Asking the `image` crate instead copies the whole file into its decoder
//! and builds every Huffman table, twice (once for the size, once more for
//! the EXIF block); with three passes asking, that was 6.5 % of a 100-photo
//! catalog (docs/spikes/010). The orientation itself is still parsed by
//! `image` (`Orientation::from_exif_chunk`), from the same bytes its own JPEG
//! decoder hands it, so the answer is the one `image` gives.

use image::metadata::Orientation;

/// The parts of a JPEG header the asset passes read.
pub(crate) struct Header<'a> {
    /// The first frame header (SOFn): its marker and segment bytes.
    pub(crate) frame: Option<(u8, &'a [u8])>,
    /// The payload of the last `Exif\0\0` APP1 segment, after those six
    /// bytes -- as zune-jpeg keeps it.
    pub(crate) exif: Option<&'a [u8]>,
}

/// The header of `bytes` up to the first scan, or `None` if `bytes` is not a
/// JPEG or its markers are malformed. Bounds-checked throughout: arbitrary
/// bytes return `None`, never panic.
pub(crate) fn read(bytes: &[u8]) -> Option<Header<'_>> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut header = Header {
        frame: None,
        exif: None,
    };
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
            // Start of scan (or a premature end): the header is over.
            0xD9 | 0xDA => return Some(header),
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]));
        if len < 2 {
            return None;
        }
        let segment = bytes.get(i + 2..i + len)?;
        match marker {
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF if header.frame.is_none() => {
                header.frame = Some((marker, segment));
            }
            0xE1 => {
                if let Some(exif) = segment.strip_prefix(b"Exif\0\0") {
                    header.exif = Some(exif);
                }
            }
            _ => {}
        }
        i += len;
    }
}

impl Header<'_> {
    /// The EXIF orientation, "as stored" when there is none (or it is
    /// unreadable) -- what `image`'s JPEG decoder reports.
    pub(crate) fn orientation(&self) -> Orientation {
        self.exif
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms)
    }

    /// Width and height from the frame header, as stored.
    pub(crate) fn dimensions(&self) -> Option<(u32, u32)> {
        let (_, segment) = self.frame?;
        let [_, h0, h1, w0, w1, ..] = *segment else {
            return None;
        };
        let (w, h) = (u16::from_be_bytes([w0, w1]), u16::from_be_bytes([h0, h1]));
        (w > 0 && h > 0).then_some((u32::from(w), u32::from(h)))
    }
}

/// `bytes`' EXIF orientation: from the JPEG header when it is one, else from
/// `image` (PNG, WebP... carry it too).
pub(crate) fn orientation(bytes: &[u8]) -> Orientation {
    if let Some(header) = read(bytes) {
        return header.orientation();
    }
    use image::ImageDecoder;
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
        .and_then(|mut d| d.orientation().ok())
        .unwrap_or(Orientation::NoTransforms)
}

/// Width and height as stored, and the EXIF orientation, of `bytes`; read
/// from the header alone.
pub(crate) fn dimensions_and_orientation(bytes: &[u8]) -> Option<((u32, u32), Orientation)> {
    if let Some(header) = read(bytes) {
        return Some((header.dimensions()?, header.orientation()));
    }
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let dimensions = decoder.dimensions();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    Some((dimensions, orientation))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    }

    /// A minimal little-endian TIFF block holding one IFD entry: the
    /// orientation tag (0x0112) set to `value`.
    fn exif_orientation_block(value: u16) -> Vec<u8> {
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD at offset 8
        tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count
        tiff.extend_from_slice(&value.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        tiff
    }

    /// `jpeg` with an `Exif` APP1 segment right after SOI.
    fn with_exif(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(tiff);
        let len = u16::try_from(payload.len() + 2).unwrap();
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// What the passes asked before: the `image` crate's own answer.
    fn image_crate(bytes: &[u8]) -> ((u32, u32), Orientation) {
        use image::ImageDecoder;
        let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        (decoder.dimensions(), decoder.orientation().unwrap())
    }

    #[test]
    fn reads_what_the_image_crate_reads() {
        let plain = jpeg(37, 21);
        assert_eq!(
            dimensions_and_orientation(&plain),
            Some(image_crate(&plain))
        );
        for value in 1..=8 {
            let tagged = with_exif(&plain, &exif_orientation_block(value));
            let ours = dimensions_and_orientation(&tagged).unwrap();
            assert_eq!(ours, image_crate(&tagged), "orientation {value}");
            assert_eq!(ours.1, Orientation::from_exif(value as u8).unwrap());
            assert_eq!(orientation(&tagged), ours.1);
        }
    }

    #[test]
    fn a_png_falls_back_to_the_image_crate() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(5, 3))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(
            dimensions_and_orientation(&png),
            Some(((5, 3), Orientation::NoTransforms))
        );
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let plain = with_exif(&jpeg(8, 8), &exif_orientation_block(6));
        for cut in 0..plain.len() {
            let _ = read(&plain[..cut]).map(|h| (h.dimensions(), h.orientation()));
        }
        assert!(read(b"not a jpeg").is_none());
        assert!(read(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01]).is_none());
    }
}
