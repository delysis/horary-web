//! Actual native neural functions; no local inference or downstream capture transplant.
use crate::{
    journal::{Journal, Operation},
    keep, load, read, verify, ControlMode, Example, Function, Objective, Plan, Result,
};
use gepa::Candidate;
use horary_prompt_program::digest;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn verify_cached_import(directory: &Path, response: &Value) -> Result<()> {
    let file = directory.join("cache-source.json");
    if file.exists() {
        let source = load(&file)?;
        if source["type"] == crate::controls::COMPONENT_IMPORT {
            crate::controls::verify_component_import(
                &source,
                &load(&directory.join("request.json"))?,
                response,
            )?;
        } else if source["type"] == crate::curriculum::IMPORT {
            crate::curriculum::verify_import(
                &source,
                &load(&directory.join("request.json"))?,
                response,
            )?;
        }
    }
    Ok(())
}

fn request_body(
    plan: &Plan,
    example: &Example,
    candidate: &Candidate,
    archival: bool,
    fixed_receipt: Option<&Value>,
) -> Value {
    let mut request = json!({"case":example,"candidate":candidate,"native_executable_sha256":plan.native_executable_sha256,
        "manifest_sha256":plan.manifest_sha256,"function":plan.function,"target_method":plan.target_method,
        "scope":plan.function.metric(),"archival_control":archival});
    if let Some(receipt) = fixed_receipt {
        request["fixed_program"] = receipt.clone();
    }
    if plan.exact_response_memoization {
        request["exact_response_memoization"] = json!({"version":horary_prompt_program::response_memo::VERSION,
            "excluded_stage":plan.signature().stage,"qualification":"Component search with exact raw-response reuse; not fresh complete-journey qualification"});
    }
    if plan.objective == Objective::ExtractorReliability {
        request["objective"] = json!(plan.objective);
    }
    if plan.initial_extractor_observation {
        request["initial_extractor_observation"] = json!(true);
    }
    if plan.selected_stage_observation {
        request["selected_stage_observation"] = json!(true);
    }
    if plan.function == Function::ReadingJourney {
        request["objective"] = json!(plan.objective);
        request["target_stage"] = json!(plan.target_stage);
    }
    request
}

/// The exact production logical identity, without quota reservation or a child.
pub(crate) fn logical_request(
    plan: &Plan,
    example: &Example,
    candidate: &Candidate,
) -> Result<Value> {
    let program = plan.program(candidate)?;
    let archival = program.is_none() && plan.control_mode == ControlMode::ArchivedCapture;
    let fixed = plan
        .fixed_program
        .as_ref()
        .map(|_| {
            crate::fixed::receipt(
                plan,
                program
                    .as_ref()
                    .ok_or("Fixed teaching has no execution program")?,
            )
        })
        .transpose()?;
    Ok(request_body(
        plan,
        example,
        candidate,
        archival,
        fixed.as_ref(),
    ))
}

