//! A separately bounded experiment may reuse completed seed measurements.
//! This is not recovery, new observation, or permission to repeat uncertain work.
use crate::{
    journal::{Journal, Operation},
    keep, load, metric, read, teacher, verify, Function, Objective, Plan, Result,
};
use horary_prompt_program::digest;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
};

const IMPORT: &str = "sealed_baseline_control_import";
pub(crate) const COMPONENT_IMPORT: &str = "sealed_selected_stage_control_import";
pub(crate) const COMPONENT_REVIEW_IMPORT: &str = "validated_selected_stage_review_import";

pub(crate) fn ordinary(base: &Path, relative: &str) -> Result<PathBuf> {
    let mut path = base.to_path_buf();
    if relative.is_empty() {
        return Err("Empty baseline artifact path".into());
    }
    for part in Path::new(relative).components() {
        if !matches!(part, Component::Normal(_)) {
            return Err("Baseline artifact escapes its operation".into());
        }
        path.push(part);
        if fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .is_symlink()
        {
            return Err("Baseline control cannot use symlinked evidence".into());
        }
    }
    Ok(path)
}

pub(crate) fn sealed(root: &Path, operation: &str) -> Result<(Value, Value)> {
    let base = ordinary(root, operation)?;
    let seal = load(&ordinary(&base, "completed.json")?)?;
    let artifacts = seal["artifacts"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 512)
        .ok_or("Baseline operation lacks a bounded completion seal")?;
    let mut names = BTreeSet::new();
    for artifact in artifacts {
        let name = artifact["file"]
            .as_str()
            .ok_or("Missing baseline artifact name")?;
        if !names.insert(name) {
            return Err("Duplicate sealed baseline artifact".into());
        }
        verify(
            &ordinary(&base, name)?,
            artifact["sha256"]
                .as_str()
                .ok_or("Missing baseline artifact hash")?,
        )?;
    }
    for required in ["request.json", "reservation.json", "response.json"] {
        if !names.contains(required) {
            return Err("Baseline operation lacks mandatory sealed evidence".into());
        }
    }
    verify(
        &ordinary(&base, "response.json")?,
        seal["response_sha256"]
            .as_str()
            .ok_or("Missing baseline response hash")?,
    )?;
    Ok((
        load(&base.join("request.json"))?,
        load(&base.join("response.json"))?,
    ))
}

pub(crate) fn absent(pid: u32) -> Result<()> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .map_err(|e| e.to_string())?;
    if pid == 0 || !output.stdout.is_empty() || output.status.code() != Some(1) {
        return Err(format!(
            "Source-round PID {pid} is not proven absent; no trial prepared"
        ));
    }
    Ok(())
}

pub(crate) fn owner_pids(owner: &Value) -> Result<Vec<u32>> {
    let pid = |v: &Value| {
        v.as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .filter(|p| *p > 0)
            .ok_or("Invalid source-round PID".to_string())
    };
    let pids = if owner.get("owner_pid").is_some() {
        if owner.get("pid").is_some() {
            return Err("Ambiguous source owner PID aliases".into());
        }
        let children = owner["children"]
            .as_array()
            .filter(|c| !c.is_empty() && c.len() <= 8)
            .ok_or("Missing or unbounded source controller PID receipt")?;
        let mut pids = vec![pid(&owner["owner_pid"])?];
        for child in children {
            pids.push(pid(child)?);
        }
        pids
    } else {
        if owner.get("children").is_some() {
            return Err("Source children lack an unambiguous owner".into());
        }
        vec![pid(&owner["pid"])?]
    };
    if pids.iter().collect::<BTreeSet<_>>().len() != pids.len() {
        return Err("Duplicate source owner/controller PID".into());
    }
    Ok(pids)
}

fn closed(root: &Path) -> Result<Plan> {
    let plan: Plan =
        serde_json::from_value(load(&ordinary(root, "plan.json")?)?).map_err(|e| e.to_string())?;
    plan.verify_sources()?; // Includes the older recovery ancestry, where present.
    let exit = load(&ordinary(root, "owner-exit.json")?)?;
    let result = load(&ordinary(root, "search-result.json")?)?;
    if exit["exit_code"] != 0
        || exit["automatic_resubmission"] != false
        || result["plan_sha256"] != digest(read(&root.join("plan.json"))?)
    {
        return Err("Source round is not a successfully closed, plan-bound experiment".into());
    }
    let owner = load(&ordinary(root, "owner-observed.json")?)?;
    let pid = |v: &Value| {
        v.as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .filter(|p| *p > 0)
            .ok_or("Invalid source-round PID".to_string())
    };
    let mut pids = owner_pids(&owner)?.into_iter().collect::<BTreeSet<_>>();
    for entry in fs::read_dir(ordinary(root, "operations")?).map_err(|e| e.to_string())? {
        let directory = entry.map_err(|e| e.to_string())?.path();
        let relative = directory
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_str()
            .ok_or("Non-UTF8 baseline path")?;
        sealed(root, relative)?; // Any unsettled operation blocks preparation.
        let submitted = directory.join("submitted.json");
        if submitted.exists() {
            pids.insert(pid(&load(&submitted)?["pid"])?);
        }
    }
    for pid in pids {
        absent(pid)?;
    }
    Ok(plan)
}

