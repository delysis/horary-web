//! One fresh production reading, with a scoped teaching override. Captured
//! specialist inputs are used only for offline inspection, never execution.
#![forbid(unsafe_code)]
use super::*;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Task {
    case_id: String,
    source_case_directory: PathBuf,
    source_request_file: PathBuf,
    source_request_sha256: String,
    source_initial_sha256: String,
    source_fixture_sha256: String,
    source_origin_sha256: String,
    source_rubric_sha256: String,
    source_branch: Option<usize>,
    baseline_manifest_sha256: String,
    target_method: Method,
    target_stage: Stage,
    #[serde(default)]
    program_file: Option<PathBuf>,
    #[serde(default)]
    inspect_prompt_only: bool,
}

fn reading_stage(stage: Stage) -> bool {
    matches!(
        stage,
        Stage::Significators
            | Stage::Condition
            | Stage::Reception
            | Stage::Contacts
            | Stage::Location
            | Stage::Judgment
    )
}

fn selected(request: &Value, branch: Option<usize>) -> Result<Value, String> {
    if let Some(branch) = branch {
        let tasks = request["tasks"]
            .as_array()
            .filter(|tasks| !tasks.is_empty() && tasks.len() <= 4)
            .ok_or("Reading source has no bounded batch")?;
        let task = tasks
            .get(branch)
            .and_then(Value::as_array)
            .filter(|task| task.len() == 4)
            .ok_or("Reading source branch is absent or malformed")?;
        let prompts = request["prompts"]
            .as_array()
            .filter(|prompts| prompts.len() == tasks.len())
            .ok_or("Reading source batch prompts are not bound to tasks")?;
        let programs = request["prompt_program"]
            .as_array()
            .filter(|programs| programs.len() == tasks.len())
            .ok_or("Reading source batch programs are not bound to tasks")?;
        Ok(
            json!({"stage":task[0],"matter":task[1],"input":task[2],"schema":task[3],
            "prompt":prompts[branch],"prompt_program":programs[branch]}),
        )
    } else if request["tasks"].is_null() && request["stage"].is_string() {
        Ok(request.clone())
    } else {
        Err("Reading source needs an explicit batch branch or one ordinary task".into())
    }
}