/// A captured control must pass today's actual executor and exact prompt match.
/// A candidate always performs fresh hosted generation. Accepted specialist
/// checkpoints are never transplanted from a control into a changed program.
pub fn evaluate(
    plan: &Plan,
    journal: &mut Journal,
    example: &Example,
    candidate: &Candidate,
) -> Result<Value> {
    let program = plan.program(candidate)?;
    let archival = program.is_none() && plan.control_mode == ControlMode::ArchivedCapture;
    let fixed_receipt = plan
        .fixed_program
        .as_ref()
        .map(|_| {
            crate::fixed::receipt(
                plan,
                program
                    .as_ref()
                    .ok_or("Fixed teaching has no execution program")?,
            )
        })
        .transpose()?;
    let request = request_body(plan, example, candidate, archival, fixed_receipt.as_ref());
    let cache_key = digest(request.to_string());
    let cache = journal
        .root
        .join("evaluation-cache")
        .join(format!("{cache_key}.json"));
    if cache.exists() {
        let index = load(&cache)?;
        let relative = index["operation"].as_str().ok_or("Invalid cache link")?;
        if !relative.starts_with("operations/")
            || Path::new(relative)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err("Cache link escapes operations".into());
        }
        let previous = journal.root.join(relative);
        verify(
            &previous.join("completed.json"),
            index["completed_sha256"]
                .as_str()
                .ok_or("Missing cache receipt digest")?,
        )?;
        let receipt = load(&previous.join("completed.json"))?;
        for artifact in receipt["artifacts"]
            .as_array()
            .ok_or("Missing cache evidence")?
        {
            let file = artifact["file"].as_str().ok_or("Bad cache artifact")?;
            if Path::new(file).is_absolute()
                || Path::new(file)
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err("Cache artifact escapes operation".into());
            }
            verify(
                &previous.join(file),
                artifact["sha256"]
                    .as_str()
                    .ok_or("Bad cache artifact digest")?,
            )?;
        }
        let response = load(&previous.join("response.json"))?;
        verify_cached_import(&previous, &response)?;
        if response["cache_identity"] != cache_key
            || load(&previous.join("request.json"))?["request"] != request
        {
            return Err("Cached measurement belongs to a different logical request".into());
        }
        if plan.initial_extractor_observation {
            crate::initial_extractor::verify_observation(plan, &response)?;
        }
        if plan.selected_stage_observation {
            crate::selected_stage::verify_observation(plan, &response)?;
        }
        // The logical operation identity stays identical on deterministic replay.
        // Cache availability changes after the first run; it is execution detail.
        match journal.begin(plan.function.metric(), &request, 0, 0)? {
            Operation::Reused(value) => return Ok(value),
            Operation::Fresh(directory) => {
                keep(
                    &directory.join("cache-source.json"),
                    &json!({"cache_key":cache_key,
                    "original_operation":relative,"original_completed_sha256":index["completed_sha256"]}),
                )?;
                journal.finish(&directory, &response)?;
                return Ok(response);
            }
        }
    }
    if let Some((response, source)) = crate::curriculum::native_control(plan, &request)? {
        let directory = match journal.begin(plan.function.metric(), &request, 0, 0)? {
            Operation::Reused(value) => return Ok(value),
            Operation::Fresh(directory) => directory,
        };
        keep(
            &directory.join("cache-source.json"),
            &crate::curriculum::import_receipt(plan, &journal.root, &source)?,
        )?;
        journal.finish(&directory, &response)?;
        fs::create_dir_all(cache.parent().ok_or("No cache directory")?)
            .map_err(|e| e.to_string())?;
        keep(
            &cache,
            &json!({"operation":directory.strip_prefix(&journal.root).map_err(|e|e.to_string())?,
            "completed_sha256":digest(read(&directory.join("completed.json"))?)}),
        )?;
        return Ok(response);
    }
    if let Some(previous) = crate::recovery::native_source(plan, &request)? {
        let response = load(&previous.join("response.json"))?;
        if response["cache_identity"] != cache_key {
            return Err("Imported native response differs from its exact request identity".into());
        }
        let directory = match journal.begin(plan.function.metric(), &request, 0, 0)? {
            Operation::Reused(value) => return Ok(value),
            Operation::Fresh(directory) => directory,
        };
        keep(
            &directory.join("cache-source.json"),
            &crate::recovery::import_receipt(plan, &previous)?,
        )?;
        journal.finish(&directory, &response)?;
        fs::create_dir_all(cache.parent().ok_or("No cache directory")?)
            .map_err(|e| e.to_string())?;
        keep(
            &cache,
            &json!({"operation":directory.strip_prefix(&journal.root).map_err(|e|e.to_string())?,
            "completed_sha256":digest(read(&directory.join("completed.json"))?)}),
        )?;
        return Ok(response);
    }
    if !archival {
        if std::env::var_os("HORARY_GOOGLE_KEY_FILE").is_none() {
            return Err(
                "Fresh hosted calls require the invocation-only private credential locator".into(),
            );
        }
        if let Some(pid) = plan.wait_owner_pid {
            let output = Command::new("ps")
                .args(["-p", &pid.to_string(), "-o", "pid="])
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() && !output.stdout.is_empty() {
                return Err(format!("Existing evaluation owner PID {pid} still runs; fresh Gemma calls are withheld. Completed archival controls and reflection may be reused when resumed."));
            }
        }
        // Each native function has its own hosted client/token window. Leave
        // one full quota window between children, including after the previous
        // owner exits, so restarting clients cannot bypass token pacing.
        println!(
            "{}",
            json!({"event":"provider_quota_spacing","case":example.id,"milliseconds":65_000})
        );
        for _ in 0..65 {
            thread::sleep(Duration::from_secs(1));
        }
    }
    let reservation = if archival {
        0
    } else {
        plan.logical_calls_per_function
            .checked_mul(plan.function.generation_factor())
            .ok_or("Generation budget overflow")?
    };
    let directory = match journal.begin(plan.function.metric(), &request, 0, reservation)? {
        Operation::Reused(value) => return Ok(value),
        Operation::Fresh(path) => path,
    };
    let program_file = program.as_ref().map(|_| directory.join("program.json"));
    let memo_pin = if plan.exact_response_memoization {
        let index = crate::memo::snapshot(plan, journal, example)?;
        let file = directory.join("response-memo-index.json");
        keep(&file, &index)?;
        Some(horary_prompt_program::response_memo::Pin {
            sha256: digest(read(&file)?),
            file,
        })
    } else {
        None
    };
    if let Some(program) = &program {
        keep(program_file.as_ref().ok_or("No program path")?, program)?;
    }
    let fixed_file = if let Some(receipt) = &fixed_receipt {
        keep(&directory.join("fixed-program-composition.json"), receipt)?;
        Some(crate::fixed::write_source(plan, &directory)?)
    } else {
        None
    };
    let mut task = json!({"case_id":example.id,"source_case_directory":example.source_case_directory,
        "source_request_file":example.source_request_file,"source_request_sha256":example.source_request_sha256,
        "source_initial_sha256":example.source_initial_sha256,"baseline_manifest_sha256":plan.manifest_sha256,
        "program_file":program_file,"replay_completed_capture":archival});
    if plan.function != Function::Classification {
        task.as_object_mut()
            .ok_or("Task is not an object")?
            .remove("replay_completed_capture");
        task["target_method"] = json!(plan.target_method);
        task["source_origin_sha256"] = json!(example.source_origin_sha256);
        task["source_fixture_sha256"] = json!(example.source_fixture_sha256);
        if plan.initial_extractor_observation {
            task["initial_extractor_observation"] = json!(true);
        }
        if plan.function == Function::ReadingJourney {
            if let Some(pin) = &memo_pin {
                task["exact_response_memoization"] = json!(pin);
            }
            if plan.selected_stage_observation {
                task["selected_stage_observation"] = json!(true);
            }
            task["target_stage"] = json!(plan.target_stage);
            task["source_branch"] = json!(example.source_branch);
            task["source_rubric_sha256"] = json!(example.source_rubric_sha256);
            if let Some(file) = &fixed_file {
                task["fixed_program_file"] = json!(file);
                task["fixed_program_sha256"] =
                    json!(plan.fixed_program.as_ref().ok_or("No fixed source")?.sha256);
            }
        }
    }
    let task_file = directory.join("task.json");
    keep(&task_file, &task)?;
    let evidence = directory.join("native");
    let mut command = Command::new(&plan.native_executable);
    command
        .args([plan.function.entry(), "--exact", "--ignored", "--nocapture"])
        .env("HORARY_NEURAL_TASK", &task_file)
        .env("HORARY_EVAL_EVIDENCE", &evidence)
        .env(
            "HORARY_EVAL_MAX_CALLS",
            plan.logical_calls_per_function.to_string(),
        )
        .env(
            "HORARY_EVAL_CASE_SECONDS",
            plan.function_seconds.to_string(),
        )
        .env("HORARY_GOOGLE_INPUT_TPM", "14000")
        .env_remove("HORARY_NATIVE_LLAMA_TEST_MODEL")
        .env_remove("HORARY_EVAL_PROGRAM")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_API_KEY");
    if archival {
        command.env_remove("HORARY_GOOGLE_KEY_FILE");
    }
    execute(
        &mut command,
        &directory,
        plan.function_seconds.saturating_add(30),
    )?;
    let mut outcome = load(&evidence.join("outcome.json"))?;
    let native_directory = if plan.function != Function::Classification {
        evidence.join("cases").join(&example.id)
    } else {
        evidence.clone()
    };
    let actual_attempts = outcome["physical_generation_attempts"]
        .as_u64()
        .ok_or("Native physical attempt count is missing or unknown")?;
    if actual_attempts > reservation {
        return Err("Native physical attempt reservation exceeded".into());
    }
    let focused = focused_calls(plan, &native_directory)?;
    if plan.function == Function::InputJourney {
        let first = load(&native_directory.join("first-turn.json"))?;
        let invoked = input_invocation(
            plan.target_method
                .as_deref()
                .ok_or("Missing input target method")?,
            &outcome,
            &first,
            focused.len(),
        )?;
        outcome["target_function_invoked"] = json!(invoked);
        if !invoked {
            outcome["target_function_not_invoked_reason"] = json!("The observed upstream route did not select the optimized function; native grades are retained, but this target earns no fitness");
        }
    } else if plan.function == Function::ReadingJourney && !plan.selected_stage_observation {
        crate::reading::verify_invocation(plan, &outcome, &focused)?;
    }
    let mut fixed_invocations = None;
    if let Some(program) = &program {
        let mut applied = 0;
        let mut target_applied = 0;
        for entry in
            fs::read_dir(native_directory.join("calls")).map_err(|error| error.to_string())?
        {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.to_string_lossy().ends_with("-request.json") {
                let call = load(&path)?;
                if plan.fixed_program.is_some() {
                    let (all, target) = applied_branches(&call, &program.id, plan.signature());
                    applied += all;
                    target_applied += target;
                    continue;
                }
                if call["prompt_program"]["candidate_id"] == program.id
                    || call["prompt_program"].as_array().is_some_and(|branches| {
                        branches
                            .iter()
                            .any(|branch| branch["candidate_id"] == program.id)
                    })
                {
                    applied += 1;
                }
            }
        }
        if plan.fixed_program.is_some() {
            if candidate != &plan.seed && target_applied == 0 {
                return Err("Changed target teaching was not applied; prerequisite application cannot substitute for it".into());
            }
            let receipt = json!({"version":1,"execution_program_id":program.id,
                "fixed_applied_branches":applied-target_applied,
                "mutable_target_applied_branches":target_applied,
                "mutable_target_changed":candidate != &plan.seed,
                "qualification":"Observed application counts only; unused prerequisites and target semantic correctness are not inferred."});
            keep(&directory.join("fixed-program-invocations.json"), &receipt)?;
            fixed_invocations = Some(receipt);
        } else if applied == 0 {
            // An observed upstream misclassification cannot be fixed by
            // injecting gold to force the requested extraction method.
            if plan.function == Function::Classification
                || outcome["target_function_invoked"] == true
            {
                return Err(
                    "Changed teaching was not applied to the actually invoked target function"
                        .into(),
                );
            }
        }
    }
    let mut response = json!({"outcome":outcome,"final":load(&evidence.join("final.json"))?,"source":"actual native application executor",
        "native_evidence_directory":native_directory,
        "actual_function_calls":focused,
        "archival_control":archival,"cache_identity":cache_key});
    if plan.selected_stage_observation {
        crate::selected_stage::bind(plan, &mut response)?;
    }
    if let Some(receipt) = fixed_invocations {
        response["fixed_program_invocations"] = receipt;
        response["fixed_program_composition"] =
            fixed_receipt.ok_or("No fixed composition witness")?;
    }
    if plan.objective == Objective::ExtractorReliability {
        response["objective"] = json!(plan.objective);
        let first = load(&native_directory.join("first-turn.json"))?;
        response["initial_accepted_inputs"] = json!({
            "consultation":first["session"]["method"]["consultation"],
            "canonical_question":first["session"]["question"],
            "candidate_moment":first["session"]["candidateMomentMs"],
            "source_sha256":digest(read(&native_directory.join("first-turn.json"))?),
            "scope":"Initially accepted state; no supplying-turn checkpoint transplanted"});
        if plan.initial_extractor_observation {
            response["initial_accepted_inputs"] = json!({
                "state":response["outcome"]["initial_extractor"]["state"],
                "consultation":response["outcome"]["initial_extractor"]["accepted_consultation"],
                "scope":"Actual initial component boundary; rejected records contain no accepted consultation"});
            crate::initial_extractor::bind(plan, &mut response)?;
        }
    }
    journal.finish(&directory, &response)?;
    fs::create_dir_all(cache.parent().ok_or("No cache directory")?)
        .map_err(|error| error.to_string())?;
    keep(
        &cache,
        &json!({"operation":directory.strip_prefix(&journal.root).map_err(|error|error.to_string())?,"completed_sha256":digest(read(&directory.join("completed.json"))?)}),
    )?;
    Ok(response)
}

