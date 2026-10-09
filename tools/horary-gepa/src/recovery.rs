//! Explicit, source-bound imports from one stopped paid input round.
//! Preparation never runs a model or alters the predecessor's journal.
use crate::{executable, keep, load, read, verify, Function, Plan, Result};
use horary_prompt_program::digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
};

const NATIVE: &str = "operations/00000";
const REVIEW: &str = "operations/00001";
const MAX_FILES: usize = 512;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reservations {
    pub teacher_calls: u64,
    pub review_calls: u64,
    pub physical_generation_attempts: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub version: u32,
    pub predecessor_state: PathBuf,
    pub source_hashes: BTreeMap<String, String>,
    pub audit_file: PathBuf,
    pub audit_sha256: String,
    pub owner_pids: Vec<u32>,
    pub inherited_reservations: Reservations,
}

fn ordinary(base: &Path, relative: &str) -> Result<PathBuf> {
    if relative.is_empty()
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Recovery artifact escapes its predecessor".into());
    }
    let mut path = base.to_path_buf();
    for part in Path::new(relative).components() {
        path.push(part.as_os_str());
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .is_symlink()
        {
            return Err("Recovery refuses symlinked evidence".into());
        }
    }
    Ok(path)
}

fn tree(base: &Path, relative: &str, result: &mut BTreeMap<String, String>) -> Result<()> {
    let path = ordinary(base, relative)?;
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let name = entry
                .map_err(|e| e.to_string())?
                .file_name()
                .into_string()
                .map_err(|_| "Non-UTF8 recovery artifact")?;
            tree(base, &format!("{relative}/{name}"), result)?;
        }
    } else if path.is_file() {
        if result.len() >= MAX_FILES {
            return Err("Recovery artifact count exceeds its bound".into());
        }
        result.insert(relative.into(), digest(read(&path)?));
    } else {
        return Err("Recovery requires ordinary files/directories".into());
    }
    Ok(())
}

fn pin_sources(root: &Path) -> Result<BTreeMap<String, String>> {
    let operations = ordinary(root, "operations")?;
    let mut names = fs::read_dir(operations)
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.file_name()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    if names != ["00000", "00001"] {
        return Err(
            "Recovery is bounded to one sealed native operation and one stopped paid review".into(),
        );
    }
    let mut files = BTreeMap::new();
    for path in [
        "plan.json",
        "owner-exit.json",
        "owner-observed.json",
        "platform-owner.json",
        NATIVE,
        REVIEW,
    ] {
        tree(root, path, &mut files)?;
    }
    Ok(files)
}

