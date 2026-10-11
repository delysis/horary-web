//! An observation of one real production reading stage, not a partial reading.
//! This module is compiled only with the synthetic native test harness.
use super::*;

pub(super) fn disabled(value: &bool) -> bool {
    !*value
}

pub(super) fn measurement(
    target: (&str, Stage, Method),
    raw: &Value,
    snapshots: (&Value, &Value),
    calls: &[Value],
    complete: bool,
    run_error: Option<&str>,
) -> Result<Value, String> {
    let (id, stage, method) = target;
    let focused = super::reading::focused(calls, stage, method)?;
    let (groups, providers) = super::reading::counts(calls)?;
    let (first, final_state) = snapshots;
    let authentic = !focused.is_empty()
        && focused.iter().all(|call| {
            super::reading::authentic_permit(first, call)
                || super::reading::authentic_permit(final_state, call)
        });
    let error = run_error
        .or(first["result"]["Err"].as_str())
        .or(final_state["follow_up"]["result"]["Err"].as_str());
    let settled = complete
        && super::reading::provider_responses_settled(calls)
        && raw["deadline_cancelled"] != true
        && raw["group_cancelled"] != true
        && raw["provider_stop"].is_null()
        && raw["infrastructure_error"].is_null();
    let boundary = [("first-turn.json", first), ("final.json", final_state)]
        .into_iter()
        .find_map(|(name, snapshot)| {
            snapshot["session"]["audit"]
                .as_array()?
                .iter()
                .find(|event| {
                    event["event"] == "selected_stage_boundary"
                        && event["target_stage"] == json!(stage)
                })
                .map(|event| (name, snapshot, event))
        });
    let abort = (settled
        && error.is_none_or(|e| {
            e == "Synthetic case call budget exhausted; no successful completion implied"
        }))
    .then(|| super::reading::semantic_abort(raw, snapshots, calls, complete))
    .flatten()
    .filter(|witness| {
        witness["stage"] == json!(stage)
            && horary_prompt_program::signature(stage.name(), &witness["original_input"]).method
                == Some(method.name())
    });
    let mut observation = json!({"version":1,"target_stage":stage,"target_method":method,
        "state":"unobserved","all_provider_responses_settled":settled,
        "request_result_pairs_complete":complete,"target_attempts":focused.len(),
        "last_sequence":calls.last().map(|call| &call["request"]["sequence"]),
        "reading_delivered":false,"post_boundary_conversation_executed":false});
    if authentic && settled && error.is_none() {
        if let Some((name, snapshot, event)) = boundary {
            accepted(snapshot, event, &focused, calls)?;
            observation["state"] = json!("accepted");
            observation["snapshot"] = json!(name);
            observation["boundary"] = event.clone();
        }
    }
    if authentic && observation["state"] == "unobserved" {
        if let Some(witness) = &abort {
            observation["state"] = json!("native_rejected");
            observation["snapshot"] = witness["snapshot"].clone();
            observation["rejection"] = witness.clone();
        }
    }
    let observed = observation["state"] != "unobserved";
    let mut outcome = raw.clone();
    outcome["id"] = json!(id);
    outcome["scope"] = json!("selected_stage_function_only");
    outcome["selected_stage_observation"] = json!(true);
    outcome["selected_stage"] = observation;
    outcome["target_stage"] = json!(stage);
    outcome["target_method"] = json!(method);
    outcome["target_signature_calls"] = json!(focused.len());
    outcome["target_function_invoked"] = json!(!focused.is_empty());
    outcome["target_stage_authentic_inputs"] = json!(authentic);
    outcome["target_bindings"] = json!(focused);
    outcome["physical_generation_attempts"] = json!(input_journey_generation_attempts(calls));
    outcome["physical_generation_attempts_complete"] = json!(complete);
    outcome["logical_call_groups"] = json!(groups);
    outcome["logical_provider_calls"] = json!(providers);
    outcome["full_reading"] = json!(false);
    outcome["captured_checkpoint_reused"] = json!(false);
    outcome["reading_semantic_review"] =
        json!("unobserved; this boundary is a component measurement, never a delivered reading");
    outcome["hurdles"]["reading"] = json!({"status":"not_run","reason":"Stopped at the selected native stage before presentation"});
    outcome["execution_status"] = json!(if observed {
        "selected_stage_observed"
    } else {
        "selected_stage_unobserved"
    });
    outcome["execution_error"] = json!(error);
    if observation_rejected(&outcome) {
        outcome["known_native_semantic_abort"] = json!(true);
        outcome["semantic_abort_witness"] = abort.ok_or("No settled target rejection")?;
    }
    if !observed {
        outcome["target_function_not_invoked_reason"] = json!("Real upstream, source or provider work did not establish a settled target boundary; no component score is implied");
    }
    Ok(outcome)
}