fn applied_branches(
    call: &Value,
    id: &str,
    target: horary_prompt_program::Signature<'_>,
) -> (u64, u64) {
    let branches: Vec<_> = call["prompt_program"].as_array().map_or_else(
        || vec![&call["prompt_program"]],
        |branches| branches.iter().collect(),
    );
    let mut all = 0;
    let mut mutable = 0;
    for branch in branches {
        if branch["candidate_id"] != id {
            continue;
        }
        all += 1;
        if branch["selector"]["stage"] == target.stage
            && branch["selector"]["recognition_phase"].as_str() == target.recognition_phase
            && branch["selector"]["method"].as_str() == target.method
        {
            mutable += 1;
        }
    }
    (all, mutable)
}

#[cfg(test)]
mod fixed_program_tests {
    use super::*;
    #[test]
    fn prerequisite_application_cannot_masquerade_as_mutable_target_application() {
        let target = horary_prompt_program::Signature {
            stage: "condition",
            recognition_phase: None,
            method: Some("movable_deal"),
        };
        let fixed = json!({"candidate_id":"combined","selector":{"stage":"intake","recognition_phase":"complete_selected_program","method":"movable_deal"}});
        let changed = json!({"candidate_id":"combined","selector":{"stage":"condition","recognition_phase":null,"method":"movable_deal"}});
        let other = json!({"candidate_id":"another-program","selector":{"stage":"condition","recognition_phase":null,"method":"movable_deal"}});
        assert_eq!(
            applied_branches(&json!({"prompt_program":fixed}), "combined", target),
            (1, 0)
        );
        assert_eq!(
            applied_branches(
                &json!({"prompt_program":[fixed,changed,other,null]}),
                "combined",
                target
            ),
            (2, 1)
        );
        assert_eq!(
            applied_branches(&json!({"prompt_program":null}), "combined", target),
            (0, 0)
        );
    }
}

