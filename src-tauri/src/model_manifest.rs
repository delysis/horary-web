use crate::llama::{sanitize_model_id, LlamaError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, io};

const BUNDLED_MODEL_MANIFEST: &str = include_str!("../model-manifest.json");
const DEFAULT_TEMPERATURE_MAX: f64 = 2.0;
const DEFAULT_MAX_TOKENS_MAX: u64 = 16_384;

#[derive(Debug, Serialize)]
pub struct ModelManifestError {
    pub message: String,
}

impl From<io::Error> for ModelManifestError {
    fn from(value: io::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for ModelManifestError {
    fn from(value: serde_json::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl From<reqwest::Error> for ModelManifestError {
    fn from(value: reqwest::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

impl From<LlamaError> for ModelManifestError {
    fn from(value: LlamaError) -> Self {
        Self {
            message: value.message,
        }
    }
}

pub type ModelManifestResult<T> = Result<T, ModelManifestError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub version: String,
    pub updated_at: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifestEntry {
    pub id: String,
    pub display_name: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub source: Option<ModelManifestSource>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub min_ram_gb: Option<u32>,
    #[serde(default)]
    pub recommended_ram_gb: Option<u32>,
    #[serde(default)]
    pub default_context_tokens: Option<u32>,
    #[serde(default)]
    pub default_params: Option<Value>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifestSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadModelRequest {
    pub model_id: String,
}

pub fn bundled_model_manifest() -> ModelManifestResult<ModelManifest> {
    parse_model_manifest(BUNDLED_MODEL_MANIFEST)
}

pub fn parse_model_manifest(input: &str) -> ModelManifestResult<ModelManifest> {
    let manifest: ModelManifest = serde_json::from_str(input)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &ModelManifest) -> ModelManifestResult<()> {
    if manifest.version.trim().is_empty() {
        return Err(ModelManifestError {
            message: "model manifest version is required".to_string(),
        });
    }
    if manifest.updated_at.trim().is_empty() {
        return Err(ModelManifestError {
            message: "model manifest updatedAt is required".to_string(),
        });
    }

    let mut ids = HashSet::new();
    for entry in &manifest.models {
        validate_manifest_entry(entry)?;
        if !ids.insert(entry.id.clone()) {
            return Err(ModelManifestError {
                message: format!("duplicate manifest model id: {}", entry.id),
            });
        }
    }
    Ok(())
}

fn validate_manifest_entry(entry: &ModelManifestEntry) -> ModelManifestResult<()> {
    if sanitize_model_id(&entry.id)?.as_str() != entry.id {
        return Err(ModelManifestError {
            message: format!("manifest model id must already be normalized: {}", entry.id),
        });
    }
    if entry.display_name.trim().is_empty() {
        return Err(ModelManifestError {
            message: format!("manifest model displayName is required: {}", entry.id),
        });
    }
    if entry.format != "gguf" {
        return Err(ModelManifestError {
            message: format!("manifest model must use gguf format: {}", entry.id),
        });
    }
    if let Some(sha) = &entry.sha256 {
        validate_sha256_hex(sha)?;
    }
    if let Some(source) = &entry.source {
        if source.source_type.trim().is_empty() {
            return Err(ModelManifestError {
                message: format!("manifest model source type is required: {}", entry.id),
            });
        }
        if let Some(url) = source.url.as_deref() {
            validate_download_url(url)?;
            validate_downloadable_entry(entry)?;
        }
    }
    Ok(())
}

fn validate_downloadable_entry(entry: &ModelManifestEntry) -> ModelManifestResult<()> {
    if !entry.enabled {
        return Err(ModelManifestError {
            message: format!("manifest model is disabled: {}", entry.id),
        });
    }
    if entry.format != "gguf" {
        return Err(ModelManifestError {
            message: format!("manifest model must use gguf format: {}", entry.id),
        });
    }
    if entry.sha256.as_deref().is_none() {
        return Err(ModelManifestError {
            message: format!(
                "manifest model requires sha256 before download: {}",
                entry.id
            ),
        });
    }
    validate_sha256_hex(entry.sha256.as_deref().unwrap())?;
    let Some(source) = &entry.source else {
        return Err(ModelManifestError {
            message: format!(
                "manifest model requires source before download: {}",
                entry.id
            ),
        });
    };
    if source.url.as_deref().is_none() {
        return Err(ModelManifestError {
            message: format!(
                "manifest model requires source.url before download: {}",
                entry.id
            ),
        });
    }
    validate_positive_u64(entry.size_bytes, "sizeBytes", &entry.id)?;
    let has_license = match entry.license.as_deref() {
        Some(license) => !license.trim().is_empty(),
        None => false,
    };
    if !has_license {
        return Err(ModelManifestError {
            message: format!("manifest model requires verified license: {}", entry.id),
        });
    }
    let min_ram = validate_positive_u32(entry.min_ram_gb, "minRamGb", &entry.id)?;
    let recommended_ram =
        validate_positive_u32(entry.recommended_ram_gb, "recommendedRamGb", &entry.id)?;
    if recommended_ram < min_ram {
        return Err(ModelManifestError {
            message: format!(
                "manifest model recommendedRamGb must be greater than or equal to minRamGb: {}",
                entry.id
            ),
        });
    }
    validate_positive_u32(
        entry.default_context_tokens,
        "defaultContextTokens",
        &entry.id,
    )?;
    validate_default_params(entry)?;
    Ok(())
}

fn validate_positive_u64(value: Option<u64>, field: &str, id: &str) -> ModelManifestResult<u64> {
    match value {
        Some(value) if value > 0 => Ok(value),
        _ => Err(ModelManifestError {
            message: format!("manifest model requires positive {field}: {id}"),
        }),
    }
}

fn validate_positive_u32(value: Option<u32>, field: &str, id: &str) -> ModelManifestResult<u32> {
    match value {
        Some(value) if value > 0 => Ok(value),
        _ => Err(ModelManifestError {
            message: format!("manifest model requires positive {field}: {id}"),
        }),
    }
}

fn validate_default_params(entry: &ModelManifestEntry) -> ModelManifestResult<()> {
    let Some(params) = entry.default_params.as_ref().and_then(Value::as_object) else {
        return Err(ModelManifestError {
            message: format!("manifest model requires defaultParams object: {}", entry.id),
        });
    };

    let temperature = params
        .get("temperature")
        .and_then(Value::as_f64)
        .ok_or_else(|| ModelManifestError {
            message: format!(
                "manifest model defaultParams.temperature must be a number: {}",
                entry.id
            ),
        })?;
    if !(0.0..=DEFAULT_TEMPERATURE_MAX).contains(&temperature) {
        return Err(ModelManifestError {
            message: format!(
                "manifest model defaultParams.temperature must be between 0 and {DEFAULT_TEMPERATURE_MAX}: {}",
                entry.id
            ),
        });
    }

    let top_p = params
        .get("top_p")
        .and_then(Value::as_f64)
        .ok_or_else(|| ModelManifestError {
            message: format!(
                "manifest model defaultParams.top_p must be a number: {}",
                entry.id
            ),
        })?;
    if !(0.0..=1.0).contains(&top_p) || top_p == 0.0 {
        return Err(ModelManifestError {
            message: format!(
                "manifest model defaultParams.top_p must be greater than 0 and no more than 1: {}",
                entry.id
            ),
        });
    }

    let max_tokens = params
        .get("max_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| ModelManifestError {
            message: format!(
                "manifest model defaultParams.max_tokens must be a positive integer: {}",
                entry.id
            ),
        })?;
    if max_tokens == 0 || max_tokens > DEFAULT_MAX_TOKENS_MAX {
        return Err(ModelManifestError {
            message: format!(
                "manifest model defaultParams.max_tokens must be between 1 and {DEFAULT_MAX_TOKENS_MAX}: {}",
                entry.id
            ),
        });
    }

    Ok(())
}

fn validate_sha256_hex(value: &str) -> ModelManifestResult<()> {
    if value.len() != 64 || !value.chars().all(|char| char.is_ascii_hexdigit()) {
        return Err(ModelManifestError {
            message: "sha256 must be a 64-character hex string".to_string(),
        });
    }
    Ok(())
}

fn validate_download_url(value: &str) -> ModelManifestResult<()> {
    let url = reqwest::Url::parse(value).map_err(|error| ModelManifestError {
        message: format!("invalid model download URL: {error}"),
    })?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if matches!(url.host_str(), Some("127.0.0.1" | "localhost")) => Ok(()),
        _ => Err(ModelManifestError {
            message: "model download URL must use https or loopback http".to_string(),
        }),
    }
}

fn default_format() -> String {
    "gguf".to_string()
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_pinned_recommended_pair() {
        let manifest = bundled_model_manifest().unwrap();

        assert_eq!(manifest.models.len(), 2);
        assert!(manifest.models.iter().all(|m| m
            .source
            .as_ref()
            .unwrap()
            .revision
            .as_ref()
            .unwrap()
            .len()
            == 40));
    }

    #[test]
    fn rejects_duplicate_or_unsafe_manifest_ids() {
        let unsafe_id = r#"{
            "version": "1",
            "updatedAt": "2026-06-30",
            "models": [{
                "id": "../bad",
                "displayName": "Bad",
                "source": { "type": "url", "url": "https://example.com/model.gguf" },
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }]
        }"#;
        let duplicate_id = r#"{
            "version": "1",
            "updatedAt": "2026-06-30",
            "models": [
                { "id": "model", "displayName": "One" },
                { "id": "model", "displayName": "Two" }
            ]
        }"#;

        assert!(parse_model_manifest(unsafe_id)
            .unwrap_err()
            .message
            .contains("normalized"));
        assert!(parse_model_manifest(duplicate_id)
            .unwrap_err()
            .message
            .contains("duplicate"));
    }

    #[test]
    fn rejects_downloadable_models_without_checksums() {
        let manifest = r#"{
            "version": "1",
            "updatedAt": "2026-06-30",
            "models": [{
                "id": "model",
                "displayName": "Model",
                "source": { "type": "url", "url": "https://example.com/model.gguf" }
            }]
        }"#;

        assert!(parse_model_manifest(manifest)
            .unwrap_err()
            .message
            .contains("requires sha256"));
    }

    #[test]
    fn rejects_downloadable_models_without_runtime_profile() {
        let missing_profile = r#"{
            "version": "1",
            "updatedAt": "2026-06-30",
            "models": [{
                "id": "model",
                "displayName": "Model",
                "source": { "type": "url", "url": "https://example.com/model.gguf" },
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "sizeBytes": 1024,
                "license": "Apache-2.0",
                "minRamGb": 8,
                "recommendedRamGb": 16,
                "defaultContextTokens": 8192
            }]
        }"#;
        let bad_memory_profile = r#"{
            "version": "1",
            "updatedAt": "2026-06-30",
            "models": [{
                "id": "model",
                "displayName": "Model",
                "source": { "type": "url", "url": "https://example.com/model.gguf" },
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "sizeBytes": 1024,
                "license": "Apache-2.0",
                "minRamGb": 16,
                "recommendedRamGb": 8,
                "defaultContextTokens": 8192,
                "defaultParams": { "temperature": 0.4, "top_p": 0.9, "max_tokens": 1200 }
            }]
        }"#;

        assert!(parse_model_manifest(missing_profile)
            .unwrap_err()
            .message
            .contains("defaultParams"));
        assert!(parse_model_manifest(bad_memory_profile)
            .unwrap_err()
            .message
            .contains("recommendedRamGb"));
    }

    #[test]
    fn rejects_non_https_remote_download_urls() {
        let manifest = r#"{
            "version": "1",
            "updatedAt": "2026-06-30",
            "models": [{
                "id": "model",
                "displayName": "Model",
                "source": { "type": "url", "url": "http://example.com/model.gguf" },
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }]
        }"#;

        assert!(parse_model_manifest(manifest)
            .unwrap_err()
            .message
            .contains("https or loopback"));
    }
}
