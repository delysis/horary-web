//! A bound update is a different neural function from first-turn extraction.
//! These witnesses never substitute a checkpoint or qualify a whole journey.
use crate::{load, read, ControlMode, Example, Function, Objective, Plan, Result};
use horary_prompt_program::{digest, original_input, signature};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf};

pub const VERSION: &str = "horary-supplying-extractor-review-2026-10-11.1";
pub(crate) const KEY: &str = "supplying_extractor";

fn recognition_snapshot(consultation: &Value) -> Result<Value> {
    // Native Consultation::recognition_snapshot clones the accepted table and
    // clears only changes (reading_contracts.rs). Preserve every other field.
    let mut snapshot = consultation.clone();
    let object = snapshot.as_object_mut().ok_or("No actual consultation")?;
    if !object.get("changes").is_some_and(Value::is_array) {
        return Err("Actual consultation has no native changes array".into());
    }
    object.insert("changes".into(), json!([]));
    Ok(snapshot)
}

fn expected_need(fixture: &Value, consultation: &Value, actual: &Value) -> bool {
    let person_id = |id: &str| {
        consultation["people"].as_object().and_then(|people| {
            people.get(id).and_then(|p| p["id"].as_str()).or_else(|| {
                let mut matches = people.values().filter(|p| {
                    p["label"]
                        .as_str()
                        .is_some_and(|label| label.eq_ignore_ascii_case(id))
                });
                let first = matches.next()?;
                matches
                    .next()
                    .is_none()
                    .then(|| first["id"].as_str())
                    .flatten()
            })
        })
    };
    std::iter::once(&fixture["expected"]["needs"])
        .chain(
            fixture["expected"]["needs_alternatives"]
                .as_array()
                .into_iter()
                .flatten(),
        )
        .filter_map(Value::as_array)
        .flatten()
        .any(|candidate| {
            candidate == actual
                || candidate["kind"] == "person_relationship"
                    && actual["kind"] == "person_relationship"
                    && candidate["id"]
                        .as_str()
                        .zip(actual["id"].as_str())
                        .is_some_and(|(expected, actual)| {
                            person_id(expected)
                                .zip(person_id(actual))
                                .is_some_and(|(e, a)| e == a)
                        })
        })
}

