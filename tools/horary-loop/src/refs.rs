//! Short references economize model tokens; the host restores exact citations.
#![forbid(unsafe_code)]
use crate::{packet::Packet, store::Result};
use horary_prompt_program::Evidence;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub citations: BTreeMap<String, Evidence>,
}
impl Index {
    pub fn verify_prompt(&self, prompt: &str) -> Result<()> {
        let submitted = prompt
            .split_once("COMPACT PACKET:\n")
            .or_else(|| prompt.split_once("COMPACT TRAINING PACKET:\n"));
        let Some((_, source)) = submitted else {
            return if self.citations.is_empty() {
                Ok(())
            } else {
                Err("Evidence map has no original submitted receipt table".into())
            };
        };
        let context: Value = serde_json::from_str(source)
            .map_err(|e| format!("Read original submitted receipt table: {e}"))?;
        if context["receipt_table"] != self.table() {
            return Err("Evidence map differs from the original submitted receipt table".into());
        }
        Ok(())
    }
    fn add(&mut self, evidence: Evidence) -> String {
        if let Some((id, _)) = self.citations.iter().find(|(_, old)| *old == &evidence) {
            return id.clone();
        }
        let id = format!("r{}", self.citations.len() + 1);
        self.citations.insert(id.clone(), evidence);
        id
    }
    fn citation(
        &mut self,
        packet: &Packet,
        case_id: &str,
        file: &str,
        pointer: &str,
    ) -> Result<String> {
        let case = packet
            .cases
            .iter()
            .find(|c| c.id == case_id)
            .ok_or("Unknown citation case")?;
        let reference = case
            .files
            .iter()
            .find(|f| f.file == file)
            .ok_or("Citation file absent")?;
        Ok(self.add(Evidence {
            case_id: case_id.into(),
            file: file.into(),
            json_pointer: pointer.into(),
            sha256: reference.sha256.clone(),
        }))
    }
    pub fn expand(&self, value: &mut Value) -> Result<()> {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.expand(item)?;
                }
            }
            Value::Object(fields) => {
                if let Some(refs) = fields.remove("evidence_refs") {
                    if fields.contains_key("evidence") {
                        return Err("Both short and full evidence provided".into());
                    }
                    let refs = refs.as_array().ok_or("Evidence refs must be an array")?;
                    if refs.len() > 2 {
                        return Err("At most two evidence references per claim".into());
                    }
                    let mut expanded = Vec::new();
                    for reference in refs {
                        let key = reference
                            .as_str()
                            .ok_or("Evidence reference must be a string")?;
                        expanded.push(
                            serde_json::to_value(
                                self.citations
                                    .get(key)
                                    .ok_or_else(|| format!("Unknown evidence reference {key}"))?,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                    }
                    fields.insert("evidence".into(), Value::Array(expanded));
                }
                for field in fields.values_mut() {
                    self.expand(field)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn shorten(&mut self, value: &mut Value) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.shorten(item);
                }
            }
            Value::Object(fields) => {
                if let Some(evidence) = fields.remove("evidence") {
                    let refs = evidence
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| serde_json::from_value::<Evidence>(v.clone()).ok())
                        .take(2)
                        .map(|e| Value::String(self.add(e)))
                        .collect();
                    fields.insert("evidence_refs".into(), Value::Array(refs));
                }
                for field in fields.values_mut() {
                    self.shorten(field);
                }
            }
            _ => {}
        }
    }
    fn table(&self) -> Value {
        let mut files: BTreeMap<(String, String, String), String> = BTreeMap::new();
        let mut file_table = BTreeMap::new();
        let mut receipts = BTreeMap::new();
        for (id, evidence) in &self.citations {
            let key = (
                evidence.case_id.clone(),
                evidence.file.clone(),
                evidence.sha256.clone(),
            );
            let next = format!("f{}", files.len() + 1);
            let file_id = files.entry(key).or_insert(next).clone();
            file_table.entry(file_id.clone()).or_insert_with(||json!({"case_id":evidence.case_id,"file":evidence.file,"sha256":evidence.sha256}));
            receipts.insert(
                id.clone(),
                json!({"file":file_id,"pointer":evidence.json_pointer}),
            );
        }
        json!({"files":file_table,"receipts":receipts})
    }
}