fn input_invocation(target: &str, outcome: &Value, first: &Value, count: usize) -> Result<bool> {
    if outcome["target_signature_calls"].as_u64() != Some(count as u64) {
        return Err("Native target invocation count differs from the actual trace".into());
    }
    if count > 0 {
        return Ok(true);
    }
    if first
        .pointer("/session/method/consultation/frame/observation/value/method")
        .and_then(Value::as_str)
        == Some(target)
    {
        return Err("The selected target method never invoked its required extractor; preserve this native orchestration failure".into());
    }
    // A valid alternative method or unresolved classification is an observed
    // upstream route, not permission to inject the target/gold into the oracle.
    Ok(false)
}

fn focused_calls(plan: &Plan, directory: &Path) -> Result<Vec<Value>> {
    let initial_end = if plan.initial_extractor_observation {
        Some(
            load(&directory.join("initial-extractor-observation.json"))?["last_sequence"]
                .as_u64()
                .ok_or("Missing actual initial extractor boundary")?,
        )
    } else {
        None
    };
    let mut files = fs::read_dir(directory.join("calls"))
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    files.sort();
    let mut calls = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|f| f.to_str())
            .ok_or("Invalid call filename")?;
        if !name.ends_with("-request.json") {
            continue;
        }
        let call = load(&file)?;
        if let Some(last) = initial_end {
            let sequence = call["sequence"]
                .as_u64()
                .ok_or("Initial request lacks its actual sequence")?;
            if sequence > last {
                continue;
            }
        }
        if plan.function == Function::ReadingJourney {
            let result_name = name.replace("-request.json", "-result.json");
            let result = load(&directory.join("calls").join(result_name))?;
            for (branch, task) in crate::reading::matching_tasks(plan, &call)? {
                let observed_result = if let Some(branch) = branch {
                    if result["result"].get("Err").is_some() {
                        result["result"].clone()
                    } else {
                        json!({"Ok":result["result"]["Ok"].as_array()
                            .and_then(|results| results.get(branch))
                            .ok_or("Reading branch lacks its actual generation result")?})
                    }
                } else {
                    result["result"].clone()
                };
                let mut focused = json!({"stage":task["stage"],"branch":branch,"sequence":call["sequence"],
                    "input":task["input"],"schema":task["schema"],"guide_sha256":task["guide_sha256"],
                    "prompt_program":task["prompt_program"],"result":observed_result,
                    "source_request_file":format!("calls/{name}"),"source_request_sha256":digest(read(&file)?),
                    "source_result_file":format!("calls/{}",name.replace("-request.json","-result.json")),
                    "source_result_sha256":digest(read(&directory.join("calls").join(name.replace("-request.json","-result.json")))?)});
                if let Some(protocol) = task.get("executor_guide_identity") {
                    focused["executor_guide_identity"] = protocol.clone();
                }
                if plan.selected_stage_observation {
                    focused["schema_sha256"] = task["schema_sha256"].clone();
                }
                calls.push(focused);
            }
            continue;
        }
        let input = horary_prompt_program::original_input(&call["input"]);
        let scope = horary_prompt_program::signature("intake", input);
        let desired = plan.signature();
        if call["stage"] == "intake"
            && scope.recognition_phase == desired.recognition_phase
            && scope.method == desired.method
        {
            let result_name = name.replace("-request.json", "-result.json");
            let result = load(&directory.join("calls").join(result_name))?;
            let mut focused = json!({"input":call["input"],"schema":call["schema"],"guide_sha256":call["guide_sha256"],
                "prompt_program":call["prompt_program"],"result":result["result"],"source_request_sha256":digest(read(&file)?)});
            if let Some(protocol) = call.get("executor_guide_identity") {
                horary_prompt_program::captured_record_guide_sha256(&call, None)?;
                focused["executor_guide_identity"] = protocol.clone();
            }
            if plan.initial_extractor_observation {
                focused["sequence"] = call["sequence"].clone();
                focused["source_request_file"] = json!(format!("calls/{name}"));
                focused["source_result_file"] = json!(format!(
                    "calls/{}",
                    name.replace("-request.json", "-result.json")
                ));
                focused["source_result_sha256"] = json!(digest(read(
                    &directory
                        .join("calls")
                        .join(name.replace("-request.json", "-result.json"))
                )?));
            }
            calls.push(focused);
        }
    }
    Ok(calls)
}

