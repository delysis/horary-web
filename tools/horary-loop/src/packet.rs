#![forbid(unsafe_code)]
use crate::{
    store::{self, FileRef, Result},
    types::{FollowUpProvenance, Partition, Split, SplitCase},
};
use horary_prompt_program::{digest, Evidence, Signature};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CasePacket {
    pub id: String,
    pub method: String,
    pub mode: String,
    pub partition: Partition,
    pub fingerprint: String,
    pub files: Vec<FileRef>,
    pub summary: Value,
    pub calls: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Guide {
    pub stage: String,
    pub recognition_phase: Option<String>,
    pub method: Option<String>,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}
impl Guide {
    pub fn signature(&self) -> Signature<'_> {
        Signature {
            stage: &self.stage,
            recognition_phase: self.recognition_phase.as_deref(),
            method: self.method.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub version: u32,
    pub phase: String,
    pub manifest_sha256: String,
    pub candidate_sha256: Option<String>,
    pub qualification: String,
    pub cases: Vec<CasePacket>,
    pub guides: Vec<Guide>,
    #[serde(default)]
    pub guide_documents: BTreeMap<String, Vec<usize>>,
    #[serde(default)]
    pub teaching_chunks: Vec<String>,
    pub schemas: BTreeMap<String, Value>,
    pub inputs: BTreeMap<String, Value>,
    pub outputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub consultations: BTreeMap<String, Value>,
    #[serde(default)]
    pub omissions: Vec<String>,
}

impl Packet {
    pub fn guide_text(&self, guide: &Guide) -> Result<String> {
        if let Some(text) = &guide.text {
            return Ok(text.clone());
        }
        let chunks = self
            .guide_documents
            .get(&guide.sha256)
            .ok_or("Guide body absent")?;
        let text = chunks
            .iter()
            .map(|i| {
                self.teaching_chunks
                    .get(*i)
                    .map(String::as_str)
                    .ok_or_else(|| "Guide chunk absent".to_owned())
            })
            .collect::<Result<Vec<_>>>()?
            .concat();
        if digest(&text) != guide.sha256 {
            return Err("Reconstructed guide digest differs".into());
        }
        Ok(text)
    }
}

pub fn split(fixtures: &Path) -> Result<Split> {
    let mut files = fs::read_dir(fixtures)
        .map_err(|e| e.to_string())?
        .map(|r| r.map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    files.sort_by_key(|e| e.file_name());
    let mut cases = Vec::new();
    let mut fixture_files = Vec::new();
    let mut ids = BTreeSet::new();
    let mut coverage: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for file in files {
        let path = file.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        if !file.file_type().map_err(|e| e.to_string())?.is_file() {
            return Err("Fixture banks must be ordinary files".into());
        }
        let bytes = store::read(&path)?;
        let rows: Vec<Value> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let bank = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid fixture name")?;
        let file_sha = digest(&bytes);
        fixture_files.push(FileRef {
            file: file
                .file_name()
                .to_str()
                .ok_or("Invalid fixture name")?
                .into(),
            sha256: file_sha.clone(),
            bytes: bytes.len() as u64,
        });
        for row in rows {
            let id = row["id"].as_str().ok_or("Fixture needs ID")?;
            let method = row["method"].as_str().ok_or("Fixture needs method")?;
            let mode = row["mode"].as_str().ok_or("Fixture needs mode")?;
            if !store::safe_id(id) || !ids.insert(id.to_owned()) {
                return Err(format!("Invalid or repeated fixture ID {id}"));
            }
            if !["explicit", "implicit", "missing"].contains(&mode) {
                return Err(format!("Invalid scenario mode {mode}"));
            }
            let partition = if bank == "edge" {
                if parity(id) == 0 {
                    Partition::ReservedValidation
                } else {
                    Partition::Training
                }
            } else {
                coverage
                    .entry(method.into())
                    .or_default()
                    .insert(mode.into());
                let reserved = if parity(method) == 0 {
                    "explicit"
                } else {
                    "implicit"
                };
                if mode == reserved {
                    Partition::ReservedValidation
                } else {
                    Partition::Training
                }
            };
            cases.push(SplitCase {
                id: id.into(),
                method: method.into(),
                mode: mode.into(),
                bank: bank.into(),
                partition,
                fixture_sha256: file_sha.clone(),
            });
        }
    }
    if cases.is_empty() {
        return Err("Empty fixture bank".into());
    }
    for (method, modes) in coverage {
        if ["explicit", "implicit", "missing"]
            .iter()
            .any(|mode| !modes.contains(*mode))
        {
            return Err(format!(
                "{method} must supply explicit, implicit and missing before the split can freeze"
            ));
        }
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Split { version: 1,
        policy: "SHA256(method)[0] parity: even reserves explicit, odd implicit; missing stays training. Edge IDs: even SHA256(id)[0] reserves validation.".into(),
        qualification: "Reserved validation, not blind: this bank has already informed manual engineering. Writer never receives reserved words, gold, traces or prompt versions. No scalar can authorize promotion.".into(),
        fixture_files, cases })
}

fn parity(value: &str) -> u8 {
    u8::from_str_radix(&digest(value)[..2], 16).expect("digest is hexadecimal") & 1
}

pub fn original_input(mut value: &Value) -> &Value {
    while let Some(next) = value.get("original_input") {
        value = next;
    }
    value
}

fn method(input: &Value) -> Option<String> {
    input
        .pointer("/consultation/frame/observation/value/method")
        .or_else(|| input.pointer("/reading_request/binding/frame/method"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn state_summary(state: &Value) -> Value {
    let hurdles = state["hurdles"].clone();
    let state = state.get("session").unwrap_or(state);
    let records = state
        .pointer("/method/records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|record| json!({"stage":record["stage"],"revision":record["revision"],
            "guide_sha256":record["guideSha256"],"schema_sha256":record["schemaSha256"],
            "input_sha256":record["inputSha256"],"worksheet":record["worksheet"],
            "source_passages":record["sourcePassages"],"validation_error":record["validationError"]}))
        .collect::<Vec<_>>();
    let sections = state["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|section| {
            json!({"stage":section.get("method_stage").or_else(||section.get("methodStage")),
            "roles":section["roles"],"rules":section["rules"],"evidence":section["evidence"],
            "body":section["body"],"because":section["because"],"draft":section["draft"]})
        })
        .collect::<Vec<_>>();
    json!({"messages":state["messages"],"consultation":state.pointer("/method/consultation"),
        "candidateMomentMs":state["candidateMomentMs"],"methodResult":state.pointer("/method/result"),
        "methodRecords":records,"flow":state.pointer("/method/flow"),
        "sections":sections,"hurdles":hurdles,
        "pending":state.pointer("/method/flow/pending"),
        "chart":{"moment":state.pointer("/chart/timestampMs").or_else(||state.pointer("/chart/timestamp_ms")),"place":state["place"],
            "created":!state["chart"].is_null()},"chartAfterMessage":state["chartAfterMessage"]})
}

struct CallBranch {
    request: Value,
    result: Option<Value>,
    input_pointer: String,
    guide_pointer: String,
    result_pointer: String,
    provider_pointer: String,
}

/// Batch branches keep the parent file's hash and exact JSON pointer. Virtual
/// per-stage projections never replace or rewrite the original request/result.
fn call_branches(request: &Value, result: Option<&Value>) -> Result<Vec<CallBranch>> {
    let Some(tasks) = request.get("tasks") else {
        return Ok(vec![CallBranch {
            request: request.clone(),
            result: result.cloned(),
            input_pointer: "/input".into(),
            guide_pointer: "/prompt/0/content".into(),
            result_pointer: "/result/Ok/content".into(),
            provider_pointer: "/provider_receipt".into(),
        }]);
    };
    let tasks = tasks
        .as_array()
        .ok_or("Native batch tasks are not an array")?;
    let prompts = request["prompts"]
        .as_array()
        .ok_or("Native batch lacks exact branch prompts")?;
    if tasks.is_empty() || tasks.len() > 4 || tasks.len() != prompts.len() {
        return Err("Native analysis batch needs one to four matched tasks/prompts".into());
    }
    let outputs = result.and_then(|value| value.pointer("/result/Ok"));
    if let Some(outputs) = outputs {
        if outputs
            .as_array()
            .is_none_or(|outputs| outputs.len() != tasks.len())
        {
            return Err("Native batch results do not match its branch count".into());
        }
    }
    tasks
        .iter()
        .zip(prompts)
        .enumerate()
        .map(|(index, (task, prompt))| {
            let task = task.as_array().ok_or("Native batch task is not a tuple")?;
            if task.len() != 4 || task[0].as_str().is_none() {
                return Err("Native batch task must be [stage,matter,input,schema]".into());
            }
            let guide = prompt
                .pointer("/0/content")
                .and_then(Value::as_str)
                .ok_or("Native branch lacks actual system teaching")?;
            let program = request["prompt_program"]
                .get(index)
                .cloned()
                .unwrap_or(Value::Null);
            let guide_sha = program["original_guide_sha256"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| digest(guide));
            let projected_result = result.map(|result| {
                let generation = if let Some(outputs) = outputs {
                    json!({"Ok":outputs[index]})
                } else {
                    result["result"].clone()
                };
                json!({"wall_ms":result["wall_ms"],"result":generation,
                    "provider_receipt":result["provider_receipts"].get(index)})
            });
            Ok(CallBranch {
                request: json!({"sequence":request["sequence"],"batch_branch":index,
                    "batch_size":tasks.len(),"stage":task[0],"matter":task[1],
                    "input":task[2],"schema":task[3],"prompt":prompt,
                    "guide_sha256":guide_sha,"prompt_program":program,"decoder":request["decoder"],
                    "provider":request["provider"]}),
                result: projected_result,
                input_pointer: format!("/tasks/{index}/2"),
                guide_pointer: format!("/prompts/{index}/0/content"),
                result_pointer: format!("/result/Ok/{index}/content"),
                provider_pointer: format!("/provider_receipts/{index}"),
            })
        })
        .collect()
}

fn compact_consultation(packet: &mut Packet, value: &mut Value) {
    if let Some(consultation) = value.get_mut("consultation") {
        if !consultation.is_object() {
            return;
        }
        let mut kept = consultation.clone();
        kept.as_object_mut()
            .expect("object checked")
            .remove("changes");
        let sha = digest(kept.to_string());
        packet.consultations.entry(sha.clone()).or_insert(kept);
        *consultation = json!({"consultation_ref":sha});
    }
}

fn follow_up_provenance(after: &Value, case_id: &str, source: &FileRef) -> FollowUpProvenance {
    let executed = matches!(
        after["status"].as_str(),
        Some(
            "executed"
                | "executed after matching elicitation"
                | "executed after one eligible authored proposal"
        )
    );
    let result_recorded = after["result"]
        .as_object()
        .is_some_and(|r| r.len() == 1 && (r.contains_key("Ok") || r.contains_key("Err")));
    let user_turn_submitted = executed
        && result_recorded
        && after["words"]
            .as_str()
            .is_some_and(|words| !words.trim().is_empty());
    let assistant_reply_observed = user_turn_submitted
        && after
            .pointer("/grade/fresh_assistant_reply")
            .and_then(Value::as_bool)
            == Some(true)
        && after
            .pointer("/grade/after_state_grade/actual/reply")
            .and_then(Value::as_str)
            .is_some_and(|reply| !reply.trim().is_empty());
    FollowUpProvenance {
        version: 1,
        user_turn_submitted,
        assistant_reply_observed,
        source: Evidence {
            case_id: case_id.into(),
            file: source.file.clone(),
            json_pointer: "/follow_up".into(),
            sha256: source.sha256.clone(),
        },
    }
}

/// Verify both current and legacy compact projections against the saved receipt.
pub fn recorded_follow_up(state: &Path, case: &CasePacket) -> Result<FollowUpProvenance> {
    let file = format!("cases/{}/outcome.json", case.id);
    let source = case
        .files
        .iter()
        .find(|f| f.file == file)
        .ok_or("Follow-up has no immutable native outcome")?;
    let outcome = store::resolve_blob(state, source)?;
    let after = &outcome["follow_up"];
    let kept = &case.summary["follow_up"];
    for field in ["status", "words", "result"] {
        if kept[field] != after[field] {
            return Err(format!("Follow-up projection changed native {field}"));
        }
    }
    let provenance = follow_up_provenance(after, &case.id, source);
    if let Some(value) = kept.get("execution_provenance") {
        let supplied: FollowUpProvenance =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        if supplied != provenance {
            return Err(
                "Follow-up execution provenance differs from the immutable native receipt".into(),
            );
        }
    }
    Ok(provenance)
}

/// A reviewer cannot turn a partial or blocked run into an observed judgment by
/// changing the compact projection. Presence comes from the immutable final.
pub fn recorded_reading(state: &Path, case: &CasePacket) -> Result<bool> {
    let snapshot = |name: &str| {
        let file = format!("cases/{}/{name}.json", case.id);
        let source = case
            .files
            .iter()
            .find(|source| source.file == file)
            .ok_or_else(|| format!("Full reading lacks immutable {name}"))?;
        store::resolve_blob(state, source)
    };
    let first = snapshot("first-turn")?;
    let final_state = snapshot("final")?;
    let rubric = snapshot("reading-rubric")?;
    if first["hurdles"].is_null()
        || final_state["hurdles"].is_null()
        || first["hurdles"] != case.summary["first_hurdles"]
        || final_state["hurdles"] != case.summary["final_hurdles"]
        || rubric != case.summary["reading_rubric"]
        || rubric["case_id"] != case.id
        || final_state.pointer("/session/method/result")
            != case.summary.pointer("/final_state/methodResult")
    {
        return Err(
            "Full-reading projection differs from immutable hurdles, rubric or result".into(),
        );
    }
    let blocked = matches!(
        final_state
            .pointer("/hurdles/reading/status")
            .and_then(Value::as_str),
        Some("blocked" | "not_run" | "awaiting_information")
    );
    Ok(!blocked
        && final_state
            .pointer("/session/method/result/result")
            .and_then(Value::as_str)
            == Some("judgment"))
}

fn compact_follow_up(outcome: &Value, case_id: &str, source: &FileRef) -> Value {
    let after = &outcome["follow_up"];
    if after.is_null() {
        return Value::Null;
    }
    let grade = &after["grade"];
    let actual = &grade["after_state_grade"]["actual"];
    json!({"status":after["status"],"words":after["words"],"result":after["result"],
        "execution_provenance":follow_up_provenance(after,case_id,source),
        "pass":grade["pass"],"failures":grade["failures"],"input_complete":grade["input_complete"],
        "current_needs":grade["current_needs"],"native_needs":grade["native_needs"],
        "fresh_assistant_reply":grade["fresh_assistant_reply"],"question_preserved":grade["question_preserved"],
        "candidate_moment_preserved":grade["candidate_moment_preserved"],
        "native_handoff_recorded":grade["native_handoff_recorded"],
        "after_state_grade":{"semantic_pass":grade["after_state_grade"]["semantic_pass"],
            "mismatches":grade["after_state_grade"]["mismatches"],
            "actual":{"frame":actual["frame"],"question":actual["question"],"needs":actual["needs"],
                "ready":actual["ready"],"anchor":actual["anchor"],"reply":actual["reply"]}}})
}

pub fn completed(campaign: &Path, split: &Split, validation: bool) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for case in &split.cases {
        if (case.partition == Partition::ReservedValidation) != validation {
            continue;
        }
        let dir = campaign.join("cases").join(&case.id);
        let outcome = dir.join("outcome.json");
        if !outcome.exists() {
            continue;
        }
        let data = store::json(&outcome)?;
        if data["id"] != case.id {
            return Err(format!("Outcome ID does not match {}", case.id));
        }
        // A final receipt is required; a currently executing case is never reviewed.
        if !dir.join("final.json").is_file() {
            return Err(format!(
                "Completed outcome without final state: {}",
                case.id
            ));
        }
        ids.push(case.id.clone());
    }
    Ok(ids)
}

pub fn build(
    campaign: &Path,
    state: &Path,
    split: &Split,
    ids: &[String],
    manifest_sha: &str,
    validation: bool,
    candidate_sha: Option<String>,
) -> Result<Packet> {
    let manifest = store::json(&campaign.join("manifest.json"))?;
    let full_reading =
        manifest["full_reading"] == true || manifest["entry_point"] == "horary_pipeline::run";
    let mut packet = Packet {
        version: 1,
        phase: if validation { "validation" } else { "training" }.into(),
        manifest_sha256: manifest_sha.into(),
        candidate_sha256: candidate_sha,
        qualification: split.qualification.clone(),
        cases: Vec::new(),
        guides: Vec::new(),
        guide_documents: BTreeMap::new(),
        teaching_chunks: Vec::new(),
        schemas: BTreeMap::new(),
        inputs: BTreeMap::new(),
        outputs: BTreeMap::new(),
        consultations: BTreeMap::new(),
        omissions: vec!["Reviewer packet is a compact processing view, not a replacement for exact original receipts. Every original JSON/JSONL file remains immutable in blobs and retains its file/SHA citation.".into(),
            "Repeated guide paragraphs are teaching_chunks; guide_documents list their order and reconstruct the exact guide SHA. Guide scopes are metadata, not duplicated source.".into(),
            "Repair wrappers are represented as the original accepted task, rejected worksheet and native feedback by sequence. Full repair messages/input remain in each hashed request.".into(),
            "Duplicate consultation histories/changes and chart geometry are omitted. Full-reading views retain checked worksheets/roles/source references, native flow/results and numerical facts in actual stage inputs; exact primary records remain available by hash.".into()],
    };
    let mut guide_keys = BTreeSet::new();
    let mut chunks = BTreeMap::new();
    let canonical_fixtures: Vec<Value> =
        serde_json::from_slice(&store::read(&campaign.join("fixtures.json"))?)
            .map_err(|e| e.to_string())?;
    for id in ids {
        let spec = split
            .cases
            .iter()
            .find(|c| &c.id == id)
            .ok_or("Case outside fixed split")?;
        if (spec.partition == Partition::ReservedValidation) != validation {
            return Err("Training/validation packet mixing is forbidden".into());
        }
        let dir = campaign.join("cases").join(id);
        let mut paths = Vec::new();
        store::collect_files(campaign, &dir, &mut paths)?;
        let mut refs = Vec::new();
        let mut values = BTreeMap::new();
        for (relative, path) in paths {
            let bytes = store::read(&path)?;
            let reference = store::snapshot(state, &relative, &bytes)?;
            if path.extension().is_some_and(|e| e == "json") {
                values.insert(
                    relative.clone(),
                    serde_json::from_slice::<Value>(&bytes)
                        .map_err(|e| format!("{relative}: {e}"))?,
                );
            }
            refs.push(reference);
        }
        let fingerprint = digest(serde_json::to_vec(&refs).map_err(|e| e.to_string())?);
        let outcome = values
            .get(&format!("cases/{id}/outcome.json"))
            .ok_or("Missing outcome")?;
        let first = values
            .get(&format!("cases/{id}/first-turn.json"))
            .ok_or("Missing first turn")?;
        let final_state = values
            .get(&format!("cases/{id}/final.json"))
            .ok_or("Missing final state")?;
        let fixture = values
            .get(&format!("cases/{id}/fixture.json"))
            .ok_or("Missing authored fixture")?;
        if canonical_fixtures.iter().find(|row| row["id"] == *id) != Some(fixture) {
            return Err(format!("Per-case fixture differs from frozen gold: {id}"));
        }
        let mut calls = Vec::new();
        for (file, request) in values
            .iter()
            .filter(|(file, _)| file.ends_with("-request.json"))
        {
            let result_file = file.replace("-request.json", "-result.json");
            for branch in call_branches(request, values.get(&result_file))? {
                let request = &branch.request;
                let original = original_input(&request["input"]);
                let stage = request["stage"]
                    .as_str()
                    .ok_or("Call has no typed stage")?
                    .to_owned();
                let phase = original["recognition_phase"].as_str().map(str::to_owned);
                let selected_method = method(original);
                let guide = request
                    .pointer("/prompt/0/content")
                    .and_then(Value::as_str)
                    .ok_or("Call has no guide")?;
                let base_guide_sha = request["guide_sha256"]
                    .as_str()
                    .ok_or("Call has no guide digest")?;
                let guide_sha_owned = digest(guide);
                if guide_sha_owned != base_guide_sha
                    && (request
                        .pointer("/prompt_program/original_guide_sha256")
                        .and_then(Value::as_str)
                        != Some(base_guide_sha)
                        || request
                            .pointer("/prompt_program/replacement_guide_sha256")
                            .and_then(Value::as_str)
                            != Some(guide_sha_owned.as_str()))
                {
                    return Err(format!(
                        "Guide does not match baseline or applied program receipt at {file}"
                    ));
                }
                let guide_sha = guide_sha_owned.as_str();
                let key = (
                    stage.clone(),
                    phase.clone(),
                    selected_method.clone(),
                    guide_sha.to_owned(),
                );
                if guide_keys.insert(key) {
                    packet.guides.push(Guide {
                        stage: stage.clone(),
                        recognition_phase: phase.clone(),
                        method: selected_method.clone(),
                        sha256: guide_sha.into(),
                        text: None,
                    });
                }
                if !packet.guide_documents.contains_key(guide_sha) {
                    let mut document = Vec::new();
                    for part in guide.split_inclusive("\n\n") {
                        let key = digest(part);
                        let index = *chunks.entry(key).or_insert_with(|| {
                            let index = packet.teaching_chunks.len();
                            packet.teaching_chunks.push(part.to_owned());
                            index
                        });
                        document.push(index);
                    }
                    packet.guide_documents.insert(guide_sha.into(), document);
                }
                let schema = &request["schema"];
                let schema_sha = digest(schema.to_string());
                packet
                    .schemas
                    .entry(schema_sha.clone())
                    .or_insert_with(|| schema.clone());
                let mut input = original.clone();
                compact_consultation(&mut packet, &mut input);
                let input_sha = digest(input.to_string());
                packet.inputs.entry(input_sha.clone()).or_insert(input);
                let result = branch.result.as_ref();
                // A hosted analysis batch may reject as a group while individual
                // branches already returned usable proposals. Keep those proposals
                // as observed attempts; the outer error still prevents acceptance.
                let native_raw = result.and_then(|r| r.pointer("/result/Ok/content"));
                let provider_raw =
                    result.and_then(|r| r.pointer("/provider_receipt/native_result/Ok/content"));
                let raw = native_raw.or(provider_raw);
                let result_pointer = if native_raw.is_some() || provider_raw.is_none() {
                    branch.result_pointer.clone()
                } else {
                    format!("{}/native_result/Ok/content", branch.provider_pointer)
                };
                let provider_receipt = result
                    .and_then(|r| r.get("provider_receipt"))
                    .filter(|r| !r.is_null());
                let output_sha = raw.map(|r| digest(r.to_string()));
                if let (Some(raw), Some(sha)) = (raw, &output_sha) {
                    packet
                        .outputs
                        .entry(sha.clone())
                        .or_insert_with(|| raw.clone());
                }
                calls.push(json!({"sequence":request["sequence"],"stage":stage,"recognition_phase":phase,
                "batch_branch":request["batch_branch"],"batch_size":request["batch_size"],
                "method":selected_method,"guide_sha256":guide_sha,"schema_sha256":schema_sha,
                "input_sha256":input_sha,"output_sha256":output_sha,
                "request_input_sha256":digest(request["input"].to_string()),
                "rejected_worksheet":request["input"]["previous_worksheet"],
                "native_validation_error":request["input"]["native_validation_error"],
                "result_error":result.and_then(|r|r.pointer("/result/Err")),
                "wall_ms":result.map(|r|&r["wall_ms"]),
                "metrics":result.and_then(|r|r.pointer("/result/Ok")).map(|v|json!({
                    "batchSize":v["batchSize"],"elapsedMs":v["elapsedMs"],"promptTokens":v["promptTokens"],
                    "generatedTokens":v["generatedTokens"],"lessonPrepareMs":v["lessonPrepareMs"],
                    "cachedPromptTokens":v["cachedPromptTokens"],"prefilledPromptTokens":v["prefilledPromptTokens"]})),
                "decoder":request["decoder"],"provider":request["provider"],
                "provider_wire":provider_receipt.map(|r|json!({"submitted":r["submitted"],
                    "request_sha256":r.get("request").map(|v|digest(v.to_string())),
                    "response_sha256":r.get("response").map(|v|digest(v.to_string())),
                    "http_status":r.pointer("/response/http_status"),"transport_error":r.pointer("/response/transport_error"),
                    "provider_error":r.pointer("/native_result/Err"),
                    "queue_ms":r["queue_ms"],"http_wall_ms":r["http_wall_ms"]})),
                "provider_pointer":provider_receipt.map(|_|&branch.provider_pointer),
                "request_file":file,"result_file":result_file,
                "input_pointer":branch.input_pointer,"guide_pointer":branch.guide_pointer,
                "result_pointer":result_pointer}));
            }
        }
        let actual = &outcome["grade"]["actual"];
        let mut first_state = state_summary(first);
        let mut final_summary = state_summary(final_state);
        compact_consultation(&mut packet, &mut first_state);
        compact_consultation(&mut packet, &mut final_summary);
        let outcome_source = refs
            .iter()
            .find(|f| f.file == format!("cases/{id}/outcome.json"))
            .ok_or("Missing native outcome snapshot")?;
        let follow_up = compact_follow_up(outcome, id, outcome_source);
        let reading_rubric = values.get(&format!("cases/{id}/reading-rubric.json"));
        if full_reading && reading_rubric.is_none() {
            return Err(format!(
                "Full reading {id} lacks its source-bound reading-rubric.json"
            ));
        }
        if full_reading
            && (first["hurdles"].is_null()
                || final_state["hurdles"].is_null()
                || reading_rubric.is_some_and(|rubric| rubric["case_id"] != *id))
        {
            return Err(format!(
                "Full reading {id} has absent hurdles or a mismatched rubric ID"
            ));
        }
        packet.cases.push(CasePacket { id:id.clone(),method:spec.method.clone(),mode:spec.mode.clone(),
            partition:spec.partition,fingerprint,files:refs, summary:json!({
                "full_reading":full_reading,"reading_rubric":reading_rubric,
                "inference":manifest["model"],
                "first_hurdles":first["hurdles"],"final_hurdles":final_state["hurdles"],
                "words":fixture["words"],"scripted_follow_up":fixture["follow_up"],
                "expected":fixture["expected"],"follow_up_expected":fixture["follow_up_expected"],
                "first_execution_completed":outcome["first_turn_execution_completed"],
                "native_semantic_pass":outcome["grade"]["semantic_pass"],
                "native_journey_pass":outcome["follow_up_pass"],"mismatches":outcome["grade"]["mismatches"],
                "fluidity_heuristics":outcome["grade"]["fluidity_review_flags"],
                "execution_status":outcome["execution_status"],"group_cancelled":outcome["group_cancelled"],
                "deadline_cancelled":outcome["deadline_cancelled"],"infrastructure_error":outcome["infrastructure_error"],
                "first_actual":{"frame":actual["frame"],"question":actual["question"],"needs":actual["needs"],
                    "requested":actual["requested"],"ready":actual["ready"],"plan":actual["plan"],
                    "anchor":actual["anchor"],"reader_place":actual["reader_place"],"reply":actual["reply"],
                    "native_result_kind":actual["native_result"]["result"]},
                "follow_up":follow_up,
                "first_state":first_state,"final_state":final_summary}), calls });
    }
    Ok(packet)
}

pub fn validate_evidence(packet: &Packet, state: &Path, evidence: &Evidence) -> Result<()> {
    let case = packet
        .cases
        .iter()
        .find(|c| c.id == evidence.case_id)
        .ok_or("Citation refers to a case not in this job")?;
    store::safe_relative(&evidence.file)?;
    let reference = case
        .files
        .iter()
        .find(|f| f.file == evidence.file)
        .ok_or("Citation is not an immutable input record")?;
    if reference.sha256 != evidence.sha256 {
        return Err("Citation hash mismatch".into());
    }
    let value = store::resolve_blob(state, reference)?;
    if value.pointer(&evidence.json_pointer).is_none() {
        return Err(format!(
            "Citation JSON pointer absent: {}{}",
            evidence.file, evidence.json_pointer
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis_batch() -> (Value, Value) {
        let guide = "Check the accepted roles.\n<book_extracts>Printed p. 147: Source witness.</book_extracts>\nMethod teaching.";
        let tasks: Vec<_> = ["condition", "reception", "contacts"].iter().map(|stage| {
            json!([stage,"lost_object",{"reading_request":{"binding":{"frame":{"method":"lost_object"}}},
                "facts":[{"id":"jupiter-position","longitude":121.25}],"roles":[{"id":"owner","house":1}]},
                {"type":"object"}])
        }).collect();
        let prompts: Vec<_> = tasks
            .iter()
            .map(|task| {
                json!([
            {"role":"system","content":guide},{"role":"user","content":task[2].to_string()}])
            })
            .collect();
        let outputs: Vec<_> = tasks
            .iter()
            .map(|task| {
                json!({"content":json!({"stage":task[0],"because":"Native testimony."}).to_string(),
            "batchSize":3,"promptTokens":42,"generatedTokens":12})
            })
            .collect();
        (
            json!({"sequence":7,"tasks":tasks,"prompts":prompts,"decoder":"production independent batch",
            "provider":{"provider":"google_gemini_api","id":"gemma-4-26b-a4b-it","local_inference":false}}),
            json!({"wall_ms":150,"result":{"Ok":outputs}}),
        )
    }

    #[test]
    fn batch_normalization_preserves_exact_branch_pointers_and_group_errors() {
        let (request, result) = analysis_batch();
        let branches = call_branches(&request, Some(&result)).unwrap();
        assert_eq!(branches.len(), 3);
        for (index, branch) in branches.iter().enumerate() {
            assert_eq!(
                request.pointer(&branch.input_pointer),
                Some(&branch.request["input"])
            );
            assert_eq!(
                request.pointer(&branch.guide_pointer),
                branch.request.pointer("/prompt/0/content")
            );
            assert_eq!(
                result.pointer(&branch.result_pointer),
                branch
                    .result
                    .as_ref()
                    .and_then(|r| r.pointer("/result/Ok/content"))
            );
            assert_eq!(branch.request["batch_branch"], index);
            assert_eq!(branch.request["batch_size"], 3);
            assert_eq!(branch.request["provider"], request["provider"]);
        }
        let interrupted = json!({"wall_ms":150,"result":{"Err":"Shared native batch interrupted"}});
        assert!(call_branches(&request, Some(&interrupted))
            .unwrap()
            .iter()
            .all(|b| b.result.as_ref().unwrap()["result"] == interrupted["result"]));
        let mut mismatch = result;
        mismatch["result"]["Ok"].as_array_mut().unwrap().pop();
        assert!(call_branches(&request, Some(&mismatch)).is_err());
        let mut invalid = request.clone();
        invalid["prompts"].as_array_mut().unwrap().pop();
        assert!(call_branches(&invalid, None).is_err());
        let mut oversized = request;
        for _ in 0..2 {
            let task = oversized["tasks"][0].clone();
            let prompt = oversized["prompts"][0].clone();
            oversized["tasks"].as_array_mut().unwrap().push(task);
            oversized["prompts"].as_array_mut().unwrap().push(prompt);
        }
        assert!(call_branches(&oversized, None).is_err());
    }

    #[test]
    fn full_packet_preserves_reading_witnesses_and_successful_hosted_attempts_after_batch_failure()
    {
        let dir = tempfile::tempdir().unwrap();
        let campaign = dir.path().join("campaign");
        let state = dir.path().join("review");
        let case_dir = campaign.join("cases/train");
        let fixture = json!({"id":"train","method":"lost_object","mode":"explicit","words":"Where is my ring?","expected":{"needs":[]}});
        let manifest = json!({"full_reading":true,"entry_point":"horary_pipeline::run",
            "model":{"provider":"google_gemini_api","id":"gemma-4-26b-a4b-it","local_inference":false}});
        let hurdles = json!({"classification":{"status":"pass"},"extraction":{"status":"pass"},
            "elicitation":{"status":"pass"},"reading":{"status":"structure_pass_review_pending"}});
        let final_state = json!({"hurdles":hurdles,"session":{"messages":[{"role":"assistant","content":"The ring may be near the doorway."}],
            "chart":{"timestampMs":1791388800000_i64},"place":{"label":"Woodbridge, VA"},"candidateMomentMs":1791388800000_i64,
            "method":{"result":{"result":"judgment","answer":"The ring may be near the doorway.","evidence":["jupiter-position"],
                "worksheet":{"checks":{"location":{"evidence":["jupiter-position"]}}}},
                "records":[{"stage":"location","revision":1,"guideSha256":digest("guide"),"worksheet":{"house":1,"because":"Location testimony."},"sourcePassages":["lost-location"]}],
                "flow":{"pending":null,"jobs":[{"stage":"location","phase":"complete"}]}},
            "sections":[{"method_stage":"location","roles":[{"id":"owner","house":1}],"rules":["lost-location"],"evidence":["jupiter-position"],"body":"A location testimony."}]}});
        let rubric = json!({"case_id":"train","source":{"printed_pages":"147","rule_ids":["lost-location"]},
            "required_roles":[{"role":"owner","house_or_derivation":"1"}],
            "decisive_tests":[{"id":"ring_location","test":"Use actual location testimonies"}],
            "forbidden_inferences":["Invented address"]});
        let outcome = json!({"id":"train","first_turn_execution_completed":true,
            "grade":{"semantic_pass":true,"actual":{"reply":"The ring may be near the doorway.","needs":[]}},
            "follow_up":{"status":"not scripted"},"follow_up_pass":null});
        store::atomic_json(&campaign.join("manifest.json"), &manifest, true).unwrap();
        store::atomic_json(
            &campaign.join("fixtures.json"),
            &vec![fixture.clone()],
            true,
        )
        .unwrap();
        for (name, value) in [
            ("fixture", &fixture),
            ("first-turn", &final_state),
            ("final", &final_state),
            ("outcome", &outcome),
            ("reading-rubric", &rubric),
        ] {
            store::atomic_json(&case_dir.join(format!("{name}.json")), value, true).unwrap();
        }
        let (request, mut result) = analysis_batch();
        let native_outputs = result["result"]["Ok"].clone();
        result["result"] =
            json!({"Err":"Contacts HTTP branch failed; no batch worksheet accepted"});
        result["provider_receipts"] = json!([
            {"submitted":true,"request":{"contents":[{"parts":[{"text":"Exact condition wire"}]}]},"response":{"http_status":200,"body":{"usageMetadata":{"promptTokenCount":42}}},
                "generation_attempts":[{"attempt":1,"submitted":true,"http_status":503,"body":{"error":{"message":"Original service failure"}}},{"attempt":2,"submitted":true,"http_status":200,"body":{"usageMetadata":{"promptTokenCount":42}}}],
                "native_result":{"Ok":native_outputs[0]}},
            {"submitted":true,"request":{"contents":[{"parts":[{"text":"Exact reception wire"}]}]},"response":{"http_status":200},"native_result":{"Ok":native_outputs[1]}},
            {"submitted":true,"request":{"contents":[{"parts":[{"text":"Exact contacts wire"}]}]},"response":{"http_status":429},"native_result":{"Err":"Quota"}}
        ]);
        store::atomic_json(&case_dir.join("calls/0007-request.json"), &request, true).unwrap();
        store::atomic_json(&case_dir.join("calls/0007-result.json"), &result, true).unwrap();
        let split = Split {
            version: 1,
            policy: "test".into(),
            qualification: "Synthetic method review".into(),
            fixture_files: vec![],
            cases: vec![SplitCase {
                id: "train".into(),
                method: "lost_object".into(),
                mode: "explicit".into(),
                bank: "core".into(),
                partition: Partition::Training,
                fixture_sha256: digest("fixture"),
            }],
        };
        let packet = build(
            &campaign,
            &state,
            &split,
            &["train".into()],
            &digest(manifest.to_string()),
            false,
            None,
        )
        .unwrap();
        assert_eq!(packet.cases[0].calls.len(), 3);
        assert_eq!(packet.outputs.len(), 2);
        assert_eq!(
            packet.cases[0].summary["final_state"]["methodResult"]["answer"],
            "The ring may be near the doorway."
        );
        assert_eq!(
            packet.cases[0].summary["final_state"]["methodRecords"][0]["worksheet"]["house"],
            1
        );
        assert_eq!(
            packet.cases[0].summary["final_state"]["sections"][0]["roles"][0]["house"],
            1
        );
        assert_eq!(
            packet.cases[0].summary["final_state"]["chart"]["moment"],
            1791388800000_i64
        );
        assert!(recorded_reading(&state, &packet.cases[0]).unwrap());
        for call in &packet.cases[0].calls {
            assert!(call["result_error"]
                .as_str()
                .unwrap()
                .contains("no batch worksheet accepted"));
            assert_eq!(call["provider"]["local_inference"], false);
        }
        let (view, index) = crate::refs::context(&packet, None).unwrap();
        assert!(view["source_documents"]
            .to_string()
            .contains("Printed p. 147"));
        assert!(view["inputs"].to_string().contains("121.25"));
        assert!(
            view["cases"][0]["summary"]["reading_rubric"]["decisive_tests"][0]["test"].is_string()
        );
        for evidence in index.citations.values() {
            validate_evidence(&packet, &state, evidence).unwrap();
        }
        let condition_result = index
            .citations
            .values()
            .find(|e| e.json_pointer == "/provider_receipts/0/native_result/Ok/content")
            .unwrap();
        assert_eq!(condition_result.file, "cases/train/calls/0007-result.json");
        let result_source = packet.cases[0]
            .files
            .iter()
            .find(|source| source.file == condition_result.file)
            .unwrap();
        let original_result = store::resolve_blob(&state, result_source).unwrap();
        assert_eq!(
            original_result["provider_receipts"][0]["generation_attempts"],
            result["provider_receipts"][0]["generation_attempts"]
        );
        let reviews = vec![
            json!({"findings":[{"repair_owner":"prompt","stage":"condition","recognition_phase":null,"method":"lost_object"}]}),
        ];
        let (writer, _) = crate::refs::context(&packet, Some(&reviews)).unwrap();
        assert_eq!(writer["source_documents"], json!({}));
        assert!(!writer["teaching_chunks"]
            .to_string()
            .contains("Printed p. 147"));
        assert_eq!(
            store::json(&case_dir.join("calls/0007-result.json")).unwrap(),
            result
        );
    }
    #[test]
    fn compaction_keeps_native_follow_up_execution_provenance() {
        let source = FileRef {
            file: "cases/train/outcome.json".into(),
            sha256: digest("native outcome"),
            bytes: 20,
        };
        for status in [
            "executed",
            "executed after matching elicitation",
            "executed after one eligible authored proposal",
        ] {
            let outcome = json!({"follow_up":{"status":status,"words":"It is my cat.","result":{"Ok":null},
                "grade":{"fresh_assistant_reply":true,"after_state_grade":{"actual":{"reply":"What kind of cat?"}}}}});
            let kept = compact_follow_up(&outcome, "train", &source);
            assert!(kept.get("grade").is_none());
            assert_eq!(kept["execution_provenance"]["user_turn_submitted"], true);
            assert_eq!(
                kept["execution_provenance"]["assistant_reply_observed"],
                true
            );
            assert_eq!(
                kept["execution_provenance"]["source"]["sha256"],
                source.sha256
            );
            assert_eq!(
                kept["execution_provenance"]["source"]["json_pointer"],
                "/follow_up"
            );
        }
        let withheld = json!({"follow_up":{"status":"withheld because the intended fact was not correctly elicited","words":"It is my cat."}});
        let kept = compact_follow_up(&withheld, "train", &source);
        assert_eq!(kept["execution_provenance"]["user_turn_submitted"], false);
        assert_eq!(
            kept["execution_provenance"]["assistant_reply_observed"],
            false
        );
        let interrupted = json!({"follow_up":{"status":"executed after matching elicitation","words":"It is my cat.","result":{"Err":"cancelled"},
            "grade":{"fresh_assistant_reply":false,"after_state_grade":{"actual":{"reply":"Stale first-turn reply"}}}}});
        let kept = compact_follow_up(&interrupted, "train", &source);
        assert_eq!(kept["execution_provenance"]["user_turn_submitted"], true);
        assert_eq!(
            kept["execution_provenance"]["assistant_reply_observed"],
            false
        );
    }
    #[test]
    fn split_is_stable_one_sufficient_mode_per_method_and_missing_train() {
        let dir = tempfile::tempdir().unwrap();
        let rows:Vec<_> = ["relationship","lost_object","trust"].iter().flat_map(|method|
            ["explicit","implicit","missing"].iter().map(move |mode| json!({"id":format!("{method}-{mode}"),"method":method,"mode":mode,"words":"secret"}))).collect();
        store::atomic_json(&dir.path().join("core.json"), &rows, true).unwrap();
        let a = split(dir.path()).unwrap();
        let b = split(dir.path()).unwrap();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        for method in ["relationship", "lost_object", "trust"] {
            let rows: Vec<_> = a.cases.iter().filter(|c| c.method == method).collect();
            assert_eq!(
                rows.iter()
                    .filter(|c| c.partition == Partition::ReservedValidation)
                    .count(),
                1
            );
            assert!(
                rows.iter().find(|c| c.mode == "missing").unwrap().partition == Partition::Training
            );
        }
        assert!(!serde_json::to_string(&a).unwrap().contains("secret"));
    }
    #[test]
    fn repair_scope_uses_original_accepted_input_not_rejected_worksheet() {
        let input = json!({"original_input":{"original_input":{"recognition_phase":"classify_question","consultation":{"frame":{"observation":{"value":{"method":"relationship"}}}}}},"previous_worksheet":{"recognition_phase":"complete_selected_program"}});
        assert_eq!(
            original_input(&input)["recognition_phase"],
            "classify_question"
        );
        assert_eq!(
            method(original_input(&input)).as_deref(),
            Some("relationship")
        );
    }
}
