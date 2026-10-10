//! Seal a fresh, closed native campaign without inventing recovery ancestry.
//!
//! The launcher writes `submitted.json` before model work, then writes
//! `owner-exit.json` after waiting for its native child. Both use RECEIPT_VERSION
//! and kind `fresh_owned_native_hosted`. Submitted fields are campaign,
//! owner_pid, native_pid, selected_ids, native_executable,
//! native_executable_sha256 and automatic_resubmission=false. Exit fields are
//! campaign, the same PIDs, submitted_sha256, native_exit_code and
//! automatic_resubmission=false. Native exit 101 is valid only with the native
//! completed marker, all selected outcomes and fully settled provider calls.
//!
//! Before sealing, the launcher writes a CORPUS_VERSION closure with campaign,
//! submitted_sha256, owner_exit_sha256, native_executable and its SHA256,
//! `files` (all required root witnesses) and `case_files` (every selected case's
//! complete ordinary-file tree). The closure never asserts semantic success.
//! No receipt or captured outcome is modified here. An entire origins directory
//! appears atomically only after all witnesses pass; repeating the same seal is
//! idempotent. A changed, uncertain or still-running campaign is rejected.
#![forbid(unsafe_code)]

use crate::{load, read, Result};
use horary_prompt_program::{digest, original_input};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

pub const RECEIPT_VERSION: &str = "horary-fresh-native-campaign-2026-10-10.1";
pub const CORPUS_VERSION: &str = "horary-fresh-native-corpus-2026-10-10.1";
const KIND: &str = "fresh_owned_native_hosted";
const MODEL: &str = "gemma-4-26b-a4b-it";
const SEMANTIC_STOP: &str =
    "Synthetic case call budget exhausted; no successful completion implied";