pub fn inspect(plan: &Plan, state: &Path, example: &Example) -> Result<std::path::PathBuf> {
    let parent = state.join("inspections");
    fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
    let directory = parent.join(&example.id);
    fs::create_dir(&directory)
        .map_err(|error| format!("Native inspection must be fresh: {error}"))?;
    let evidence = directory.join("native");
    let task_file = directory.join("task.json");
    let mut task = json!({"case_id":example.id,"source_case_directory":example.source_case_directory,
        "source_request_file":example.source_request_file,"source_request_sha256":example.source_request_sha256,
        "source_initial_sha256":example.source_initial_sha256,"baseline_manifest_sha256":digest(read(&plan.campaign.join("manifest.json"))?),
        "inspect_prompt_only":true});
    if plan.function != Function::Classification {
        task["target_method"] = json!(plan.target_method);
        task["source_origin_sha256"] = json!(example.source_origin_sha256);
        task["source_fixture_sha256"] = json!(example.source_fixture_sha256);
        if plan.initial_extractor_observation {
            task["initial_extractor_observation"] = json!(true);
        }
        if plan.function == Function::ReadingJourney {
            task["target_stage"] = json!(plan.target_stage);
            task["source_branch"] = json!(example.source_branch);
            task["source_rubric_sha256"] = json!(example.source_rubric_sha256);
        }
    }
    keep(&task_file, &task)?;
    let mut command = Command::new(&plan.native_executable);
    command
        .args([plan.function.entry(), "--exact", "--ignored", "--nocapture"])
        .env("HORARY_NEURAL_TASK", &task_file)
        .env("HORARY_EVAL_EVIDENCE", &evidence);
    horary_loop::remove_provider_credentials(&mut command);
    command.env_remove("HORARY_NATIVE_LLAMA_TEST_MODEL");
    execute(&mut command, &directory, 60)?;
    let file = evidence.join("inspection.json");
    let inspection = load(&file)?;
    let scope = match plan.function {
        Function::Classification => "classification_prompt_inspection_only",
        Function::InputJourney => "input_journey_prompt_inspection_only",
        Function::ReadingJourney => "reading_journey_prompt_inspection_only",
    };
    if inspection["scope"] != scope
        || inspection["case_id"] != example.id
        || inspection["model_constructed"] != false
        || inspection["new_generation_attempts"] != 0
    {
        return Err("Native prompt inspection has an invalid execution boundary".into());
    }
    Ok(file)
}