/// A failed review preflight must not repurchase a completed student control.
/// Unlike a successful search, this bounded predecessor contains one sealed
/// native observation and no submitted teacher/reviewer operation.
fn stopped_component(root: &Path) -> Result<(Plan, Value, Value, u64)> {
    let plan: Plan =
        serde_json::from_value(load(&ordinary(root, "plan.json")?)?).map_err(|e| e.to_string())?;
    plan.verify_sources()?;
    let exit = load(&ordinary(root, "owner-exit.json")?)?;
    if !plan.selected_stage_observation
        || plan.function != Function::ReadingJourney
        || plan.objective != Objective::SelectedStageReliability
        || plan.recovery.is_some()
        || exit["exit_code"] != 1
        || exit["automatic_resubmission"] != false
        || root.join("search-result.json").exists()
    {
        return Err(
            "Component continuation requires a stopped, unqualified selected-stage round".into(),
        );
    }
    let names = fs::read_dir(ordinary(root, "operations")?)
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.file_name()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    if names != ["00000"] {
        return Err("Component continuation accepts exactly one sealed native operation; any uncertain or paid reviewer work blocks it".into());
    }
    let (request, response) = sealed(root, "operations/00000")?;
    if request["plan_sha256"] != digest(read(&root.join("plan.json"))?)
        || request["kind"] != "reading_journey"
        || request["request"]["candidate"] != json!(plan.seed)
        || response["cache_identity"] != digest(request["request"].to_string())
    {
        return Err(
            "Completed component control is not the exact predecessor seed measurement".into(),
        );
    }
    crate::selected_stage::verify_observation(&plan, &response)?;
    let reservation = load(&ordinary(root, "operations/00000/reservation.json")?)?;
    let physical = reservation["physical_generation_attempts"]
        .as_u64()
        .filter(|p| *p > 0)
        .ok_or("Unknown inherited component reservation")?;
    if reservation["teacher_calls"] != 0 || reservation["review_calls"] != 0 {
        return Err("Component continuation must retain any paid teacher/reviewer work instead of replaying it".into());
    }
    let owner = load(&ordinary(root, "owner-observed.json")?)?;
    let mut pids = owner_pids(&owner)?;
    let child = load(&ordinary(root, "operations/00000/submitted.json")?)?["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or("No recorded native child PID")?;
    pids.push(child);
    for pid in pids {
        absent(pid)?;
    }
    Ok((plan, request, response, physical))
}

fn continuation_plan(original: &Plan, plan: &Plan, inherited: u64) -> Result<()> {
    continuation_plan_with_review(original, plan, inherited, 0)
}

fn continuation_plan_with_review(
    original: &Plan,
    plan: &Plan,
    inherited: u64,
    inherited_reviews: u64,
) -> Result<()> {
    let mut expected = original.clone();
    expected.controller_executable = plan.controller_executable.clone();
    expected.controller_executable_sha256 = plan.controller_executable_sha256.clone();
    expected.max_physical_generation_attempts = original
        .max_physical_generation_attempts
        .checked_sub(inherited)
        .ok_or("Inherited reservation exceeds the original bound")?;
    expected.max_review_calls = original
        .max_review_calls
        .checked_sub(inherited_reviews)
        .ok_or("Inherited review reservation exceeds the original bound")?;
    if serde_json::to_value(expected).map_err(|e| e.to_string())?
        != serde_json::to_value(plan).map_err(|e| e.to_string())?
    {
        return Err("Component continuation changed student inputs, native code, guide, pools, book, seed or original bounds".into());
    }
    Ok(())
}

fn review_artifacts(root: &Path) -> Result<Vec<Value>> {
    fn walk(root: &Path, relative: &str, files: &mut Vec<Value>) -> Result<()> {
        let path = ordinary(root, relative)?;
        if path.is_dir() {
            for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "Non-UTF8 review artifact")?;
                walk(root, &format!("{relative}/{name}"), files)?;
            }
        } else if path.is_file() {
            if files.len() >= 128 {
                return Err("Unbounded saved review tree".into());
            }
            files.push(json!({"file":relative,"sha256":digest(read(&path)?)}));
        } else {
            return Err("Nonordinary saved review artifact".into());
        }
        Ok(())
    }
    let mut files = Vec::new();
    for name in [
        "plan.json",
        "owner-exit.json",
        "owner-observed.json",
        "operations/00001",
    ] {
        walk(root, name, &mut files)?;
    }
    files.sort_by(|a, b| a["file"].as_str().cmp(&b["file"].as_str()));
    Ok(files)
}