fn source_scope(
    task: &Task,
    case: &Case,
    request: &Value,
    initial: &Value,
    fixture: &Value,
    origin: &Value,
    request_name: &str,
) -> Result<(Matter, Value), String> {
    if !reading_stage(task.target_stage)
        || case.id != task.case_id
        || *fixture != serde_json::to_value(case).map_err(|e| e.to_string())?
        || *initial != input_journey_initial(case)
        || case.method != task.target_method
    {
        return Err(
            "Reading source differs from its authored question, method or fresh starting state"
                .into(),
        );
    }
    let sequence = request["sequence"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or("Reading source request lacks a sequence")?;
    if origin["case_id"] != case.id
        || request_name != format!("calls/{sequence:04}-request.json")
        || [
            ("initial.json", &task.source_initial_sha256),
            ("fixture.json", &task.source_fixture_sha256),
            ("reading-rubric.json", &task.source_rubric_sha256),
            (request_name, &task.source_request_sha256),
        ]
        .iter()
        .any(|(name, sha)| {
            origin["files"][*name]
                .as_str()
                .is_none_or(|actual| !actual.eq_ignore_ascii_case(sha))
        })
    {
        return Err(
            "Reading origin does not seal its initial question, fixture, rubric and request".into(),
        );
    }
    let call = selected(request, task.source_branch)?;
    let input = crate::horary_step::original_input(&call["input"]);
    let scope = horary_prompt_program::signature(task.target_stage.name(), input);
    if call["stage"] != task.target_stage.name()
        || scope.recognition_phase.is_some()
        || scope.method != Some(task.target_method.name())
        || !call["prompt_program"].is_null()
        || !input["reading_request"]["binding"].is_object()
        || !call["schema"].is_object()
        || !call["prompt"].is_array()
    {
        return Err("Reading inspection requires an unmodified actual selected-stage capture with a bound reading permit".into());
    }
    if task.source_branch.is_none()
        && (call["schema_sha256"] != horary_prompt_program::digest(call["schema"].to_string())
            || call["prompt_sha256"] != horary_prompt_program::digest(call["prompt"].to_string()))
    {
        return Err("Reading source prompt/schema hashes are inconsistent".into());
    }
    let matter = serde_json::from_value(call["matter"].clone()).map_err(|e| e.to_string())?;
    Ok((matter, call["input"].clone()))
}

fn program(
    task: &Task,
    bytes: Option<&[u8]>,
) -> Result<Option<horary_prompt_program::Program>, String> {
    let program = bytes
        .map(horary_prompt_program::Program::parse)
        .transpose()?;
    if program.as_ref().is_some_and(|p| {
        !p.baseline_manifest_sha256
            .eq_ignore_ascii_case(&task.baseline_manifest_sha256)
            || p.overrides.len() != 1
            || p.overrides[0].stage != task.target_stage.name()
            || p.overrides[0].recognition_phase.is_some()
            || p.overrides[0].method.as_deref() != Some(task.target_method.name())
    }) {
        return Err(
            "Reading program may change only its selected stage/method on its pinned baseline"
                .into(),
        );
    }
    Ok(program)
}

fn counts(calls: &[Value]) -> Result<(u64, u64), String> {
    let mut providers = 0u64;
    for call in calls {
        let request = &call["request"];
        let count = if let Some(tasks) = request["tasks"].as_array() {
            if tasks.is_empty()
                || tasks.len() > 4
                || tasks
                    .iter()
                    .any(|task| task.as_array().is_none_or(|tuple| tuple.len() != 4))
            {
                return Err("Reading batch exceeds the four-branch provider reservation".into());
            }
            let mut stages = BTreeSet::new();
            for task in tasks {
                let stage = task[0].as_str().ok_or("Unknown reading batch stage")?;
                if !matches!(
                    stage,
                    "place" | "moment" | "condition" | "reception" | "contacts" | "location"
                ) || !stages.insert(stage)
                {
                    return Err("Reading batch contains an unsupported or duplicate branch".into());
                }
            }
            tasks.len() as u64
        } else {
            let stage: Stage = serde_json::from_value(request["stage"].clone())
                .map_err(|_| "Unknown reading call stage")?;
            if !reading_stage(stage)
                && !matches!(
                    stage,
                    Stage::Intake | Stage::Place | Stage::Moment | Stage::Conversation
                )
            {
                return Err("Reading executor encountered an unreserved neural function".into());
            }
            1
        };
        providers = providers
            .checked_add(count)
            .ok_or("Reading provider count overflow")?;
    }
    Ok((calls.len() as u64, providers))
}

fn focused(calls: &[Value], stage: Stage, method: Method) -> Result<Vec<Value>, String> {
    let mut output = Vec::new();
    for call in calls {
        let request = &call["request"];
        let branches: Vec<_> = request["tasks"]
            .as_array()
            .map_or_else(|| vec![None], |tasks| (0..tasks.len()).map(Some).collect());
        for branch in branches {
            let task = selected(request, branch)?;
            let input = crate::horary_step::original_input(&task["input"]);
            let signature = horary_prompt_program::signature(stage.name(), input);
            if task["stage"] == stage.name()
                && signature.method == Some(method.name())
                && signature.recognition_phase.is_none()
            {
                output.push(json!({"sequence":request["sequence"],"branch":branch,
                    "binding":input["reading_request"]["binding"],
                    "reading_request":input["reading_request"]}));
            }
        }
    }
    Ok(output)
}

/// A real production ReadyReading has no deserializer or public constructor.
/// Require its audit witness too; a source capture cannot grant this permit.
fn authentic_permit(snapshot: &Value, focused: &Value) -> bool {
    let binding = &focused["binding"];
    binding.is_object()
        && snapshot["session"]["audit"]
            .as_array()
            .is_some_and(|events| {
                events.iter().any(|event| {
                    event["event"] == "contract_handoff"
                        && event["binding"] == *binding
                        && event["request"] == focused["reading_request"]
                })
            })
}

const SEMANTIC_BUDGET_STOP: &str =
    "Synthetic case call budget exhausted; no successful completion implied";

pub(super) fn provider_responses_settled(calls: &[Value]) -> bool {
    fn settled(calls: &[Value]) -> Option<()> {
        for call in calls {
            let expected = call["request"]["tasks"].as_array().map_or(1, Vec::len);
            let results = if call["request"]["tasks"].is_array() {
                call["result"]["Ok"].as_array()?.iter().collect::<Vec<_>>()
            } else {
                vec![call["result"].get("Ok")?]
            };
            let receipts = if call["provider_receipt"].is_object() {
                vec![&call["provider_receipt"]]
            } else {
                call["provider_receipts"].as_array()?.iter().collect()
            };
            if results.len() != expected || receipts.len() != expected {
                return None;
            }
            for (result, receipt) in results.iter().zip(receipts) {
                let attempts = receipt["generation_attempts"].as_array()?;
                if receipt["submitted"] != true
                    || receipt["provider"] != "google_gemini_api"
                    || receipt["response"]["http_status"] != 200
                    || receipt["native_result"]["Ok"] != **result
                    || attempts.is_empty()
                    || attempts.iter().any(|attempt| {
                        attempt["submitted"] != true
                            || !attempt["error"].is_null()
                            || attempt["http_status"].as_u64().is_none()
                    })
                {
                    return None;
                }
            }
        }
        Some(())
    }
    !calls.is_empty() && settled(calls).is_some()
}

/// An explicit cap after settled native rejections is a measurable negative.
/// Cancellation, transport or missing paid observations are never zero scores.
pub(super) fn semantic_abort(
    raw: &Value,
    snapshots: (&Value, &Value),
    calls: &[Value],
    complete: bool,
) -> Option<Value> {
    if !complete
        || calls.is_empty()
        || raw["deadline_cancelled"] == true
        || raw["group_cancelled"] == true
        || !raw["provider_stop"].is_null()
        || !raw["infrastructure_error"].is_null()
        || !provider_responses_settled(calls)
    {
        return None;
    }
    let (first, final_state) = snapshots;
    let (name, snapshot, error) =
        if final_state["follow_up"]["result"]["Err"] == SEMANTIC_BUDGET_STOP {
            (
                "final.json",
                final_state,
                final_state["follow_up"]["result"]["Err"].as_str()?,
            )
        } else if first["result"]["Err"] == SEMANTIC_BUDGET_STOP {
            ("first-turn.json", first, first["result"]["Err"].as_str()?)
        } else {
            return None;
        };
    let records = snapshot["session"]["method"]["records"].as_array()?;
    let (index, record) = records.iter().enumerate().next_back()?;
    let rejection = record["validationError"]
        .as_str()
        .filter(|error| !error.is_empty())?;
    let original = crate::horary_step::original_input(&record["input"]);
    let input_sha = horary_prompt_program::digest(original.to_string());
    let job = snapshot["session"]["method"]["flow"]["jobs"]
        .as_array()?
        .iter()
        .find(|job| {
            job["stage"] == record["stage"]
                && job["revision"] == record["revision"]
                && job["inputSha256"] == input_sha
                && job["phase"]["state"] == "paused"
                && job["phase"]["error"] == error
        })?;
    Some(
        json!({"snapshot":name,"record_index":index,"stage":record["stage"],
        "revision":record["revision"],"input_sha256":input_sha,"original_input":original,"validation_error":rejection,
        "paused_job":job,"stop":error,"all_provider_responses_settled":true,
        "request_result_pairs_complete":true,"observed_call_groups":calls.len(),
        "qualification":"Fully observed bounded native validation failure; no uncertain provider call was converted to a score."}),
    )
}

fn measurement(
    case: &Case,
    task: &Task,
    raw: &Value,
    snapshots: (&Value, &Value),
    calls: &[Value],
    complete: bool,
    run_error: Option<&str>,
) -> Result<Value, String> {
    let (first, final_state) = snapshots;
    if raw["full_reading"] != true {
        return Err("Reading adapter requires actual production full-reading execution".into());
    }
    let (groups, providers) = counts(calls)?;
    let focused = focused(calls, task.target_stage, task.target_method)?;
    let authentic = focused
        .iter()
        .all(|call| authentic_permit(first, call) || authentic_permit(final_state, call));
    if !authentic {
        return Err("Selected reading call lacks an actual native accepted-input permit; no component score allowed".into());
    }
    let mut outcome = raw.clone();
    outcome["scope"] = json!("reading_journey_function_only");
    outcome["id"] = json!(case.id);
    outcome["target_stage"] = json!(task.target_stage);
    outcome["target_method"] = json!(task.target_method);
    outcome["target_signature_calls"] = json!(focused.len());
    outcome["target_function_invoked"] = json!(!focused.is_empty());
    outcome["target_stage_authentic_inputs"] = json!(authentic && !focused.is_empty());
    outcome["target_bindings"] = json!(focused);
    outcome["physical_generation_attempts"] = json!(input_journey_generation_attempts(calls));
    outcome["physical_generation_attempts_complete"] = json!(complete);
    outcome["logical_call_groups"] = json!(groups);
    outcome["logical_provider_calls"] = json!(providers);
    outcome["full_reading"] = json!(true);
    outcome["captured_checkpoint_reused"] = json!(false);
    outcome["reading_semantic_review"] =
        json!("pending independent source-cited judgment; native completion is not correctness");
    let error = run_error
        .or(first["result"]["Err"].as_str())
        .or(final_state["follow_up"]["result"]["Err"].as_str());
    let abort = run_error
        .is_none_or(|error| error == SEMANTIC_BUDGET_STOP)
        .then(|| semantic_abort(raw, snapshots, calls, complete))
        .flatten();
    if let Some(witness) = abort {
        outcome["known_native_semantic_abort"] = json!(true);
        outcome["semantic_abort_witness"] = witness;
        outcome["execution_status"] = json!("observed_native_validation_exhaustion");
        outcome["infrastructure_error"] = Value::Null;
    } else if !complete || error.is_some() {
        outcome["execution_status"] = json!("reading_executor_interrupted");
        outcome["infrastructure_error"] =
            json!({"kind":"incomplete_reading_execution","error":error,"calls_complete":complete});
    }
    outcome["execution_error"] = json!(error);
    if focused.is_empty() {
        outcome["target_function_not_invoked_reason"] = json!("Fresh upstream execution did not invoke this reading stage; no checkpoint, gold method or score was injected");
    }
    Ok(outcome)
}

#[test]
#[ignore = "Fresh hosted production reading: hash-bound task/evidence and invocation-only credential required"]
fn real_model_reading_function() -> Result<(), String> {
    let task_bytes = fs::read(PathBuf::from(
        std::env::var_os("HORARY_NEURAL_TASK").ok_or("Set HORARY_NEURAL_TASK")?,
    ))
    .map_err(|e| e.to_string())?;
    let task: Task = serde_json::from_slice(&task_bytes).map_err(|e| e.to_string())?;
    let case = catalogue()?
        .into_iter()
        .find(|case| case.id == task.case_id)
        .ok_or("Reading task names no authored synthetic case")?;
    let source_dir = task
        .source_case_directory
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if source_dir.file_name().and_then(|s| s.to_str()) != Some(case.id.as_str()) {
        return Err("Reading source directory must name its authored case".into());
    }
    let path = source_dir
        .join(&task.source_request_file)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let name = path
        .strip_prefix(&source_dir)
        .map_err(|_| "Reading request escapes its source case")?
        .to_str()
        .ok_or("Non-UTF8 reading source request")?;
    let campaign = source_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("Reading campaign unavailable")?;
    let source_files = [
        (
            "source-request.json",
            path.clone(),
            &task.source_request_sha256,
        ),
        (
            "source-initial.json",
            source_dir.join("initial.json"),
            &task.source_initial_sha256,
        ),
        (
            "source-fixture.json",
            source_dir.join("fixture.json"),
            &task.source_fixture_sha256,
        ),
        (
            "source-origin.json",
            campaign
                .join("case-origins")
                .join(format!("{}.json", case.id)),
            &task.source_origin_sha256,
        ),
        (
            "source-rubric.json",
            source_dir.join("reading-rubric.json"),
            &task.source_rubric_sha256,
        ),
        (
            "source-manifest.json",
            campaign.join("manifest.json"),
            &task.baseline_manifest_sha256,
        ),
    ];
    let mut bytes = Vec::new();
    let mut values = Vec::new();
    for (_, path, sha) in &source_files {
        let source = classification_source_bytes(path, sha)?;
        values.push(serde_json::from_slice::<Value>(&source).map_err(|e| e.to_string())?);
        bytes.push(source);
    }
    let (matter, input) = source_scope(
        &task, &case, &values[0], &values[1], &values[2], &values[3], name,
    )?;
    if serde_json::to_vec_pretty(&input_journey_initial(&case)).map_err(|e| e.to_string())?
        != bytes[1]
    {
        return Err("Fresh authored initial bytes differ; no hosted request submitted".into());
    }
    let rubric: crate::reading_eval::Rubric =
        serde_json::from_slice(&bytes[4]).map_err(|e| e.to_string())?;
    if rubric.case_id != case.id
        || rubric.declared_method != case.method
        || rubric.reading_method != task.target_method
    {
        return Err("Reading rubric identity/method differs from the source task".into());
    }
    let provenance = input_journey_source_fingerprint(&values[3], &values[5])?;
    let program_bytes = task
        .program_file
        .as_ref()
        .map(fs::read)
        .transpose()
        .map_err(|e| e.to_string())?;
    let program = program(&task, program_bytes.as_deref())?;
    let schema = crate::horary_step::response_schema_for(task.target_stage, matter, &input, &[]);
    let (prompt, applied) =
        trial_prompt(task.target_stage, matter, &input, &schema, program.as_ref())?;
    let messages: Value = serde_json::from_str(&prompt).map_err(|e| e.to_string())?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Repository root unavailable")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let dir = PathBuf::from(
        std::env::var_os("HORARY_EVAL_EVIDENCE").ok_or("Set fresh HORARY_EVAL_EVIDENCE")?,
    );
    fs::create_dir(&dir).map_err(|e| format!("Reading evidence must be fresh: {e}"))?;
    write_new(&dir.join("task.json"), &task)?;
    for ((name, _, _), source) in source_files.iter().zip(&bytes) {
        atomic_evidence_file(&dir.join(name), true, |file| {
            file.write_all(source).map_err(|e| e.to_string())
        })?;
    }
    if task.inspect_prompt_only {
        if task.program_file.is_some() {
            return Err("Offline reading inspection cannot apply teaching".into());
        }
        write_new(
            &dir.join("inspection.json"),
            &json!({"scope":"reading_journey_prompt_inspection_only",
            "case_id":case.id,"target_method":task.target_method,"stage":task.target_stage,
            "prompt":messages,"schema":schema,"input":input,
            "guide_sha256":horary_prompt_program::digest(messages[0]["content"].as_str().ok_or("No selected stage guide")?),
            "source_provenance":provenance,"source_request_sha256":task.source_request_sha256,
            "source_rubric_sha256":task.source_rubric_sha256,"model_constructed":false,
            "new_generation_attempts":0,"captured_input_used_for":"Offline prompt inspection only; production execution starts from authored original words"}),
        )?;
        return Ok(());
    }
    let max_calls = std::env::var("HORARY_EVAL_MAX_CALLS")
        .map_err(|_| "Set bounded reading call cap")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let seconds = std::env::var("HORARY_EVAL_CASE_SECONDS")
        .map_err(|_| "Set bounded reading deadline")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let provider_cap = max_calls
        .checked_mul(4)
        .filter(|cap| *cap > 0)
        .ok_or("Invalid reading provider cap")?;
    let reservation = provider_cap
        .checked_mul(3)
        .ok_or("Reading physical reservation overflow")?;
    if seconds == 0 {
        return Err("Reading deadline must be positive".into());
    }
    let keyfile = PathBuf::from(
        std::env::var_os("HORARY_GOOGLE_KEY_FILE")
            .ok_or("Fresh hosted credential file locator required")?,
    );
    if keyfile
        .canonicalize()
        .map_err(|_| "Hosted credential unavailable")?
        .starts_with(&root)
    {
        return Err("Keep hosted credential outside source checkout".into());
    }
    let hosted = Arc::new(crate::hosted_gemma_eval::Client::from_file(&keyfile, 1)?);
    write_new(
        &dir.join("manifest.json"),
        &json!({"version":EVALUATOR_VERSION,
        "scope":"reading_journey_function_only","task_sha256":horary_prompt_program::digest(task_bytes),
        "source_provenance":provenance,"sources":source_hashes(&root)?,"model":hosted.metadata(),
        "max_provider_calls_per_group":4,"physical_generation_attempt_reservation":reservation,
        "logical_call_group_cap":max_calls,"logical_provider_call_cap":provider_cap,"case_seconds":seconds,
        "target_stage":task.target_stage,"target_method":task.target_method,"inspected_current_target_program":applied,
        "entry_point":"evaluate_case -> horary_pipeline::run","fresh_initial_verified_before_network":true,
        "captured_checkpoint_reused":false,"full_reading":true,"rubric_supplied_to_student":false}),
    )?;
    if let Some(source) = &program_bytes {
        atomic_evidence_file(&dir.join("prompt-program.json"), true, |file| {
            file.write_all(source).map_err(|e| e.to_string())
        })?;
    }
    fs::create_dir(dir.join("cases")).map_err(|e| e.to_string())?;
    let rubrics = BTreeMap::from([(case.id.clone(), rubric)]);
    let state = NativeLlamaState::default();
    let run = evaluate_case(
        &state,
        &dir,
        case.clone(),
        CaseRun {
            full_reading: true,
            initial_extractor_target: None,
            seconds,
            max_calls,
            dispatcher: None,
            shared_cancelled: None,
            group: None,
            program: program.map(Arc::new),
            rubrics: Some(Arc::new(rubrics)),
            hosted: Some(hosted),
        },
    );
    let case_dir = dir.join("cases").join(&case.id);
    let read = |name: &str| -> Result<Value, String> {
        let path = case_dir.join(name);
        if !path.exists() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    };
    let raw = run
        .as_ref()
        .ok()
        .cloned()
        .unwrap_or(json!({"full_reading":true}));
    let first = read("first-turn.json")?;
    let final_state = read("final.json")?;
    let (calls, complete) = input_journey_calls(&case_dir)?;
    let initial_check =
        classification_source_bytes(&case_dir.join("initial.json"), &task.source_initial_sha256);
    let initial_verified = initial_check.is_ok();
    let run_error = run.as_ref().err().cloned().or_else(|| initial_check.err());
    let mut outcome = measurement(
        &case,
        &task,
        &raw,
        (&first, &final_state),
        &calls,
        complete,
        run_error.as_deref(),
    )?;
    outcome["fresh_initial_sha256_verified"] = json!(initial_verified);
    outcome["native_evidence_directory"] = json!(case_dir);
    outcome["physical_generation_attempt_reservation"] = json!(reservation);
    outcome["source_provenance"] = provenance;
    write_new(&dir.join("calls.json"), &calls)?;
    let mut final_copy = final_state;
    if !final_copy.is_object() {
        final_copy = json!({"session":null});
    }
    final_copy["scope"] = json!("reading_journey_function_only");
    final_copy["full_reading"] = json!(true);
    final_copy["native_evidence_directory"] = json!(case_dir);
    write_new(&dir.join("final.json"), &final_copy)?;
    write_new(&dir.join("outcome.json"), &outcome)?;
    println!("{outcome}");
    if outcome["known_native_semantic_abort"] == true {
        Ok(())
    } else {
        run_error.map_or(Ok(()), Err)
    }
}