/// Reserve and retain submission identity before spawning. An interrupted
/// process remains a partial operation; callers never automatically replay it.
pub fn execute(command: &mut Command, directory: &Path, seconds: u64) -> Result<()> {
    let stdout = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("stdout.log"))
        .map_err(|error| error.to_string())?;
    let stderr = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("stderr.log"))
        .map_err(|error| error.to_string())?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    keep(
        &directory.join("prepared.json"),
        &json!({"executable":command.get_program(),"args":command.get_args().collect::<Vec<_>>(),"deadline_seconds":seconds}),
    )?;
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    record_submission(&mut child, directory)?;
    wait(&mut child, directory, seconds)
}
pub fn wait(child: &mut std::process::Child, directory: &Path, seconds: u64) -> Result<()> {
    let start = Instant::now();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                return abort_owned_child(
                    child,
                    directory,
                    &format!("Child status failed: {error}"),
                )
            }
        };
        if let Some(status) = status {
            keep(
                &directory.join("exit.json"),
                &json!({"code":status.code(),"success":status.success()}),
            )?;
            return if status.success() {
                Ok(())
            } else {
                Err(format!("Child failed; retain {}", directory.display()))
            };
        }
        if start.elapsed().as_secs() > seconds {
            return abort_owned_child(
                child,
                directory,
                "Child deadline; preserve partial without resubmission",
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn teacher_process(
    command: &mut Command,
    directory: &Path,
    prompt: &[u8],
    seconds: u64,
) -> Result<()> {
    let stdout = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("events.jsonl"))
        .map_err(|error| error.to_string())?;
    let stderr = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("stderr.log"))
        .map_err(|error| error.to_string())?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    record_submission(&mut child, directory)?;
    let Some(mut stdin) = child.stdin.take() else {
        return abort_owned_child(&mut child, directory, "No teacher stdin");
    };
    let bytes = prompt.to_vec();
    let writer = thread::spawn(move || stdin.write_all(&bytes));
    let result = wait(&mut child, directory, seconds);
    writer
        .join()
        .map_err(|_| "Teacher prompt writer panicked")?
        .map_err(|error| error.to_string())?;
    result
}

