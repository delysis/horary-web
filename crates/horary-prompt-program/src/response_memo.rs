//! Opt-in raw response reuse. No accepted record, chart or permit is loaded.
#![forbid(unsafe_code)]
use crate::digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

pub const VERSION: &str = "horary-exact-response-memo-1";
type Result<T> = std::result::Result<T, String>;
const NATIVE_VALIDATION_STOP: &str =
    "Synthetic case call budget exhausted; no successful completion implied";

/// A completion seal alone also seals failed or interrupted operations. Use
/// the native reading protocol's actual settlement fields before importing
/// one. This is response-source eligibility, never a semantic success grade.
pub fn native_outcome_is_settled(outcome: &Value) -> bool {
    if outcome["physical_generation_attempts_complete"] != true
        || outcome["fresh_initial_sha256_verified"] != true
        || outcome["captured_checkpoint_reused"] != false
        || outcome["target_function_invoked"] != true
        || outcome["target_stage_authentic_inputs"] != true
        || outcome["deadline_cancelled"] != false
        || outcome["group_cancelled"] != false
        || outcome
            .get("infrastructure_error")
            .is_none_or(|v| !v.is_null())
        || outcome.get("provider_stop").is_none_or(|v| !v.is_null())
    {
        return false;
    }
    let rejected = || {
        let witness = &outcome["semantic_abort_witness"];
        let stop = witness["stop"].as_str();
        let known = (stop == Some(NATIVE_VALIDATION_STOP) && witness.get("stop_kind").is_none())
            || (stop == Some(crate::rejection_cycle::STOP)
                && witness["stop_kind"] == "no_progress"
                && witness["no_progress"]["event"] == "stage_no_progress"
                && witness["no_progress"]["version"] == 1);
        outcome["known_native_semantic_abort"] == true
            && known
            && outcome["execution_error"] == witness["stop"]
            && witness["all_provider_responses_settled"] == true
            && witness["request_result_pairs_complete"] == true
            && witness["validation_error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
    };
    match outcome["execution_status"].as_str() {
        Some("completed") => {
            outcome["scope"] == "reading_journey_function_only"
                && outcome["full_reading"] == true
                && outcome["first_turn_execution_completed"] == true
                && matches!(
                    outcome.get("follow_up_execution_completed"),
                    Some(Value::Null | Value::Bool(true))
                )
                && outcome.get("execution_error").is_some_and(Value::is_null)
                && outcome["known_native_semantic_abort"] != true
        }
        Some("observed_native_validation_exhaustion" | "observed_native_no_progress") => {
            outcome["scope"] == "reading_journey_function_only"
                && outcome["full_reading"] == true
                && ((outcome["execution_status"] == "observed_native_no_progress")
                    == (outcome["semantic_abort_witness"]["stop_kind"] == "no_progress"))
                && rejected()
        }
        Some("selected_stage_observed") => {
            let stage = &outcome["selected_stage"];
            outcome["scope"] == "selected_stage_function_only"
                && outcome["full_reading"] == false
                && outcome["selected_stage_observation"] == true
                && outcome["target_function_invoked"] == true
                && outcome["target_stage_authentic_inputs"] == true
                && stage["version"] == 1
                && stage["all_provider_responses_settled"] == true
                && stage["request_result_pairs_complete"] == true
                && stage["target_attempts"].as_u64().is_some_and(|n| n > 0)
                && match stage["state"].as_str() {
                    Some("accepted") => {
                        outcome.get("execution_error").is_some_and(Value::is_null)
                            && outcome["known_native_semantic_abort"] != true
                    }
                    Some("native_rejected") => rejected(),
                    _ => false,
                }
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub file: PathBuf,
    pub sha256: String,
}
impl Pin {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        ordinary(&self.file)?;
        let bytes = fs::read(&self.file).map_err(|e| e.to_string())?;
        if digest(&bytes) != self.sha256 {
            return Err(format!("Memo source changed: {}", self.file.display()));
        }
        Ok(bytes)
    }
    pub fn value(&self) -> Result<Value> {
        serde_json::from_slice(&self.bytes()?).map_err(|e| e.to_string())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    CorpusCase,
    NativeOperation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub root: PathBuf,
    pub kind: Kind,
    pub seal: Pin,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub version: String,
    pub case_id: String,
    pub fixture_sha256: String,
    pub initial_sha256: String,
    pub excluded_stage: String,
    pub sources: Vec<Source>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    pub case_id: String,
    pub fixture_sha256: String,
    pub initial_sha256: String,
    pub stage: String,
    pub matter: Value,
    pub prompt: Value,
    pub schema: Value,
    pub input: Value,
    pub model_decoding: Value,
    pub wire_request: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<ExactHashes>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactHashes {
    pub input: String,
    pub schema: String,
    pub prompt: String,
    pub wire_request: String,
}
impl Signature {
    pub fn key(&self) -> Result<String> {
        let exact = match &self.exact {
            Some(exact) => exact.clone(),
            None => {
                ExactHashes::values(&self.input, &self.schema, &self.prompt, &self.wire_request)?
            }
        };
        canonical_hash(&json!({"case_id":self.case_id,"fixture_sha256":self.fixture_sha256,"initial_sha256":self.initial_sha256,
            "stage":self.stage,"matter":self.matter,"model_decoding":self.model_decoding,"exact":exact}).to_string())
    }
}

// JSON object ordering and indentation are representation, while array order,
// string contents and exact numeric tokens belong to the model function.
// RawValue prevents the optimizer's legacy float parser from rounding an
// astronomy value while checking the app's exact response key.
fn canonical_raw(raw: &str) -> Result<String> {
    use serde_json::value::RawValue;
    let raw = raw.trim();
    match raw.as_bytes().first() {
        Some(b'{') => {
            let fields: BTreeMap<String, Box<RawValue>> =
                serde_json::from_str(raw).map_err(|e| e.to_string())?;
            let parts = fields
                .iter()
                .map(|(key, value)| {
                    Ok(format!(
                        "{}:{}",
                        serde_json::to_string(key).map_err(|e| e.to_string())?,
                        canonical_raw(value.get())?
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("{{{}}}", parts.join(",")))
        }
        Some(b'[') => {
            let fields: Vec<Box<RawValue>> =
                serde_json::from_str(raw).map_err(|e| e.to_string())?;
            Ok(format!(
                "[{}]",
                fields
                    .iter()
                    .map(|value| canonical_raw(value.get()))
                    .collect::<Result<Vec<_>>>()?
                    .join(",")
            ))
        }
        _ => {
            serde_json::from_str::<Box<RawValue>>(raw).map_err(|e| e.to_string())?;
            Ok(raw.into())
        }
    }
}
fn canonical_hash(raw: &str) -> Result<String> {
    Ok(digest(canonical_raw(raw)?))
}
fn raw_field(raw: &str, field: &str) -> Result<String> {
    let fields: BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_str(raw).map_err(|e| e.to_string())?;
    Ok(fields
        .get(field)
        .ok_or_else(|| format!("Missing exact memo field {field}"))?
        .get()
        .into())
}
fn raw_index(raw: &str, index: usize) -> Result<String> {
    let fields: Vec<Box<serde_json::value::RawValue>> =
        serde_json::from_str(raw).map_err(|e| e.to_string())?;
    Ok(fields
        .get(index)
        .ok_or("Missing exact memo branch")?
        .get()
        .into())
}
impl ExactHashes {
    pub fn values(input: &Value, schema: &Value, prompt: &Value, wire: &Value) -> Result<Self> {
        Ok(Self {
            input: canonical_hash(&input.to_string())?,
            schema: canonical_hash(&schema.to_string())?,
            prompt: canonical_hash(&prompt.to_string())?,
            wire_request: canonical_hash(&wire.to_string())?,
        })
    }
    fn trace(request: &str, branch: Option<usize>, wire: &str) -> Result<Self> {
        let (input, schema, prompt) = if let Some(i) = branch {
            let task = raw_index(&raw_field(request, "tasks")?, i)?;
            (
                raw_index(&task, 2)?,
                raw_index(&task, 3)?,
                raw_index(&raw_field(request, "prompts")?, i)?,
            )
        } else {
            (
                raw_field(request, "input")?,
                raw_field(request, "schema")?,
                raw_field(request, "prompt")?,
            )
        };
        Ok(Self {
            input: canonical_hash(&input)?,
            schema: canonical_hash(&schema)?,
            prompt: canonical_hash(&prompt)?,
            wire_request: canonical_hash(wire)?,
        })
    }
}

/// Read only current opt-in request metadata. No state/expected facts enter
/// the key. The native caller captures its exact current wire translation.
pub fn signature_from_request(bytes: &[u8], branch: Option<usize>) -> Result<Signature> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let request: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let wire_raw = if let Some(i) = branch {
        raw_index(&raw_field(text, "memo_wire_requests")?, i)?
    } else {
        raw_field(text, "memo_wire_request")?
    };
    let (stage, matter, input, schema, prompt) = if let Some(i) = branch {
        let task = request["tasks"]
            .as_array()
            .and_then(|t| t.get(i))
            .and_then(Value::as_array)
            .filter(|t| t.len() == 4)
            .ok_or("Bad current memo branch")?;
        (
            task[0].clone(),
            task[1].clone(),
            task[2].clone(),
            task[3].clone(),
            request["prompts"][i].clone(),
        )
    } else {
        (
            request["stage"].clone(),
            request["matter"].clone(),
            request["input"].clone(),
            request["schema"].clone(),
            request["prompt"].clone(),
        )
    };
    let context = &request["memo_context"];
    Ok(Signature {
        case_id: context["case_id"]
            .as_str()
            .ok_or("Missing memo case")?
            .into(),
        fixture_sha256: context["fixture_sha256"]
            .as_str()
            .ok_or("Missing memo fixture")?
            .into(),
        initial_sha256: context["initial_sha256"]
            .as_str()
            .ok_or("Missing memo initial provenance")?
            .into(),
        stage: stage.as_str().ok_or("Missing memo stage")?.into(),
        matter,
        input,
        schema,
        prompt,
        model_decoding: model_decoding(&request["provider"])?,
        wire_request: serde_json::from_str(&wire_raw).map_err(|e| e.to_string())?,
        exact: Some(ExactHashes::trace(text, branch, &wire_raw)?),
    })
}

/// Operational quota/concurrency settings do not change the neural function.
/// The requested provider/model and every generation decoding field do.
pub fn model_decoding(provider: &Value) -> Result<Value> {
    let mut values = serde_json::Map::new();
    for field in [
        "provider",
        "id",
        "endpoint",
        "decoding",
        "thinking_level",
        "temperature",
        "seed",
        "local_inference",
    ] {
        let value = provider
            .get(field)
            .filter(|v| !v.is_null())
            .ok_or("Memo needs a complete model/decoding signature")?;
        values.insert(field.into(), value.clone());
    }
    if provider["provider"] != "google_gemini_api" || provider["local_inference"] != false {
        return Err("Exact response memoization is limited to hosted text reading searches".into());
    }
    Ok(Value::Object(values))
}
pub fn ordinary(file: &Path) -> Result<()> {
    if !file.is_absolute()
        || !fs::symlink_metadata(file)
            .map_err(|e| e.to_string())?
            .is_file()
    {
        return Err("Memo witness must be an ordinary absolute file".into());
    }
    Ok(())
}
fn inside(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.components().next().is_none()
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("Memo witness escapes its source".into());
    }
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part.as_os_str());
        if fs::symlink_metadata(&current)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Memo source cannot contain symlink aliases".into());
        }
    }
    Ok(current)
}
fn maps(source: &Source, id: &str) -> Result<(BTreeMap<String, String>, String)> {
    if !source.root.is_absolute()
        || !fs::symlink_metadata(&source.root)
            .map_err(|e| e.to_string())?
            .is_dir()
    {
        return Err("Memo source root must be an ordinary absolute directory".into());
    }
    let seal = source.seal.value()?;
    let (files, prefix) = match source.kind {
        Kind::CorpusCase => {
            if source.seal.file != source.root.join("case-origins").join(format!("{id}.json"))
                || seal["case_id"] != id
                || seal["origin"] != "fresh_completed_native_campaign"
                || seal["selected_outcome_unchanged"] != true
                || seal["archived_answer_replay_allowed"] == false
            {
                return Err("Memo corpus source is not a sealed replay-eligible case".into());
            }
            let files = seal["files"]
                .as_object()
                .ok_or("No corpus case seal")?
                .iter()
                .map(|(file, sha)| {
                    Ok((
                        format!("cases/{id}/{file}"),
                        sha.as_str().ok_or("Invalid case hash")?.into(),
                    ))
                })
                .collect::<Result<BTreeMap<String, String>>>()?;
            (files, format!("cases/{id}"))
        }
        Kind::NativeOperation => {
            if source.seal.file != source.root.join("completed.json") {
                return Err("Wrong native completion seal".into());
            }
            let mut files = BTreeMap::new();
            for entry in seal["artifacts"]
                .as_array()
                .ok_or("No completed artifact seal")?
            {
                let name = entry["file"].as_str().ok_or("Missing artifact path")?;
                let hash = entry["sha256"].as_str().ok_or("Missing artifact hash")?;
                if files.insert(name.to_owned(), hash.to_owned()).is_some() {
                    return Err("Duplicate completed memo artifact".into());
                }
            }
            if files.get("response.json").map(String::as_str) != seal["response_sha256"].as_str()
                || !files.contains_key("response.json")
            {
                return Err("Native response is not bound by both completion seals".into());
            }
            let response_path = inside(&source.root, "response.json")?;
            let response = Pin {
                file: response_path,
                sha256: seal["response_sha256"]
                    .as_str()
                    .ok_or("Missing response hash")?
                    .into(),
            }
            .value()?;
            let request_path = inside(&source.root, "request.json")?;
            let request = Pin {
                file: request_path,
                sha256: files
                    .get("request.json")
                    .ok_or("Unsealed operation request")?
                    .clone(),
            }
            .value()?;
            if request["kind"] != "reading_journey"
                || request["request"]["case"]["id"] != id
                || response["outcome"]["id"] != id
                || !native_outcome_is_settled(&response["outcome"])
            {
                return Err(
                    "Memo operation is not a completed same-case reading measurement".into(),
                );
            }
            if response["outcome"]["semantic_abort_witness"]["stop_kind"] == "no_progress" {
                let witness = &response["outcome"]["semantic_abort_witness"];
                let name = witness["snapshot"]
                    .as_str()
                    .filter(|name| matches!(*name, "first-turn.json" | "final.json"))
                    .ok_or("No-progress memo source lacks its snapshot")?;
                let snapshot =
                    pinned(source, &files, &format!("native/cases/{id}/{name}"))?.value()?;
                crate::rejection_cycle::verify_stop(&snapshot, witness)?;
            }
            (files, format!("native/cases/{id}"))
        }
    };
    if files.len() > 10000 {
        return Err("Memo source file bound exceeded".into());
    }
    for (file, sha) in &files {
        Pin {
            file: inside(&source.root, file)?,
            sha256: sha.clone(),
        }
        .bytes()?;
    }
    Ok((files, prefix))
}

fn pinned(source: &Source, files: &BTreeMap<String, String>, relative: &str) -> Result<Pin> {
    Ok(Pin {
        file: inside(&source.root, relative)?,
        sha256: files
            .get(relative)
            .ok_or_else(|| format!("Unsealed memo witness {relative}"))?
            .clone(),
    })
}

pub fn settled(receipt: &Value, result: &Value) -> bool {
    receipt["submitted"] == true
        && receipt["provider"] == "google_gemini_api"
        && receipt["response"]["http_status"] == 200
        && receipt["native_result"]["Ok"] == *result
        && result["content"].is_string()
        && body_content(&receipt["response"]["body"]).as_deref() == result["content"].as_str()
        && receipt["generation_attempts"]
            .as_array()
            .is_some_and(|attempts| {
                !attempts.is_empty()
                    && attempts.iter().all(|a| {
                        a["submitted"] == true
                            && a["error"].is_null()
                            && a["http_status"].as_u64().is_some()
                    })
                    && attempts.last().is_some_and(|a| {
                        a["http_status"] == 200 && a["body"] == receipt["response"]["body"]
                    })
            })
}
fn body_content(body: &Value) -> Option<String> {
    let candidates = body["candidates"].as_array()?;
    if candidates.len() != 1
        || !matches!(
            candidates[0]["finishReason"].as_str(),
            Some("STOP" | "MAX_TOKENS")
        )
    {
        return None;
    }
    let mut output = String::new();
    for part in candidates[0]["content"]["parts"].as_array()? {
        if part["thought"] != true {
            output.push_str(part["text"].as_str()?);
        }
    }
    (!output.trim().is_empty()).then_some(output)
}

#[derive(Clone)]
struct Entry {
    signature: Signature,
    response: Value,
    witness: Value,
}
pub struct Memo {
    index: Index,
    entries: Vec<Entry>,
    pub excluded: Vec<Value>,
}
impl Memo {
    pub fn load(index: Index) -> Result<Self> {
        if index.version != VERSION
            || index.sources.len() > 128
            || index.case_id.is_empty()
            || !index
                .case_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("Invalid or unbounded memo index".into());
        }
        let mut entries = Vec::new();
        let mut excluded = Vec::new();
        for source in &index.sources {
            let (files, prefix) = maps(source, &index.case_id)?;
            let fixture = pinned(source, &files, &format!("{prefix}/fixture.json"))?;
            let initial = pinned(source, &files, &format!("{prefix}/initial.json"))?;
            if fixture.sha256 != index.fixture_sha256 || initial.sha256 != index.initial_sha256 {
                return Err(
                    "Memo source differs from the exact authored fixture/device/clock".into(),
                );
            }
            let manifest_file = match source.kind {
                Kind::CorpusCase => "manifest.json",
                Kind::NativeOperation => "native/manifest.json",
            };
            let manifest = match source.kind {
                Kind::NativeOperation => pinned(source, &files, manifest_file)?,
                Kind::CorpusCase => {
                    let seal = source.seal.value()?;
                    let path = PathBuf::from(
                        seal["source_directory"]
                            .as_str()
                            .ok_or("No case source directory")?,
                    )
                    .parent()
                    .and_then(Path::parent)
                    .ok_or("No source manifest parent")?
                    .join("manifest.json");
                    Pin {
                        file: path,
                        sha256: seal["source_manifest_sha256"]
                            .as_str()
                            .ok_or("No sealed source manifest")?
                            .into(),
                    }
                }
            };
            let manifest_value = manifest.value()?;
            let model = model_decoding(&manifest_value["model"])?;
            let native_sha = match source.kind {
                Kind::CorpusCase => {
                    source.seal.value()?["fresh_corpus"]["native_executable_sha256"].clone()
                }
                Kind::NativeOperation => pinned(source, &files, "request.json")?.value()?
                    ["request"]["native_executable_sha256"]
                    .clone(),
            };
            if native_sha.as_str().is_none_or(|s| s.len() != 64) {
                return Err("Memo source lacks its original native executable ancestry".into());
            }
            for name in files.keys().filter(|file| {
                file.starts_with(&format!("{prefix}/calls/")) && file.ends_with("-request.json")
            }) {
                let request = pinned(source, &files, name)?;
                let result_name = name.replace("-request.json", "-result.json");
                let result = pinned(source, &files, &result_name)?;
                let request_bytes = request.bytes()?;
                let result_bytes = result.bytes()?;
                let request_text =
                    std::str::from_utf8(&request_bytes).map_err(|e| e.to_string())?;
                let result_text = std::str::from_utf8(&result_bytes).map_err(|e| e.to_string())?;
                let req: Value =
                    serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
                let res: Value =
                    serde_json::from_slice(&result_bytes).map_err(|e| e.to_string())?;
                if res["request"] != req {
                    return Err("Memo request/result binding differs".into());
                }
                if model_decoding(&req["provider"])? != model {
                    return Err("Memo call/model fingerprint differs".into());
                }
                let branches = if let Some(tasks) = req["tasks"].as_array() {
                    if tasks.is_empty()
                        || tasks.len() > 4
                        || req["prompts"]
                            .as_array()
                            .is_none_or(|p| p.len() != tasks.len())
                    {
                        return Err("Malformed source analysis batch".into());
                    }
                    tasks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            if t.as_array().is_none_or(|a| a.len() != 4) {
                                return Err("Malformed memo task".into());
                            }
                            // An independently completed branch remains a settled
                            // witness when an unrelated peer made the outer batch
                            // fail. Its own original HTTP/result seal is decisive.
                            let receipt = res["provider_receipts"][i].clone();
                            let output = if res["result"]["Ok"].is_array() {
                                res["result"]["Ok"][i].clone()
                            } else {
                                receipt["native_result"]["Ok"].clone()
                            };
                            Ok((
                                t[0].clone(),
                                t[1].clone(),
                                t[2].clone(),
                                t[3].clone(),
                                req["prompts"][i].clone(),
                                output,
                                receipt,
                                Some(i),
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?
                } else {
                    vec![(
                        req["stage"].clone(),
                        req["matter"].clone(),
                        req["input"].clone(),
                        req["schema"].clone(),
                        req["prompt"].clone(),
                        res["result"]["Ok"].clone(),
                        res["provider_receipt"].clone(),
                        None,
                    )]
                };
                for (stage, matter, input, schema, prompt, output, receipt, branch) in branches {
                    let stage = stage.as_str().ok_or("No source stage")?.to_owned();
                    if stage == index.excluded_stage {
                        continue;
                    }
                    if !settled(&receipt, &output) {
                        excluded.push(json!({"request":request,"result":result,"branch":branch,"reason":"Not an original settled hosted response; failed, uncertain or already reused attempts remain in their source"}));
                        continue;
                    }
                    let raw_receipt = if let Some(i) = branch {
                        raw_index(&raw_field(result_text, "provider_receipts")?, i)?
                    } else {
                        raw_field(result_text, "provider_receipt")?
                    };
                    let raw_wire = raw_field(&raw_receipt, "request")?;
                    let signature = Signature {
                        case_id: index.case_id.clone(),
                        fixture_sha256: fixture.sha256.clone(),
                        initial_sha256: initial.sha256.clone(),
                        stage,
                        matter,
                        prompt,
                        schema,
                        input,
                        model_decoding: model.clone(),
                        wire_request: receipt["request"].clone(),
                        exact: Some(ExactHashes::trace(request_text, branch, &raw_wire)?),
                    };
                    entries.push(Entry{witness:json!({"version":VERSION,"key_sha256":signature.key()?,"source":source,"manifest":manifest,"request":request,"result":result,"branch":branch,
                        "source_native_fingerprint":{"native_executable_sha256":native_sha,"repository_sha":manifest_value["repository_sha"],"sources":manifest_value["sources"],
                            "executor_guide_identity":req["executor_guide_identity"]},"original_provider_receipt":receipt,
                        "origin":"Exact settled model response reused; current native validation still runs. This is not a new provider attempt or independent replication."}),signature,response:output});
                }
            }
        }
        Ok(Self {
            index,
            entries,
            excluded,
        })
    }
    pub fn index(&self) -> &Index {
        &self.index
    }
    pub fn lookup(&self, signature: &Signature) -> Result<Option<(Value, Value)>> {
        if signature.case_id != self.index.case_id
            || signature.fixture_sha256 != self.index.fixture_sha256
            || signature.initial_sha256 != self.index.initial_sha256
            || signature.stage == self.index.excluded_stage
        {
            return Ok(None);
        }
        let key = signature.key()?;
        for entry in &self.entries {
            if entry.signature.key()? == key {
                // Recheck all source pins at the moment of reuse, not merely preparation.
                let source: Source = serde_json::from_value(entry.witness["source"].clone())
                    .map_err(|e| e.to_string())?;
                maps(&source, &self.index.case_id)?;
                for field in ["manifest", "request", "result"] {
                    serde_json::from_value::<Pin>(entry.witness[field].clone())
                        .map_err(|e| e.to_string())?
                        .bytes()?;
                }
                return Ok(Some((entry.response.clone(), entry.witness.clone())));
            }
        }
        Ok(None)
    }
}

/// Current trace validators re-open the original sealed witness. A cache label
/// or old provider receipt alone cannot certify a settled current result.
pub fn verify_reuse(witness: &Value, current: &Signature, result: &Value) -> Result<()> {
    if witness["version"] != VERSION || witness["key_sha256"] != current.key()? {
        return Err("Memo execution key changed".into());
    }
    let source: Source =
        serde_json::from_value(witness["source"].clone()).map_err(|e| e.to_string())?;
    let memo = Memo::load(Index {
        version: VERSION.into(),
        case_id: current.case_id.clone(),
        fixture_sha256: current.fixture_sha256.clone(),
        initial_sha256: current.initial_sha256.clone(),
        excluded_stage: "no-target-stage-can-match".into(),
        sources: vec![source],
    })?;
    let Some((original, expected)) = memo.lookup(current)? else {
        return Err("Memo execution has no original settled exact response".into());
    };
    if original != *result || expected != *witness {
        return Err("Memo raw response or lineage changed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "horary-response-memo-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn write(file: &Path, value: &Value) -> Pin {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec_pretty(value).unwrap();
        fs::write(file, &bytes).unwrap();
        Pin {
            file: file.into(),
            sha256: digest(bytes),
        }
    }
    fn model() -> Value {
        json!({"provider":"google_gemini_api","id":"synthetic-model","endpoint":"https://example.invalid/generate",
        "decoding":"unconstrained","thinking_level":"minimal","temperature":0,"seed":0,"local_inference":false,"http_concurrency":4})
    }
    fn request(stage: &str, input: Value) -> Value {
        json!({"stage":stage,"matter":"general","input":input,"schema":{"type":"object"},
            "prompt":[{"role":"system","content":"Actual synthetic teaching"},{"role":"user","content":"Actual supplied synthetic input"}],
            "provider":model(),"executor_guide_identity":"effective-system-v1"})
    }
    fn receipt(request: &Value, content: &str) -> Value {
        let result = json!({"content":content,"elapsedMs":12});
        let body =
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":content}]}}]});
        json!({"provider":"google_gemini_api","submitted":true,"request":{"prompt":request["prompt"],"maximum":1000},
            "response":{"http_status":200,"body":body},"native_result":{"Ok":result},
            "generation_attempts":[{"submitted":true,"http_status":200,"body":body}]})
    }
    fn seal(root: &Path) -> Source {
        fn visit(root: &Path, path: &Path, files: &mut Vec<Value>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    visit(root, &path, files);
                } else if path.file_name().unwrap() != "completed.json" {
                    files.push(json!({"file":path.strip_prefix(root).unwrap(),"sha256":digest(fs::read(&path).unwrap())}));
                }
            }
        }
        let mut files = vec![];
        visit(root, root, &mut files);
        let pin = write(
            &root.join("completed.json"),
            &json!({"artifacts":files,"response_sha256":digest(fs::read(root.join("response.json")).unwrap())}),
        );
        Source {
            root: root.into(),
            kind: Kind::NativeOperation,
            seal: pin,
        }
    }
    fn completed_outcome() -> Value {
        json!({"id":"synthetic","scope":"reading_journey_function_only","full_reading":true,
            "execution_status":"completed","first_turn_execution_completed":true,
            "follow_up_execution_completed":null,"execution_error":null,
            "physical_generation_attempts_complete":true,"fresh_initial_sha256_verified":true,
            "target_function_invoked":true,"target_stage_authentic_inputs":true,
            "captured_checkpoint_reused":false,"deadline_cancelled":false,"group_cancelled":false,
            "infrastructure_error":null,"provider_stop":null})
    }
    fn rejected_outcome() -> Value {
        let mut outcome = completed_outcome();
        outcome["execution_status"] = json!("observed_native_validation_exhaustion");
        outcome["first_turn_execution_completed"] = json!(false);
        outcome["execution_error"] = json!(NATIVE_VALIDATION_STOP);
        outcome["known_native_semantic_abort"] = json!(true);
        outcome["semantic_abort_witness"] = json!({"stop":NATIVE_VALIDATION_STOP,
            "all_provider_responses_settled":true,"request_result_pairs_complete":true,
            "validation_error":"Synthetic worksheet omits a supplied native fact"});
        outcome
    }
    fn observed_outcome(rejected: bool) -> Value {
        let mut outcome = if rejected {
            rejected_outcome()
        } else {
            completed_outcome()
        };
        outcome["execution_status"] = json!("selected_stage_observed");
        outcome["scope"] = json!("selected_stage_function_only");
        outcome["full_reading"] = json!(false);
        outcome["selected_stage_observation"] = json!(true);
        outcome["target_function_invoked"] = json!(true);
        outcome["target_stage_authentic_inputs"] = json!(true);
        outcome["selected_stage"] = json!({"version":1,
            "state":if rejected {"native_rejected"} else {"accepted"},
            "all_provider_responses_settled":true,"request_result_pairs_complete":true,
            "target_attempts":2});
        outcome
    }

    #[test]
    fn no_progress_memo_source_requires_a_sealed_exact_native_cycle_not_a_flag() {
        let directory = Directory::new();
        let _ = source(&directory.0, vec![]);
        let (session, event) = crate::rejection_cycle::tests::session();
        let record = &session["method"]["records"][2];
        let witness = json!({"snapshot":"first-turn.json","record_index":2,"stage":"condition","revision":7,
            "input_sha256":event["inputSha256"],"original_input":crate::original_input(&record["input"]),
            "validation_error":record["validationError"],"stop":crate::rejection_cycle::STOP,"stop_kind":"no_progress",
            "paused_job":session["method"]["flow"]["jobs"][0],"no_progress":event,
            "all_provider_responses_settled":true,"request_result_pairs_complete":true});
        let mut outcome = rejected_outcome();
        outcome["execution_status"] = json!("observed_native_no_progress");
        outcome["execution_error"] = json!(crate::rejection_cycle::STOP);
        outcome["semantic_abort_witness"] = witness;
        let snapshot = json!({"result":{"Err":crate::rejection_cycle::STOP},"session":session});
        write(
            &directory.0.join("native/cases/synthetic/first-turn.json"),
            &snapshot,
        );
        write(
            &directory.0.join("response.json"),
            &json!({"outcome":outcome}),
        );
        let source = seal(&directory.0);
        assert!(native_outcome_is_settled(&outcome));
        maps(&source, "synthetic").unwrap();
        let mut bad = snapshot;
        bad["session"]["method"]["records"][2]["validationError"] = Value::Null;
        write(
            &directory.0.join("native/cases/synthetic/first-turn.json"),
            &bad,
        );
        assert!(maps(&source, "synthetic").is_err());
        let resealed = seal(&directory.0);
        assert!(maps(&resealed, "synthetic").is_err());
        let mut bad = outcome;
        bad["deadline_cancelled"] = json!(true);
        assert!(!native_outcome_is_settled(&bad));
    }
    fn source(root: &Path, calls: Vec<(Value, Value)>) -> (Source, Index) {
        let case = root.join("native/cases/synthetic");
        let fixture = write(
            &case.join("fixture.json"),
            &json!({"id":"synthetic","words":"Synthetic user concern; not a gold patch"}),
        );
        let initial = write(
            &case.join("initial.json"),
            &json!({"session":{"question":"","chart":null},"clock":17}),
        );
        write(
            &root.join("request.json"),
            &json!({"kind":"reading_journey","request":{"case":{"id":"synthetic",
            "source_fixture_sha256":fixture.sha256,"source_initial_sha256":initial.sha256},"native_executable_sha256":digest("original-native")}}),
        );
        write(
            &root.join("response.json"),
            &json!({"outcome":completed_outcome()}),
        );
        write(
            &root.join("native/manifest.json"),
            &json!({"model":model(),"repository_sha":"original-tree",
            "sources":{"source.rs":digest("original-source")}}),
        );
        for (n, (request, result)) in calls.into_iter().enumerate() {
            write(
                &case.join(format!("calls/{:04}-request.json", n + 1)),
                &request,
            );
            write(
                &case.join(format!("calls/{:04}-result.json", n + 1)),
                &result,
            );
        }
        let source = seal(root);
        let index = Index {
            version: VERSION.into(),
            case_id: "synthetic".into(),
            fixture_sha256: fixture.sha256,
            initial_sha256: initial.sha256,
            excluded_stage: "condition".into(),
            sources: vec![source.clone()],
        };
        (source, index)
    }
    fn call(request: Value, content: &str) -> (Value, Value) {
        let receipt = receipt(&request, content);
        let result =
            json!({"request":request,"result":receipt["native_result"],"provider_receipt":receipt});
        (request, result)
    }
    fn signature(index: &Index, request: &Value, result: &Value) -> Signature {
        Signature {
            case_id: index.case_id.clone(),
            fixture_sha256: index.fixture_sha256.clone(),
            initial_sha256: index.initial_sha256.clone(),
            stage: request["stage"].as_str().unwrap().into(),
            matter: request["matter"].clone(),
            prompt: request["prompt"].clone(),
            schema: request["schema"].clone(),
            input: request["input"].clone(),
            model_decoding: model_decoding(&request["provider"]).unwrap(),
            wire_request: result["provider_receipt"]["request"].clone(),
            exact: None,
        }
    }
    #[test]
    fn authentic_native_outcomes_use_settlement_not_an_invented_interruption_field() {
        let mut completed = completed_outcome();
        // Failed semantics do not conceal an otherwise settled raw response.
        completed["hurdles"] =
            json!({"extraction":{"status":"fail"},"reading":{"status":"blocked"}});
        for outcome in [
            completed,
            rejected_outcome(),
            observed_outcome(false),
            observed_outcome(true),
        ] {
            assert!(outcome.get("execution_interrupted").is_none());
            assert!(native_outcome_is_settled(&outcome), "{outcome}");
        }
        assert!(!native_outcome_is_settled(
            &json!({"id":"synthetic","execution_interrupted":false})
        ));
    }
    #[test]
    fn native_uncertainty_unknown_protocol_and_unobserved_stage_cannot_be_reused() {
        for field in [
            "physical_generation_attempts_complete",
            "fresh_initial_sha256_verified",
            "captured_checkpoint_reused",
            "target_function_invoked",
            "target_stage_authentic_inputs",
            "deadline_cancelled",
            "group_cancelled",
            "infrastructure_error",
            "provider_stop",
            "execution_error",
            "follow_up_execution_completed",
        ] {
            let mut missing = completed_outcome();
            missing.as_object_mut().unwrap().remove(field);
            assert!(!native_outcome_is_settled(&missing), "missing {field}");
        }
        for status in [
            "reading_executor_interrupted",
            "deadline_cancelled",
            "call_budget_exhausted",
            "future_protocol",
        ] {
            let mut outcome = completed_outcome();
            outcome["execution_status"] = json!(status);
            assert!(!native_outcome_is_settled(&outcome), "{status}");
        }
        for (field, value) in [
            ("deadline_cancelled", json!(true)),
            ("group_cancelled", json!(true)),
            ("provider_stop", json!("HTTP429")),
            ("infrastructure_error", json!("missing result")),
            ("follow_up_execution_completed", json!(false)),
            ("first_turn_execution_completed", json!(false)),
            ("target_function_invoked", json!(false)),
            ("target_stage_authentic_inputs", json!(false)),
        ] {
            let mut outcome = completed_outcome();
            outcome[field] = value;
            assert!(!native_outcome_is_settled(&outcome), "{field}");
        }
        let mut rejected = rejected_outcome();
        rejected["semantic_abort_witness"]["all_provider_responses_settled"] = json!(false);
        assert!(!native_outcome_is_settled(&rejected));
        let mut observed = observed_outcome(false);
        observed["selected_stage"]["state"] = json!("unobserved");
        assert!(!native_outcome_is_settled(&observed));
        observed["selected_stage"]["state"] = json!("native_rejected");
        assert!(!native_outcome_is_settled(&observed));
    }
    #[test]
    fn sealed_completed_native_operation_without_invented_field_reuses_its_raw_answer() {
        let dir = Directory::new();
        let (request, result) = call(
            request("place", json!({"query":"Synthetic"})),
            "invalid JSON retained",
        );
        let (_, index) = source(&dir.0, vec![(request.clone(), result.clone())]);
        let response: Value =
            serde_json::from_slice(&fs::read(dir.0.join("response.json")).unwrap()).unwrap();
        assert!(response["outcome"].get("execution_interrupted").is_none());
        let current = signature(&index, &request, &result);
        let (raw, witness) = Memo::load(index)
            .unwrap()
            .lookup(&current)
            .unwrap()
            .unwrap();
        assert_eq!(raw["content"], "invalid JSON retained");
        verify_reuse(&witness, &current, &raw).unwrap();

        let mut interrupted = response;
        interrupted["outcome"]["infrastructure_error"] = json!("Synthetic uncertain provider work");
        write(&dir.0.join("response.json"), &interrupted);
        let original = seal(&dir.0);
        assert!(Memo::load(Index {
            version: VERSION.into(),
            case_id: current.case_id,
            fixture_sha256: current.fixture_sha256,
            initial_sha256: current.initial_sha256,
            excluded_stage: "condition".into(),
            sources: vec![original]
        })
        .is_err());
    }
    #[test]
    fn exact_signature_keeps_native_invalid_answer_and_first_witness_without_cherry_picking() {
        let dir = Directory::new();
        let (request, result) = call(
            request("place", json!({"query":"Synthetic"})),
            "this is deliberately not JSON",
        );
        let second = call(request.clone(), "{\"mode\":\"now\"}");
        let (_, index) = source(&dir.0, vec![(request.clone(), result.clone()), second]);
        let memo = Memo::load(index.clone()).unwrap();
        let signature = signature(&index, &request, &result);
        let (raw, witness) = memo.lookup(&signature).unwrap().unwrap();
        assert_eq!(raw["content"], "this is deliberately not JSON");
        assert!(witness["request"]["file"]
            .as_str()
            .unwrap()
            .contains("0001-request"));
        verify_reuse(&witness, &signature, &raw).unwrap();
        let mut wrong = raw.clone();
        wrong["content"] = json!("{\"invented\":true}");
        assert!(verify_reuse(&witness, &signature, &wrong).is_err());
        // Current requests change after native rejection; stale answers are
        // not substituted for the different repair input.
        let mut repair = signature.clone();
        repair.input = json!({"original_input":repair.input,"validation_error":"Native rejection"});
        assert!(memo.lookup(&repair).unwrap().is_none());
    }
    #[test]
    fn target_and_all_exact_request_changes_are_cache_misses() {
        let dir = Directory::new();
        let (request, result) = call(
            request("place", json!({"query":"Synthetic"})),
            "{\"mode\":\"now\"}",
        );
        let (_, index) = source(&dir.0, vec![(request.clone(), result.clone())]);
        let memo = Memo::load(index.clone()).unwrap();
        let actual = signature(&index, &request, &result);
        for changed in [
            "case", "fixture", "initial", "stage", "input", "guide", "schema", "wire", "model",
            "repair",
        ] {
            let mut signature = actual.clone();
            match changed {
                "case" => signature.case_id = "another-case".into(),
                "fixture" => signature.fixture_sha256 = digest("other fixture"),
                "initial" => signature.initial_sha256 = digest("other device/clock"),
                "stage" => signature.stage = "condition".into(),
                "input" => signature.input["query"] = json!("Different"),
                "guide" => signature.prompt[0]["content"] = json!("Changed actual guide"),
                "schema" => signature.schema["required"] = json!(["newField"]),
                "wire" => signature.wire_request["maximum"] = json!(1400),
                "model" => signature.model_decoding["id"] = json!("different-model"),
                "repair" => {
                    signature.stage = "condition".into();
                    signature.input = json!({"original_input":signature.input,"error":"repair"});
                }
                _ => unreachable!(),
            }
            assert!(memo.lookup(&signature).unwrap().is_none(), "{changed}");
        }
    }
    #[test]
    fn seal_drift_uncertainty_and_fabricated_body_never_become_reuse() {
        let dir = Directory::new();
        let (request, result) = call(request("place", json!({})), "a raw invalid native answer");
        let (original, index) = source(&dir.0, vec![(request.clone(), result.clone())]);
        let actual = signature(&index, &request, &result);
        let memo = Memo::load(index.clone()).unwrap();
        fs::write(
            dir.0.join("native/cases/synthetic/calls/0001-result.json"),
            "changed",
        )
        .unwrap();
        assert!(memo.lookup(&actual).is_err());
        write(
            &dir.0.join("native/cases/synthetic/calls/0001-result.json"),
            &result,
        );
        assert!(memo.lookup(&actual).unwrap().is_some());
        for problem in [
            "uncertain",
            "not_submitted",
            "body",
            "response_seal",
            "duplicate",
            "escape",
        ] {
            let mut changed = result.clone();
            match problem {
                "uncertain" => {
                    changed["provider_receipt"]["generation_attempts"][0]["error"] =
                        json!("Transport uncertainty")
                }
                "not_submitted" => changed["provider_receipt"]["submitted"] = json!(false),
                "body" => {
                    changed["provider_receipt"]["response"]["body"]["candidates"][0]["content"]
                        ["parts"][0]["text"] = json!("Different actual response")
                }
                _ => {}
            }
            write(
                &dir.0.join("native/cases/synthetic/calls/0001-result.json"),
                &changed,
            );
            let new_source = seal(&dir.0);
            let mut modified = index.clone();
            modified.sources = vec![new_source.clone()];
            if matches!(problem, "response_seal" | "duplicate" | "escape") {
                let mut body = new_source.seal.value().unwrap();
                if problem == "response_seal" {
                    body["artifacts"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|e| e["file"] != "response.json");
                }
                if problem == "duplicate" {
                    let duplicate = body["artifacts"][0].clone();
                    body["artifacts"].as_array_mut().unwrap().push(duplicate);
                }
                if problem == "escape" {
                    body["artifacts"][0]["file"] = json!("../elsewhere");
                }
                modified.sources[0].seal = write(&dir.0.join("completed.json"), &body);
                assert!(Memo::load(modified).is_err(), "{problem}");
            } else {
                let rejected = Memo::load(modified).unwrap();
                assert!(rejected.lookup(&actual).unwrap().is_none(), "{problem}");
                assert_eq!(rejected.excluded.len(), 1);
            }
        }
        assert!(original.seal.bytes().is_err());
    }
    #[test]
    fn mixed_batch_preserves_branch_order_and_successful_peer_of_outer_failure() {
        let dir = Directory::new();
        let branches = [
            request("condition", json!({"role":"a"})),
            request("reception", json!({"role":"a"})),
            request("contacts", json!({"role":"a"})),
        ];
        let tasks = branches
            .iter()
            .map(|r| json!([r["stage"], r["matter"], r["input"], r["schema"]]))
            .collect::<Vec<_>>();
        let batch = json!({"tasks":tasks,"prompts":branches.iter().map(|r|r["prompt"].clone()).collect::<Vec<_>>(),"provider":model()});
        let receipts = branches
            .iter()
            .enumerate()
            .map(|(i, r)| receipt(r, &format!("raw branch {i}")))
            .collect::<Vec<_>>();
        let result = json!({"request":batch,"result":{"Err":"one unobserved peer"},"provider_receipts":receipts});
        let (_, index) = source(&dir.0, vec![(batch.clone(), result.clone())]);
        let memo = Memo::load(index.clone()).unwrap();
        let mut request = branches[1].clone();
        let mut result = result.clone();
        result["provider_receipt"] = result["provider_receipts"][1].clone();
        let actual = signature(&index, &request, &result);
        let (raw, witness) = memo.lookup(&actual).unwrap().unwrap();
        assert_eq!(raw["content"], "raw branch 1");
        assert_eq!(witness["branch"], 1);
        request["stage"] = json!("condition");
        assert!(memo
            .lookup(&signature(&index, &request, &result))
            .unwrap()
            .is_none());
        assert_eq!(witness["original_provider_receipt"]["submitted"], true);
    }

    #[test]
    fn raw_signature_keeps_exact_numbers_across_parser_features_without_rewriting_inputs() {
        let raw = r#"{"stage":"place","matter":"general","input":{"x":1.000000000000000000000000000001},
            "schema":{"type":"object"},"prompt":[{"role":"system","content":"Actual guide"},{"role":"user","content":"Actual user"}],
            "provider":{"provider":"google_gemini_api","id":"synthetic-model","endpoint":"https://example.invalid",
                "decoding":"unconstrained","thinking_level":"minimal","temperature":0,"seed":0,"local_inference":false},
            "memo_context":{"case_id":"synthetic","fixture_sha256":"fixture","initial_sha256":"initial"},
            "memo_wire_request":{"maximum":1000,"contents":[{"role":"user","text":"Actual user"}]}}"#;
        let first = signature_from_request(raw.as_bytes(), None).unwrap();
        let rounded = raw.replace("1.000000000000000000000000000001", "1");
        let second = signature_from_request(rounded.as_bytes(), None).unwrap();
        assert_ne!(first.key().unwrap(), second.key().unwrap());
        assert_eq!(
            canonical_hash("{\"a\":1,\"b\":[2,3]}").unwrap(),
            canonical_hash("{ \"b\" : [ 2 , 3 ], \"a\" : 1 }").unwrap()
        );
        assert_ne!(
            canonical_hash("[2,3]").unwrap(),
            canonical_hash("[3,2]").unwrap()
        );
        let mut quota = model();
        quota["http_concurrency"] = json!(1);
        assert_eq!(
            model_decoding(&quota).unwrap(),
            model_decoding(&model()).unwrap()
        );
        quota["temperature"] = json!(1);
        assert_ne!(
            model_decoding(&quota).unwrap(),
            model_decoding(&model()).unwrap()
        );
        let invalid = json!({"provider":"google_gemini_api","submitted":true,"response":{"http_status":200},
            "native_result":{"Ok":null},"generation_attempts":[{"submitted":true,"http_status":200}]});
        assert!(!settled(&invalid, &Value::Null));
    }

    #[test]
    fn raw_current_request_verifies_the_same_original_numeric_witness() {
        let dir = Directory::new();
        let (request, result) = call(
            request(
                "place",
                json!({"period":74.42100212548267,"latitude":38.6571999911827}),
            ),
            "an original native-invalid answer",
        );
        let (_, index) = source(&dir.0, vec![(request.clone(), result.clone())]);
        let memo = Memo::load(index.clone()).unwrap();
        let actual = signature(&index, &request, &result);
        let mut current = request.clone();
        current["memo_context"] = json!({"case_id":index.case_id,"fixture_sha256":index.fixture_sha256,"initial_sha256":index.initial_sha256});
        current["memo_wire_request"] = result["provider_receipt"]["request"].clone();
        let current_bytes = serde_json::to_vec_pretty(&current).unwrap();
        let readback = signature_from_request(&current_bytes, None).unwrap();
        assert_eq!(readback.key().unwrap(), actual.key().unwrap());
        let (raw, witness) = memo.lookup(&readback).unwrap().unwrap();
        verify_reuse(&witness, &readback, &raw).unwrap();
    }
}
