use serde_json::Value as JsonValue;

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Json,
    Yaml,
}

impl DataFormat {
    /// Guesses the format from a file extension (`.json`, `.yaml`/`.yml`).
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

/// Parses a data payload (FR-1: "contra payloads JSON o YAML") into a
/// `serde_json::Value` context for template evaluation.
pub fn load_data(source: &str, format: DataFormat) -> Result<JsonValue, CoreError> {
    match format {
        DataFormat::Json => Ok(serde_json::from_str(source)?),
        DataFormat::Yaml => Ok(serde_yaml::from_str(source)?),
    }
}