fn record_submission(child: &mut std::process::Child, directory: &Path) -> Result<()> {
    match keep(
        &directory.join("submitted.json"),
        &json!({"pid":child.id()}),
    ) {
        Ok(()) => Ok(()),
        Err(error) => abort_owned_child(
            child,
            directory,
            &format!("Submission receipt failed: {error}"),
        ),
    }
}

fn abort_owned_child(
    child: &mut std::process::Child,
    directory: &Path,
    reason: &str,
) -> Result<()> {
    let kill_error = child.kill().err().map(|error| error.to_string());
    let reaped = child
        .wait()
        .map(|status| json!({"code":status.code(),"success":status.success()}));
    let receipt = keep(
        &directory.join("interrupted.json"),
        &json!({"reason":reason,
        "pid":child.id(),"kill_error":kill_error,"reaped":reaped.as_ref().ok(),
        "reap_error":reaped.as_ref().err().map(|error|error.to_string()),"uncertain_requests_may_exist":true}),
    );
    Err(format!(
        "{reason}; owned child termination recorded at {}{}",
        directory.display(),
        receipt.err().map_or(String::new(), |error| format!(
            "; interruption receipt also failed: {error}"
        ))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn later_component_cache_reuse_rechecks_original_closure_pins() {
        let source = tempfile::tempdir().unwrap();
        let cached = tempfile::tempdir().unwrap();
        let plan = source.path().join("plan.json");
        fs::write(&plan, "{}").unwrap();
        keep(&cached.path().join("request.json"), &json!({})).unwrap();
        keep(&cached.path().join("cache-source.json"), &json!({
            "type":crate::controls::COMPONENT_IMPORT,"source_operation":"operations/00000",
            "new_paid_reservations":0,"source_state":source.path(),"source_plan_sha256":digest("{}")
        })).unwrap();
        fs::write(&plan, "{\"closure_drift\":true}").unwrap();
        let error = verify_cached_import(cached.path(), &json!({})).unwrap_err();
        assert!(error.contains("Source changed"), "{error}");
        assert!(error.contains("plan.json"), "{error}");
        // This check precedes reserving/submitting a new operation or returning
        // a cached score; it requires no provider key or native child.
        assert!(!cached.path().join("reservation.json").exists());
    }
    #[test]
    fn focused_capture_retains_explicit_effective_identity_without_changing_legacy_fields() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("calls")).unwrap();
        let input = json!({"recognition_phase":"complete_selected_program","consultation":{"frame":{"state":"resolved",
            "observation":{"value":{"method":"movable_deal"}}}}});
        let path = dir.path().join("calls/0001-request.json");
        let mut request = json!({"sequence":1,"stage":"intake","input":input,"schema":{},
            "prompt":[{"role":"system","content":"Actual synthetic teaching"}],
            "guide_sha256":digest("Actual synthetic teaching"),
            "prompt_program":{"original_guide_sha256":digest("Original synthetic teaching"),
                "replacement_guide_sha256":digest("Actual synthetic teaching")}});
        keep(&path, &request).unwrap();
        keep(
            &dir.path().join("calls/0001-result.json"),
            &json!({"result":{"Ok":{"content":"authored"}}}),
        )
        .unwrap();
        let mut plan = crate::tests::plan();
        plan.function = Function::InputJourney;
        plan.target_method = Some("movable_deal".into());
        let legacy = focused_calls(&plan, dir.path()).unwrap().remove(0);
        assert!(legacy.get("executor_guide_identity").is_none());
        request["executor_guide_identity"] = json!(horary_prompt_program::EFFECTIVE_GUIDE_CAPTURE);
        fs::write(&path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
        let mut current = focused_calls(&plan, dir.path()).unwrap().remove(0);
        assert_eq!(
            current["executor_guide_identity"],
            horary_prompt_program::EFFECTIVE_GUIDE_CAPTURE
        );
        current
            .as_object_mut()
            .unwrap()
            .remove("executor_guide_identity");
        current["source_request_sha256"] = legacy["source_request_sha256"].clone();
        assert_eq!(current, legacy);
        request["executor_guide_identity"] = json!("unknown-future-protocol");
        fs::write(&path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
        assert!(focused_calls(&plan, dir.path()).is_err());
    }

    #[test]
    fn initial_focused_count_excludes_later_supply_but_legacy_keeps_its_trace() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("calls")).unwrap();
        let input = json!({"recognition_phase":"complete_selected_program","consultation":{"frame":{"state":"resolved",
            "observation":{"value":{"method":"movable_deal"}}}}});
        for sequence in [2, 5] {
            keep(&dir.path().join(format!("calls/{sequence:04}-request.json")),&json!({"sequence":sequence,"stage":"intake","input":input,"schema":{},"guide_sha256":"0".repeat(64),"prompt_program":null})).unwrap();
            keep(
                &dir.path().join(format!("calls/{sequence:04}-result.json")),
                &json!({"result":{"Ok":{"content":"authored"}}}),
            )
            .unwrap();
        }
        keep(
            &dir.path().join("initial-extractor-observation.json"),
            &json!({"last_sequence":2}),
        )
        .unwrap();
        let mut plan = crate::tests::plan();
        plan.function = Function::InputJourney;
        plan.target_method = Some("movable_deal".into());
        assert_eq!(focused_calls(&plan, dir.path()).unwrap().len(), 2);
        plan.initial_extractor_observation = true;
        let calls = focused_calls(&plan, dir.path()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["sequence"], 2);
        assert_eq!(calls[0]["source_request_file"], "calls/0002-request.json");
    }
    #[test]
    fn alternate_routes_are_observed_but_missing_target_dispatch_is_an_error() {
        let outcome =
            json!({"target_signature_calls":0,"hurdles":{"classification":{"status":"pass"}}});
        let mut first = json!({"session":{"method":{"consultation":{"frame":{"state":"resolved",
            "observation":{"value":{"method":"investment"}}}}}}});
        assert!(!input_invocation("movable_deal", &outcome, &first, 0).unwrap());
        first["session"]["method"]["consultation"]["frame"]["observation"]["value"]["method"] =
            json!("movable_deal");
        assert!(input_invocation("movable_deal", &outcome, &first, 0)
            .unwrap_err()
            .contains("orchestration"));
        assert!(input_invocation("movable_deal", &outcome, &first, 1)
            .unwrap_err()
            .contains("actual trace"));
        let invoked = json!({"target_signature_calls":1});
        assert!(input_invocation("movable_deal", &invoked, &first, 1).unwrap());
    }
    #[test]
    fn failed_submission_receipt_terminates_and_reaps_its_owned_child() {
        let directory = tempfile::tempdir().unwrap();
        keep(
            &directory.path().join("submitted.json"),
            &json!({"pid":"different"}),
        )
        .unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "read unused_value"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let error = record_submission(&mut child, directory.path()).unwrap_err();
        assert!(error.contains("Submission receipt failed"));
        assert!(child.try_wait().unwrap().is_some());
        let receipt = load(&directory.path().join("interrupted.json")).unwrap();
        assert!(receipt["reaped"].is_object());
        assert_eq!(receipt["uncertain_requests_may_exist"], true);
    }
}
