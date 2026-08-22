//! Request-body shapes shared by both handlers for the printpdf-level
//! options `RenderOptions` exposes: image re-encoding and plain PDF
//! metadata. Kept here rather than duplicated per binary the same reason
//! `auth`/`remote_image` are.

use pdfcn_core::{DocumentMetadata, ImageFormat, ImageOptimization};
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