type Hashes = BTreeMap<String, String>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submitted {
    version: String,
    kind: String,
    campaign: PathBuf,
    owner_pid: u32,
    native_pid: u32,
    selected_ids: Vec<String>,
    native_executable: PathBuf,
    native_executable_sha256: String,
    automatic_resubmission: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exit {
    version: String,
    kind: String,
    campaign: PathBuf,
    owner_pid: u32,
    native_pid: u32,
    submitted_sha256: String,
    native_exit_code: i32,
    automatic_resubmission: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Closure {
    version: String,
    campaign: PathBuf,
    submitted_sha256: String,
    owner_exit_sha256: String,
    native_executable: PathBuf,
    native_executable_sha256: String,
    files: Hashes,
    case_files: BTreeMap<String, Hashes>,
}

fn ordinary(path: &Path, directory: bool) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err("Fresh corpus paths must be absolute".into());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if !matches!(
            part,
            Component::RootDir | Component::Prefix(_) | Component::Normal(_)
        ) {
            return Err("Fresh corpus path contains an alias or traversal".into());
        }
        current.push(part);
        let metadata = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
        if metadata.is_symlink() {
            return Err(format!("Symlinked corpus evidence: {}", current.display()));
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(format!("Nonordinary corpus artifact: {}", path.display()));
    }
    Ok(path.to_owned())
}

fn pinned(path: &Path, sha: &str) -> Result<()> {
    ordinary(path, false)?;
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || digest(read(path)?) != sha
    {
        return Err(format!(
            "Changed or invalid corpus seal: {}",
            path.display()
        ));
    }
    Ok(())
}

fn absent(pid: u32) -> Result<()> {
    if pid == 0 {
        return Err("Invalid fresh-campaign PID".into());
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.stdout.is_empty() || output.status.code() != Some(1) {
        return Err(format!(
            "Campaign owner/child PID {pid} is not proven absent"
        ));
    }
    Ok(())
}

fn tree(root: &Path) -> Result<Hashes> {
    fn visit(base: &Path, directory: &Path, files: &mut Hashes) -> Result<()> {
        ordinary(directory, true)?;
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let kind = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if kind.is_dir() {
                visit(base, &path, files)?;
            } else {
                ordinary(&path, false)?;
                let name = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Non-UTF8 corpus path")?
                    .replace('\\', "/");
                files.insert(name, digest(read(&path)?));
            }
        }
        Ok(())
    }
    let mut files = Hashes::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

// Native hashes compact serialized typed fixtures/rubrics before pretty writing
// them. Remove only JSON whitespace, retaining native key order and string bytes.
fn compact_hash(path: &Path) -> Result<String> {
    let bytes = read(path)?;
    let _: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let (mut quoted, mut escaped) = (false, false);
    let mut compact = Vec::with_capacity(bytes.len());
    for byte in bytes {
        if quoted || !byte.is_ascii_whitespace() {
            compact.push(byte);
        }
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        }
    }
    Ok(digest(compact))
}

fn wire(prompt: &Value, config: &Value) -> Result<Value> {
    let config_keys = config
        .as_object()
        .ok_or("Missing Google generation config")?;
    if config_keys
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from(["temperature", "seed", "maxOutputTokens", "thinkingConfig"])
        || config["temperature"] != 0
        || config["seed"] != 0
        || config["maxOutputTokens"].as_u64().is_none_or(|n| n == 0)
        || config["thinkingConfig"] != json!({"thinkingLevel":"minimal"})
    {
        return Err("Google corpus decoding is not the captured unconstrained contract".into());
    }
    let mut system = Vec::new();
    let mut contents = Vec::new();
    for message in prompt
        .as_array()
        .ok_or("Captured prompt is not a message array")?
    {
        let text = message["content"]
            .as_str()
            .ok_or("Captured prompt lacks text")?;
        match message["role"].as_str() {
            Some("system") if contents.is_empty() => system.push(json!({"text":text})),
            Some("user") => contents.push(json!({"role":"user","parts":[{"text":text}]})),
            Some("assistant") => contents.push(json!({"role":"model","parts":[{"text":text}]})),
            _ => return Err("Unsupported captured Google message role".into()),
        }
    }
    if system.is_empty() || contents.is_empty() {
        return Err("Google call lacks actual system teaching or user input".into());
    }
    Ok(json!({"systemInstruction":{"parts":system},"contents":contents,"generationConfig":config}))
}

fn settled_calls(directory: &Path, manifest: &Value, outcome: &Value) -> Result<usize> {
    let calls = directory.join("calls");
    ordinary(&calls, true)?;
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&calls).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        ordinary(&path, false)?;
        names.insert(
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid native call filename")?
                .to_owned(),
        );
    }
    let count = outcome["model_calls"]
        .as_u64()
        .ok_or("Missing actual native call count")?;
    if count == 0
        || count > manifest["case_max_calls"].as_u64().unwrap_or(0)
        || names.len() != count as usize * 2
    {
        return Err("Native calls are absent, partial or outside their bound".into());
    }
    let mut physical = 0;
    for sequence in 1..=count {
        let request_name = format!("{sequence:04}-request.json");
        let result_name = format!("{sequence:04}-result.json");
        if !names.contains(&request_name) || !names.contains(&result_name) {
            return Err("Native request/result pairs are incomplete".into());
        }
        let request = load(&calls.join(request_name))?;
        let result = load(&calls.join(result_name))?;
        if request["sequence"] != sequence
            || result["request"] != request
            || request["provider"] != manifest["model"]
            || !request["decoder"]
                .as_str()
                .is_some_and(|s| s.contains("unconstrained"))
            || result["result"].get("Err").is_some()
        {
            return Err("Native call identity or settled result is inconsistent".into());
        }
        let (prompts, outputs, receipts) = if let Some(tasks) = request["tasks"].as_array() {
            let prompts = request["prompts"]
                .as_array()
                .ok_or("Missing parallel prompts")?;
            let outputs = result["result"]["Ok"]
                .as_array()
                .ok_or("Unsettled native batch")?;
            let receipts = result["provider_receipts"]
                .as_array()
                .ok_or("Missing parallel Google receipts")?;
            if tasks.is_empty()
                || tasks.len() > 4
                || prompts.len() != tasks.len()
                || outputs.len() != tasks.len()
                || receipts.len() != tasks.len()
            {
                return Err("Parallel native branches are incomplete".into());
            }
            (
                prompts.iter().collect::<Vec<_>>(),
                outputs.iter().collect::<Vec<_>>(),
                receipts.iter().collect::<Vec<_>>(),
            )
        } else {
            if !request["stage"].is_string() {
                return Err("Native single task lacks its stage".into());
            }
            (
                vec![&request["prompt"]],
                vec![result["result"].get("Ok").ok_or("Unsettled native call")?],
                vec![&result["provider_receipt"]],
            )
        };
        for ((prompt, output), receipt) in prompts.iter().zip(outputs).zip(receipts) {
            if receipt["submitted"] != true
                || receipt["provider"] != "google_gemini_api"
                || receipt["model"] != MODEL
                || receipt["response"]["http_status"] != 200
                || receipt["native_result"] != json!({"Ok":output})
                || receipt["request"] != wire(prompt, &receipt["request"]["generationConfig"])?
                || receipt["token_count_response"]["http_status"] != 200
            {
                return Err("Google response is uncertain, constrained or mismatched".into());
            }
            let attempts = receipt["generation_attempts"]
                .as_array()
                .filter(|a| !a.is_empty() && a.len() <= 3)
                .ok_or("Missing settled physical attempts")?;
            for (index, attempt) in attempts.iter().enumerate() {
                let status = attempt["http_status"].as_u64();
                if attempt["attempt"].as_u64() != Some(index as u64 + 1)
                    || attempt["submitted"] != true
                    || !attempt["error"].is_null()
                    || if index + 1 == attempts.len() {
                        status != Some(200) || attempt["body"] != receipt["response"]["body"]
                    } else {
                        !matches!(status, Some(500 | 502 | 503 | 504))
                    }
                {
                    return Err(
                        "Uncertain or unsupported provider attempt remains in the corpus".into(),
                    );
                }
            }
            let candidates = receipt["response"]["body"]["candidates"]
                .as_array()
                .filter(|c| c.len() == 1)
                .ok_or("Provider delivered no unique output")?;
            if !matches!(
                candidates[0]["finishReason"].as_str(),
                Some("STOP" | "MAX_TOKENS")
            ) {
                return Err("Provider did not settle an answer".into());
            }
            let mut content = String::new();
            for part in candidates[0]["content"]["parts"]
                .as_array()
                .ok_or("Missing provider answer parts")?
            {
                if part["thought"] != true {
                    content.push_str(part["text"].as_str().ok_or("Non-text provider answer")?);
                }
            }
            if content.trim().is_empty() || output["content"] != content {
                return Err("Decoded native output differs from the actual provider answer".into());
            }
            physical += attempts.len();
        }
    }
    if outcome["hosted_http_requests"].as_u64() != Some(physical as u64) {
        return Err("Physical hosted attempt count disagrees with retained receipts".into());
    }
    Ok(physical)
}