fn pid_absent(pid: u32) -> Result<()> {
    if pid == 0 {
        return Err("Recovery owner PID is invalid".into());
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.stdout.is_empty() {
        return Err(format!(
            "Predecessor owner/child PID {pid} still runs; recovery is withheld"
        ));
    }
    if output.status.code() != Some(1) {
        return Err("Exact PID absence could not be verified".into());
    }
    Ok(())
}

fn reservations(root: &Path) -> Result<Reservations> {
    let native = load(&ordinary(root, &format!("{NATIVE}/reservation.json"))?)?;
    let judge = load(&ordinary(root, &format!("{REVIEW}/reservation.json"))?)?;
    if native["teacher_calls"] != 0
        || native["review_calls"] != 0
        || judge["teacher_calls"] != 0
        || judge["physical_generation_attempts"] != 0
        || judge["review_calls"] != 1
    {
        return Err("Predecessor reservations do not describe the bounded recovery".into());
    }
    Ok(Reservations {
        teacher_calls: 0,
        review_calls: 1,
        physical_generation_attempts: native["physical_generation_attempts"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or("Unknown predecessor native reservation")?,
    })
}

impl Recovery {
    pub fn verify_sources(&self) -> Result<()> {
        if self.version != 1
            || pin_sources(&self.predecessor_state)? != self.source_hashes
            || reservations(&self.predecessor_state)? != self.inherited_reservations
        {
            return Err("Recovery predecessor evidence or reservations changed".into());
        }
        verify(&self.audit_file, &self.audit_sha256)?;
        let audit = load(&self.audit_file)?;
        if audit["kind"] != "input-review-metadata-boundary-audit"
            || audit["round"] != json!(self.predecessor_state)
            || audit["plan_sha256"] != self.source_hashes["plan.json"]
            || audit["native_operation"] != NATIVE
            || audit["judge_operation"] != REVIEW
            || [
                "metadata_only_recovery_supported",
                "all_source_hashes_verified",
                "tool_free_completed_judge",
                "only_native_metadata_mismatch",
            ]
            .iter()
            .any(|key| audit[*key] != true)
            || audit["changed_field"] != "native_journey_pass"
            || audit["original_value"] != false
            || !audit.get("bound_value").is_some_and(Value::is_null)
        {
            return Err("Independent audit does not authorize this metadata-only recovery".into());
        }
        let hashes = audit["source_hashes"]
            .as_object()
            .ok_or("Audit lacks bound source hashes")?;
        for required in [
            "plan.json",
            "operations/00000/completed.json",
            "operations/00000/response.json",
            "operations/00000/request.json",
            "operations/00001/request.json",
            "operations/00001/answer.json",
            "operations/00001/packet.json",
            "operations/00001/events.jsonl",
            "operations/00001/exit.json",
        ] {
            let audit_hash = hashes
                .get(required)
                .and_then(Value::as_str)
                .ok_or("Independent recovery audit lacks a mandatory source hash")?;
            let pinned_hash = self
                .source_hashes
                .get(required)
                .ok_or("Recovery ancestry lacks a mandatory source hash")?;
            if audit_hash != pinned_hash {
                return Err("Independent recovery audit is bound to different evidence".into());
            }
        }
        for (file, sha) in hashes {
            verify(
                &ordinary(&self.predecessor_state, file)?,
                sha.as_str().ok_or("Invalid audit digest")?,
            )?;
        }
        let root = &self.predecessor_state;
        let exit = load(&ordinary(root, "owner-exit.json")?)?;
        if exit["exit_code"].as_i64().is_none() || exit["automatic_resubmission"] != false {
            return Err("Predecessor lacks a terminal no-resubmission owner receipt".into());
        }
        let expected_pids = [
            load(&ordinary(root, "owner-observed.json")?)?["pid"].as_u64(),
            load(&ordinary(root, &format!("{NATIVE}/submitted.json"))?)?["pid"].as_u64(),
            load(&ordinary(root, &format!("{REVIEW}/submitted.json"))?)?["pid"].as_u64(),
        ]
        .into_iter()
        .map(|p| {
            p.and_then(|p| u32::try_from(p).ok())
                .ok_or("Missing predecessor owner/child PID".into())
        })
        .collect::<Result<Vec<_>>>()?;
        if expected_pids != self.owner_pids {
            return Err("Predecessor PID ancestry changed".into());
        }
        for pid in &self.owner_pids {
            pid_absent(*pid)?;
        }
        self.verify_native_seal()?;
        for op in [NATIVE, REVIEW] {
            let exit = load(&ordinary(root, &format!("{op}/exit.json"))?)?;
            if exit["code"] != 0 || exit["success"] != true {
                return Err("Paid predecessor operation did not exit successfully".into());
            }
        }
        let events = horary_loop::review_events::inspect(&ordinary(
            root,
            &format!("{REVIEW}/events.jsonl"),
        )?)?;
        if events.violation.is_some() || events.completed_turns != 1 {
            return Err("Paid predecessor judge was not a completed tool-free turn".into());
        }
        let prepared = load(&ordinary(root, &format!("{REVIEW}/prepared.json"))?)?;
        if prepared["role"] != "independent input judge" || prepared["model_override"] != false {
            return Err("Predecessor judge role or model boundary differs".into());
        }
        for file in ["answer.json", "packet.json"] {
            if !load(&ordinary(root, &format!("{REVIEW}/{file}"))?)?.is_object() {
                return Err("Paid predecessor judge artifact is incomplete".into());
            }
        }
        Ok(())
    }

    fn verify_native_seal(&self) -> Result<()> {
        let seal = load(&ordinary(
            &self.predecessor_state,
            &format!("{NATIVE}/completed.json"),
        )?)?;
        let mut sealed = BTreeMap::new();
        for item in seal["artifacts"]
            .as_array()
            .ok_or("Native operation is unsealed")?
        {
            let file = format!(
                "{NATIVE}/{}",
                item["file"].as_str().ok_or("Bad native artifact")?
            );
            let hash = item["sha256"].as_str().ok_or("Bad native artifact hash")?;
            if self.source_hashes.get(&file).map(String::as_str) != Some(hash)
                || sealed.insert(file, hash).is_some()
            {
                return Err("Native seal differs from recovery artifacts".into());
            }
        }
        let expected = self
            .source_hashes
            .iter()
            .filter(|(name, _)| {
                name.starts_with(&format!("{NATIVE}/")) && !name.ends_with("/completed.json")
            })
            .map(|(name, hash)| (name.clone(), hash.as_str()))
            .collect::<BTreeMap<_, _>>();
        let response_hash = self
            .source_hashes
            .get(&format!("{NATIVE}/response.json"))
            .ok_or("Native completion seal lacks a mandatory response hash")?;
        if expected != sealed || seal["response_sha256"] != *response_hash {
            return Err("Native completion seal does not cover all predecessor artifacts".into());
        }
        let response = load(&ordinary(
            &self.predecessor_state,
            &format!("{NATIVE}/response.json"),
        )?)?;
        crate::metric::require_completed(&response["outcome"], "input_journey")?;
        Ok(())
    }

    pub fn verify_plan(&self, plan: &Plan) -> Result<()> {
        self.verify_sources()?;
        let mut expected: Plan =
            serde_json::from_value(load(&self.predecessor_state.join("plan.json"))?)
                .map_err(|e| e.to_string())?;
        if expected.recovery.is_some() || expected.function != Function::InputJourney {
            return Err("Recovery requires an unrecovered input-round predecessor".into());
        }
        deduct(&mut expected, &self.inherited_reservations)?;
        expected.controller_executable = plan.controller_executable.clone();
        expected.controller_executable_sha256 = plan.controller_executable_sha256.clone();
        expected.codex = plan.codex.clone();
        expected.codex_snapshot = plan.codex_snapshot.clone();
        expected.recovery = plan.recovery.clone();
        if serde_json::to_value(expected).map_err(|e| e.to_string())?
            != serde_json::to_value(plan).map_err(|e| e.to_string())?
        {
            return Err(
                "Recovery changed the frozen program, case pools, sources or bounded remainder"
                    .into(),
            );
        }
        Ok(())
    }

    fn receipt(&self, operation: &str) -> Value {
        json!({"type":"predecessor_import","version":1,"predecessor_state":self.predecessor_state,
            "operation":operation,"source_hashes":self.source_hashes,"audit_file":self.audit_file,"audit_sha256":self.audit_sha256,
            "inherited_reservations":self.inherited_reservations,"new_paid_reservations":0})
    }
}

fn deduct(plan: &mut Plan, spent: &Reservations) -> Result<()> {
    plan.max_teacher_calls = plan
        .max_teacher_calls
        .checked_sub(spent.teacher_calls)
        .ok_or("Inherited teacher reservations exceed budget")?;
    plan.max_review_calls = plan
        .max_review_calls
        .checked_sub(spent.review_calls)
        .ok_or("Inherited review reservations exceed budget")?;
    plan.max_physical_generation_attempts = plan
        .max_physical_generation_attempts
        .checked_sub(spent.physical_generation_attempts)
        .ok_or("Inherited native reservations exceed budget")?;
    Ok(())
}

/// An exact completed native request can be reused before pacing, quota or payment.
pub fn native_source(plan: &Plan, request: &Value) -> Result<Option<PathBuf>> {
    let Some(recovery) = &plan.recovery else {
        return Ok(None);
    };
    recovery.verify_plan(plan)?;
    let directory = recovery.predecessor_state.join(NATIVE);
    Ok((load(&directory.join("request.json"))?["request"] == *request).then_some(directory))
}

/// A paid review is eligible only for the exact imported native evaluation.
pub fn review_source(plan: &Plan, evaluation: &Value) -> Result<Option<PathBuf>> {
    let Some(recovery) = &plan.recovery else {
        return Ok(None);
    };
    recovery.verify_plan(plan)?;
    let directory = recovery.predecessor_state.join(REVIEW);
    let hash = digest(evaluation.to_string());
    Ok(
        (load(&directory.join("request.json"))?["request"]["native_evaluation_sha256"] == hash
            && load(
                &recovery
                    .predecessor_state
                    .join(NATIVE)
                    .join("response.json"),
            )? == *evaluation)
            .then_some(directory),
    )
}

pub fn import_receipt(plan: &Plan, directory: &Path) -> Result<Value> {
    let recovery = plan.recovery.as_ref().ok_or("No recovery ancestry")?;
    recovery.verify_plan(plan)?;
    let relative = directory
        .strip_prefix(&recovery.predecessor_state)
        .map_err(|e| e.to_string())?
        .to_str()
        .ok_or("Non-UTF8 predecessor path")?;
    if ![NATIVE, REVIEW].contains(&relative) {
        return Err("Import is not a pinned predecessor operation".into());
    }
    Ok(recovery.receipt(relative))
}

/// Read-only status verification, with no invented fresh attempts or scores.
pub fn verify_import(recovery: &Recovery, receipt: &Value) -> Result<()> {
    recovery.verify_sources()?;
    let op = receipt["operation"]
        .as_str()
        .filter(|op| [NATIVE, REVIEW].contains(op))
        .ok_or("Invalid recovery import operation")?;
    if *receipt != recovery.receipt(op) {
        return Err("Cache import differs from pinned ancestry".into());
    }
    Ok(())
}

/// Copy only public controller bytes into an unused recovery state.
pub fn prepare(predecessor: &Path, state: &Path, audit: &Path, controller: &Path) -> Result<Plan> {
    if state.exists() {
        return Err("Recovery state must be new; preserve existing attempts".into());
    }
    let predecessor_state = predecessor.canonicalize().map_err(|e| e.to_string())?;
    let mut plan: Plan = serde_json::from_value(load(&predecessor_state.join("plan.json"))?)
        .map_err(|e| e.to_string())?;
    plan.verify_sources()?;
    let source_hashes = pin_sources(&predecessor_state)?;
    let owner_pids = [
        "owner-observed.json",
        "operations/00000/submitted.json",
        "operations/00001/submitted.json",
    ]
    .into_iter()
    .map(|p| {
        load(&ordinary(&predecessor_state, p)?)?["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or("Missing predecessor PID".into())
    })
    .collect::<Result<Vec<_>>>()?;
    let recovery = Recovery {
        version: 1,
        inherited_reservations: reservations(&predecessor_state)?,
        predecessor_state,
        source_hashes,
        audit_file: audit.canonicalize().map_err(|e| e.to_string())?,
        audit_sha256: digest(read(audit)?),
        owner_pids,
    };
    recovery.verify_sources()?;
    deduct(&mut plan, &recovery.inherited_reservations)?;
    let controller_sha256 = digest(read(controller)?);
    fs::create_dir(state).map_err(|e| e.to_string())?;
    let executables = state.join("executables");
    fs::create_dir(&executables).map_err(|e| e.to_string())?;
    let copied = executables.join(if cfg!(windows) {
        "horary-gepa.exe"
    } else {
        "horary-gepa"
    });
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&copied)
        .map_err(|e| e.to_string())?;
    output
        .write_all(&read(controller)?)
        .and_then(|_| output.sync_all())
        .map_err(|e| e.to_string())?;
    if digest(read(controller)?) != controller_sha256 || digest(read(&copied)?) != controller_sha256
    {
        return Err(
            "Controller changed during recovery preparation; unsealed copy is retained".into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&copied, fs::Permissions::from_mode(0o500))
            .map_err(|e| e.to_string())?;
    }
    let snapshot = executable::snapshot_codex(&plan.codex, state)?;
    plan.controller_executable = copied.canonicalize().map_err(|e| e.to_string())?;
    plan.controller_executable_sha256 = controller_sha256;
    plan.codex = snapshot.executable;
    plan.codex_snapshot = Some(snapshot.support);
    plan.recovery = Some(recovery);
    plan.verify_sources()?;
    keep(&state.join("plan.json"), &plan)?;
    Ok(plan)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::journal::Journal;

    fn write(root: &Path, file: &str, value: Value) {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }

    fn reseal(root: &Path) {
        let mut files = BTreeMap::new();
        tree(root, NATIVE, &mut files).unwrap();
        let artifacts = files.iter().filter(|(file, _)| !file.ends_with("/completed.json"))
            .map(|(file, sha)|json!({"file":file.strip_prefix(&format!("{NATIVE}/")).unwrap(),"sha256":sha}))
            .collect::<Vec<_>>();
        write(
            root,
            &format!("{NATIVE}/completed.json"),
            json!({"artifacts":artifacts,
            "response_sha256":digest(read(&root.join(NATIVE).join("response.json")).unwrap())}),
        );
    }

    fn refresh_audit(recovery: &mut Recovery) {
        recovery.source_hashes = pin_sources(&recovery.predecessor_state).unwrap();
        let audit = json!({"kind":"input-review-metadata-boundary-audit","round":recovery.predecessor_state,
            "plan_sha256":recovery.source_hashes["plan.json"],"native_operation":NATIVE,"judge_operation":REVIEW,
            "metadata_only_recovery_supported":true,"all_source_hashes_verified":true,"tool_free_completed_judge":true,
            "only_native_metadata_mismatch":true,"changed_field":"native_journey_pass","original_value":false,"bound_value":null,
            "source_hashes":recovery.source_hashes});
        fs::write(
            &recovery.audit_file,
            serde_json::to_vec_pretty(&audit).unwrap(),
        )
        .unwrap();
        recovery.audit_sha256 = digest(read(&recovery.audit_file).unwrap());
    }

    fn fixture() -> (tempfile::TempDir, Plan, Value, Value) {
        let temporary = tempfile::tempdir().unwrap();
        let old_root = temporary.path().join("stopped");
        fs::create_dir(&old_root).unwrap();
        let mut old = crate::tests::plan();
        old.function = Function::InputJourney;
        old.control_mode = crate::ControlMode::FreshHosted;
        old.target_method = Some("movable_deal".into());
        old.max_review_calls = 8;
        old.max_physical_generation_attempts = 1152;
        old.logical_calls_per_function = 24;
        write(&old_root, "plan.json", serde_json::to_value(&old).unwrap());
        let pids = (0..3)
            .map(|_| {
                let mut child = Command::new("true").spawn().unwrap();
                let pid = child.id();
                child.wait().unwrap();
                pid
            })
            .collect::<Vec<_>>();
        write(&old_root, "owner-observed.json", json!({"pid":pids[0]}));
        write(
            &old_root,
            "owner-exit.json",
            json!({"exit_code":1,"automatic_resubmission":false}),
        );
        write(&old_root, "platform-owner.json", json!({"run_once":true}));
        let example = &old.development[0];
        let request = json!({"case":example,"candidate":old.seed,"native_executable_sha256":old.native_executable_sha256,
            "manifest_sha256":old.manifest_sha256,"function":old.function,"target_method":old.target_method,
            "scope":old.function.metric(),"archival_control":false});
        let response = json!({"cache_identity":digest(request.to_string()),"outcome":{"id":example.id,"execution_status":"completed",
            "first_turn_execution_completed":true,"follow_up_execution_completed":null,"physical_generation_attempts":6,
            "input_journey_pass":false,"follow_up_pass":null},"final":{"synthetic":true}});
        let plan_sha = digest(read(&old_root.join("plan.json")).unwrap());
        write(
            &old_root,
            &format!("{NATIVE}/request.json"),
            json!({"sequence":0,"plan_sha256":plan_sha,"kind":"input_journey","request":request}),
        );
        write(
            &old_root,
            &format!("{NATIVE}/response.json"),
            response.clone(),
        );
        write(
            &old_root,
            &format!("{NATIVE}/reservation.json"),
            json!({"teacher_calls":0,"review_calls":0,"physical_generation_attempts":144}),
        );
        write(
            &old_root,
            &format!("{REVIEW}/reservation.json"),
            json!({"teacher_calls":0,"review_calls":1,"physical_generation_attempts":0}),
        );
        for (op, pid) in [(NATIVE, pids[1]), (REVIEW, pids[2])] {
            write(
                &old_root,
                &format!("{op}/submitted.json"),
                json!({"pid":pid}),
            );
            write(
                &old_root,
                &format!("{op}/exit.json"),
                json!({"code":0,"success":true}),
            );
        }
        reseal(&old_root);
        write(
            &old_root,
            &format!("{REVIEW}/request.json"),
            json!({"sequence":1,"plan_sha256":plan_sha,"kind":"codex_input_review",
            "request":{"native_evaluation_sha256":digest(response.to_string())}}),
        );
        write(
            &old_root,
            &format!("{REVIEW}/prepared.json"),
            json!({"role":"independent input judge","model_override":false}),
        );
        write(
            &old_root,
            &format!("{REVIEW}/packet.json"),
            json!({"synthetic":true}),
        );
        write(
            &old_root,
            &format!("{REVIEW}/answer.json"),
            json!({"synthetic_paid_answer":true}),
        );
        fs::write(
            old_root.join(REVIEW).join("events.jsonl"),
            "{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n",
        )
        .unwrap();
        let mut recovery = Recovery {
            version: 1,
            predecessor_state: old_root,
            source_hashes: BTreeMap::new(),
            audit_file: temporary.path().join("audit.json"),
            audit_sha256: String::new(),
            owner_pids: pids,
            inherited_reservations: Reservations {
                teacher_calls: 0,
                review_calls: 1,
                physical_generation_attempts: 144,
            },
        };
        refresh_audit(&mut recovery);
        let mut plan = old;
        deduct(&mut plan, &recovery.inherited_reservations).unwrap();
        plan.recovery = Some(recovery);
        (temporary, plan, request, response)
    }

    #[test]
    fn inherited_caps_and_exact_request_import_do_not_repurchase_the_native_call() {
        let (root, plan, request, response) = fixture();
        assert_eq!(
            (
                plan.max_physical_generation_attempts,
                plan.max_review_calls,
                plan.max_teacher_calls
            ),
            (1008, 7, 2)
        );
        assert!(native_source(&plan, &request).unwrap().is_some());
        let mut other = request.clone();
        other["candidate"] = json!({"changed":true});
        assert!(native_source(&plan, &other).unwrap().is_none());
        assert!(review_source(&plan, &response).unwrap().is_some());
        let mut other = response.clone();
        other["final"] = json!({"other_observation":true});
        assert!(review_source(&plan, &other).unwrap().is_none());
        let state = root.path().join("new");
        let mut journal = Journal::open(&state, &plan).unwrap();
        let actual =
            crate::native::evaluate(&plan, &mut journal, &plan.development[0], &plan.seed).unwrap();
        assert_eq!(actual, response);
        let reservation = load(&state.join("operations/00000/reservation.json")).unwrap();
        assert_eq!(reservation["physical_generation_attempts"], 0);
        assert_eq!(reservation["review_calls"], 0);
        assert_eq!(
            load(&state.join("operations/00000/cache-source.json")).unwrap()["type"],
            "predecessor_import"
        );
        assert!(!state.join("operations/00000/submitted.json").exists());
        let status = crate::status::inspect(&state).unwrap();
        assert_eq!(status["observed_native_attempts"]["physical_attempts"], 0);
        assert_eq!(status["observed_native_attempts"]["cache_reuses"], 1);
        assert_eq!(
            status["recovery"]["inherited_reservations"]["physical_generation_attempts"],
            144
        );
        assert_eq!(status["operation_counts"]["settled"], 1);
    }

    #[test]
    fn altered_ancestry_or_program_is_unverified_without_rewriting_or_resubmission() {
        let (root, mut plan, request, _) = fixture();
        let state = root.path().join("new");
        let mut journal = Journal::open(&state, &plan).unwrap();
        crate::native::evaluate(&plan, &mut journal, &plan.development[0], &plan.seed).unwrap();
        let mut materialized = serde_json::to_value(&plan).unwrap();
        materialized["max_physical_generation_attempts"] = json!(1152);
        fs::write(
            state.join("plan.json"),
            serde_json::to_vec_pretty(&materialized).unwrap(),
        )
        .unwrap();
        let status = crate::status::inspect(&state).unwrap();
        assert!(status["recovery"].is_null());
        assert_eq!(status["operation_counts"]["unverified"], 1);
        plan.guide.push_str("changed teaching");
        assert!(native_source(&plan, &request)
            .unwrap_err()
            .contains("changed the frozen program"));
        let recovery = plan.recovery.as_ref().unwrap();
        let response = recovery
            .predecessor_state
            .join(NATIVE)
            .join("response.json");
        let original = read(&response).unwrap();
        fs::write(&response, b"{}").unwrap();
        assert!(recovery.verify_sources().is_err());
        assert_eq!(read(&response).unwrap(), b"{}");
        assert!(!original.is_empty());
    }

    #[test]
    fn a_live_predecessor_or_non_tool_free_judge_cannot_license_recovery() {
        let (_root, mut plan, _, _) = fixture();
        let recovery = plan.recovery.as_mut().unwrap();
        recovery.owner_pids[0] = std::process::id();
        write(
            &recovery.predecessor_state,
            "owner-observed.json",
            json!({"pid":std::process::id()}),
        );
        refresh_audit(recovery);
        assert!(recovery
            .verify_sources()
            .unwrap_err()
            .contains("still runs"));
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        recovery.owner_pids[0] = pid;
        write(
            &recovery.predecessor_state,
            "owner-observed.json",
            json!({"pid":pid}),
        );
        fs::write(recovery.predecessor_state.join(REVIEW).join("events.jsonl"),
            "{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\"}}\n{\"type\":\"turn.completed\"}\n").unwrap();
        refresh_audit(recovery);
        assert!(recovery.verify_sources().unwrap_err().contains("tool-free"));
    }

    #[test]
    fn missing_seals_and_invalid_metadata_audits_are_errors_not_zero_scores() {
        let (_root, mut plan, _, _) = fixture();
        let recovery = plan.recovery.as_mut().unwrap();
        let mut audit = load(&recovery.audit_file).unwrap();
        audit["metadata_only_recovery_supported"] = json!(false);
        fs::write(
            &recovery.audit_file,
            serde_json::to_vec_pretty(&audit).unwrap(),
        )
        .unwrap();
        recovery.audit_sha256 = digest(read(&recovery.audit_file).unwrap());
        assert!(recovery
            .verify_sources()
            .unwrap_err()
            .contains("does not authorize"));
        let mut seal = load(
            &recovery
                .predecessor_state
                .join(NATIVE)
                .join("completed.json"),
        )
        .unwrap();
        seal["artifacts"].as_array_mut().unwrap().pop();
        write(
            &recovery.predecessor_state,
            &format!("{NATIVE}/completed.json"),
            seal,
        );
        refresh_audit(recovery);
        assert!(recovery
            .verify_sources()
            .unwrap_err()
            .contains("does not cover all"));
    }

    #[test]
    fn a_mandatory_response_missing_from_both_maps_is_an_error_not_a_match_or_panic() {
        let (_root, mut plan, _, _) = fixture();
        let recovery = plan.recovery.as_mut().unwrap();
        fs::remove_file(
            recovery
                .predecessor_state
                .join(NATIVE)
                .join("response.json"),
        )
        .unwrap();
        let seal_file = format!("{NATIVE}/completed.json");
        let mut seal = load(&recovery.predecessor_state.join(&seal_file)).unwrap();
        seal["artifacts"]
            .as_array_mut()
            .unwrap()
            .retain(|artifact| artifact["file"] != "response.json");
        write(&recovery.predecessor_state, &seal_file, seal);
        refresh_audit(recovery);
        let required = format!("{NATIVE}/response.json");
        assert!(!recovery.source_hashes.contains_key(&required));
        assert!(load(&recovery.audit_file).unwrap()["source_hashes"]
            .get(&required)
            .is_none());
        assert!(recovery
            .verify_sources()
            .unwrap_err()
            .contains("lacks a mandatory source hash"));
        assert!(recovery
            .verify_native_seal()
            .unwrap_err()
            .contains("lacks a mandatory response hash"));
    }

    #[test]
    fn escaped_artifacts_and_additional_operations_are_never_imported() {
        let (_root, plan, _, _) = fixture();
        let recovery = plan.recovery.as_ref().unwrap();
        assert!(ordinary(&recovery.predecessor_state, "../outside").is_err());
        std::os::unix::fs::symlink(
            &recovery.audit_file,
            recovery
                .predecessor_state
                .join(REVIEW)
                .join("unexpected-alias"),
        )
        .unwrap();
        assert!(recovery.verify_sources().unwrap_err().contains("symlinked"));
        fs::remove_file(
            recovery
                .predecessor_state
                .join(REVIEW)
                .join("unexpected-alias"),
        )
        .unwrap();
        fs::create_dir(recovery.predecessor_state.join("operations/00002")).unwrap();
        assert!(recovery
            .verify_sources()
            .unwrap_err()
            .contains("bounded to one"));
    }
}
