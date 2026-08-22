//! Request-body shapes shared by both handlers for the printpdf-level
//! options `RenderOptions` exposes: image re-encoding and plain PDF
//! metadata. Kept here rather than duplicated per binary the same reason
//! `auth`/`remote_image` are.

use pdfcn_core::{DocumentMetadata, FacturXProfile, ImageFormat, ImageOptimization};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ImageOptimizationDto {
    #[serde(default)]
    pub quality: Option<f32>,
    #[serde(default)]
    pub max_size: Option<String>,
    #[serde(default)]
    pub greyscale: Option<bool>,
    #[serde(default)]
    pub format: Option<String>,
}

impl ImageOptimizationDto {
    pub fn into_image_optimization(self) -> Result<ImageOptimization, String> {
        let format = match self.format.as_deref() {
            None => None,
            Some("auto") => Some(ImageFormat::Auto),
            Some("jpeg") => Some(ImageFormat::Jpeg),
            Some("lossless") => Some(ImageFormat::Lossless),
            Some("raw") => Some(ImageFormat::Raw),
            Some(other) => {
                return Err(format!(
                    "invalid image_optimization.format \"{other}\" (expected \"auto\", \"jpeg\", \"lossless\", or \"raw\")"
                ))
            }
        };
        Ok(ImageOptimization {
            quality: self.quality,
            max_size: self.max_size,
            greyscale: self.greyscale,
            format,
        })
    }
}

#[derive(Deserialize, Default)]
pub struct MetadataDto {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub producer: Option<String>,
}

impl MetadataDto {
    pub fn into_document_metadata(self) -> DocumentMetadata {
        DocumentMetadata {
            title: self.title,
            author: self.author,
            subject: self.subject,
            keywords: self.keywords,
            producer: self.producer,
        }
    }
}

/// Requests splicing a Factur-X invoice attachment into the rendered PDF
/// (a post-processing pass, see `pdfcn_core::embed_factur_x_invoice`) --
/// present only when the caller wants a hybrid invoice, absent otherwise.
#[derive(Deserialize)]
pub struct FacturXDto {
    /// EN 16931/CII invoice XML, plain UTF-8 text (no base64 needed for a
    /// JSON request body).
    pub xml: String,
    /// minimum, basic-wl, basic, en16931 (default), or extended.
    #[serde(default)]
    pub profile: Option<String>,
    /// Optional caller-supplied sRGB ICC profile, base64-encoded. Omitted
    /// entirely rather than guessed if the caller doesn't supply one --
    /// see docs/spikes/002-factur-x-embedding.md.
    #[serde(default)]
    pub icc_base64: Option<String>,
}

impl FacturXDto {
    pub fn parse_profile(profile: Option<&str>) -> Result<FacturXProfile, String> {
        match profile.unwrap_or("en16931").to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "minimum" => Ok(FacturXProfile::Minimum),
            "basic-wl" => Ok(FacturXProfile::BasicWl),
            "basic" => Ok(FacturXProfile::Basic),
            "en16931" => Ok(FacturXProfile::En16931),
            "extended" => Ok(FacturXProfile::Extended),
            other => Err(format!(
                "invalid factur_x.profile \"{other}\" (expected minimum, basic-wl, basic, en16931, or extended)"
            )),
        }
    }
}