fn question_state(snapshot: &Value, words: &[&str]) -> Result<()> {
    let users = snapshot["session"]["messages"]
        .as_array()
        .ok_or("Native state has no conversation")?
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| {
            m["text"]
                .as_str()
                .ok_or("Native user message lacks actual words")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if users != words {
        return Err("Original question or legitimately executed supplying words changed".into());
    }
    Ok(())
}

fn semantic_stop(snapshot: &Value, error: &Value) -> bool {
    if *error != SEMANTIC_STOP {
        return false;
    }
    let Some(record) = snapshot["session"]["method"]["records"]
        .as_array()
        .and_then(|r| r.last())
    else {
        return false;
    };
    record["validationError"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        && record["inputSha256"]
            .as_str()
            .is_some_and(|s| s.len() == 64)
        && snapshot["session"]["method"]["flow"]["jobs"]
            .as_array()
            .is_some_and(|jobs| {
                jobs.iter().any(|job| {
                    job["stage"] == record["stage"]
                        && job["revision"] == record["revision"]
                        && job["inputSha256"] == record["inputSha256"]
                        && job["phase"] == json!({"state":"paused","error":SEMANTIC_STOP})
                })
            })
}

fn case(
    directory: &Path,
    fixture: &Value,
    outcome: &Value,
    manifest: &Value,
    rubric: Option<&Value>,
) -> Result<()> {
    let id = fixture["id"]
        .as_str()
        .ok_or("Missing source fixture identity")?;
    if load(&directory.join("fixture.json"))? != *fixture
        || outcome["id"] != id
        || outcome["method"] != fixture["method"]
        || outcome["mode"] != fixture["mode"]
        || outcome["full_reading"] != manifest["full_reading"]
        || outcome["decoder_mode"] != "hosted_unconstrained_text"
        || outcome["deadline_cancelled"] != false
        || outcome["group_cancelled"] != false
        || !outcome["provider_stop"].is_null()
        || !outcome["infrastructure_error"].is_null()
    {
        return Err(format!(
            "Case {id} has changed identity or unsettled execution"
        ));
    }
    if let Some(rubric) = rubric {
        if load(&directory.join("reading-rubric.json"))? != *rubric
            || rubric["case_id"] != id
            || rubric["declared_method"] != fixture["method"]
        {
            return Err(format!(
                "Case {id} does not retain the exact root source rubric"
            ));
        }
    } else if fs::symlink_metadata(directory.join("reading-rubric.json")).is_ok() {
        return Err("Case has an unbound source rubric".into());
    }
    let first = load(&directory.join("first-turn.json"))?;
    let final_state = load(&directory.join("final.json"))?;
    let initial = load(&directory.join("initial.json"))?;
    let words = fixture["words"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("No original fixture question")?;
    question_state(&initial, &[words])?;
    question_state(&first, &[words])?;
    let session = &initial["session"];
    if session["messages"] != json!([{"role":"user","text":words}])
        || session["question"] != ""
        || !session["chart"].is_null()
        || !session["place"].is_null()
        || !session["candidateMomentMs"].is_null()
        || !session["method"]["consultation"].is_null()
        || !session["method"]["result"].is_null()
        || session["method"]["records"] != json!([])
        || session["sections"] != json!([])
        || session["facts"] != json!([])
    {
        return Err("Initial native state contains transplanted reading or input facts".into());
    }
    let device = fixture["device_available"].as_bool().unwrap_or(true);
    if initial["candidates"]
        != if device {
            json!([manifest["device"]])
        } else {
            json!([])
        }
        || session["deviceContext"]["timezone"] != manifest["frozen_clock"]["timezone"]
    {
        return Err("Initial source device context differs from the native manifest".into());
    }
    let supplying = outcome["follow_up"]["status"]
        .as_str()
        .is_some_and(|s| s.starts_with("executed after "));
    if outcome["follow_up_scripted"] != json!(fixture["follow_up"].is_string())
        || final_state["follow_up"] != outcome["follow_up"]
    {
        return Err("Native supplying-turn identity is inconsistent".into());
    }
    let mut source_words = vec![words];
    if supplying {
        let supplied = fixture["follow_up"]
            .as_str()
            .ok_or("Executed supplying turn lacks source words")?;
        if outcome["follow_up"]["words"] != supplied {
            return Err("Native supplying turn changed its authored words".into());
        }
        source_words.push(supplied);
    }
    question_state(&final_state, &source_words)?;
    let first_completed = outcome["first_turn_execution_completed"] == true;
    let follow_completed = &outcome["follow_up_execution_completed"];
    if (first_completed && first["result"].get("Ok").is_none())
        || (!first_completed && !semantic_stop(&first, &first["result"]["Err"]))
        || (supplying
            && follow_completed.as_bool() == Some(true)
            && final_state["follow_up"]["result"].get("Ok").is_none())
        || (supplying
            && follow_completed.as_bool() != Some(true)
            && !semantic_stop(&final_state, &final_state["follow_up"]["result"]["Err"]))
        || (!supplying && !follow_completed.is_null())
        || !matches!(
            outcome["execution_status"].as_str(),
            Some("completed" | "call_budget_exhausted" | "observed_native_validation_exhaustion")
        )
    {
        return Err("Native terminal outcome is an unknown interruption, not a settled semantic observation".into());
    }
    settled_calls(directory, manifest, outcome)?;
    // Inspect the native record, never synthesize normalized gold or a score.
    if outcome["known_native_semantic_abort"] == true {
        let witness = &outcome["semantic_abort_witness"];
        let snapshot = match witness["snapshot"].as_str() {
            Some("first-turn.json") => &first,
            Some("final.json") => &final_state,
            _ => return Err("Known semantic abort lacks its bound native snapshot".into()),
        };
        let records = snapshot["session"]["method"]["records"]
            .as_array()
            .ok_or("No native semantic records")?;
        let record = records.last().ok_or("No native semantic rejection")?;
        if witness["record_index"].as_u64() != Some(records.len() as u64 - 1)
            || witness["stage"] != record["stage"]
            || witness["revision"] != record["revision"]
            || witness["validation_error"] != record["validationError"]
            || witness["original_input"] != *original_input(&record["input"])
            || witness["input_sha256"] != record["inputSha256"]
            || witness["stop"] != SEMANTIC_STOP
            || !snapshot["session"]["method"]["flow"]["jobs"]
                .as_array()
                .is_some_and(|jobs| jobs.contains(&witness["paused_job"]))
            || witness["all_provider_responses_settled"] != true
            || witness["request_result_pairs_complete"] != true
        {
            return Err("Known semantic abort differs from its actual native rejection".into());
        }
    }
    Ok(())
}

fn verify_pins(
    campaign: &Path,
    closure_path: &Path,
    closure_sha: &str,
    closure: &Closure,
    submitted: &Submitted,
) -> Result<()> {
    pinned(closure_path, closure_sha)?;
    pinned(&campaign.join("submitted.json"), &closure.submitted_sha256)?;
    pinned(
        &campaign.join("owner-exit.json"),
        &closure.owner_exit_sha256,
    )?;
    pinned(
        &closure.native_executable,
        &closure.native_executable_sha256,
    )?;
    for (file, sha) in &closure.files {
        if Path::new(file)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("Root closure witness escapes its campaign".into());
        }
        pinned(&campaign.join(file), sha)?;
    }
    for (id, hashes) in &closure.case_files {
        if !safe_id(id) || tree(&campaign.join("cases").join(id))? != *hashes {
            return Err(format!("Changed or partial case closure: {id}"));
        }
    }
    absent(submitted.owner_pid)?;
    absent(submitted.native_pid)
}

/// Materialize compatible `case-origins` for one fresh closed hosted campaign.
/// This is evidence sealing only: no inference, recovery, score or truth claim.
pub fn seal(campaign: &Path, closure_path: &Path) -> Result<Value> {
    let campaign = ordinary(campaign, true)?;
    let closure_path = ordinary(closure_path, false)?;
    let closure_sha = digest(read(&closure_path)?);
    let closure: Closure =
        serde_json::from_value(load(&closure_path)?).map_err(|e| e.to_string())?;
    let submitted: Submitted =
        serde_json::from_value(load(&ordinary(&campaign.join("submitted.json"), false)?)?)
            .map_err(|e| e.to_string())?;
    let exit: Exit =
        serde_json::from_value(load(&ordinary(&campaign.join("owner-exit.json"), false)?)?)
            .map_err(|e| e.to_string())?;
    if closure.version != CORPUS_VERSION
        || submitted.version != RECEIPT_VERSION
        || exit.version != RECEIPT_VERSION
        || submitted.kind != KIND
        || exit.kind != KIND
        || closure.campaign != campaign
        || submitted.campaign != campaign
        || exit.campaign != campaign
        || submitted.automatic_resubmission
        || exit.automatic_resubmission
        || submitted.owner_pid == submitted.native_pid
        || exit.owner_pid != submitted.owner_pid
        || exit.native_pid != submitted.native_pid
        || exit.submitted_sha256 != closure.submitted_sha256
        || submitted.native_executable != closure.native_executable
        || submitted.native_executable_sha256 != closure.native_executable_sha256
        || !matches!(exit.native_exit_code, 0 | 101)
        || fs::symlink_metadata(campaign.join("interrupted.json")).is_ok()
        || fs::symlink_metadata(campaign.join("recovery.json")).is_ok()
    {
        return Err(
            "Fresh campaign ownership, executable or closure identity is inconsistent".into(),
        );
    }
    verify_pins(&campaign, &closure_path, &closure_sha, &closure, &submitted)?;
    let manifest = load(&campaign.join("manifest.json"))?;
    let fixtures = load(&campaign.join("fixtures.json"))?;
    let report = load(&campaign.join("report.json"))?;
    let completed = load(&campaign.join("completed.json"))?;
    let full = manifest["full_reading"]
        .as_bool()
        .ok_or("Missing native campaign scope")?;
    let mut required_files = BTreeSet::from([
        "manifest.json",
        "fixtures.json",
        "report.json",
        "completed.json",
    ]);
    if full {
        required_files.insert("reading-rubrics.json");
    }
    if !manifest["prompt_program"].is_null() {
        required_files.insert("prompt-program.json");
    }
    if closure
        .files
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != required_files
        || manifest["model"]["provider"] != "google_gemini_api"
        || manifest["model"]["id"] != MODEL
        || manifest["model"]["local_inference"] != false
        || manifest["model"]["credential_in_evidence"] != false
        || !manifest["model"]["decoding"]
            .as_str()
            .is_some_and(|s| s.contains("unconstrained"))
        || manifest["expected_answers_sent_to_model"] != false
        || report["manifest"] != manifest
        || report["campaign_state"]["status"] != "completed"
        || manifest["fixture_sha256"] != compact_hash(&campaign.join("fixtures.json"))?
    {
        return Err(
            "Campaign is not the complete authentic hosted unconstrained native source".into(),
        );
    }
    let rubric_bank = if full {
        if manifest["reading_rubric_sha256"]
            != compact_hash(&campaign.join("reading-rubrics.json"))?
        {
            return Err("Native rubric bank differs from its manifest".into());
        }
        Some(load(&campaign.join("reading-rubrics.json"))?)
    } else {
        None
    };
    if !manifest["prompt_program"].is_null()
        && manifest["prompt_program"]["sha256"] != closure.files["prompt-program.json"]
    {
        return Err("Captured prompt program differs from its native manifest".into());
    }
    let selected = submitted
        .selected_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if selected.is_empty()
        || selected.len() != submitted.selected_ids.len()
        || selected.iter().any(|id| !safe_id(id))
        || closure
            .case_files
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != selected
        || manifest["selected_count"].as_u64() != Some(selected.len() as u64)
        || report["completed"].as_u64() != Some(selected.len() as u64)
        || completed["completed"].as_u64() != Some(selected.len() as u64)
        || completed["campaign_failures"] != report["campaign_failures"]
        || report["campaign_failures"]
            .as_u64()
            .is_none_or(|n| n > selected.len() as u64)
        || (exit.native_exit_code == 0) != (report["campaign_failures"] == 0)
    {
        return Err("Selected native cases or terminal exit do not agree with the closure".into());
    }
    let mut source_fixtures = BTreeMap::new();
    for fixture in fixtures.as_array().ok_or("Missing authored fixture bank")? {
        let id = fixture["id"]
            .as_str()
            .filter(|id| safe_id(id))
            .ok_or("Invalid authored fixture ID")?;
        if source_fixtures.insert(id, fixture).is_some() {
            return Err("Duplicate authored fixture identity".into());
        }
    }
    if manifest["all_fixture_count"].as_u64() != Some(source_fixtures.len() as u64) {
        return Err("Native source fixture count is inconsistent".into());
    }
    let filter = manifest["selection_filter"]
        .as_str()
        .ok_or("Missing native selection filter")?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let modes = manifest["selection_modes"]
        .as_array()
        .ok_or("Missing native selection modes")?;
    let native_selected = source_fixtures
        .iter()
        .filter(|(id, f)| {
            (filter.is_empty()
                || filter
                    .iter()
                    .any(|token| token == *id || f["method"] == *token))
                && (modes.is_empty() || modes.contains(&f["mode"]))
        })
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    if native_selected != selected {
        return Err("Submission differs from the actual native fixture filter".into());
    }
    let cases_dir = ordinary(&campaign.join("cases"), true)?;
    let mut directories = BTreeSet::new();
    for entry in fs::read_dir(&cases_dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        ordinary(&path, true)?;
        directories.insert(
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid case directory")?
                .to_owned(),
        );
    }
    if directories
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != selected
    {
        return Err("Campaign contains an unselected or unfinished case directory".into());
    }
    let mut outcomes = BTreeMap::new();
    for outcome in report["cases"]
        .as_array()
        .ok_or("Missing terminal native report cases")?
    {
        let id = outcome["id"]
            .as_str()
            .ok_or("Native outcome lacks its identity")?;
        if outcomes.insert(id, outcome).is_some() {
            return Err("Duplicate native report outcome".into());
        }
    }
    if outcomes.keys().copied().collect::<BTreeSet<_>>() != selected {
        return Err("Native report is not complete for its selected cases".into());
    }
    let mut origins = BTreeMap::new();
    for id in selected {
        let fixture = source_fixtures
            .get(id)
            .ok_or("Selected fixture disappeared")?;
        let directory = campaign.join("cases").join(id);
        let outcome = load(&directory.join("outcome.json"))?;
        if outcomes[id] != &outcome {
            return Err("Native report rewrites a terminal outcome".into());
        }
        for witness in [
            "fixture.json",
            "initial.json",
            "first-turn.json",
            "final.json",
            "outcome.json",
            "trace.html",
        ] {
            if !closure.case_files[id].contains_key(witness) {
                return Err(format!("Case {id} lacks {witness}"));
            }
        }
        case(
            &directory,
            fixture,
            &outcome,
            &manifest,
            rubric_bank.as_ref().map(|b| &b[id]),
        )?;
        origins.insert(format!("{id}.json"), json!({"case_id":id,"origin":"fresh_completed_native_campaign",
            "source_directory":directory,"source_manifest_sha256":closure.files["manifest.json"],
            "files":closure.case_files[id],"selected_outcome_unchanged":true,
            "fresh_corpus":{"closure":closure_path,"closure_sha256":closure_sha,
                "submitted_sha256":closure.submitted_sha256,"owner_exit_sha256":closure.owner_exit_sha256,
                "native_executable_sha256":closure.native_executable_sha256,"native_exit_code":exit.native_exit_code},
            "qualification":"Closed evidence only; native semantic failures remain failures and interpretation needs independent source review."}));
    }
    // All hashes and owner absences are rechecked immediately before publishing.
    verify_pins(&campaign, &closure_path, &closure_sha, &closure, &submitted)?;
    let target = campaign.join("case-origins");
    if fs::symlink_metadata(&target).is_ok() {
        let expected = origins
            .iter()
            .map(|(name, v)| {
                (
                    name.clone(),
                    digest(serde_json::to_vec_pretty(v).expect("Value serializes")),
                )
            })
            .collect::<Hashes>();
        if tree(&target)? != expected {
            return Err("Existing case origins are changed or partial; preserve them".into());
        }
    } else {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let staging = campaign.join(format!(".corpus-origins-{}-{nonce}", std::process::id()));
        fs::create_dir(&staging).map_err(|e| e.to_string())?;
        for (name, origin) in &origins {
            let bytes = serde_json::to_vec_pretty(origin).map_err(|e| e.to_string())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staging.join(name))
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        verify_pins(&campaign, &closure_path, &closure_sha, &closure, &submitted)?;
        fs::rename(&staging, &target).map_err(|e| e.to_string())?;
        fs::File::open(&campaign)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(
        json!({"version":CORPUS_VERSION,"campaign":campaign,"closure":closure_path,"closure_sha256":closure_sha,
        "case_count":origins.len(),"source_manifest_sha256":closure.files["manifest.json"],
        "case_origins":tree(&target)?,"native_outcomes_unchanged":true,"new_provider_calls":0,
        "qualification":"A sealed corpus does not qualify source-correct interpretation or model improvement."}),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // Controlled receipt fixtures exercise sealing, not model quality. No test
    // contacts Google, runs the native application or reads a credential.
    struct Campaign {
        _temp: tempfile::TempDir,
        root: PathBuf,
        closure: PathBuf,
    }

    fn write(path: &Path, value: &Value) {
        fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn exited_pid() -> u32 {
        let mut child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        pid
    }

    impl Campaign {
        fn new(semantic_failure: bool, full: bool) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(temp.path()).unwrap();
            let directory = root.join("cases/a");
            fs::create_dir_all(directory.join("calls")).unwrap();
            fs::create_dir(directory.join("checkpoints")).unwrap();
            let model = json!({"provider":"google_gemini_api","id":MODEL,
                "local_inference":false,"credential_in_evidence":false,
                "decoding":"hosted unconstrained text"});
            let fixture = json!({"id":"a","method":"movable_deal","mode":"explicit",
                "words":"Will the buyer complete the purchase?","device_available":true,
                "expected":{"facet":"event","facts":[],"needs":[],"ready":true}});
            write(&root.join("fixtures.json"), &json!([fixture]));
            write(&directory.join("fixture.json"), &fixture);
            let rubric = json!({"case_id":"a","declared_method":"movable_deal",
                "reading_method":"movable_deal","source":{"printed_pages":"172"}});
            if full {
                write(&root.join("reading-rubrics.json"), &json!({"a":rubric}));
                write(&directory.join("reading-rubric.json"), &rubric);
            }
            let mut manifest = json!({"model":model,"full_reading":full,"prompt_program":null,
                "fixture_sha256":compact_hash(&root.join("fixtures.json")).unwrap(),
                "expected_answers_sent_to_model":false,"selected_count":1,"all_fixture_count":1,
                "selection_filter":"a","selection_modes":[],"case_max_calls":3,
                "frozen_clock":{"timezone":"America/New_York"},
                "device":{"id":"device-location","latitude":38.657,"longitude":-77.249,"timezone":"America/New_York"}});
            if full {
                manifest["reading_rubric_sha256"] =
                    json!(compact_hash(&root.join("reading-rubrics.json")).unwrap());
            }
            write(&root.join("manifest.json"), &manifest);
            let question = &fixture["words"];
            let initial_session = json!({"messages":[{"role":"user","text":question}],
                "question":"","chart":null,"place":null,"candidateMomentMs":null,
                "method":{"consultation":null,"result":null,"records":[]},
                "sections":[],"facts":[],"deviceContext":{"timezone":"America/New_York"}});
            write(
                &directory.join("initial.json"),
                &json!({"session":initial_session,"candidates":[manifest["device"]]}),
            );
            let mut session = initial_session;
            let native_result = if semantic_failure {
                let input = json!({"source_question":question});
                let input_sha = digest(input.to_string());
                session["method"]["records"] = json!([{"stage":"condition","revision":1,
                    "input":input,"inputSha256":input_sha,"validationError":"Native evidence field omitted"}]);
                session["method"]["flow"] = json!({"jobs":[{"stage":"condition","revision":1,
                    "inputSha256":input_sha,"phase":{"state":"paused","error":SEMANTIC_STOP}}]});
                json!({"Err":SEMANTIC_STOP})
            } else {
                session["question"] = question.clone();
                json!({"Ok":null})
            };
            write(
                &directory.join("first-turn.json"),
                &json!({"session":session,"result":native_result}),
            );
            let follow = json!({"status":"not scripted"});
            write(
                &directory.join("final.json"),
                &json!({"session":session,"follow_up":follow}),
            );
            fs::write(
                directory.join("trace.html"),
                "Offline controlled native receipt fixture",
            )
            .unwrap();
            let prompt = json!([{"role":"system","content":"Actual example teaching"},
                {"role":"user","content":question}]);
            let request = json!({"sequence":1,"stage":"condition","provider":model,
                "prompt":prompt,"schema":{"type":"object"},
                "input":{"source_question":question},"decoder":"hosted unconstrained text"});
            write(&directory.join("calls/0001-request.json"), &request);
            let config = json!({"temperature":0,"seed":0,"maxOutputTokens":1000,
                "thinkingConfig":{"thinkingLevel":"minimal"}});
            let content = if semantic_failure {
                "{\"omitted\":true}"
            } else {
                "{\"answer\":\"pending native source review\"}"
            };
            let output = json!({"content":content});
            let body = json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":content}]}}]});
            let receipt = json!({"provider":"google_gemini_api","model":MODEL,"submitted":true,
                "request":wire(&prompt,&config).unwrap(),"native_result":{"Ok":output},
                "response":{"http_status":200,"body":body},"token_count_response":{"http_status":200},
                "generation_attempts":[{"attempt":1,"submitted":true,"http_status":200,"body":body}]});
            write(
                &directory.join("calls/0001-result.json"),
                &json!({"request":request,"result":{"Ok":output},"provider_receipt":receipt}),
            );
            let outcome = json!({"id":"a","method":"movable_deal","mode":"explicit","full_reading":full,
                "grade":{"semantic_pass":!semantic_failure},"first_turn_execution_completed":!semantic_failure,
                "follow_up_execution_completed":null,"follow_up_scripted":false,"follow_up_pass":null,
                "follow_up":follow,"model_calls":1,"hosted_http_requests":1,
                "provider_stop":null,"infrastructure_error":null,"deadline_cancelled":false,"group_cancelled":false,
                "execution_status":if semantic_failure{"call_budget_exhausted"}else{"completed"},
                "decoder_mode":"hosted_unconstrained_text","hurdles":{"reading":{"status":"structure_pass_review_pending"}}});
            write(&directory.join("outcome.json"), &outcome);
            write(
                &root.join("report.json"),
                &json!({"manifest":manifest,"campaign_state":{"status":"completed"},
                "completed":1,"campaign_failures":usize::from(semantic_failure),"cases":[outcome]}),
            );
            write(
                &root.join("completed.json"),
                &json!({"completed":1,"campaign_failures":usize::from(semantic_failure)}),
            );
            let executable = root.join("native-snapshot");
            fs::write(
                &executable,
                "Offline executable identity fixture; never executed",
            )
            .unwrap();
            let owner_pid = exited_pid();
            let native_pid = exited_pid();
            let submitted = json!({"version":RECEIPT_VERSION,"kind":KIND,"campaign":root,
                "owner_pid":owner_pid,"native_pid":native_pid,"selected_ids":["a"],
                "native_executable":executable,"native_executable_sha256":digest(read(&executable).unwrap()),
                "automatic_resubmission":false});
            write(&root.join("submitted.json"), &submitted);
            write(
                &root.join("owner-exit.json"),
                &json!({"version":RECEIPT_VERSION,"kind":KIND,"campaign":root,
                "owner_pid":owner_pid,"native_pid":native_pid,
                "submitted_sha256":digest(read(&root.join("submitted.json")).unwrap()),
                "native_exit_code":if semantic_failure{101}else{0},"automatic_resubmission":false}),
            );
            let result = Self {
                _temp: temp,
                closure: root.join("closure.json"),
                root,
            };
            result.close();
            result
        }

        // Used only to author a new controlled closed fixture for negative
        // tests. Production sealing must never update a launcher's closure.
        fn close(&self) {
            let submitted = load(&self.root.join("submitted.json")).unwrap();
            let manifest = load(&self.root.join("manifest.json")).unwrap();
            let mut files = Hashes::new();
            for name in [
                "manifest.json",
                "fixtures.json",
                "report.json",
                "completed.json",
            ] {
                files.insert(name.into(), digest(read(&self.root.join(name)).unwrap()));
            }
            if manifest["full_reading"] == true {
                files.insert(
                    "reading-rubrics.json".into(),
                    digest(read(&self.root.join("reading-rubrics.json")).unwrap()),
                );
            }
            write(
                &self.closure,
                &json!({"version":CORPUS_VERSION,"campaign":self.root,
                "submitted_sha256":digest(read(&self.root.join("submitted.json")).unwrap()),
                "owner_exit_sha256":digest(read(&self.root.join("owner-exit.json")).unwrap()),
                "native_executable":submitted["native_executable"],"native_executable_sha256":submitted["native_executable_sha256"],
                "files":files,"case_files":{"a":tree(&self.root.join("cases/a")).unwrap()}}),
            );
        }
    }

    #[test]
    fn fresh_closed_origins_are_atomic_idempotent_and_keep_exact_case_files() {
        let c = Campaign::new(false, true);
        let before = tree(&c.root.join("cases/a")).unwrap();
        let receipt = seal(&c.root, &c.closure).unwrap();
        assert_eq!(receipt["new_provider_calls"], 0);
        let origin = load(&c.root.join("case-origins/a.json")).unwrap();
        assert_eq!(origin["origin"], "fresh_completed_native_campaign");
        assert_eq!(origin["source_directory"], json!(c.root.join("cases/a")));
        assert_eq!(origin["files"], json!(before));
        assert_eq!(tree(&c.root.join("cases/a")).unwrap(), before);
        assert!(!c.root.join("recovery.json").exists());
        assert!(!c.root.join("orphaned-worker.json").exists());
        assert_eq!(seal(&c.root, &c.closure).unwrap(), receipt);
    }

    #[test]
    fn exit_101_keeps_settled_native_semantic_failure_without_normalizing_it() {
        let c = Campaign::new(true, true);
        let path = c.root.join("cases/a/outcome.json");
        let before = read(&path).unwrap();
        seal(&c.root, &c.closure).unwrap();
        assert_eq!(read(&path).unwrap(), before);
        let outcome = load(&path).unwrap();
        assert_eq!(outcome["grade"]["semantic_pass"], false);
        assert_eq!(outcome["execution_status"], "call_budget_exhausted");
        assert!(outcome.get("known_native_semantic_abort").is_none());
        assert_eq!(
            load(&c.root.join("case-origins/a.json")).unwrap()["fresh_corpus"]["native_exit_code"],
            101
        );
    }

    #[test]
    fn input_only_corpus_requires_no_unbound_reading_rubric() {
        let c = Campaign::new(false, false);
        seal(&c.root, &c.closure).unwrap();
        assert!(
            load(&c.root.join("case-origins/a.json")).unwrap()["files"]["reading-rubric.json"]
                .is_null()
        );
    }

    #[test]
    fn retained_service_retries_and_parallel_branch_receipts_remain_supported() {
        for parallel in [false, true] {
            let c = Campaign::new(false, true);
            let directory = c.root.join("cases/a");
            let request_path = directory.join("calls/0001-request.json");
            let result_path = directory.join("calls/0001-result.json");
            let original = load(&result_path).unwrap();
            let receipt = original["provider_receipt"].clone();
            let mut result = original.clone();
            let physical = if parallel {
                let request = json!({"sequence":1,"tasks":[["condition","general",{},{}],["reception","general",{},{}]],
                    "prompts":[original["request"]["prompt"],original["request"]["prompt"]],
                    "provider":original["request"]["provider"],"decoder":"hosted parallel unconstrained text branches"});
                write(&request_path, &request);
                result = json!({"request":request,"result":{"Ok":[receipt["native_result"]["Ok"],receipt["native_result"]["Ok"]]},
                    "provider_receipts":[receipt,receipt]});
                2
            } else {
                let mut final_attempt = receipt["generation_attempts"][0].clone();
                final_attempt["attempt"] = json!(3);
                result["provider_receipt"]["generation_attempts"] = json!([
                    {"attempt":1,"submitted":true,"http_status":500,"body":{"error":{"message":"Settled service failure"}}},
                    {"attempt":2,"submitted":true,"http_status":500,"body":{"error":{"message":"Settled service failure"}}},
                    final_attempt]);
                3
            };
            write(&result_path, &result);
            let outcome_path = directory.join("outcome.json");
            let mut outcome = load(&outcome_path).unwrap();
            outcome["hosted_http_requests"] = json!(physical);
            write(&outcome_path, &outcome);
            let mut report = load(&c.root.join("report.json")).unwrap();
            report["cases"][0] = outcome;
            write(&c.root.join("report.json"), &report);
            c.close();
            seal(&c.root, &c.closure).unwrap();
        }
    }

    #[test]
    fn live_owner_blocks_publication_even_with_complete_and_consistent_receipts() {
        let c = Campaign::new(false, true);
        let mut submitted = load(&c.root.join("submitted.json")).unwrap();
        submitted["owner_pid"] = json!(std::process::id());
        write(&c.root.join("submitted.json"), &submitted);
        let mut exit = load(&c.root.join("owner-exit.json")).unwrap();
        exit["owner_pid"] = submitted["owner_pid"].clone();
        exit["submitted_sha256"] = json!(digest(read(&c.root.join("submitted.json")).unwrap()));
        write(&c.root.join("owner-exit.json"), &exit);
        c.close();
        assert!(seal(&c.root, &c.closure)
            .unwrap_err()
            .contains("not proven absent"));
        assert!(!c.root.join("case-origins").exists());
    }

    #[test]
    fn uncertain_provider_attempt_and_unpaired_request_are_never_semantic_observations() {
        for uncertainty in [true, false] {
            let c = Campaign::new(true, true);
            let path = c.root.join("cases/a/calls/0001-result.json");
            let mut result = load(&path).unwrap();
            if uncertainty {
                result["provider_receipt"]["generation_attempts"][0]["error"] =
                    json!("Submission transport status unknown");
            } else {
                result["request"]["input"] = json!({"source_question":"Some other question"});
            }
            write(&path, &result);
            c.close();
            assert!(seal(&c.root, &c.closure).is_err());
            assert!(!c.root.join("case-origins").exists());
        }
    }

    #[test]
    fn stale_closure_and_changed_executable_or_case_provenance_are_rejected() {
        for file in [
            "cases/a/fixture.json",
            "cases/a/calls/0001-request.json",
            "native-snapshot",
            "submitted.json",
            "report.json",
        ] {
            let c = Campaign::new(false, true);
            fs::write(c.root.join(file), "Changed after closure").unwrap();
            assert!(seal(&c.root, &c.closure).is_err(), "{file}");
            assert!(!c.root.join("case-origins").exists());
        }
    }

    #[test]
    fn incomplete_campaign_root_rubric_mismatch_and_transplanted_question_are_rejected() {
        for defect in [
            "running",
            "rubric",
            "question",
            "semantic_snapshot",
            "completed_flag",
        ] {
            let c = Campaign::new(defect == "semantic_snapshot", true);
            let path = match defect {
                "running" => c.root.join("report.json"),
                "rubric" => c.root.join("cases/a/reading-rubric.json"),
                "question" => c.root.join("cases/a/initial.json"),
                _ => c.root.join("cases/a/first-turn.json"),
            };
            let mut value = load(&path).unwrap();
            match defect {
                "running" => value["campaign_state"]["status"] = json!("running"),
                "rubric" => value["source"]["printed_pages"] = json!("Unbound source"),
                "question" => {
                    value["session"]["messages"][0]["text"] = json!("Transplanted question")
                }
                "completed_flag" => value["result"] = json!({"Err":"Unknown executor failure"}),
                _ => value["session"]["method"]["records"][0]["validationError"] = Value::Null,
            }
            write(&path, &value);
            c.close();
            assert!(seal(&c.root, &c.closure).is_err(), "{defect}");
            assert!(!c.root.join("case-origins").exists());
        }
    }

    #[test]
    fn constrained_google_wire_and_partial_existing_origins_are_rejected() {
        let c = Campaign::new(false, true);
        let path = c.root.join("cases/a/calls/0001-result.json");
        let mut result = load(&path).unwrap();
        result["provider_receipt"]["request"]["generationConfig"]["responseSchema"] =
            json!({"type":"object"});
        write(&path, &result);
        c.close();
        assert!(seal(&c.root, &c.closure).is_err());
        let c = Campaign::new(false, true);
        fs::create_dir(c.root.join("case-origins")).unwrap();
        assert!(seal(&c.root, &c.closure).unwrap_err().contains("partial"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_case_evidence_is_rejected_without_following_it() {
        let c = Campaign::new(false, true);
        let source = c.root.join("cases/a/initial.json");
        let moved = c.root.join("initial-outside-case.json");
        fs::rename(&source, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &source).unwrap();
        assert!(seal(&c.root, &c.closure).unwrap_err().contains("Symlink"));
        assert!(!c.root.join("case-origins").exists());
    }
}
