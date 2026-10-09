//! Actual native neural functions; no local inference or downstream capture transplant.
use crate::{
    journal::{Journal, Operation},
    keep, load, read, verify, ControlMode, Example, Function, Plan, Result,
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
    let request = json!({"case":example,"candidate":candidate,"native_executable_sha256":plan.native_executable_sha256,
        "manifest_sha256":plan.manifest_sha256,"function":plan.function,"target_method":plan.target_method,
        "scope":plan.function.metric(),"archival_control":archival});
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
        if response["cache_identity"] != cache_key
            || load(&previous.join("request.json"))?["request"] != request
        {
            return Err("Cached measurement belongs to a different logical request".into());
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
    if let Some(program) = &program {
        keep(program_file.as_ref().ok_or("No program path")?, program)?;
    }
    let mut task = json!({"case_id":example.id,"source_case_directory":example.source_case_directory,
        "source_request_file":example.source_request_file,"source_request_sha256":example.source_request_sha256,
        "source_initial_sha256":example.source_initial_sha256,"baseline_manifest_sha256":plan.manifest_sha256,
        "program_file":program_file,"replay_completed_capture":archival});
    if plan.function == Function::InputJourney {
        task.as_object_mut()
            .ok_or("Task is not an object")?
            .remove("replay_completed_capture");
        task["target_method"] = json!(plan.target_method);
        task["source_origin_sha256"] = json!(example.source_origin_sha256);
        task["source_fixture_sha256"] = json!(example.source_fixture_sha256);
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
    let native_directory = if plan.function == Function::InputJourney {
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
    }
    if let Some(program) = &program {
        let mut applied = 0;
        for entry in
            fs::read_dir(native_directory.join("calls")).map_err(|error| error.to_string())?
        {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.to_string_lossy().ends_with("-request.json") {
                let call = load(&path)?;
                if call["prompt_program"]["candidate_id"] == program.id {
                    applied += 1;
                }
            }
        }
        if applied == 0 {
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
    let response = json!({"outcome":outcome,"final":load(&evidence.join("final.json"))?,"source":"actual native application executor",
        "native_evidence_directory":native_directory,
        "actual_function_calls":focused,
        "archival_control":archival,"cache_identity":cache_key});
    journal.finish(&directory, &response)?;
    fs::create_dir_all(cache.parent().ok_or("No cache directory")?)
        .map_err(|error| error.to_string())?;
    keep(
        &cache,
        &json!({"operation":directory.strip_prefix(&journal.root).map_err(|error|error.to_string())?,"completed_sha256":digest(read(&directory.join("completed.json"))?)}),
    )?;
    Ok(response)
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
        let input = horary_prompt_program::original_input(&call["input"]);
        let scope = horary_prompt_program::signature("intake", input);
        let desired = plan.signature();
        if call["stage"] == "intake"
            && scope.recognition_phase == desired.recognition_phase
            && scope.method == desired.method
        {
            let result_name = name.replace("-request.json", "-result.json");
            let result = load(&directory.join("calls").join(result_name))?;
            calls.push(json!({"input":call["input"],"schema":call["schema"],"guide_sha256":call["guide_sha256"],
                "prompt_program":call["prompt_program"],"result":result["result"],"source_request_sha256":digest(read(&file)?)}));
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
    if plan.function == Function::InputJourney {
        task["target_method"] = json!(plan.target_method);
        task["source_origin_sha256"] = json!(example.source_origin_sha256);
        task["source_fixture_sha256"] = json!(example.source_fixture_sha256);
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
    let scope = if plan.function == Function::Classification {
        "classification_prompt_inspection_only"
    } else {
        "input_journey_prompt_inspection_only"
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