fn summarize_contract(schema: &Value) -> Value {
    match schema {
        Value::Object(fields) => {
            let mut kept = serde_json::Map::new();
            for (key, value) in fields {
                if key == "enum" && value.as_array().is_some_and(|v| v.len() > 20) {
                    kept.insert("enum_count".into(), json!(value.as_array().map(Vec::len)));
                } else {
                    kept.insert(key.clone(), summarize_contract(value));
                }
            }
            Value::Object(kept)
        }
        Value::Array(items) => Value::Array(items.iter().map(summarize_contract).collect()),
        _ => schema.clone(),
    }
}
fn prompt_scopes(reviews: &[Value]) -> BTreeSet<(String, Option<String>, Option<String>)> {
    reviews
        .iter()
        .flat_map(|review| review["findings"].as_array().into_iter().flatten())
        .filter(|finding| finding["repair_owner"] == "prompt")
        .filter_map(|finding| {
            Some((
                finding["stage"].as_str()?.into(),
                finding["recognition_phase"].as_str().map(str::to_owned),
                finding["method"].as_str().map(str::to_owned),
            ))
        })
        .collect()
}

/// Keep mutable regions separate: joining across a withheld quotation would
/// give the writer an old_text that never existed in the actual system guide.
fn guide_document(
    text: &str,
    chunks: &mut Vec<String>,
    chunk_ids: &mut BTreeMap<String, usize>,
) -> Result<Value> {
    let parts = horary_prompt_program::guide_parts(text)?;
    let teaching_regions = parts
        .teaching
        .iter()
        .map(|region| {
            let ids = region
                .split_inclusive("\n\n")
                .map(|part| {
                    if let Some(id) = chunk_ids.get(part) {
                        *id
                    } else {
                        let id = chunks.len();
                        chunks.push(part.to_owned());
                        chunk_ids.insert(part.to_owned(), id);
                        id
                    }
                })
                .collect::<Vec<_>>();
            json!({"sha256":horary_prompt_program::digest(region),"bytes":region.len(),"chunks":ids})
        })
        .collect::<Vec<_>>();
    let quoted_source = parts
        .quoted_source
        .iter()
        .map(|block| json!({"sha256":horary_prompt_program::digest(block),"bytes":block.len()}))
        .collect::<Vec<_>>();
    Ok(json!({"teaching_regions":teaching_regions,"quoted_source":quoted_source}))
}

