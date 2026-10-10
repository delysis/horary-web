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

fn ordinary(base: &Path, relative: &str) -> Result<PathBuf> {
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

fn sealed(root: &Path, operation: &str) -> Result<(Value, Value)> {
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

fn absent(pid: u32) -> Result<()> {
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

fn owner_pids(owner: &Value) -> Result<Vec<u32>> {
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
