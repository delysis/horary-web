//! Frozen inference and pipeline scope shared by both trial coordinators.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

pub const GOOGLE_MODEL: &str = "gemma-4-26b-a4b-it";
const GOOGLE_ENDPOINT: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemma-4-26b-a4b-it:generateContent";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Native,
    Google,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenExecution {
    pub provider: Provider,
    pub full_reading: bool,
    pub model: Value,
    pub decoder: String,
    pub case_flow_parallelism: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_model: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_model_sha256: Option<String>,
}

pub fn file_digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|e| format!("Read pinned inference file {}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

impl FrozenExecution {
    /// Credentials are deliberately absent: this specification can safely be
    /// serialized into an immutable plan and forwarded to a reviewer.
    pub fn from_manifest(manifest: &Value, native_model: Option<&Path>) -> Result<Self, String> {
        let full_reading = match manifest["entry_point"].as_str() {
            Some("horary_pipeline::run") => true,
            Some("horary_pipeline::run_elicitation") => false,
            _ => return Err("Discovery has no recognized pipeline entry point".into()),
        };
        if let Some(declared) = manifest.get("full_reading") {
            if declared.as_bool() != Some(full_reading) {
                return Err("Discovery full-reading scope conflicts with its entry point".into());
            }
        }
        let decoder = manifest["decoder"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Discovery has no decoder identity")?
            .to_owned();
        let parallelism = match manifest.get("case_flow_parallelism") {
            Some(value) => value
                .as_u64()
                .ok_or("Discovery case-flow parallelism is invalid")?,
            None => 1,
        };
        if !matches!(parallelism, 1 | 4) {
            return Err("Discovery case-flow parallelism must be 1 or 4".into());
        }
        let model = &manifest["model"];
        let provider = match model["provider"].as_str() {
            Some("google_gemini_api") => Provider::Google,
            Some("native_llama") => Provider::Native,
            None if model["sha256"].is_string() => Provider::Native,
            _ => {
                return Err(
                    "Discovery provider is unsupported; no inference fallback is permitted".into(),
                )
            }
        };
        let (model, native_model, native_model_sha256) = match provider {
            Provider::Native => {
                if full_reading && parallelism != 1 {
                    return Err("Full native readings require case-flow parallelism 1 (specialist analysis still batches)".into());
                }
                let path = native_model
                    .ok_or("Native discovery requires --model with the pinned weights")?;
                let path =
                    fs::canonicalize(path).map_err(|e| format!("Resolve native weights: {e}"))?;
                let sha = model["sha256"]
                    .as_str()
                    .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or("Native discovery has no valid weights SHA256")?;
                if model["path"].as_str().is_none_or(str::is_empty) {
                    return Err("Native discovery has no weights path".into());
                }
                let bytes = fs::metadata(&path).map_err(|e| e.to_string())?.len();
                if model["bytes"].as_u64() != Some(bytes) || file_digest(&path)? != sha {
                    return Err("Native weights bytes/SHA256 differ from frozen discovery".into());
                }
                // The same immutable weights may be supplied at a restored HF
                // cache path. That invocation path is pinned for both trials.
                (
                    json!({"provider":"native_llama","path":path,"bytes":bytes,"sha256":sha}),
                    Some(path),
                    Some(sha.to_owned()),
                )
            }
            Provider::Google => {
                if native_model.is_some() {
                    return Err(
                        "Hosted discovery does not accept --model or local-weight fallback".into(),
                    );
                }
                if model["id"] != GOOGLE_MODEL
                    || model["endpoint"] != GOOGLE_ENDPOINT
                    || model["local_inference"] != false
                    || model["credential_in_evidence"] != false
                    || model.get("path").is_some()
                    || model.get("sha256").is_some()
                {
                    return Err("Hosted discovery must pin the supported Google model/endpoint without weights or credentials".into());
                }
                (model.clone(), None, None)
            }
        };
        Ok(Self {
            provider,
            full_reading,
            model,
            decoder,
            case_flow_parallelism: parallelism,
            native_model,
            native_model_sha256,
        })
    }

    pub fn require_manifest(&self, manifest: &Value) -> Result<(), String> {
        let expected_entry = if self.full_reading {
            "horary_pipeline::run"
        } else {
            "horary_pipeline::run_elicitation"
        };
        if manifest["entry_point"] != expected_entry
            || manifest
                .get("full_reading")
                .is_some_and(|v| v.as_bool() != Some(self.full_reading))
            || manifest["decoder"] != self.decoder
            || match manifest.get("case_flow_parallelism") {
                Some(value) => value.as_u64() != Some(self.case_flow_parallelism),
                None => self.case_flow_parallelism != 1,
            }
        {
            return Err(
                "Trial pipeline/decoder/full-reading scope differs from frozen discovery".into(),
            );
        }
        let mut actual = manifest["model"].clone();
        if self.provider == Provider::Native && actual["provider"].is_null() {
            actual["provider"] = json!("native_llama");
        }
        if self.provider == Provider::Native && actual["path"] != self.model["path"] {
            let path = actual["path"]
                .as_str()
                .ok_or("Trial native weights path is absent")?;
            actual["path"] = json!(fs::canonicalize(path)
                .map_err(|_| "Trial native weights path differs from the pinned invocation")?);
        }
        if actual != self.model {
            return Err("Trial provider/model identity differs from frozen discovery".into());
        }
        Ok(())
    }

    pub fn apply_environment(&self, command: &mut Command) -> Result<(), String> {
        let keyfile = if self.provider == Provider::Google {
            Some(std::env::var_os("HORARY_GOOGLE_KEY_FILE").filter(|v|!v.is_empty())
                .ok_or("Hosted trial requires HORARY_GOOGLE_KEY_FILE in the invocation environment")?)
        } else {
            None
        };
        self.apply_environment_with_keyfile(command, keyfile.as_deref())
    }

    /// Build the provider environment from an invocation's locator. This is
    /// separate from the serializable plan and enables credential-free mocks.
    pub fn apply_environment_with_keyfile(
        &self,
        command: &mut Command,
        keyfile: Option<&std::ffi::OsStr>,
    ) -> Result<(), String> {
        command
            .env(
                "HORARY_EVAL_FULL",
                if self.full_reading { "1" } else { "0" },
            )
            .env("HORARY_EVAL_BATCH", self.case_flow_parallelism.to_string())
            .env_remove("HORARY_EVAL_MODES")
            .env_remove("GEMINI_API_KEY")
            .env_remove("GOOGLE_API_KEY");
        match self.provider {
            Provider::Native => {
                command
                    .env("HORARY_EVAL_PROVIDER", "native")
                    .env(
                        "HORARY_NATIVE_LLAMA_TEST_MODEL",
                        self.native_model
                            .as_ref()
                            .ok_or("Missing pinned native model")?,
                    )
                    .env_remove("HORARY_GOOGLE_KEY_FILE")
                    .env_remove("HORARY_GOOGLE_INPUT_TPM");
            }
            Provider::Google => {
                command
                    .env("HORARY_EVAL_PROVIDER", "google")
                    .env_remove("HORARY_NATIVE_LLAMA_TEST_MODEL")
                    .env(
                        "HORARY_GOOGLE_KEY_FILE",
                        keyfile
                            .ok_or("Hosted trial requires invocation-only credential locator")?,
                    );
                if let Some(tokens) = self.model.get("input_tokens_per_61_seconds") {
                    let tokens = tokens
                        .as_u64()
                        .filter(|tokens| *tokens > 0)
                        .ok_or("Invalid pinned hosted input-token budget")?;
                    command.env("HORARY_GOOGLE_INPUT_TPM", tokens.to_string());
                } else {
                    command.env_remove("HORARY_GOOGLE_INPUT_TPM");
                }
            }
        }
        Ok(())
    }

    pub fn legacy_native_scope(&self) -> bool {
        self.provider == Provider::Native && !self.full_reading && self.case_flow_parallelism == 1
    }

    /// Each hosted process has its own token limiter. Do not start a second
    /// inference client against a discovery that still owns project quota.
    pub fn require_settled_discovery(&self, discovery: &Path) -> Result<(), String> {
        if self.provider == Provider::Native {
            return Ok(());
        }
        if !discovery.join("completed.json").is_file() {
            return Err("Hosted paired inference withheld: discovery is still running or interrupted; wait for its completed marker".into());
        }
        let bytes = fs::read(discovery.join("report.json"))
            .map_err(|_| "Hosted discovery has no closed report")?;
        let report: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Hosted discovery closed report is unusable")?;
        if report["campaign_state"]["status"] != "completed" {
            return Err(
                "Hosted paired inference withheld: discovery did not settle successfully".into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosted_manifest() -> Value {
        json!({"entry_point":"horary_pipeline::run","full_reading":true,"case_flow_parallelism":1,
        "decoder":"hosted unconstrained text","model":{"provider":"google_gemini_api","id":GOOGLE_MODEL,"endpoint":GOOGLE_ENDPOINT,
            "local_inference":false,"credential_in_evidence":false,"input_tokens_per_61_seconds":14000}})
    }

    #[test]
    fn hosted_full_scope_sets_explicit_provider_without_weight_or_credential_plan() {
        let manifest = hosted_manifest();
        let scope = FrozenExecution::from_manifest(&manifest, None).unwrap();
        let mut command = Command::new("must-not-run");
        command.env("HORARY_NATIVE_LLAMA_TEST_MODEL", "wrong-local-weights");
        scope
            .apply_environment_with_keyfile(
                &mut command,
                Some(std::ffi::OsStr::new("synthetic-private-keyfile")),
            )
            .unwrap();
        let env: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().to_string(),
                    v.map(|v| v.to_string_lossy().to_string()),
                )
            })
            .collect();
        assert_eq!(env["HORARY_EVAL_PROVIDER"].as_deref(), Some("google"));
        assert_eq!(env["HORARY_EVAL_FULL"].as_deref(), Some("1"));
        assert_eq!(env["HORARY_NATIVE_LLAMA_TEST_MODEL"], None);
        assert!(scope.native_model_sha256.is_none());
        assert!(!serde_json::to_string(&scope)
            .unwrap()
            .contains("synthetic-private-keyfile"));
        assert!(FrozenExecution::from_manifest(&manifest, Some(Path::new("unused.gguf"))).is_err());
        assert!(scope
            .apply_environment_with_keyfile(&mut command, None)
            .is_err());
    }

    #[test]
    fn native_weights_must_exist_and_match_before_any_child_or_paid_review() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weights.gguf");
        fs::write(&path, b"synthetic weights").unwrap();
        let manifest = json!({"entry_point":"horary_pipeline::run_elicitation","full_reading":false,"case_flow_parallelism":1,
            "decoder":"native constrained single","model":{"provider":"native_llama","path":path,"bytes":17,"sha256":file_digest(&path).unwrap()}});
        assert!(FrozenExecution::from_manifest(&manifest, None).is_err());
        let scope = FrozenExecution::from_manifest(&manifest, Some(&path)).unwrap();
        scope.require_manifest(&manifest).unwrap();
        let mut command = Command::new("must-not-run");
        scope
            .apply_environment_with_keyfile(&mut command, None)
            .unwrap();
        assert!(
            command
                .get_envs()
                .any(|(k, v)| k == "HORARY_EVAL_PROVIDER"
                    && v == Some(std::ffi::OsStr::new("native")))
        );
        fs::write(&path, b"wrong weights!!!!").unwrap();
        assert!(FrozenExecution::from_manifest(&manifest, Some(&path)).is_err());
        let mut missing = manifest;
        missing["model"].as_object_mut().unwrap().remove("sha256");
        assert!(FrozenExecution::from_manifest(&missing, Some(&path)).is_err());
    }

    #[test]
    fn cached_or_new_trials_reject_provider_model_decoder_and_full_scope_drift() {
        let manifest = hosted_manifest();
        let scope = FrozenExecution::from_manifest(&manifest, None).unwrap();
        scope.require_manifest(&manifest).unwrap();
        for pointer in [
            "/model/provider",
            "/model/id",
            "/model/endpoint",
            "/decoder",
            "/entry_point",
            "/model/input_tokens_per_61_seconds",
        ] {
            let mut changed = manifest.clone();
            *changed.pointer_mut(pointer).unwrap() = json!("drift");
            assert!(scope.require_manifest(&changed).is_err(), "{pointer}");
        }
        let mut changed = manifest.clone();
        changed["full_reading"] = json!(false);
        assert!(scope.require_manifest(&changed).is_err());
        changed = manifest;
        changed["case_flow_parallelism"] = json!(4);
        assert!(scope.require_manifest(&changed).is_err());
    }

    #[test]
    fn hosted_inference_waits_for_closed_discovery_without_stopping_or_rewriting_it() {
        let root = tempfile::tempdir().unwrap();
        let scope = FrozenExecution::from_manifest(&hosted_manifest(), None).unwrap();
        assert!(scope.require_settled_discovery(root.path()).is_err());
        fs::write(root.path().join("completed.json"), b"{}").unwrap();
        fs::write(
            root.path().join("report.json"),
            br#"{"campaign_state":{"status":"active"}}"#,
        )
        .unwrap();
        let before = fs::read(root.path().join("report.json")).unwrap();
        assert!(scope.require_settled_discovery(root.path()).is_err());
        assert_eq!(fs::read(root.path().join("report.json")).unwrap(), before);
        fs::write(
            root.path().join("report.json"),
            br#"{"campaign_state":{"status":"completed"}}"#,
        )
        .unwrap();
        scope.require_settled_discovery(root.path()).unwrap();
    }
}