pub(crate) fn source_binding(plan: &Plan, evaluation: &Value) -> Result<Value> {
    let o = &evaluation["outcome"];
    let observation = &o[KEY];
    if !plan.supplying_extractor_observation
        || plan.function != Function::InputJourney
        || plan.objective != Objective::ExtractorReliability
        || plan.control_mode != ControlMode::FreshHosted
        || plan.initial_extractor_observation
        || o["scope"] != "supplying_extractor_function_only"
        || o["full_reading"] != false
        || o["execution_status"] != "supplying_extractor_observed"
        || observation["version"] != 1
        || observation["recognition_phase"] != "update_selected_program"
        || observation["target_method"].as_str() != plan.target_method.as_deref()
        || !matches!(
            observation["state"].as_str(),
            Some("accepted" | "native_rejected")
        )
        || observation["bound_inquiry_observed"] != true
        || observation["all_provider_responses_settled"] != true
        || observation["request_result_pairs_complete"] != true
        || o["target_function_invoked"] != true
        || o["physical_generation_attempts_complete"] != true
        || !o["provider_stop"].is_null()
        || !o["infrastructure_error"].is_null()
        || o["deadline_cancelled"] != false
        || o["group_cancelled"] != false
    {
        return Err("Supplying extractor is unbound, uninvoked or uncertain; no paid review or fabricated fitness".into());
    }
    let dir = PathBuf::from(
        evaluation["native_evidence_directory"]
            .as_str()
            .ok_or("No supplying evidence")?,
    );
    let count = crate::reading::verify_settled_calls(&dir)?;
    let first = load(&dir.join("first-turn.json"))?;
    let final_state = load(&dir.join("final.json"))?;
    let initial = load(&dir.join("initial.json"))?;
    let fixture = load(&dir.join("fixture.json"))?;
    let before = &first["session"];
    let after = &final_state["session"];
    let actual_clipboard = recognition_snapshot(&before["method"]["consultation"])?;
    let prior = before["messages"].as_array().ok_or("No inquiry messages")?;
    let messages = after["messages"].as_array().ok_or("No supplied messages")?;
    let need = &before["method"]["consultation"]["requested"];
    let words = fixture["follow_up"]
        .as_str()
        .ok_or("No authored supplying turn")?;
    if first["result"].get("Ok").is_none()
        || need.is_null()
        || !expected_need(&fixture, &before["method"]["consultation"], need)
        || !before["audit"].as_array().is_some_and(|events| {
            events.iter().any(|e| {
                e["event"] == "conversation_reminder_selected"
                    && e["key"] == *need
                    && e["ask"].as_str().is_some_and(|ask| !ask.is_empty())
            })
        })
        || prior.first()
            != initial["session"]["messages"]
                .as_array()
                .and_then(|m| m.first())
        || prior
            .first()
            .is_none_or(|m| m["role"] != "user" || m["text"] != fixture["words"])
        || prior.iter().filter(|m| m["role"] == "user").count() != 1
        || prior.last().is_none_or(|m| {
            m["role"] != "assistant" || m["text"].as_str().is_none_or(|s| s.trim().is_empty())
        })
        || messages.len() != prior.len() + 1
        || &messages[..prior.len()] != prior.as_slice()
        || messages
            .last()
            .is_none_or(|m| m["role"] != "user" || m["text"] != words)
        || !before["chart"].is_null()
        || !after["chart"].is_null()
        || observation["last_sequence"].as_u64() != Some(count)
    {
        return Err("Supplying observation did not arise from its original question and actual bound inquiry".into());
    }
    let mut hashes = BTreeMap::new();
    for name in [
        "initial.json",
        "fixture.json",
        "first-turn.json",
        "final.json",
        "outcome.json",
        "supplying-extractor-observation.json",
    ] {
        hashes.insert(name.to_owned(), digest(read(&dir.join(name))?));
    }
    let mut requests = fs::read_dir(dir.join("calls"))
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    requests.sort();
    let mut target = Vec::new();
    let mut started = false;
    let mut sequences = Vec::new();
    for file in requests {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid call path")?;
        if !name.ends_with("-request.json") && !name.ends_with("-result.json") {
            continue;
        }
        hashes.insert(format!("calls/{name}"), digest(read(&file)?));
        if !name.ends_with("-request.json") {
            continue;
        }
        let request = load(&file)?;
        let sequence = request["sequence"]
            .as_u64()
            .ok_or("No supplying call sequence")?;
        if name != format!("{sequence:04}-request.json") || !request["tasks"].is_null() {
            return Err("Supplying component includes an unsupported call".into());
        }
        sequences.push(sequence);
        let input = original_input(&request["input"]);
        let scope = signature("intake", input);
        if request["stage"] == "intake"
            && scope.recognition_phase == Some("update_selected_program")
            && scope.method == plan.target_method.as_deref()
        {
            started = true;
            if input["latest_words"] != words
                || input["canonical_question"] != before["question"]
                || input["pending_requirement"] != *need
                || input["last_reader_question"] != prior.last().ok_or("No actual ask")?["text"]
                || input["consultation"] != actual_clipboard
            {
                return Err(
                    "Target supplying request differs from its actual bound source words/inquiry/clipboard"
                        .into(),
                );
            }
            let result = load(
                &dir.join("calls")
                    .join(name.replace("-request.json", "-result.json")),
            )?;
            target.push((request, result));
        } else if started
            || !matches!(
                request["stage"].as_str(),
                Some("intake" | "place" | "moment" | "conversation")
            )
        {
            return Err("A later fixed actor entered the supplying measurement".into());
        }
    }
    if sequences != (1..=count).collect::<Vec<_>>()
        || target.is_empty()
        || observation["supplying_target_attempts"].as_u64() != Some(target.len() as u64)
        || o["target_signature_calls"] != observation["supplying_target_attempts"]
    {
        return Err("Supplying target attempts differ from actual settled calls".into());
    }
    let focused = evaluation["actual_function_calls"]
        .as_array()
        .ok_or("No supplying call projection")?;
    if focused.len() != target.len() {
        return Err("Supplying projection has the wrong attempt count".into());
    }
    for (projected, (request, result)) in focused.iter().zip(&target) {
        let sequence = request["sequence"]
            .as_u64()
            .ok_or("No projected sequence")?;
        let rn = format!("calls/{sequence:04}-request.json");
        let sn = format!("calls/{sequence:04}-result.json");
        if projected["sequence"] != request["sequence"]
            || projected["source_request_file"] != rn
            || projected["source_result_file"] != sn
            || projected["source_request_sha256"].as_str() != hashes.get(&rn).map(String::as_str)
            || projected["source_result_sha256"].as_str() != hashes.get(&sn).map(String::as_str)
            || [
                "input",
                "schema",
                "guide_sha256",
                "prompt_program",
                "executor_guide_identity",
            ]
            .iter()
            .any(|k| projected[*k] != request[*k])
            || projected["result"] != result["result"]
        {
            return Err("Supplying feedback differs from its exact request/result receipts".into());
        }
    }
    let records = after["method"]["records"]
        .as_array()
        .ok_or("No supplying native records")?;
    let index = if observation["state"] == "accepted" {
        &observation["boundary"]["record_index"]
    } else {
        &o["semantic_abort_witness"]["record_index"]
    };
    let index = index.as_u64().ok_or("No supplying native record index")? as usize;
    let record = records.get(index).ok_or("Absent supplying record")?;
    let last = target.last().ok_or("No actual target result")?;
    if record["stage"] != "intake"
        || record["raw"] != record["generation"]["content"]
        || record["input"] != last.0["input"]
        || record["generation"] != last.1["result"]["Ok"]
        || record["guideSha256"]
            != horary_prompt_program::captured_record_guide_sha256(&last.0, None)?
        || record["schemaSha256"] != last.0["schema_sha256"]
        || last.0["schema_sha256"]
            .as_str()
            .is_none_or(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
        || record["inputSha256"]
            .as_str()
            .is_none_or(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(
            "Supplying native record is not its exact generation/input/schema/guide".into(),
        );
    }
    if observation["state"] == "accepted" {
        let b = &observation["boundary"];
        if index + 1 != records.len()
            || final_state["follow_up"]["result"].get("Ok").is_none()
            || !record["validationError"].is_null()
            || b["event"] != "supplying_extractor_boundary"
            || b["version"] != 1
            || b["recognition_phase"] != "update_selected_program"
            || b["target_method"] != observation["target_method"]
            || b["last_sequence"] != observation["last_sequence"]
            || b["record_input_sha256"] != record["inputSha256"]
            || b["native_job"]["inputSha256"]
                != crate::selected_stage::native_original_hash(records, record)?
            || b["revision"] != after["revision"]
            || b["after_message"].as_u64() != Some(messages.len() as u64)
            || b["before_consultation"] != before["method"]["consultation"]
            || b["bound_requirement"] != *need
            || b["accepted_consultation"] != after["method"]["consultation"]
            || observation["accepted_consultation"] != b["accepted_consultation"]
            || observation["native_grade"]["actual"]["consultation"]
                != observation["accepted_consultation"]
            || b["later_chart_executed"] != false
            || b["later_conversation_executed"] != false
            || !after["audit"]
                .as_array()
                .is_some_and(|a| a.iter().any(|e| e == b))
            || !after["method"]["flow"]["jobs"].as_array().is_some_and(|a| {
                a.iter().any(|j| {
                    j == &b["native_job"]
                        && j["revision"] == record["revision"]
                        && j["stage"] == "intake"
                        && j["phase"]["state"] == "complete"
                        && j["phase"]["record_index"].as_u64() == Some(index as u64)
                })
            })
        {
            return Err("Supplying acceptance lacks its actual post-apply native job/state".into());
        }
    } else {
        if !observation["accepted_consultation"].is_null()
            || !observation["native_grade"].is_null()
            || o["known_native_semantic_abort"] != true
            || o["semantic_abort_witness"]["snapshot"] != "final.json"
            || o["semantic_abort_witness"]["original_input"] != *original_input(&record["input"])
        {
            return Err(
                "Rejected supplying extractor invents accepted state or lacks its paused lineage"
                    .into(),
            );
        }
        crate::reading::verify_semantic_abort(evaluation, &dir)?;
        let original_sha = crate::selected_stage::native_original_hash(records, record)?;
        if o["semantic_abort_witness"]["input_sha256"] != original_sha
            || o["semantic_abort_witness"]["paused_job"]["inputSha256"] != original_sha
        {
            return Err("Rejected supplying work lacks its original native job hash".into());
        }
    }
    if load(&dir.join("supplying-extractor-observation.json"))? != *observation
        || load(&dir.join("outcome.json"))?["id"] != o["id"]
    {
        return Err("Supplying native observation drift".into());
    }
    Ok(
        json!({"version":1,"supplying_observation_sha256":digest(observation.to_string()),"sources":hashes}),
    )
}

pub(crate) fn packet(
    plan: &Plan,
    example: &Example,
    evaluation: &Value,
) -> Result<crate::review::Packet> {
    crate::initial_extractor::verify_observation(plan, evaluation)?;
    let dir = PathBuf::from(
        evaluation["native_evidence_directory"]
            .as_str()
            .ok_or("No native supplying evidence")?,
    );
    let o = &evaluation["outcome"];
    let mut p = crate::review::Packet::new();
    p.source(
        "fixture.json",
        read(&example.source_case_directory.join("fixture.json"))?,
        &[""],
    )?;
    if digest(&p.sources[0].bytes) != example.source_fixture_sha256 {
        return Err("Supplying authored fixture drift".into());
    }
    p.source(
        "book-source.json",
        serde_json::to_vec(
            &plan
                .review_book
                .as_ref()
                .ok_or("No pinned book")?
                .excerpts()?,
        )
        .map_err(|e| e.to_string())?,
        &[""],
    )?;
    p.source(
        "supplying-observation.json",
        serde_json::to_vec(&o[KEY]).map_err(|e| e.to_string())?,
        &[""],
    )?;
    p.source(
        "native-first-turn.json",
        read(&dir.join("first-turn.json"))?,
        &["/session/messages", "/session/method/consultation"],
    )?;
    let index = if o[KEY]["state"] == "accepted" {
        &o[KEY]["boundary"]["record_index"]
    } else {
        &o["semantic_abort_witness"]["record_index"]
    };
    let pointer = format!(
        "/session/method/records/{}",
        index.as_u64().ok_or("No supplying witness")?
    );
    p.source(
        "native-supplying-turn.json",
        read(&dir.join("final.json"))?,
        &[
            "/session/messages",
            "/session/method/consultation",
            "/session/method/flow/jobs",
            &pointer,
        ],
    )?;
    for call in evaluation["actual_function_calls"]
        .as_array()
        .ok_or("No actual supplying calls")?
    {
        for (field, pointers) in [
            (
                "source_request_file",
                vec!["/input", "/schema", "/prompt_program"],
            ),
            ("source_result_file", vec!["/result"]),
        ] {
            let name = call[field].as_str().ok_or("No supplying call path")?;
            p.source(name, read(&dir.join(name))?, &pointers)?;
        }
    }
    for e in p.refs.values_mut() {
        e.case_id = example.id.clone();
    }
    p.context["version"] = json!(VERSION);
    p.context["case_id"] = json!(example.id);
    p.context["receipt_table"] = json!(p.refs);
    p.context["source_binding"] = evaluation["supplying_extractor_binding"].clone();
    p.context["scope"]=json!("Only update_selected_program after the real bound inquiry. Compare actual pre-update consultation, supplied words, target attempts and post-apply state. Unchanged upstream defects, inquiry fluency, place/moment/chart, later guru and interpretation are outside component merit. No whole journey qualifies.");
    if p.context.to_string().len() > 400_000 {
        return Err("Supplying review exceeds 400KB; no truncation or submission".into());
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(dir: &std::path::Path, name: &str, v: &Value) {
        fs::write(dir.join(name), serde_json::to_vec_pretty(v).unwrap()).unwrap();
    }
    fn fixture() -> (tempfile::TempDir, Plan, Value) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("calls")).unwrap();
        let mut plan = crate::tests::plan();
        plan.function = Function::InputJourney;
        plan.objective = Objective::ExtractorReliability;
        plan.control_mode = ControlMode::FreshHosted;
        plan.supplying_extractor_observation = true;
        plan.target_method = Some("movable_deal".into());
        let need = json!({"kind":"owner"});
        let words = "These fish are mine";
        let before = json!({"requested":need,"frame":{"state":"resolved","observation":{"value":{"method":"movable_deal","facet":"profit"}}},"facts":{},"changes":["Actual prior change"]});
        let after = json!({"requested":need,"frame":before["frame"],"subject":{"owner":"querent"},"facts":{}});
        let messages = json!([{"role":"user","text":"Would selling these fish be profitable?"},{"role":"assistant","text":"Who owns the fish?"}]);
        let input = json!({"recognition_phase":"update_selected_program","consultation":recognition_snapshot(&before).unwrap(),"latest_words":words,
            "canonical_question":"Would selling these fish be profitable?","pending_requirement":need,"last_reader_question":"Who owns the fish?"});
        let generation = json!({"content":"{\"owner\":\"querent\"}"});
        let mut focused = Vec::new();
        for sequence in 1..=2 {
            let request = json!({"sequence":sequence,"stage":if sequence==1 {"conversation"}else{"intake"},"matter":"other",
                "input":if sequence==1 {json!({"reminders":[{"key":need}]})}else{input.clone()},"schema":{},
                "prompt":[{"role":"system","content":"Authored system guide"}],"guide_sha256":digest("Authored system guide"),"prompt_program":null,"schema_sha256":digest("{}")});
            let result = json!({"request":request,"result":{"Ok":generation},"provider_receipt":{"submitted":true,"provider":"google_gemini_api","response":{"http_status":200},"native_result":{"Ok":generation},"generation_attempts":[{"submitted":true,"http_status":200,"error":null}]}});
            let rn = format!("calls/{sequence:04}-request.json");
            let sn = format!("calls/{sequence:04}-result.json");
            put(dir.path(), &rn, &request);
            put(dir.path(), &sn, &result);
            if sequence == 2 {
                focused.push(json!({"sequence":sequence,"input":input,"schema":{},"guide_sha256":request["guide_sha256"],"prompt_program":null,"result":result["result"],
                "source_request_file":rn,"source_result_file":sn,"source_request_sha256":digest(read(&dir.path().join(rn)).unwrap()),"source_result_sha256":digest(read(&dir.path().join(sn)).unwrap())}));
            }
        }
        let record = json!({"stage":"intake","input":input,"inputSha256":digest(input.to_string()),"schemaSha256":digest("{}"),"guideSha256":digest("Authored system guide"),"revision":0,"generation":generation,"raw":generation["content"],"validationError":null});
        let job = json!({"stage":"intake","revision":0,"inputSha256":record["inputSha256"],"phase":{"state":"complete","record_index":0}});
        let boundary = json!({"event":"supplying_extractor_boundary","version":1,"recognition_phase":"update_selected_program","target_method":"movable_deal","after_message":3,"record_index":0,"last_sequence":2,"revision":0,"record_input_sha256":record["inputSha256"],"native_job":job,"before_consultation":before,"bound_requirement":need,"accepted_consultation":after,"later_chart_executed":false,"later_conversation_executed":false});
        let first = json!({"result":{"Ok":null},"session":{"question":"Would selling these fish be profitable?","messages":messages,"chart":null,"audit":[{"event":"conversation_reminder_selected","key":need,"ask":"need_0"}],"method":{"consultation":before}}});
        let mut supplied = messages.as_array().unwrap().clone();
        supplied.push(json!({"role":"user","text":words}));
        let final_state = json!({"follow_up":{"result":{"Ok":null}},"session":{"question":first["session"]["question"],"revision":0,"messages":supplied,"chart":null,"audit":[boundary],"method":{"consultation":after,"records":[record],"flow":{"jobs":[job]}}}});
        let o = json!({"id":"synthetic","scope":"supplying_extractor_function_only","execution_status":"supplying_extractor_observed","full_reading":false,"target_function_invoked":true,"physical_generation_attempts_complete":true,"deadline_cancelled":false,"group_cancelled":false,
            "supplying_extractor":{"version":1,"state":"accepted","target_method":"movable_deal","recognition_phase":"update_selected_program","boundary":boundary,"accepted_consultation":after,"native_grade":{"semantic_pass":true,"actual":{"consultation":after},"hurdles":{"extraction":{"status":"pass"}}},"supplying_target_attempts":1,"last_sequence":2,"all_provider_responses_settled":true,"request_result_pairs_complete":true,"bound_inquiry_observed":true},"target_signature_calls":1});
        put(
            dir.path(),
            "initial.json",
            &json!({"session":{"messages":[messages[0]]}}),
        );
        put(
            dir.path(),
            "fixture.json",
            &json!({"id":"synthetic","words":messages[0]["text"],"follow_up":words,"expected":{"needs":[need]}}),
        );
        put(dir.path(), "first-turn.json", &first);
        put(dir.path(), "final.json", &final_state);
        put(dir.path(), "outcome.json", &o);
        put(dir.path(), "supplying-extractor-observation.json", &o[KEY]);
        let evaluation = json!({"outcome":o,"native_evidence_directory":dir.path(),"actual_function_calls":focused});
        (dir, plan, evaluation)
    }
    #[test]
    fn supplying_source_binding_requires_real_inquiry_and_exact_native_acceptance() {
        let (dir, plan, evaluation) = fixture();
        source_binding(&plan, &evaluation).unwrap();
        let mut final_state = load(&dir.path().join("final.json")).unwrap();
        final_state["session"]["method"]["records"][0]["guideSha256"] = json!("stale");
        put(dir.path(), "final.json", &final_state);
        assert!(source_binding(&plan, &evaluation)
            .unwrap_err()
            .contains("guide"));
    }
    #[test]
    fn supplying_clipboard_is_exact_actual_snapshot_with_only_changes_cleared() {
        for mutation in ["stale", "drop", "extra"] {
            let (dir, plan, evaluation) = fixture();
            source_binding(&plan, &evaluation).unwrap();
            let mut request = load(&dir.path().join("calls/0002-request.json")).unwrap();
            let clipboard = &mut request["input"]["consultation"];
            assert_eq!(clipboard["changes"], json!([]));
            match mutation {
                "stale" => clipboard["frame"]["observation"]["value"]["facet"] = json!("event"),
                "drop" => {
                    clipboard.as_object_mut().unwrap().remove("facts");
                }
                _ => clipboard["fabricated"] = json!(true),
            }
            let mut result = load(&dir.path().join("calls/0002-result.json")).unwrap();
            result["request"] = request.clone();
            put(dir.path(), "calls/0002-request.json", &request);
            put(dir.path(), "calls/0002-result.json", &result);
            assert!(
                source_binding(&plan, &evaluation)
                    .unwrap_err()
                    .contains("clipboard"),
                "{mutation}"
            );
        }
        assert!(recognition_snapshot(&json!({"changes":null})).is_err());
    }
    #[test]
    fn supplying_uninvoked_uncertain_and_wrong_bound_need_have_no_review() {
        let (dir, plan, evaluation) = fixture();
        for (key, value) in [
            ("target_function_invoked", json!(false)),
            ("provider_stop", json!("unknown")),
            ("deadline_cancelled", json!(true)),
        ] {
            let mut e = evaluation.clone();
            e["outcome"][key] = value;
            assert!(source_binding(&plan, &e).is_err());
        }
        let mut first = load(&dir.path().join("first-turn.json")).unwrap();
        first["session"]["method"]["consultation"]["requested"] = json!({"kind":"frame"});
        put(dir.path(), "first-turn.json", &first);
        assert!(source_binding(&plan, &evaluation)
            .unwrap_err()
            .contains("bound inquiry"));
    }
    #[test]
    fn supplying_settled_rejection_binds_paused_original_lineage_without_accepted_facts() {
        let (dir, plan, mut e) = fixture();
        let mut final_state = load(&dir.path().join("final.json")).unwrap();
        let mut record = final_state["session"]["method"]["records"][0].clone();
        record["validationError"] = json!("Wrong supplied owner field");
        let stop = "Synthetic case call budget exhausted; no successful completion implied";
        let job = json!({"stage":"intake","revision":0,"inputSha256":record["inputSha256"],"phase":{"state":"paused","error":stop}});
        final_state["session"]["method"]["records"] = json!([record]);
        final_state["session"]["method"]["flow"]["jobs"] = json!([job]);
        final_state["session"]["audit"] = json!([]);
        final_state["follow_up"]["result"] = json!({"Err":stop});
        e["outcome"][KEY]["state"] = json!("native_rejected");
        for field in ["boundary", "accepted_consultation", "native_grade"] {
            e["outcome"][KEY][field] = Value::Null;
        }
        e["outcome"]["known_native_semantic_abort"] = json!(true);
        e["outcome"]["semantic_abort_witness"] = json!({"snapshot":"final.json","record_index":0,"stage":"intake","revision":0,"input_sha256":record["inputSha256"],"original_input":record["input"],"validation_error":record["validationError"],"paused_job":job,"stop":stop,"observed_call_groups":2});
        put(dir.path(), "final.json", &final_state);
        put(
            dir.path(),
            "supplying-extractor-observation.json",
            &e["outcome"][KEY],
        );
        source_binding(&plan, &e).unwrap();
        e["outcome"]["semantic_abort_witness"]["input_sha256"] = json!("0".repeat(64));
        assert!(source_binding(&plan, &e).is_err());
    }
}
