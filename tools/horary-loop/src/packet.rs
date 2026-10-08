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
    let state = state.get("session").unwrap_or(state);
    json!({"messages":state["messages"],"consultation":state.pointer("/method/consultation"),
        "candidateMomentMs":state["candidateMomentMs"],"methodResult":state.pointer("/method/result"),
        "pending":state.pointer("/method/flow/pending"),
        "chart":{"moment":state.pointer("/chart/timestamp_ms"),"place":state["place"],
            "created":!state["chart"].is_null()},"chartAfterMessage":state["chartAfterMessage"]})
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
            "Duplicate consultation histories/changes, chart geometry and repeated ReadyReading payloads are omitted here; accepted sourced facts and dialogue remain visible.".into()],
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
            let result = values.get(&result_file);
            let raw = result.and_then(|r| r.pointer("/result/Ok/content"));
            let output_sha = raw.map(|r| digest(r.to_string()));
            if let (Some(raw), Some(sha)) = (raw, &output_sha) {
                packet
                    .outputs
                    .entry(sha.clone())
                    .or_insert_with(|| raw.clone());
            }
            calls.push(json!({"sequence":request["sequence"],"stage":stage,"recognition_phase":phase,
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
                "decoder":request["decoder"],"request_file":file,"result_file":result_file}));
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
        packet.cases.push(CasePacket { id:id.clone(),method:spec.method.clone(),mode:spec.mode.clone(),
            partition:spec.partition,fingerprint,files:refs, summary:json!({
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