/// All full primary files are preserved in Packet; this is the bounded model view.
pub fn context(packet: &Packet, reviews: Option<&[Value]>) -> Result<(Value, Index)> {
    let mut index = Index::default();
    let mut cases = Vec::new();
    let mut calls = Vec::new();
    let mut outputs = BTreeMap::new();
    let mut output_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    let mut input_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut consultations = BTreeMap::new();
    let scopes = reviews.map(prompt_scopes);
    let edit_stage = reviews.and_then(|reviews| {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for finding in reviews
            .iter()
            .flat_map(|review| review["findings"].as_array().into_iter().flatten())
        {
            if finding["repair_owner"] == "prompt" {
                if let Some(stage) = finding["stage"].as_str() {
                    *counts.entry(stage.into()).or_default() += 1;
                }
            }
        }
        counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(stage, _)| stage)
    });
    let mut selected_guides = Vec::new();
    let mut guide_documents = BTreeMap::new();
    let mut chunks = Vec::new();
    let mut chunk_ids = BTreeMap::new();
    let mut schema_hashes = BTreeSet::new();
    for guide in &packet.guides {
        if scopes.as_ref().is_some_and(|scopes| {
            !scopes.iter().any(|(stage, phase, method)| {
                stage == &guide.stage
                    && phase
                        .as_ref()
                        .is_none_or(|p| guide.recognition_phase.as_ref() == Some(p))
                    && method
                        .as_ref()
                        .is_none_or(|m| guide.method.as_ref() == Some(m))
            })
        }) {
            continue;
        }
        let full_text = packet.guide_text(guide)?;
        let regions = horary_prompt_program::guide_parts(&full_text)?;
        if edit_stage.as_deref() == Some(guide.stage.as_str())
            && !guide_documents.contains_key(&guide.sha256)
        {
            guide_documents.insert(
                guide.sha256.clone(),
                guide_document(&full_text, &mut chunks, &mut chunk_ids)?,
            );
        }
        selected_guides.push(json!({"stage":guide.stage,"recognition_phase":guide.recognition_phase,"method":guide.method,"sha256":guide.sha256,
            "teaching_regions":regions.teaching.iter().map(|region|json!({"sha256":horary_prompt_program::digest(region),"bytes":region.len()})).collect::<Vec<_>>(),
            "quoted_source":regions.quoted_source.iter().map(|block|json!({"sha256":horary_prompt_program::digest(block),"bytes":block.len()})).collect::<Vec<_>>() }));
    }
    for case in &packet.cases {
        let root = format!("cases/{}", case.id);
        let outcome_file = format!("{root}/outcome.json");
        let fixture_file = format!("{root}/fixture.json");
        let first_file = format!("{root}/first-turn.json");
        let final_file = format!("{root}/final.json");
        let grade_ref = index.citation(packet, &case.id, &outcome_file, "/grade")?;
        let reply_ref = index.citation(packet, &case.id, &outcome_file, "/grade/actual/reply")?;
        let expected_ref = index.citation(packet, &case.id, &fixture_file, "/expected")?;
        let words_ref = index.citation(packet, &case.id, &fixture_file, "/words")?;
        let first_ref = index.citation(packet, &case.id, &first_file, "/session/messages")?;
        let final_ref = index.citation(packet, &case.id, &final_file, "/session/messages")?;
        let mut summary = case.summary.clone();
        // Exact first/final wrappers are projected in the compact current format.
        for state in ["first_state", "final_state"] {
            let consultation = &summary[state]["consultation"]["consultation_ref"];
            if let Some(sha) = consultation.as_str() {
                if let Some(value) = packet.consultations.get(sha) {
                    consultations.insert(sha.to_owned(), value.clone());
                }
            }
        }
        let after = &summary["follow_up"];
        let submitted = after
            .pointer("/execution_provenance/user_turn_submitted")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                matches!(
                    after["status"].as_str(),
                    Some(
                        "executed"
                            | "executed after matching elicitation"
                            | "executed after one eligible authored proposal"
                    )
                ) && after["words"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty())
                    && after["result"].is_object()
            });
        let after_ref = if submitted && !after["after_state_grade"].is_null() {
            Some(index.citation(packet, &case.id, &outcome_file, "/follow_up/grade")?)
        } else {
            None
        };
        summary["evidence_refs"] = json!({"native_grade":grade_ref,"reply":reply_ref,"expected":expected_ref,
            "words":words_ref,"first_dialogue":first_ref,"final_dialogue":final_ref,"after_grade":after_ref});
        // ReadyReading is revalidated by the native grader, not by repeating bindings in the model prompt.
        for state in ["first_state", "final_state"] {
            if let Some(object) = summary[state].as_object_mut() {
                let kind = object
                    .get("methodResult")
                    .map(|r| r["result"].clone())
                    .unwrap_or(Value::Null);
                object.insert("methodResult".into(), json!({"result":kind}));
            }
        }
        cases.push(json!({"id":case.id,"method":case.method,"mode":case.mode,"summary":summary}));
        for call in &case.calls {
            let input_sha = call["input_sha256"].as_str().ok_or("Call input missing")?;
            let input = packet
                .inputs
                .get(input_sha)
                .ok_or("Referenced input absent")?;
            let input_id = if let Some(id) = input_ids.get(input_sha) {
                id.clone()
            } else {
                let id = format!("i{}", input_ids.len() + 1);
                input_ids.insert(input_sha.to_owned(), id.clone());
                let mut view = input.clone();
                if let Some(object) = view.as_object_mut() {
                    // The quoted guide already teaches these repeated invariant paragraphs.
                    for field in [
                        "instruction",
                        "context_authority",
                        "legacy_user_fact_sources",
                        "available_revisions",
                        "authority",
                        "anchor_policy",
                        "state_reminder",
                    ] {
                        object.remove(field);
                    }
                    if let Some(consultation) = object.get("consultation") {
                        if let Some(sha) = consultation["consultation_ref"].as_str() {
                            // First/final facts are canonical in the view; intermediate states remain exact in source receipts.
                            if !consultations.contains_key(sha) {
                                object.remove("consultation");
                            }
                        }
                    }
                }
                inputs.insert(id.clone(), view);
                id
            };
            let output_id = call["output_sha256"].as_str().map(|sha| {
                if let Some(id) = output_ids.get(sha) {
                    return id.clone();
                }
                let id = format!("o{}", output_ids.len() + 1);
                output_ids.insert(sha.to_owned(), id.clone());
                outputs.insert(
                    id.clone(),
                    packet.outputs.get(sha).cloned().unwrap_or(Value::Null),
                );
                id
            });
            let request = call["request_file"]
                .as_str()
                .ok_or("Request source missing")?;
            let result = call["result_file"]
                .as_str()
                .ok_or("Result source missing")?;
            let input_ref = index.citation(packet, &case.id, request, "/input")?;
            let guide_ref = index.citation(packet, &case.id, request, "/prompt/0/content")?;
            let result_ref = if output_id.is_some() {
                index.citation(packet, &case.id, result, "/result/Ok/content")?
            } else {
                index.citation(packet, &case.id, result, "/result")?
            };
            schema_hashes.insert(
                call["schema_sha256"]
                    .as_str()
                    .ok_or("Missing schema digest")?
                    .to_owned(),
            );
            calls.push(json!({"case_id":case.id,"sequence":call["sequence"],"stage":call["stage"],
                "phase":call["recognition_phase"],"method":call["method"],"guide_sha":call["guide_sha256"],
                "schema_sha":call["schema_sha256"],"input":input_id,"output":output_id,
                "native_error":call["native_validation_error"],"backend_error":call["result_error"],
                "input_ref":input_ref,"guide_ref":guide_ref,"result_ref":result_ref}));
        }
    }
    let schemas: BTreeMap<_, _> = schema_hashes
        .into_iter()
        .filter_map(|sha| {
            packet
                .schemas
                .get(&sha)
                .map(|v| (sha, summarize_contract(v)))
        })
        .collect();
    let mut reviews = reviews.map(|r| r.to_vec()).unwrap_or_default();
    for review in &mut reviews {
        index.shorten(review);
        if let Some(findings) = review["findings"].as_array_mut() {
            for finding in findings {
                if let Some(summary) = finding["summary"].as_str() {
                    finding["summary"] = json!(summary.chars().take(240).collect::<String>());
                }
            }
        }
    }
    let view = json!({"format":"compact-ref-v1","phase":packet.phase,"manifest_sha256":packet.manifest_sha256,
        "candidate_sha256":packet.candidate_sha256,"qualification":packet.qualification,
        "omissions":["Full originals are retained by SHA; this bounded view omits duplicate consultation changes/histories, chart geometry, repeated ReadyReading bindings and invariant input instructions.",
            "Inputs are original accepted task views plus native repair errors by sequence; intermediate accepted state is omitted unless final/first. Large schema enums show enum_count; exact schema stays immutable.",
            "guides are typed selectors with full guide SHA. Each document contains ordered teaching_regions; concatenate teaching_chunks within one region only. Immutable book_extracts blocks separate regions, are omitted and retained by hash/length in full originals. This projection cannot certify quoted source interpretation. Writer receives only scopes implicated by training findings.",
            "Output evidence_refs resolve through receipt_table. The host expands them and verifies every file hash and JSON pointer. Never invent a reference."],
        "editable_stage":edit_stage,"cases":cases,"calls":calls,"outputs":outputs,"inputs":inputs,"accepted_consultations":consultations,
        "guide_scopes":selected_guides,"guide_documents":guide_documents,"teaching_chunks":chunks,
        "schema_summaries":schemas,"training_reviews":reviews,"receipt_table":index.table()});
    Ok((view, index))
}

