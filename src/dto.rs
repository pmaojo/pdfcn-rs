//! Request-body shapes shared by both handlers for the printpdf-level
//! options `RenderOptions` exposes: image re-encoding and plain PDF
//! metadata. Kept here rather than duplicated per binary the same reason
//! `auth`/`remote_image` are.

use pdfcn_core::{DocumentMetadata, FacturXProfile, ImageFormat, ImageOptimization, PageConfig};
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
            Some(name) => Some(ImageFormat::from_name(name).ok_or_else(|| {
                format!(
                    "invalid image_optimization.format \"{name}\" (expected \"auto\", \"jpeg\", \"lossless\", or \"raw\")"
                )
            })?),
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

/// A request's `page` object, shared by both handlers. Lenient by contract:
/// a missing or unrecognised size or orientation is the default (see
/// `PageConfig::from_request`).
#[derive(Clone, Deserialize)]
pub struct PageConfigDto {
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub orientation: Option<String>,
    #[serde(default)]
    pub margin_mm: Option<f32>,
}

impl PageConfigDto {
    pub fn into_page_config(self) -> PageConfig {
        PageConfig::from_request(
            self.size.as_deref(),
            self.orientation.as_deref(),
            self.margin_mm,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcn_core::{Orientation, PageSize};

    fn page(json: &str) -> PageConfig {
        serde_json::from_str::<PageConfigDto>(json)
            .unwrap()
            .into_page_config()
    }

    #[test]
    fn page_dto_reads_size_orientation_and_margin() {
        let config = page(r#"{"size":"letter","orientation":"landscape","margin_mm":12}"#);
        assert!(matches!(config.size, PageSize::Letter));
        assert!(matches!(config.orientation, Orientation::Landscape));
        assert_eq!(config.margin_mm, 12.0);
    }

    #[test]
    fn page_dto_falls_back_to_the_defaults_on_unknown_or_missing_fields() {
        let config = page(r#"{"size":"bogus","orientation":"sideways"}"#);
        let default = PageConfig::default();
        assert!(matches!(config.size, PageSize::A4));
        assert!(matches!(config.orientation, Orientation::Portrait));
        assert_eq!(config.margin_mm, default.margin_mm);
    }

    #[test]
    fn image_format_names_are_checked() {
        let dto = |format: &str| ImageOptimizationDto {
            quality: None,
            max_size: None,
            greyscale: None,
            format: Some(format.to_string()),
        };
        assert!(dto("jpeg").into_image_optimization().is_ok());
        assert!(dto("webp").into_image_optimization().is_err());
    }
}
