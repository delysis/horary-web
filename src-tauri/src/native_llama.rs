#[cfg(feature = "native-llama")]
use llama_cpp_2::llama_backend::LlamaBackend;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const NATIVE_LLAMA_BACKEND: &str = "native llama.cpp";
pub const NATIVE_LLAMA_BINDING_CRATE: &str = "llama-cpp-2";
pub const NATIVE_LLAMA_BINDING_VERSION: &str = "0.1.150";
pub const NATIVE_LLAMA_DEFAULT_PARALLEL_SEQUENCES: u32 = 4;
pub const NATIVE_LLAMA_MAX_PARALLEL_SEQUENCES: u32 = 16;
pub const NATIVE_LLAMA_DEFAULT_DRAFT_TOKENS: u32 = 3;
pub const NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE: bool = true;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaRuntimeInfo {
    pub backend: &'static str,
    pub binding_crate: &'static str,
    pub binding_version: &'static str,
    pub compiled: bool,
    pub supports_gpu_offload: Option<bool>,
    pub continuous_batching: NativeLlamaContinuousBatching,
    pub kv_cache: NativeLlamaKvCache,
    pub speculative_decoding: NativeLlamaSpeculativeDecoding,
    pub worker_plan: NativeLlamaWorkerPlan,
    pub cache_key_example: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaContinuousBatching {
    pub enabled: bool,
    pub default_parallel_sequences: u32,
    pub max_parallel_sequences: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaKvCache {
    pub hot_resident_sequence_cache: bool,
    pub cold_disk_sequence_cache: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaSpeculativeDecoding {
    pub mtp_when_draft_model_present: bool,
    pub native_runtime_available: bool,
    pub preferred_type: &'static str,
    pub default_draft_tokens: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaWorkerPlan {
    pub scheduler: &'static str,
    pub request_ordering: &'static str,
    pub hot_cache_tier: &'static str,
    pub cold_cache_tier: &'static str,
    pub cache_key_fields: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaCacheKeyMaterial {
    pub model_sha256: String,
    pub binding_crate: String,
    pub binding_version: String,
    pub context_tokens: u32,
    pub prompt_version: String,
    pub schema_version: String,
    pub tradition_profile: String,
    pub judgement_pipeline_version: String,
    pub generation_defaults_digest: String,
}

pub fn native_llama_runtime_info() -> NativeLlamaRuntimeInfo {
    let example_material = NativeLlamaCacheKeyMaterial {
        model_sha256: "0".repeat(64),
        binding_crate: NATIVE_LLAMA_BINDING_CRATE.to_string(),
        binding_version: NATIVE_LLAMA_BINDING_VERSION.to_string(),
        context_tokens: 8192,
        prompt_version: "horary-interpretation-v2".to_string(),
        schema_version: "2026-07-01".to_string(),
        tradition_profile: "traditional-horary-textbook-v1".to_string(),
        judgement_pipeline_version: "2026-07-01".to_string(),
        generation_defaults_digest: "temperature=0.2;max_tokens=1600".to_string(),
    };
    NativeLlamaRuntimeInfo {
        backend: NATIVE_LLAMA_BACKEND,
        binding_crate: NATIVE_LLAMA_BINDING_CRATE,
        binding_version: NATIVE_LLAMA_BINDING_VERSION,
        compiled: cfg!(feature = "native-llama"),
        supports_gpu_offload: native_gpu_offload_support(),
        continuous_batching: NativeLlamaContinuousBatching {
            enabled: true,
            default_parallel_sequences: NATIVE_LLAMA_DEFAULT_PARALLEL_SEQUENCES,
            max_parallel_sequences: NATIVE_LLAMA_MAX_PARALLEL_SEQUENCES,
        },
        kv_cache: NativeLlamaKvCache {
            hot_resident_sequence_cache: true,
            cold_disk_sequence_cache: true,
        },
        speculative_decoding: NativeLlamaSpeculativeDecoding {
            mtp_when_draft_model_present: NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE,
            native_runtime_available: NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE,
            preferred_type: "draft-mtp",
            default_draft_tokens: NATIVE_LLAMA_DEFAULT_DRAFT_TOKENS,
        },
        worker_plan: NativeLlamaWorkerPlan {
            scheduler: "single-owner worker thread with batched prefill/decode slots",
            request_ordering: "parallel microtask requests may interleave decode, but each request emits tokens in sequence order",
            hot_cache_tier: "resident llama.cpp sequence state keyed by stable prompt prefix",
            cold_cache_tier: "disk sequence-state file keyed by native_llama_cache_key",
            cache_key_fields: &[
                "modelSha256",
                "bindingCrate",
                "bindingVersion",
                "contextTokens",
                "promptVersion",
                "schemaVersion",
                "traditionProfile",
                "judgementPipelineVersion",
                "generationDefaultsDigest",
            ],
        },
        cache_key_example: native_llama_cache_key(&example_material),
    }
}

pub fn native_llama_cache_key(material: &NativeLlamaCacheKeyMaterial) -> String {
    let mut hasher = Sha256::new();
    update_hash_field(&mut hasher, "modelSha256", &material.model_sha256);
    update_hash_field(&mut hasher, "bindingCrate", &material.binding_crate);
    update_hash_field(&mut hasher, "bindingVersion", &material.binding_version);
    update_hash_field(
        &mut hasher,
        "contextTokens",
        &material.context_tokens.to_string(),
    );
    update_hash_field(&mut hasher, "promptVersion", &material.prompt_version);
    update_hash_field(&mut hasher, "schemaVersion", &material.schema_version);
    update_hash_field(&mut hasher, "traditionProfile", &material.tradition_profile);
    update_hash_field(
        &mut hasher,
        "judgementPipelineVersion",
        &material.judgement_pipeline_version,
    );
    update_hash_field(
        &mut hasher,
        "generationDefaultsDigest",
        &material.generation_defaults_digest,
    );
    format!("{:x}", hasher.finalize())
}

fn update_hash_field(hasher: &mut Sha256, key: &str, value: &str) {
    hasher.update(key.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

#[cfg(feature = "native-llama")]
fn native_gpu_offload_support() -> Option<bool> {
    let mut backend = LlamaBackend::init().ok()?;
    backend.void_logs();
    Some(backend.supports_gpu_offload())
}

#[cfg(not(feature = "native-llama"))]
fn native_gpu_offload_support() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_key_material() -> NativeLlamaCacheKeyMaterial {
        NativeLlamaCacheKeyMaterial {
            model_sha256: "a".repeat(64),
            binding_crate: "llama-cpp-2".to_string(),
            binding_version: "0.1.150".to_string(),
            context_tokens: 8192,
            prompt_version: "horary-interpretation-v2".to_string(),
            schema_version: "2026-07-01".to_string(),
            tradition_profile: "traditional-horary-textbook-v1".to_string(),
            judgement_pipeline_version: "2026-07-01".to_string(),
            generation_defaults_digest: "temperature=0.2;top_p=1;max_tokens=1600".to_string(),
        }
    }

    #[test]
    fn native_runtime_info_describes_release_target() {
        let info = native_llama_runtime_info();

        assert_eq!(info.backend, "native llama.cpp");
        assert_eq!(info.binding_crate, "llama-cpp-2");
        assert!(info.continuous_batching.enabled);
        assert_eq!(info.continuous_batching.default_parallel_sequences, 4);
        assert!(info.kv_cache.hot_resident_sequence_cache);
        assert!(info.kv_cache.cold_disk_sequence_cache);
        assert!(info.speculative_decoding.mtp_when_draft_model_present);
        assert!(info.speculative_decoding.native_runtime_available);
        assert_eq!(info.speculative_decoding.preferred_type, "draft-mtp");
        assert!(info
            .worker_plan
            .cache_key_fields
            .contains(&"judgementPipelineVersion"));
    }

    #[test]
    fn native_runtime_info_matches_release_profile() {
        let profile: serde_json::Value =
            serde_json::from_str(include_str!("../native-llama-runtime.json")).unwrap();
        let info = native_llama_runtime_info();
        let example_material = NativeLlamaCacheKeyMaterial {
            model_sha256: "0".repeat(64),
            binding_crate: NATIVE_LLAMA_BINDING_CRATE.to_string(),
            binding_version: NATIVE_LLAMA_BINDING_VERSION.to_string(),
            context_tokens: 8192,
            prompt_version: "horary-interpretation-v2".to_string(),
            schema_version: "2026-07-01".to_string(),
            tradition_profile: "traditional-horary-textbook-v1".to_string(),
            judgement_pipeline_version: "2026-07-01".to_string(),
            generation_defaults_digest: "temperature=0.2;max_tokens=1600".to_string(),
        };

        assert_eq!(
            profile["rustBinding"]["crate"].as_str(),
            Some(info.binding_crate)
        );
        assert_eq!(
            profile["rustBinding"]["version"].as_str(),
            Some(info.binding_version)
        );
        assert_eq!(
            profile["capabilities"]["continuousBatching"]["defaultParallelSequences"].as_u64(),
            Some(info.continuous_batching.default_parallel_sequences as u64)
        );
        assert_eq!(
            profile["capabilities"]["continuousBatching"]["maxParallelSequences"].as_u64(),
            Some(info.continuous_batching.max_parallel_sequences as u64)
        );
        assert_eq!(
            profile["capabilities"]["speculativeDecoding"]["defaultDraftTokens"].as_u64(),
            Some(info.speculative_decoding.default_draft_tokens as u64)
        );
        assert_eq!(
            profile["capabilities"]["speculativeDecoding"]["preferredType"].as_str(),
            Some(info.speculative_decoding.preferred_type)
        );
        assert_eq!(
            profile["capabilities"]["speculativeDecoding"]["enabledWhenDraftModelPresent"]
                .as_bool(),
            Some(info.speculative_decoding.mtp_when_draft_model_present)
        );
        assert_eq!(
            profile["capabilities"]["speculativeDecoding"]["nativeWorkerActive"].as_bool(),
            Some(info.speculative_decoding.native_runtime_available)
        );
        assert_eq!(
            info.cache_key_example,
            native_llama_cache_key(&example_material)
        );
    }

    #[test]
    fn native_cache_key_is_stable_sha256_hex() {
        let base = cache_key_material();
        let same = NativeLlamaCacheKeyMaterial { ..base.clone() };
        let key = native_llama_cache_key(&base);

        assert_eq!(key, native_llama_cache_key(&same));
        assert_eq!(key.len(), 64);
        assert!(key
            .chars()
            .all(|char| char.is_ascii_hexdigit() && !char.is_ascii_uppercase()));
    }

    #[test]
    fn native_cache_key_changes_on_every_invalidation_field() {
        let base = cache_key_material();
        let same = NativeLlamaCacheKeyMaterial { ..base.clone() };
        let base_key = native_llama_cache_key(&base);
        let variants = vec![
            (
                "model_sha256",
                NativeLlamaCacheKeyMaterial {
                    model_sha256: "b".repeat(64),
                    ..base.clone()
                },
            ),
            (
                "binding_crate",
                NativeLlamaCacheKeyMaterial {
                    binding_crate: "other-binding".to_string(),
                    ..base.clone()
                },
            ),
            (
                "binding_version",
                NativeLlamaCacheKeyMaterial {
                    binding_version: "0.1.151".to_string(),
                    ..base.clone()
                },
            ),
            (
                "context_tokens",
                NativeLlamaCacheKeyMaterial {
                    context_tokens: 16_384,
                    ..base.clone()
                },
            ),
            (
                "prompt_version",
                NativeLlamaCacheKeyMaterial {
                    prompt_version: "horary-interpretation-v3".to_string(),
                    ..base.clone()
                },
            ),
            (
                "schema_version",
                NativeLlamaCacheKeyMaterial {
                    schema_version: "2026-08-01".to_string(),
                    ..base.clone()
                },
            ),
            (
                "tradition_profile",
                NativeLlamaCacheKeyMaterial {
                    tradition_profile: "traditional-horary-textbook-v2".to_string(),
                    ..base.clone()
                },
            ),
            (
                "judgement_pipeline_version",
                NativeLlamaCacheKeyMaterial {
                    judgement_pipeline_version: "2026-08-01".to_string(),
                    ..base.clone()
                },
            ),
            (
                "generation_defaults_digest",
                NativeLlamaCacheKeyMaterial {
                    generation_defaults_digest: "temperature=0.1;top_p=1;max_tokens=1600"
                        .to_string(),
                    ..base.clone()
                },
            ),
        ];

        assert_eq!(native_llama_cache_key(&base), native_llama_cache_key(&same));
        for (field, changed) in variants {
            assert_ne!(
                base_key,
                native_llama_cache_key(&changed),
                "{field} must invalidate the native prompt cache key"
            );
        }
    }
}