pub fn schema(mut schema: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                if let Some(properties) =
                    fields.get_mut("properties").and_then(Value::as_object_mut)
                {
                    if properties.remove("evidence").is_some() {
                        properties.insert("evidence_refs".into(),json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":2}));
                    }
                    for name in ["reason", "summary"] {
                        if let Some(property) = properties.get_mut(name) {
                            property["maxLength"] = json!(if name == "reason" { 160 } else { 240 });
                        }
                    }
                }
                if let Some(required) = fields.get_mut("required").and_then(Value::as_array_mut) {
                    for name in required {
                        if *name == "evidence" {
                            *name = json!("evidence_refs");
                        }
                    }
                }
                for child in fields.values_mut() {
                    visit(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    visit(child);
                }
            }
            _ => {}
        }
    }
    visit(&mut schema);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_can_see_method_appendix_without_reconstructing_quoted_source() {
        let text = "General teaching.\n<book_extracts>Source one</book_extracts>\nMethod teaching.\n<book_extracts>Source two</book_extracts>\nFinal teaching.";
        let mut chunks = Vec::new();
        let document = guide_document(text, &mut chunks, &mut BTreeMap::new()).unwrap();
        let regions = document["teaching_regions"].as_array().unwrap();
        assert_eq!(regions.len(), 3);
        let reconstructed = regions
            .iter()
            .map(|region| {
                region["chunks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| chunks[id.as_u64().unwrap() as usize].as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reconstructed,
            vec![
                "General teaching.\n",
                "\nMethod teaching.\n",
                "\nFinal teaching."
            ]
        );
        for (region, actual) in regions.iter().zip(&reconstructed) {
            assert_eq!(region["sha256"], horary_prompt_program::digest(actual));
            assert_eq!(region["bytes"], actual.len());
        }
        assert_eq!(document["quoted_source"].as_array().unwrap().len(), 2);
        assert!(!document.to_string().contains("Source one"));
        assert!(chunks.iter().all(|chunk| !chunk.contains("book_extracts")));
        assert!(
            guide_document("<book_extracts>Unclosed", &mut chunks, &mut BTreeMap::new()).is_err()
        );
    }
    #[test]
    fn short_claims_expand_without_changing_hashes_or_pointers() {
        let mut index = Index::default();
        let evidence = Evidence {
            case_id: "a".into(),
            file: "cases/a/outcome.json".into(),
            json_pointer: "/grade/semantic_pass".into(),
            sha256: "a".repeat(64),
        };
        let id = index.add(evidence.clone());
        assert_eq!(index.add(evidence.clone()), id);
        let prompt = format!(
            "Self-contained.\nCOMPACT PACKET:\n{}",
            json!({"receipt_table":index.table()})
        );
        index.verify_prompt(&prompt).unwrap();
        let mut changed = index.clone();
        changed.citations.get_mut(&id).unwrap().json_pointer = "/unrelated".into();
        assert!(changed.verify_prompt(&prompt).is_err());
        assert!(index.verify_prompt("No submitted receipt table").is_err());
        let mut result = json!({"dimension":{"evidence_refs":[id],"reason":"native result"}});
        index.expand(&mut result).unwrap();
        assert_eq!(
            result["dimension"]["evidence"][0],
            serde_json::to_value(evidence).unwrap()
        );
        assert!(index
            .expand(&mut json!({"evidence_refs":["unknown"]}))
            .is_err());
    }
    #[test]
    fn compact_output_contract_does_not_repeat_long_citations() {
        let compact = schema(crate::types::judge_schema());
        let text = compact.to_string();
        assert!(text.contains("evidence_refs"));
        assert!(!text.contains("json_pointer"));
        assert!(!text.contains("sha256"));
    }
}