#[test]
fn batch_uses_four_branch_reservation_and_rejects_hidden_fifth() {
    let tasks = json!([
        ["condition", null, {}, {}],
        ["reception", null, {}, {}],
        ["contacts", null, {}, {}],
        ["location", null, {}, {}]
    ]);
    let mut calls = vec![json!({"request":{"tasks":tasks}})];
    assert_eq!(counts(&calls).unwrap(), (1, 4));
    calls[0]["request"]["tasks"]
        .as_array_mut()
        .unwrap()
        .push(json!(["place", null, {}, {}]));
    assert!(counts(&calls).is_err());
}

#[test]
fn captured_task_or_matching_binding_alone_cannot_authorize_live_stage() {
    let binding = json!({"question":"Synthetic question","input_sha256":"x"});
    let focused = json!({"binding":binding,"reading_request":{"binding":binding}});
    let mut snapshot = json!({"session":{"audit":[]}});
    assert!(!authentic_permit(&snapshot, &focused));
    snapshot["session"]["audit"] = json!([{"event":"contract_handoff","binding":binding,
        "request":{"binding":binding}}]);
    assert!(authentic_permit(&snapshot, &focused));
    snapshot["session"]["audit"][0]["request"]["binding"]["question"] = json!("Other question");
    assert!(!authentic_permit(&snapshot, &focused));
}