fn verify_review_artifacts(root: &Path, artifacts: &Value) -> Result<()> {
    for a in artifacts
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 128)
        .ok_or("No bounded paid review source seals")?
    {
        verify(
            &ordinary(root, a["file"].as_str().ok_or("No paid source file")?)?,
            a["sha256"].as_str().ok_or("No paid source hash")?,
        )?;
    }
    if json!(review_artifacts(root)?) != *artifacts {
        return Err("Saved review artifacts changed or were added".into());
    }
    Ok(())
}

/// The provider completed this judge; only the host parser failed. Verify the
/// paid answer against the exact original wire contract instead of buying it again.
fn stopped_paid_review(
    root: &Path,
    native_root: &Path,
    original: &Plan,
    response: &Value,
    inherited: u64,
) -> Result<(Value, Value)> {
    let plan: Plan =
        serde_json::from_value(load(&ordinary(root, "plan.json")?)?).map_err(|e| e.to_string())?;
    plan.verify_sources()?;
    continuation_plan(original, &plan, inherited)?;
    let exit = load(&ordinary(root, "owner-exit.json")?)?;
    if exit["exit_code"] != 1
        || exit["automatic_resubmission"] != false
        || root.join("search-result.json").exists()
    {
        return Err("Paid review recovery requires a stopped unqualified owner".into());
    }
    let mut names = fs::read_dir(ordinary(root, "operations")?)
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.file_name()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    if names != ["00000", "00001"] {
        return Err("Additional or uncertain operations block saved review recovery".into());
    }
    let (native_request, saved_response) = sealed(root, "operations/00000")?;
    let ancestry = load(&ordinary(root, "operations/00000/cache-source.json")?)?;
    if ancestry["source_state"] != native_root.to_str().ok_or("Non-UTF8 native source")? {
        return Err("Paid review has another native control ancestry".into());
    }
    verify_component_import(&ancestry, &native_request, &saved_response)?;
    if saved_response != *response {
        return Err("Paid review changed the original native observation".into());
    }
    let dir = ordinary(root, "operations/00001")?;
    if dir.join("completed.json").exists() || dir.join("response.json").exists() {
        return Err("Use completed review replay, not partial-parser recovery".into());
    }
    let recorded = load(&ordinary(&dir, "request.json")?)?;
    let reservation = load(&ordinary(&dir, "reservation.json")?)?;
    if recorded["kind"] != "codex_reading_review"
        || recorded["plan_sha256"] != digest(read(&root.join("plan.json"))?)
        || reservation["review_calls"] != 1
        || reservation["teacher_calls"] != 0
        || reservation["physical_generation_attempts"] != 0
        || reservation["reservation_before_submission"] != true
    {
        return Err("Unknown paid review reservation or identity".into());
    }
    let id = response["outcome"]["id"].as_str().ok_or("No native case")?;
    let example = plan
        .training
        .iter()
        .chain(&plan.development)
        .find(|e| e.id == id)
        .ok_or("Paid review case is outside the original pool")?;
    for pid in owner_pids(&load(&ordinary(root, "owner-observed.json")?)?)?
        .into_iter()
        .chain([load(&ordinary(&dir, "submitted.json")?)?["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .filter(|p| *p > 0)
            .ok_or("No recorded paid reviewer PID")?])
    {
        absent(pid)?;
    }
    crate::selected_stage::recover_saved_review(&plan, example, response, &dir)
}

pub(crate) fn verify_component_review_import(
    receipt: &Value,
    request: &Value,
    response: &Value,
) -> Result<()> {
    if receipt["type"] != COMPONENT_REVIEW_IMPORT
        || receipt["new_paid_reservations"] != 0
        || receipt["inherited_review_reservations"] != 1
        || receipt["source_operation"] != "operations/00001"
    {
        return Err("Invalid saved selected-stage judge import".into());
    }
    let root = Path::new(
        receipt["source_state"]
            .as_str()
            .ok_or("No paid review source")?,
    );
    verify_review_artifacts(root, &receipt["source_artifacts"])?;
    let native_root = Path::new(
        receipt["native_source_state"]
            .as_str()
            .ok_or("No native source")?,
    );
    let (original, _, native_response, inherited) = stopped_component(native_root)?;
    let (logical, validated) =
        stopped_paid_review(root, native_root, &original, &native_response, inherited)?;
    if request["kind"] != "codex_reading_review"
        || request["request"] != logical
        || *response != validated
    {
        return Err("Saved judge identity, semantic answer or citations changed".into());
    }
    let file = Path::new(
        receipt["continuation_plan"]
            .as_str()
            .ok_or("No continuation plan")?,
    );
    let sha = receipt["continuation_plan_sha256"]
        .as_str()
        .ok_or("No continuation plan hash")?;
    verify(file, sha)?;
    if request["plan_sha256"] != sha {
        return Err("Judge import belongs to another journal".into());
    }
    let plan: Plan = serde_json::from_value(load(file)?).map_err(|e| e.to_string())?;
    continuation_plan_with_review(&original, &plan, inherited, 1)?;
    plan.verify_sources()
}

pub(crate) fn verify_component_import(
    receipt: &Value,
    request: &Value,
    response: &Value,
) -> Result<()> {
    if receipt["type"] != COMPONENT_IMPORT
        || receipt["source_operation"] != "operations/00000"
        || receipt["new_paid_reservations"] != 0
    {
        return Err("Invalid selected-stage control import".into());
    }
    let root = Path::new(
        receipt["source_state"]
            .as_str()
            .ok_or("No source component round")?,
    );
    for (file, field) in [
        ("plan.json", "source_plan_sha256"),
        ("owner-exit.json", "source_owner_exit_sha256"),
        ("owner-observed.json", "source_owner_observed_sha256"),
        ("operations/00000/completed.json", "source_completed_sha256"),
    ] {
        verify(
            &ordinary(root, file)?,
            receipt[field]
                .as_str()
                .ok_or("Missing component ancestry pin")?,
        )?;
    }
    let (original, recorded, answer, inherited) = stopped_component(root)?;
    if recorded["kind"] != request["kind"]
        || recorded["request"] != request["request"]
        || answer != *response
        || receipt["inherited_physical_reservations"] != inherited
    {
        return Err(
            "Component import changed the observed request, response or paid reservation".into(),
        );
    }
    let plan_file = Path::new(
        receipt["continuation_plan"]
            .as_str()
            .ok_or("No continuation plan pin")?,
    );
    let sha = receipt["continuation_plan_sha256"]
        .as_str()
        .ok_or("No continuation plan hash")?;
    verify(plan_file, sha)?;
    if request["plan_sha256"] != sha {
        return Err("Import belongs to a different continuation journal".into());
    }
    let plan: Plan = serde_json::from_value(load(plan_file)?).map_err(|e| e.to_string())?;
    let inherited_reviews = receipt["inherited_review_reservations"]
        .as_u64()
        .unwrap_or(0);
    if inherited_reviews == 1 {
        let paid_root = Path::new(
            receipt["paid_review_source"]
                .as_str()
                .ok_or("No inherited paid review")?,
        );
        verify_review_artifacts(paid_root, &receipt["paid_source_artifacts"])?;
        stopped_paid_review(paid_root, root, &original, &answer, inherited)?;
    } else if inherited_reviews != 0 {
        return Err("Unknown inherited review count".into());
    }
    continuation_plan_with_review(&original, &plan, inherited, inherited_reviews)?;
    plan.verify_sources()?;
    Ok(())
}

/// New controller, unchanged measurement program, retained hard reservations.
/// Preparation performs no student, teacher or judge invocation.
pub fn prepare_component_continuation(
    source: &Path,
    state: &Path,
    controller: &Path,
    paid_review_source: Option<&Path>,
) -> Result<Plan> {
    if state.exists() {
        return Err("Continuation state already exists; preserve it".into());
    }
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    let (original, request, response, inherited) = stopped_component(&source)?;
    let paid_review_source = paid_review_source
        .map(fs::canonicalize)
        .transpose()
        .map_err(|e| e.to_string())?;
    let paid = paid_review_source
        .as_ref()
        .map(|root| stopped_paid_review(root, &source, &original, &response, inherited))
        .transpose()?;
    let inherited_reviews = u64::from(paid.is_some());
    let mut plan = original.clone();
    plan.max_physical_generation_attempts = plan
        .max_physical_generation_attempts
        .checked_sub(inherited)
        .ok_or("Inherited reservations exceed the original bound")?;
    plan.max_review_calls = plan
        .max_review_calls
        .checked_sub(inherited_reviews)
        .ok_or("Inherited review exceeds the original bound")?;
    fs::create_dir_all(state.join("executables")).map_err(|e| e.to_string())?;
    let copied = state.join("executables/horary-gepa");
    fs::copy(controller, &copied).map_err(|e| e.to_string())?;
    plan.controller_executable = fs::canonicalize(copied).map_err(|e| e.to_string())?;
    plan.controller_executable_sha256 = digest(read(&plan.controller_executable)?);
    continuation_plan_with_review(&original, &plan, inherited, inherited_reviews)?;
    plan.verify_sources()?;
    crate::selected_stage::preflight(&plan)?;
    crate::selected_stage::preflight_observation(&plan, &response)?;
    keep(&state.join("plan.json"), &plan)?;
    let mut journal = Journal::open(state, &plan)?;
    let Operation::Fresh(directory) =
        journal.begin("reading_journey", &request["request"], 0, 0)?
    else {
        return Err("New continuation unexpectedly contains an operation".into());
    };
    let mut receipt = json!({"type":COMPONENT_IMPORT,"source_state":source,"source_operation":"operations/00000",
        "source_plan_sha256":digest(read(&source.join("plan.json"))?),
        "source_owner_exit_sha256":digest(read(&source.join("owner-exit.json"))?),
        "source_owner_observed_sha256":digest(read(&source.join("owner-observed.json"))?),
        "source_completed_sha256":digest(read(&source.join("operations/00000/completed.json"))?),
        "continuation_plan":state.join("plan.json").canonicalize().map_err(|e|e.to_string())?,
        "continuation_plan_sha256":digest(read(&state.join("plan.json"))?),
        "new_paid_reservations":0,"inherited_physical_reservations":inherited});
    if let Some(root) = &paid_review_source {
        receipt["paid_review_source"] = json!(root);
        receipt["paid_source_artifacts"] = json!(review_artifacts(root)?);
        receipt["inherited_review_reservations"] = json!(inherited_reviews);
    }
    verify_component_import(&receipt, &load(&directory.join("request.json"))?, &response)?;
    keep(&directory.join("cache-source.json"), &receipt)?;
    journal.finish(&directory, &response)?;
    fs::create_dir_all(state.join("evaluation-cache")).map_err(|e| e.to_string())?;
    keep(
        &state.join("evaluation-cache").join(format!(
            "{}.json",
            response["cache_identity"]
                .as_str()
                .ok_or("No exact control identity")?
        )),
        &json!({"operation":"operations/00000","completed_sha256":digest(read(&directory.join("completed.json"))?)}),
    )?;
    keep(&state.join("component-continuation.json"), &receipt)?;
    if let Some((logical, validated)) = paid {
        let root = paid_review_source.as_ref().ok_or("No paid source")?;
        let Operation::Fresh(directory) = journal.begin_reading_review(&logical, false)? else {
            return Err("New continuation unexpectedly contains a paid judge operation".into());
        };
        let paid_receipt = json!({"type":COMPONENT_REVIEW_IMPORT,"source_state":root,
            "native_source_state":source,"source_operation":"operations/00001",
            "source_artifacts":review_artifacts(root)?,"inherited_review_reservations":1,
            "new_paid_reservations":0,"continuation_plan":state.join("plan.json").canonicalize().map_err(|e|e.to_string())?,
            "continuation_plan_sha256":digest(read(&state.join("plan.json"))?)});
        verify_component_review_import(
            &paid_receipt,
            &load(&directory.join("request.json"))?,
            &validated,
        )?;
        keep(&directory.join("cache-source.json"), &paid_receipt)?;
        journal.finish(&directory, &validated)?;
        fs::create_dir_all(state.join("selected-stage-review-cache")).map_err(|e| e.to_string())?;
        keep(
            &state
                .join("selected-stage-review-cache")
                .join(format!("{}.json", digest(logical.to_string()))),
            &json!({"operation":"operations/00001","completed_sha256":digest(read(&directory.join("completed.json"))?)}),
        )?;
        keep(&state.join("paid-review-continuation.json"), &paid_receipt)?;
    }
    Ok(plan)
}

fn seed_measurement(plan: &Plan, request: &Value, response: &Value, id: &str) -> Result<()> {
    if request["kind"] != "input_journey"
        || request["request"]["candidate"] != json!(plan.seed)
        || request["request"]["case"]["id"] != id
        || response["outcome"]["id"] != id
        || response["cache_identity"] != digest(request["request"].to_string())
    {
        return Err("Only the exact seed's completed case measurement may be reused".into());
    }
    metric::require_completed(&response["outcome"], "input_journey")
}

/// Called on journal replay and status, not just when the source was copied.
pub(crate) fn verify_import(receipt: &Value, request: &Value, response: &Value) -> Result<()> {
    if receipt["type"] != IMPORT || receipt["new_paid_reservations"] != 0 {
        return Err("Invalid baseline-control reuse receipt".into());
    }
    let root = Path::new(
        receipt["source_state"]
            .as_str()
            .ok_or("Missing baseline source")?,
    );
    // Verify the immutable PID and closure witnesses before consulting them.
    for (file, field) in [
        ("owner-observed.json", "source_owner_observed_sha256"),
        ("search-result.json", "source_search_result_sha256"),
    ] {
        verify(
            &ordinary(root, file)?,
            receipt[field]
                .as_str()
                .ok_or("Missing pinned baseline closure/PID witness")?,
        )?;
    }
    verify(
        &root.join("plan.json"),
        receipt["source_plan_sha256"]
            .as_str()
            .ok_or("Missing source-plan hash")?,
    )?;
    let _plan = closed(root)?;
    verify(
        &root.join("owner-exit.json"),
        receipt["source_owner_exit_sha256"]
            .as_str()
            .ok_or("Missing closed-owner hash")?,
    )?;
    let operation = receipt["source_operation"]
        .as_str()
        .ok_or("Missing source operation")?;
    verify(
        &ordinary(root, &format!("{operation}/completed.json"))?,
        receipt["source_completed_sha256"]
            .as_str()
            .ok_or("Missing source seal hash")?,
    )?;
    let (original, answer) = sealed(root, operation)?;
    if original["kind"] != request["kind"]
        || original["request"] != request["request"]
        || answer != *response
    {
        return Err("Baseline reuse changed its request or observed answer".into());
    }
    Ok(())
}

/// The initial pilot's two controls are fixed, with separate training/development.
/// New paid work is capped at one feedback-inclusive reflection and two trials.
pub fn prepare(source: &Path, state: &Path, controller: &Path) -> Result<Plan> {
    if state.exists() {
        return Err("Feedback trial state already exists; inspect it, never overwrite it".into());
    }
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    let original = closed(&source)?;
    if original.function != Function::InputJourney
        || original.objective != Objective::ExtractorReliability
        || original.training.len() != 1
        || original.development.len() != 1
        || original.seed.len() != 1
    {
        return Err("Control reuse is bounded to the existing single-component input pilot".into());
    }
    let mut pairs = Vec::new();
    for (number, example) in [(0, &original.development[0]), (2, &original.training[0])] {
        let native_name = format!("operations/{number:05}");
        let review_name = format!("operations/{:05}", number + 1);
        let (request, evaluation) = sealed(&source, &native_name)?;
        seed_measurement(&original, &request, &evaluation, &example.id)?;
        let (review_request, review) = sealed(&source, &review_name)?;
        if review_request["kind"] != "codex_input_review"
            || review["case_id"] != example.id
            || review_request["request"]["native_evaluation_sha256"]
                != digest(evaluation.to_string())
        {
            return Err("Baseline review belongs to a different observed native record".into());
        }
        // Revalidate the previously paid answer against the exact current packet.
        crate::review::validate_saved(&original, example, &evaluation, &review)?;
        pairs.push((native_name, request, evaluation));
        pairs.push((review_name, review_request, review));
    }
    let mut plan = original.clone();
    plan.recovery = None;
    plan.max_teacher_calls = 1;
    plan.max_review_calls = 2;
    plan.max_physical_generation_attempts = plan
        .logical_calls_per_function
        .checked_mul(plan.function.generation_factor())
        .and_then(|n| n.checked_mul(2))
        .ok_or("Trial budget overflow")?;
    plan.max_metric_calls = 3;
    fs::create_dir_all(state.join("executables")).map_err(|e| e.to_string())?;
    let copied = state.join("executables/horary-gepa");
    fs::copy(controller, &copied).map_err(|e| e.to_string())?;
    plan.controller_executable = fs::canonicalize(copied).map_err(|e| e.to_string())?;
    plan.controller_executable_sha256 = digest(read(&plan.controller_executable)?);
    plan.verify_sources()?;
    keep(&state.join("plan.json"), &plan)?;
    keep(
        &state.join("paid-control-reuse.json"),
        &json!({"source_state":source,
        "source_plan_sha256":digest(read(&source.join("plan.json"))?),"source_bound_controls":4,
        "separate_experiment":true,"inherited_observations_not_new_calls":true,
        "original_paid_ledger_unchanged":true,"fresh_budget":{"teacher":1,"review":2,"physical_reservations":plan.max_physical_generation_attempts},
        "qualification":"New feedback-inclusive writer invocation; original blind writer and rejected child remain intact. Cached controls are not replications."}),
    )?;
    let mut journal = Journal::open(state, &plan)?;
    for (operation, request, response) in &pairs {
        let Operation::Fresh(directory) = journal.begin(
            request["kind"].as_str().ok_or("Missing source kind")?,
            &request["request"],
            0,
            0,
        )?
        else {
            return Err("Fresh trial unexpectedly reused an operation".into());
        };
        let receipt = json!({"type":IMPORT,"source_state":source,"source_operation":operation,
            "source_plan_sha256":digest(read(&source.join("plan.json"))?),
            "source_owner_exit_sha256":digest(read(&source.join("owner-exit.json"))?),
            "source_owner_observed_sha256":digest(read(&source.join("owner-observed.json"))?),
            "source_search_result_sha256":digest(read(&source.join("search-result.json"))?),
            "source_completed_sha256":digest(read(&source.join(operation).join("completed.json"))?),"new_paid_reservations":0});
        verify_import(&receipt, &load(&directory.join("request.json"))?, response)?;
        keep(&directory.join("cache-source.json"), &receipt)?;
        journal.finish(&directory, response)?;
        let cache_kind = if request["kind"] == "input_journey" {
            "evaluation-cache"
        } else {
            "review-cache"
        };
        fs::create_dir_all(state.join(cache_kind)).map_err(|e| e.to_string())?;
        keep(
            &state
                .join(cache_kind)
                .join(format!("{}.json", digest(request["request"].to_string()))),
            &json!({"operation":directory.strip_prefix(state).map_err(|e|e.to_string())?,"completed_sha256":digest(read(&directory.join("completed.json"))?)}),
        )?;
    }
    // Only the training pair reaches the writer; development stays quarantined.
    let feedback = metric::grade(&pairs[2].2["outcome"], Some(&pairs[3].2), plan.metric())?;
    let captured = vec![(
        plan.training[0].clone(),
        json!({"evaluation":pairs[2].2,"fitness":feedback,"independent_review":pairs[3].2}),
    )];
    let component = plan.seed.keys().cloned().collect::<Vec<_>>();
    let prepared = teacher::prepare(&plan, &plan.seed, &component, &captured)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(state.join("reflection-preview.txt"))
        .map_err(|e| e.to_string())?;
    file.write_all(prepared.prompt.as_bytes())
        .map_err(|e| e.to_string())?;
    keep(
        &state.join("reflection-preview-coverage.json"),
        &prepared.coverage,
    )?;
    keep(
        &state.join("offline-preflight.json"),
        &json!({"new_paid_calls":0,"new_paid_reservations":0,
        "training_ids":[plan.training[0].id],"development_not_in_reflection":true,
        "prompt_bytes":prepared.prompt.len(),"prompt_sha256":digest(&prepared.prompt),
        "actual_initial_outputs_and_repairs_included":true,"original_guidance_and_output_schema_included":true,
        "qualification":"Compiled actual final-stdin assembly and sealed control reuse; not writer quality or model improvement."}),
    )?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paid_review_continuation_deducts_the_review_without_resetting_any_bound() {
        let mut original = crate::tests::plan();
        original.max_review_calls = 8;
        let mut current = original.clone();
        current.controller_executable = "/new/controller".into();
        current.controller_executable_sha256 = digest("new controller");
        current.max_physical_generation_attempts -= 9;
        current.max_review_calls -= 1;
        continuation_plan_with_review(&original, &current, 9, 1).unwrap();
        assert!(continuation_plan(&original, &current, 9).is_err());
        for field in ["physical", "review", "teacher", "metric"] {
            let mut changed = current.clone();
            match field {
                "physical" => changed.max_physical_generation_attempts += 1,
                "review" => changed.max_review_calls += 1,
                "teacher" => changed.max_teacher_calls += 1,
                "metric" => changed.max_metric_calls += 1,
                _ => unreachable!(),
            }
            assert!(continuation_plan_with_review(&original, &changed, 9, 1).is_err());
        }
    }

    #[test]
    fn paid_review_tree_changes_fail_before_revalidation_or_cache_reuse() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("operations/00001/sources")).unwrap();
        for file in [
            "plan.json",
            "owner-exit.json",
            "owner-observed.json",
            "operations/00001/answer.json",
            "operations/00001/sources/book-source.json",
        ] {
            fs::write(root.path().join(file), "{}").unwrap();
        }
        let sealed = json!(review_artifacts(root.path()).unwrap());
        verify_review_artifacts(root.path(), &sealed).unwrap();
        fs::write(
            root.path().join("operations/00001/answer.json"),
            "{\"drift\":true}",
        )
        .unwrap();
        assert!(verify_review_artifacts(root.path(), &sealed)
            .unwrap_err()
            .contains("Source changed"));
        fs::write(root.path().join("operations/00001/answer.json"), "{}").unwrap();
        fs::write(root.path().join("operations/00001/extra.json"), "{}").unwrap();
        assert!(verify_review_artifacts(root.path(), &sealed)
            .unwrap_err()
            .contains("were added"));
    }

    #[test]
    fn paid_review_import_requires_exact_operation_and_zero_new_charges() {
        let mut receipt = json!({"type":COMPONENT_REVIEW_IMPORT,"source_operation":"operations/00000",
            "new_paid_reservations":0,"inherited_review_reservations":1});
        assert!(
            verify_component_review_import(&receipt, &json!({}), &json!({}))
                .unwrap_err()
                .contains("Invalid saved")
        );
        receipt["source_operation"] = json!("operations/00001");
        receipt["new_paid_reservations"] = json!(1);
        assert!(
            verify_component_review_import(&receipt, &json!({}), &json!({}))
                .unwrap_err()
                .contains("Invalid saved")
        );
    }

    #[test]
    fn component_continuation_retains_inputs_and_deducts_original_reservations() {
        let original = crate::tests::plan();
        let mut current = original.clone();
        current.controller_executable = "/new/controller".into();
        current.controller_executable_sha256 = digest("new controller");
        current.max_physical_generation_attempts -= 9;
        continuation_plan(&original, &current, 9).unwrap();
        let mut changed = current.clone();
        changed.max_physical_generation_attempts += 1;
        assert!(continuation_plan(&original, &changed, 9).is_err());
        changed = current.clone();
        changed.native_executable_sha256 = digest("changed native program");
        assert!(continuation_plan(&original, &changed, 9).is_err());
        changed = current.clone();
        changed.guide.push_str("undocumented teaching change");
        assert!(continuation_plan(&original, &changed, 9).is_err());
        changed = current.clone();
        changed.development[0].expected = json!({"changed_gold":true});
        assert!(continuation_plan(&original, &changed, 9).is_err());
        changed = current.clone();
        changed.rng_seed += 1;
        assert!(continuation_plan(&original, &changed, 9).is_err());
        changed = current;
        changed.max_teacher_calls += 1;
        assert!(continuation_plan(&original, &changed, 9).is_err());
        assert!(continuation_plan(&original, &changed, u64::MAX).is_err());
    }

    #[test]
    fn component_import_verifies_closure_pins_before_using_source_evidence() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("plan.json"), "{}").unwrap();
        let receipt = json!({"type":COMPONENT_IMPORT,"source_operation":"operations/00000","new_paid_reservations":0,
            "source_state":root.path(),"source_plan_sha256":digest("{}")});
        fs::write(root.path().join("plan.json"), "{\"drift\":true}").unwrap();
        let error = verify_component_import(&receipt, &json!({}), &json!({})).unwrap_err();
        assert!(error.contains("Source changed"), "{error}");
        assert!(error.contains("plan.json"), "{error}");
        let mut charged = receipt;
        charged["new_paid_reservations"] = json!(1);
        assert!(verify_component_import(&charged, &json!({}), &json!({}))
            .unwrap_err()
            .contains("Invalid selected-stage"));
    }
    #[test]
    fn recorded_live_owners_and_ambiguous_aliases_block_control_reuse() {
        assert_eq!(
            owner_pids(&json!({"owner_pid":1,"children":[2]})).unwrap(),
            vec![1, 2]
        );
        assert_eq!(owner_pids(&json!({"pid":1})).unwrap(), vec![1]);
        for owner in [
            json!({"owner_pid":1,"pid":2,"children":[3]}),
            json!({"owner_pid":1}),
            json!({"pid":1,"children":[2]}),
            json!({"owner_pid":1,"children":[]}),
            json!({"owner_pid":1,"children":[1]}),
        ] {
            assert!(owner_pids(&owner).is_err());
        }
        assert!(absent(std::process::id()).is_err());
    }
    #[test]
    fn changed_pid_and_result_witnesses_fail_before_loading_or_trusting_source_plan() {
        for changed in ["owner-observed.json", "search-result.json"] {
            let root = tempfile::tempdir().unwrap();
            for name in ["owner-observed.json", "search-result.json"] {
                fs::write(root.path().join(name), "{}").unwrap();
            }
            let receipt = json!({"type":IMPORT,"new_paid_reservations":0,"source_state":root.path(),
                "source_owner_observed_sha256":digest("{}"),"source_search_result_sha256":digest("{}")});
            fs::write(root.path().join(changed), "{\"changed\":true}").unwrap();
            let error = verify_import(&receipt, &json!({}), &json!({})).unwrap_err();
            assert!(error.contains("Source changed"), "{error}");
            assert!(error.contains(changed), "{error}");
            // No source plan exists: the witnessed drift failed before the
            // loader could use a substituted PID receipt or launch anything.
        }
    }
    #[test]
    fn changed_program_and_unobserved_measurements_cannot_become_seed_controls() {
        let plan = crate::tests::plan();
        let request =
            json!({"kind":"input_journey","request":{"candidate":plan.seed,"case":{"id":"train"}}});
        let mut response = json!({"cache_identity":digest(request["request"].to_string()),"outcome":{"id":"train"}});
        assert!(seed_measurement(&plan, &request, &response, "train").is_err());
        response["outcome"]["execution_completed"] = json!(true);
        let mut changed = request.clone();
        changed["request"]["candidate"] = json!({"other":"changed teaching"});
        assert!(seed_measurement(&plan, &changed, &response, "train")
            .unwrap_err()
            .contains("exact seed"));
        assert!(seed_measurement(&plan, &request, &response, "development").is_err());
    }
    #[test]
    fn seal_checks_are_source_bound_and_reject_missing_or_changed_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let d = root.path().join("operations/00000");
        fs::create_dir_all(&d).unwrap();
        assert!(sealed(root.path(), "operations/00000").is_err());
        let request = json!({"kind":"input_journey"});
        let response = json!({"observed":"saved"});
        let reservation = json!({"physical_generation_attempts":1});
        for (name, value) in [
            ("request.json", &request),
            ("response.json", &response),
            ("reservation.json", &reservation),
        ] {
            keep(&d.join(name), value).unwrap();
        }
        let artifacts = ["request.json", "response.json", "reservation.json"]
            .into_iter()
            .map(|file| json!({"file":file,"sha256":digest(read(&d.join(file)).unwrap())}))
            .collect::<Vec<_>>();
        keep(&d.join("completed.json"),&json!({"artifacts":artifacts,"response_sha256":digest(read(&d.join("response.json")).unwrap())})).unwrap();
        assert_eq!(
            sealed(root.path(), "operations/00000").unwrap(),
            (request, response)
        );
        fs::write(d.join("response.json"), "{}").unwrap();
        assert!(sealed(root.path(), "operations/00000").is_err());
        assert!(sealed(root.path(), "../elsewhere").is_err());
    }
}