fn observation_rejected(outcome: &Value) -> bool {
    outcome["selected_stage"]["state"] == "native_rejected"
}

fn accepted(
    snapshot: &Value,
    boundary: &Value,
    focused: &[Value],
    calls: &[Value],
) -> Result<(), String> {
    let index = boundary["record_index"]
        .as_u64()
        .ok_or("No accepted target record")? as usize;
    let records = snapshot["session"]["method"]["records"]
        .as_array()
        .ok_or("No native records")?;
    let record = records.get(index).ok_or("No accepted target record")?;
    let last = focused.last().ok_or("No actual target invocation")?;
    let actual = calls
        .iter()
        .find(|call| call["request"]["sequence"] == last["sequence"])
        .ok_or("No target call")?;
    let task = super::reading::selected(
        &actual["request"],
        last["branch"].as_u64().map(|n| n as usize),
    )?;
    let result = if let Some(branch) = last["branch"].as_u64() {
        &actual["result"]["Ok"][branch as usize]
    } else {
        &actual["result"]["Ok"]
    };
    let original = crate::horary_step::original_input(&record["input"]);
    let expected_guide = horary_prompt_program::captured_record_guide_sha256(&task, None)?;
    let messages = snapshot["session"]["messages"]
        .as_array()
        .ok_or("No real user turn")?;
    if index + 1 != records.len()
        || record["stage"] != boundary["target_stage"]
        || record["revision"] != boundary["revision"]
        || record["revision"] != snapshot["session"]["revision"]
        || !record["validationError"].is_null()
        || record["input"] != task["input"]
        || record["generation"] != *result
        || record["raw"] != result["content"]
        || record["worksheet"]
            != crate::horary_contract::decode_json(
                record["raw"].as_str().ok_or("No stage response")?,
            )?
        || record["guideSha256"] != expected_guide
        || record["schemaSha256"] != horary_prompt_program::digest(task["schema"].to_string())
        || boundary["record_input_sha256"] != record["inputSha256"]
        || boundary["native_job"]["inputSha256"]
            != horary_prompt_program::digest(original.to_string())
        || boundary["native_job"]["stage"] != record["stage"]
        || boundary["native_job"]["revision"] != record["revision"]
        || boundary["native_job"]["phase"]["state"] != "complete"
        || boundary["native_job"]["phase"]["record_index"] != json!(index)
        || boundary["reading_request"] != original["reading_request"]
        || boundary["binding"] != original["reading_request"]["binding"]
        || boundary["last_sequence"] != last["sequence"]
        || boundary["after_message"] != json!(messages.len())
        || boundary["reading_delivered"] != false
        || boundary["post_boundary_conversation_executed"] != false
        || !snapshot["session"]["method"]["result"].is_null()
        || !snapshot["session"]["method"]["flow"]["jobs"]
            .as_array()
            .is_some_and(|jobs| jobs.iter().any(|job| job == &boundary["native_job"]))
    {
        return Err("Selected-stage boundary differs from its actual native acceptance, job or current input permit".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> (Value, Value, Vec<Value>) {
        let permit =
            json!({"binding":{"frame":{"method":"movable_deal"},"question":"Synthetic question"}});
        let input = json!({"reading_request":permit,"facts":[]});
        let worksheet = json!({"summary":"Synthetic checked output"});
        let result = json!({"content":worksheet.to_string()});
        let request = json!({"sequence":1,"stage":"condition","matter":"other","input":input,"schema":{},
            "prompt":[{"role":"system","content":"Synthetic exact guide"}],"prompt_program":null});
        let call = json!({"request":request,"result":{"Ok":result},"provider_receipt":{
            "submitted":true,"provider":"google_gemini_api","response":{"http_status":200},
            "native_result":{"Ok":result},"generation_attempts":[{"submitted":true,"http_status":200,"error":null}]}});
        let record = json!({"stage":"condition","revision":1,"input":input,"inputSha256":horary_prompt_program::digest(input.to_string()),
            "guideSha256":horary_prompt_program::digest("Synthetic exact guide"),"schemaSha256":horary_prompt_program::digest("{}"),
            "raw":worksheet.to_string(),"worksheet":worksheet,"generation":result,"validationError":null});
        let job = json!({"key":"synthetic-work-key","stage":"condition","revision":1,"inputSha256":record["inputSha256"],
            "phase":{"state":"complete","record_index":0}});
        let boundary = json!({"event":"selected_stage_boundary","version":1,"target_stage":"condition","revision":1,"record_index":0,
            "record_input_sha256":record["inputSha256"],"native_job":job,"work_key":"synthetic-work-key",
            "reading_request":permit,"binding":permit["binding"],"after_message":1,"last_sequence":1,
            "reading_delivered":false,"post_boundary_conversation_executed":false});
        let snapshot = json!({"result":{"Ok":null},"session":{"revision":1,"messages":[{"role":"user","text":"Synthetic question"}],
            "method":{"records":[record],"flow":{"jobs":[job]},"result":null},
            "audit":[{"event":"contract_handoff","binding":permit["binding"],"request":permit},boundary]}});
        let raw = json!({"full_reading":true,"deadline_cancelled":false,"group_cancelled":false,"provider_stop":null,"infrastructure_error":null});
        (raw, snapshot, vec![call])
    }
    fn measure(raw: &Value, snapshot: &Value, calls: &[Value]) -> Value {
        measurement(
            ("synthetic", Stage::Condition, Method::MovableDeal),
            raw,
            (snapshot, snapshot),
            calls,
            true,
            None,
        )
        .unwrap()
    }
    #[test]
    fn exact_native_stage_is_observed_without_a_delivered_reading_or_sibling() {
        let (raw, snapshot, calls) = sample();
        let value = measure(&raw, &snapshot, &calls);
        assert_eq!(value["selected_stage"]["state"], "accepted");
        assert_eq!(value["full_reading"], false);
        assert_eq!(value["hurdles"]["reading"]["status"], "not_run");
        assert_eq!(value["selected_stage"]["target_attempts"], 1);
    }
    #[test]
    fn stage_observation_rejects_stale_permit_record_schema_and_later_work() {
        let (raw, snapshot, calls) = sample();
        for key in ["guideSha256", "schemaSha256", "inputSha256"] {
            let mut bad = snapshot.clone();
            bad["session"]["method"]["records"][0][key] = json!("stale");
            assert!(
                measurement(
                    ("synthetic", Stage::Condition, Method::MovableDeal),
                    &raw,
                    (&bad, &bad),
                    &calls,
                    true,
                    None
                )
                .is_err(),
                "{key}"
            );
        }
        let mut bad = snapshot.clone();
        bad["session"]["method"]["result"] = json!({"result":"judgment"});
        assert!(measurement(
            ("synthetic", Stage::Condition, Method::MovableDeal),
            &raw,
            (&bad, &bad),
            &calls,
            true,
            None
        )
        .is_err());
    }
    #[test]
    fn upstream_uninvoked_and_provider_uncertainty_stay_unobserved() {
        let (mut raw, snapshot, mut calls) = sample();
        raw["deadline_cancelled"] = json!(true);
        assert_eq!(
            measure(&raw, &snapshot, &calls)["selected_stage"]["state"],
            "unobserved"
        );
        raw["deadline_cancelled"] = json!(false);
        calls[0]["provider_receipt"]["generation_attempts"][0]["error"] = json!("uncertain");
        assert_eq!(
            measure(&raw, &snapshot, &calls)["selected_stage"]["state"],
            "unobserved"
        );
        calls[0]["request"]["stage"] = json!("reception");
        assert_eq!(
            measure(&raw, &snapshot, &calls)["target_function_invoked"],
            false
        );
    }
    #[test]
    fn only_a_settled_paused_target_rejection_is_a_semantic_negative() {
        let (raw, mut snapshot, calls) = sample();
        let stop = "Synthetic case call budget exhausted; no successful completion implied";
        snapshot["result"] = json!({"Err":stop});
        snapshot["session"]["method"]["records"][0]["validationError"] =
            json!("Missing actual capacity witness");
        snapshot["session"]["method"]["flow"]["jobs"][0]["phase"] =
            json!({"state":"paused","error":stop});
        snapshot["session"]["audit"].as_array_mut().unwrap().pop();
        let value = measure(&raw, &snapshot, &calls);
        assert_eq!(value["selected_stage"]["state"], "native_rejected");
        assert_eq!(value["known_native_semantic_abort"], true);
        snapshot["session"]["method"]["flow"]["jobs"][0]["inputSha256"] = json!("unrelated");
        assert_eq!(
            measure(&raw, &snapshot, &calls)["selected_stage"]["state"],
            "unobserved"
        );
    }
}