#[test]
fn batch_inspection_requires_indexed_prompt_and_scope() {
    let request = json!({"tasks":[["condition","general",{},{}],["reception","general",{},{}]],
        "prompts":[[{"content":"Condition"}],[{"content":"Reception"}]],"prompt_program":[null,null]});
    assert!(selected(&request, None).is_err());
    assert_eq!(
        selected(&request, Some(1)).unwrap()["prompt"][0]["content"],
        "Reception"
    );
    assert!(selected(&request, Some(2)).is_err());
}

#[test]
fn settled_native_rejection_is_loopable_but_uncertain_or_successful_cap_is_not() {
    let input = json!({"reading_request":{"binding":{"question":"Synthetic"}}});
    let input_sha = horary_prompt_program::digest(input.to_string());
    let output = json!({"text":"A settled invalid worksheet"});
    let raw = json!({"deadline_cancelled":false,"group_cancelled":false,"provider_stop":null,
        "infrastructure_error":null});
    let job = json!({"stage":"judgment","revision":0,"inputSha256":input_sha,
        "phase":{"state":"paused","error":SEMANTIC_BUDGET_STOP}});
    let snapshot = json!({"result":{"Err":SEMANTIC_BUDGET_STOP},"session":{"method":{
        "records":[{"stage":"judgment","revision":0,"input":input,
            "validationError":"Missing required source evidence"}],"flow":{"jobs":[job]}}}});
    let mut calls = vec![
        json!({"request":{"stage":"judgment"},"result":{"Ok":output},
        "provider_receipt":{"provider":"google_gemini_api","submitted":true,
            "response":{"http_status":200},"native_result":{"Ok":output},
            "generation_attempts":[{"submitted":true,"http_status":200}]}}),
    ];
    let witness = semantic_abort(&raw, (&snapshot, &Value::Null), &calls, true).unwrap();
    assert_eq!(witness["stage"], "judgment");
    assert!(semantic_abort(&raw, (&snapshot, &Value::Null), &calls, false).is_none());
    calls[0]["provider_receipt"]["generation_attempts"][0]["error"] =
        json!("Transport uncertainty");
    assert!(semantic_abort(&raw, (&snapshot, &Value::Null), &calls, true).is_none());
    calls[0]["provider_receipt"]["generation_attempts"][0]
        .as_object_mut()
        .unwrap()
        .remove("error");
    let mut successful = snapshot.clone();
    successful["session"]["method"]["records"][0]["validationError"] = Value::Null;
    assert!(semantic_abort(&raw, (&successful, &Value::Null), &calls, true).is_none());
    let mut cancelled = raw;
    cancelled["deadline_cancelled"] = json!(true);
    assert!(semantic_abort(&cancelled, (&snapshot, &Value::Null), &calls, true).is_none());
}
