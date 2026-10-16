//! A new, bounded component curriculum, never a repaired whole-journey score.
//! Complete controls are reused only by exact logical request. The excluded
//! upstream journey stays unsealed/unscored in its original journal.
use crate::{
    controls, keep, load, native, read, selected_stage, verify, ControlMode, Function, Objective,
    Plan, Result,
};
use horary_prompt_program::digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub(crate) const IMPORT: &str = "sealed_component_curriculum_control_import";
const SCOPE: &str = "complete_input_component_curriculum";
const QUALIFICATION: &str = "Separate component curriculum. The original target-uninvoked input journey remains unscored and failed at the whole-application gate. Exact seed controls are reused; changed target programs run on original questions afresh. No full-reading, source-interpretation or promotion qualification.";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub file: PathBuf,
    pub sha256: String,
}

struct Control {
    operation: String,
    request: Value,
    response: Value,
}
struct Context {
    plan: Plan,
    controls: Vec<Control>,
    excluded: String,
    physical: u64,
    reviews: u64,
}

fn hash_files(root: &Path, relative: &str, files: &mut Vec<Value>) -> Result<()> {
    let path = controls::ordinary(root, relative)?;
    if path.is_dir() {
        let mut names = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.file_name()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        for name in names {
            let name = name.to_str().ok_or("Non-UTF8 curriculum artifact")?;
            hash_files(root, &format!("{relative}/{name}"), files)?;
        }
    } else if path.is_file() && files.len() < 2048 {
        files.push(json!({"file":relative,"sha256":digest(read(&path)?)}));
    } else {
        return Err("Nonordinary or unbounded curriculum source tree".into());
    }
    Ok(())
}

fn source_artifacts(root: &Path) -> Result<Value> {
    let mut files = Vec::new();
    for name in [
        "plan.json",
        "owner-exit.json",
        "owner-observed.json",
        "operations",
    ] {
        hash_files(root, name, &mut files)?;
    }
    Ok(json!(files))
}

fn verify_artifacts(root: &Path, artifacts: &Value) -> Result<()> {
    let rows = artifacts
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 2048)
        .ok_or("No bounded curriculum source pins")?;
    for a in rows {
        verify(
            &controls::ordinary(root, a["file"].as_str().ok_or("No curriculum source file")?)?,
            a["sha256"].as_str().ok_or("No curriculum source hash")?,
        )?;
    }
    if source_artifacts(root)? != *artifacts {
        return Err("Curriculum source files changed or were added".into());
    }
    Ok(())
}

fn pid(value: &Value) -> Result<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or("Unknown curriculum child PID".into())
}
fn component(plan: &Plan) -> Result<()> {
    if plan.function != Function::ReadingJourney
        || plan.objective != Objective::SelectedStageReliability
        || !plan.selected_stage_observation
        || plan.initial_extractor_observation
        || plan.control_mode != ControlMode::FreshHosted
        || plan.recovery.is_some()
    {
        return Err("Curriculum requires an authentic fresh selected-stage component plan".into());
    }
    Ok(())
}

/// A failed whole journey cannot become a component zero or a repaired result.
fn exclusion_header(plan: &Plan, outcome: &Value, records: &[Value], count: u64) -> Result<()> {
    let stage = plan.signature().stage;
    let o = &outcome["selected_stage"];
    if outcome["scope"] != "selected_stage_function_only"
        || outcome["execution_status"] != "selected_stage_unobserved"
        || outcome["selected_stage_observation"] != true
        || outcome["full_reading"] != false
        || outcome["target_stage"] != stage
        || outcome["target_method"] != json!(plan.target_method)
        || outcome["target_function_invoked"] != false
        || outcome["target_stage_authentic_inputs"] != false
        || outcome["captured_checkpoint_reused"] != false
        || outcome["fresh_initial_sha256_verified"] != true
        || outcome["physical_generation_attempts_complete"] != true
        || outcome["deadline_cancelled"] != false
        || outcome["group_cancelled"] != false
        || o["version"] != 1
        || o["state"] != "unobserved"
        || o["target_attempts"] != 0
        || o["all_provider_responses_settled"] != true
        || o["request_result_pairs_complete"] != true
        || o["last_sequence"] != count
        || o["reading_delivered"] != false
        || o["post_boundary_conversation_executed"] != false
        || ["provider_stop", "infrastructure_error", "execution_error"]
            .iter()
            .any(|k| outcome.get(*k).is_none_or(|v| !v.is_null()))
        || records.iter().any(|r| r["stage"] == stage)
    {
        return Err(
            "Excluded target is invoked, uncertain or not an authenticated unobserved boundary"
                .into(),
        );
    }
    Ok(())
}

fn exclusion_state(
    outcome: &Value,
    final_state: &Value,
    case_state: &Value,
    observation: &Value,
    case_dir: &Path,
) -> Result<()> {
    let mut expected = case_state.clone();
    expected["scope"] = json!("reading_journey_function_only");
    expected["full_reading"] = json!(false);
    expected["native_evidence_directory"] = json!(case_dir);
    let result = &case_state["session"]["method"]["result"];
    if *final_state != expected
        || outcome["selected_stage"] != *observation
        || result["result"] != "needs_information"
        || !result["need"].is_object()
    {
        return Err(
            "Excluded operation must retain its actual NeedsInformation state and native copies"
                .into(),
        );
    }
    Ok(())
}

fn excluded(plan: &Plan, root: &Path, closure: &Value) -> Result<String> {
    let row = &closure["unscored_operation"];
    let index = row["operation"]
        .as_u64()
        .ok_or("No unscored source operation")?;
    let case = row["case_id"].as_str().ok_or("No excluded source case")?;
    let example = plan
        .training
        .iter()
        .find(|e| e.id == case)
        .ok_or("Only original training can be excluded")?;
    let relative = format!("operations/{index:05}");
    let directory = controls::ordinary(root, &relative)?;
    if directory.join("completed.json").exists()
        || directory.join("response.json").exists()
        || row["native_exit"] != 0
        || row["all_calls_settled"] != true
        || row["target_attempts"] != 0
        || !row.get("component_score").is_some_and(Value::is_null)
    {
        return Err(
            "Unscored closure must preserve the original unsealed target-uninvoked operation"
                .into(),
        );
    }
    let pins = row["artifacts"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 512)
        .ok_or("No excluded operation source pins")?;
    let mut names = BTreeSet::new();
    for a in pins {
        let file = a["file"].as_str().ok_or("No excluded source file")?;
        if !names.insert(file) {
            return Err("Duplicate excluded artifact".into());
        }
        verify(
            &controls::ordinary(&directory, file)?,
            a["sha256"].as_str().ok_or("No excluded source hash")?,
        )?;
    }
    let mut actual = Vec::new();
    hash_files(root, &relative, &mut actual)?;
    let actual_names: BTreeSet<_> = actual
        .iter()
        .map(|a| {
            a["file"]
                .as_str()
                .and_then(|s| s.strip_prefix(&format!("{relative}/")))
                .ok_or_else(|| "Foreign excluded artifact".to_owned())
        })
        .collect::<Result<_>>()?;
    if names != actual_names {
        return Err("Excluded operation files changed or were added".into());
    }
    let request = load(&directory.join("request.json"))?;
    if request["kind"] != "reading_journey"
        || request["plan_sha256"] != digest(read(&root.join("plan.json"))?)
        || request["request"] != native::logical_request(plan, example, &plan.seed)?
        || load(&directory.join("exit.json"))? != json!({"code":0,"success":true})
    {
        return Err("Excluded native request or successful exit changed".into());
    }
    let task = load(&directory.join("task.json"))?;
    if task["case_id"] != case
        || task["selected_stage_observation"] != true
        || task["target_stage"] != json!(plan.target_stage)
        || task["target_method"] != json!(plan.target_method)
        || task["source_initial_sha256"] != example.source_initial_sha256
        || task["source_fixture_sha256"] != example.source_fixture_sha256
        || task["source_origin_sha256"] != example.source_origin_sha256
        || task["source_request_sha256"] != example.source_request_sha256
        || task["source_case_directory"] != json!(example.source_case_directory)
        || task["source_request_file"] != json!(example.source_request_file)
        || task["source_rubric_sha256"] != json!(example.source_rubric_sha256)
        || task["source_branch"] != json!(example.source_branch)
        || task["baseline_manifest_sha256"] != plan.manifest_sha256
    {
        return Err("Excluded task changed the actual original question or source identity".into());
    }
    let mut command = std::process::Command::new(&plan.native_executable);
    command.args([plan.function.entry(), "--exact", "--ignored", "--nocapture"]);
    if load(&directory.join("prepared.json"))?
        != json!({"executable":command.get_program(),
        "args":command.get_args().collect::<Vec<_>>(),"deadline_seconds":plan.function_seconds.saturating_add(30)})
    {
        return Err("Excluded operation changed native executable or bounded invocation".into());
    }
    let native = directory.join("native");
    let case_dir = native.join("cases").join(case);
    verify(
        &case_dir.join("initial.json"),
        &example.source_initial_sha256,
    )?;
    verify(
        &case_dir.join("fixture.json"),
        &example.source_fixture_sha256,
    )?;
    let count = crate::reading::verify_settled_calls(&case_dir)?;
    let final_state = load(&native.join("final.json"))?;
    let outcome = load(&native.join("outcome.json"))?;
    if outcome["id"] != case {
        return Err("Excluded native end state changed case".into());
    }
    exclusion_state(
        &outcome,
        &final_state,
        &load(&case_dir.join("final.json"))?,
        &load(&case_dir.join("selected-stage-observation.json"))?,
        &case_dir,
    )?;
    exclusion_header(
        plan,
        &outcome,
        final_state["session"]["method"]["records"]
            .as_array()
            .ok_or("No actual excluded records")?,
        count,
    )?;
    let pairs = row["call_pairs"]
        .as_array()
        .filter(|p| p.len() == usize::try_from(count).unwrap_or(usize::MAX))
        .ok_or("Closure omits actual settled call pairs")?;
    for call in 1..=count {
        let request_file = case_dir
            .join("calls")
            .join(format!("{call:04}-request.json"));
        let result_file = case_dir
            .join("calls")
            .join(format!("{call:04}-result.json"));
        let pair = pairs
            .get(usize::try_from(call - 1).map_err(|e| e.to_string())?)
            .ok_or("Missing settled pair")?;
        let request = load(&request_file)?;
        let result = load(&result_file)?;
        if pair["sequence"] != call
            || pair["request_sha256"] != digest(read(&request_file)?)
            || pair["result_sha256"] != digest(read(&result_file)?)
            || pair["stage"] != request["stage"]
            || pair["memo_reused"]
                != json!(
                    !result["response_reuse"].is_null()
                        || result["response_reuses"]
                            .as_array()
                            .is_some_and(|rows| rows.iter().any(|r| !r.is_null()))
                )
        {
            return Err("Closure call pairs differ from actual settled source files".into());
        }
        if !crate::reading::matching_tasks(plan, &request)?.is_empty() {
            return Err("Excluded task actually invoked the target".into());
        }
    }
    controls::absent(pid(&load(&directory.join("submitted.json"))?["pid"])?)?;
    Ok(case.into())
}

fn source(root: &Path, settlement: &Pin) -> Result<Context> {
    if !root.is_absolute()
        || !fs::symlink_metadata(root)
            .map_err(|e| e.to_string())?
            .is_dir()
    {
        return Err("Curriculum source must be an ordinary absolute directory".into());
    }
    verify(&settlement.file, &settlement.sha256)?;
    let closure = load(&settlement.file)?;
    let plan: Plan = serde_json::from_value(load(&controls::ordinary(root, "plan.json")?)?)
        .map_err(|e| e.to_string())?;
    component(&plan)?;
    if plan.component_curriculum.is_some() {
        return Err("Nested curricula require a separate reviewed experiment".into());
    }
    plan.verify_sources()?;
    if closure["version"] != 1
        || closure["source_state"] != json!(root)
        || closure["source_plan_sha256"] != digest(read(&root.join("plan.json"))?)
        || closure["owner_exit_sha256"] != digest(read(&root.join("owner-exit.json"))?)
        || load(&root.join("owner-exit.json"))?
            != json!({"exit_code":1,"automatic_resubmission":false})
        || root.join("search-result.json").exists()
    {
        return Err("Curriculum source is not the pinned stopped unqualified round".into());
    }
    for p in controls::owner_pids(&load(&controls::ordinary(root, "owner-observed.json")?)?)? {
        controls::absent(p)?;
    }
    let excluded = excluded(&plan, root, &closure)?;
    let last = closure["unscored_operation"]["operation"]
        .as_u64()
        .ok_or("Unknown excluded sequence")?;
    if last == 0 || last > 32 || last % 2 != 0 {
        return Err("Curriculum source must contain bounded native/reviewer control pairs and one unscored tail".into());
    }
    let mut names = fs::read_dir(controls::ordinary(root, "operations")?)
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.file_name()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    if names
        != (0..=last)
            .map(|n| std::ffi::OsString::from(format!("{n:05}")))
            .collect::<Vec<_>>()
    {
        return Err("Unknown additional source operation blocks curriculum".into());
    }
    let seals = closure["sealed_completed_operations"]
        .as_array()
        .filter(|a| a.len() == usize::try_from(last).unwrap_or(usize::MAX))
        .ok_or("Closure lacks exact completed control seals")?;
    let mut controls_out: Vec<Control> = Vec::new();
    let mut physical = 0_u64;
    let mut reviews = 0_u64;
    let mut cases = BTreeSet::new();
    for n in 0..=last {
        let name = format!("operations/{n:05}");
        let directory = root.join(&name);
        let reservation = load(&controls::ordinary(&directory, "reservation.json")?)?;
        if reservation["reservation_before_submission"] != true || reservation["teacher_calls"] != 0
        {
            return Err("Unknown or paid-teacher source work cannot be reset".into());
        }
        let reserved_physical = reservation["physical_generation_attempts"]
            .as_u64()
            .ok_or("Unknown physical reservation")?;
        let reserved_reviews = reservation["review_calls"]
            .as_u64()
            .ok_or("Unknown review reservation")?;
        if (n % 2 == 0 && reserved_reviews != 0)
            || (n % 2 == 1 && (reserved_physical != 0 || reserved_reviews > 1))
        {
            return Err("Source control reservation belongs to the wrong execution role".into());
        }
        physical = physical
            .checked_add(reserved_physical)
            .ok_or("Reservation overflow")?;
        reviews = reviews
            .checked_add(reserved_reviews)
            .ok_or("Reservation overflow")?;
        if directory.join("submitted.json").exists() {
            controls::absent(pid(&load(&directory.join("submitted.json"))?["pid"])?)?;
        }
        if n == last {
            continue;
        }
        let (request, response) = controls::sealed(root, &name)?;
        let seal = seals
            .get(usize::try_from(n).map_err(|e| e.to_string())?)
            .ok_or("No completed control seal")?;
        if seal["operation"] != n
            || seal["seal_sha256"] != digest(read(&directory.join("completed.json"))?)
            || seal["response_sha256"] != digest(read(&directory.join("response.json"))?)
            || seal["kind"] != request["kind"]
        {
            return Err(
                "Closure control pins differ from the original completed operations".into(),
            );
        }
        if request["plan_sha256"] != digest(read(&root.join("plan.json"))?)
            || request["sequence"] != n
        {
            return Err("Source control journal identity changed".into());
        }
        let ancestry = directory.join("cache-source.json");
        if ancestry.exists() {
            let receipt = load(&ancestry)?;
            if receipt["type"] == controls::COMPONENT_IMPORT {
                controls::verify_component_import(&receipt, &request, &response)?;
            } else if receipt["type"] == controls::COMPONENT_REVIEW_IMPORT {
                controls::verify_component_review_import(&receipt, &request, &response)?;
            } else {
                return Err("Unknown source control ancestry".into());
            }
        }
        if n % 2 == 0 {
            let id = response["outcome"]["id"]
                .as_str()
                .ok_or("No native control case")?;
            if id == excluded || !cases.insert(id.to_owned()) {
                return Err("Excluded or duplicate native control".into());
            }
            let example = plan
                .training
                .iter()
                .chain(&plan.development)
                .find(|e| e.id == id)
                .ok_or("Foreign source control case")?;
            if request["kind"] != "reading_journey"
                || request["request"] != native::logical_request(&plan, example, &plan.seed)?
                || response["cache_identity"] != digest(request["request"].to_string())
            {
                return Err("Seed native control logical request changed".into());
            }
            selected_stage::verify_observation(&plan, &response)?;
        } else {
            let previous = controls_out
                .last()
                .ok_or("Judge lacks actual native control")?;
            let id = previous.response["outcome"]["id"]
                .as_str()
                .ok_or("No native control case")?;
            let example = plan
                .training
                .iter()
                .chain(&plan.development)
                .find(|e| e.id == id)
                .ok_or("Foreign review case")?;
            if request["kind"] != "codex_reading_review" {
                return Err("Source pair is not an independent stage review".into());
            }
            selected_stage::verify_review(&plan, example, &previous.response, &response)?;
            if !ancestry.exists() {
                let (logical, raw_validated) = selected_stage::recover_saved_review(
                    &plan,
                    example,
                    &previous.response,
                    &directory,
                )?;
                if request["request"] != logical || response != raw_validated {
                    return Err("Paid source answer, request or citations changed".into());
                }
            }
        }
        controls_out.push(Control {
            operation: name,
            request,
            response,
        });
    }
    let ledger = &closure["paid_ledger"];
    let inherited_native = load(&root.join("operations/00000/cache-source.json"))?;
    let inherited_review = load(&root.join("operations/00001/cache-source.json"))?;
    if ledger["fresh_physical_reservations"] != physical
        || ledger["fresh_review_reservations"] != reviews
        || ledger["original_inherited_physical_reservations"]
            != inherited_native["inherited_physical_reservations"]
        || ledger["original_inherited_review_reservations"]
            != inherited_review["inherited_review_reservations"]
        || ledger["remaining_physical"]
            != plan
                .max_physical_generation_attempts
                .checked_sub(physical)
                .ok_or("Physical reservations exceed original bound")?
        || ledger["remaining_reviews"]
            != plan
                .max_review_calls
                .checked_sub(reviews)
                .ok_or("Reviewer reservations exceed original bound")?
        || ledger["remaining_teacher"] != plan.max_teacher_calls
    {
        return Err("Closure ledger differs from actual retained reservations".into());
    }
    Ok(Context {
        plan,
        controls: controls_out,
        excluded,
        physical,
        reviews,
    })
}

fn reshape(original: &Plan, excluded: &str, physical: u64, reviews: u64) -> Result<Plan> {
    if original
        .training
        .iter()
        .filter(|e| e.id == excluded)
        .count()
        != 1
    {
        return Err("Exclusion must be one original training case".into());
    }
    let mut plan = original.clone();
    plan.training.retain(|e| e.id != excluded);
    if plan.training.len() != 2
        || plan.development.len() != 1
        || original.max_teacher_calls != 1
        || original.max_metric_calls < 4
    {
        return Err("Bounded curriculum requires two retained training cases, one unchanged development case and one teacher cap".into());
    }
    plan.max_physical_generation_attempts = original
        .max_physical_generation_attempts
        .checked_sub(physical)
        .ok_or("Inherited physical budget overflow")?;
    plan.max_review_calls = original
        .max_review_calls
        .checked_sub(reviews)
        .ok_or("Inherited review budget overflow")?;
    plan.max_metric_calls = 4; // Pinned GEPA checks the soft cap at iteration boundaries: at most six observations.
    plan.qualification = QUALIFICATION.into();
    Ok(plan)
}

fn same_plan(expected: &Plan, actual: &Plan) -> Result<()> {
    if serde_json::to_value(expected).map_err(|e| e.to_string())?
        != serde_json::to_value(actual).map_err(|e| e.to_string())?
    {
        return Err(
            "Curriculum changed native/source/guide/gold/pool/seed or retained bounds".into(),
        );
    }
    Ok(())
}

fn receipt_header(receipt: &Value) -> Result<()> {
    if receipt["version"] != 1
        || receipt["scope"] != SCOPE
        || receipt["original_round_qualified"] != false
        || !receipt
            .get("excluded_component_score")
            .is_some_and(Value::is_null)
        || receipt["soft_metric_cap"] != 4
        || receipt["maximum_metric_observations"] != 6
        || receipt["qualification"] != QUALIFICATION
    {
        return Err("Curriculum scope or unobserved qualification changed".into());
    }
    Ok(())
}

fn receipt_context(plan: &Plan) -> Result<Option<(Value, Context)>> {
    let Some(pin) = &plan.component_curriculum else {
        return Ok(None);
    };
    if !pin.file.is_absolute()
        || !fs::symlink_metadata(&pin.file)
            .map_err(|e| e.to_string())?
            .is_file()
    {
        return Err("Curriculum receipt must be an ordinary absolute file".into());
    }
    verify(&pin.file, &pin.sha256)?;
    let receipt = load(&pin.file)?;
    receipt_header(&receipt)?;
    let root = Path::new(
        receipt["source_state"]
            .as_str()
            .ok_or("No curriculum source state")?,
    );
    verify_artifacts(root, &receipt["source_artifacts"])?;
    let settlement: Pin =
        serde_json::from_value(receipt["settlement"].clone()).map_err(|e| e.to_string())?;
    let context = source(root, &settlement)?;
    let mut expected = reshape(
        &context.plan,
        &context.excluded,
        context.physical,
        context.reviews,
    )?;
    expected.controller_executable = receipt["controller"]["file"]
        .as_str()
        .ok_or("No new curriculum controller pin")?
        .into();
    expected.controller_executable_sha256 = receipt["controller"]["sha256"]
        .as_str()
        .ok_or("No new curriculum controller hash")?
        .into();
    expected.component_curriculum = Some(pin.clone());
    same_plan(&expected, plan)?;
    if receipt["excluded_training_case"] != context.excluded
        || receipt["inherited_fresh_physical_reservations"] != context.physical
        || receipt["inherited_fresh_review_reservations"] != context.reviews
        || receipt["training_pool_sha256"]
            != digest(serde_json::to_vec(&plan.training).map_err(|e| e.to_string())?)
        || receipt["development_pool_sha256"]
            != digest(serde_json::to_vec(&plan.development).map_err(|e| e.to_string())?)
    {
        return Err(
            "Curriculum changed native/source/guide/gold/pool/seed or retained bounds".into(),
        );
    }
    Ok(Some((receipt, context)))
}

pub(crate) fn verify_plan(plan: &Plan) -> Result<()> {
    receipt_context(plan).map(|_| ())
}

pub fn prepare(
    root: &Path,
    state: &Path,
    settlement_file: &Path,
    controller: &Path,
) -> Result<Plan> {
    if state.exists() {
        return Err("Curriculum state already exists; preserve it".into());
    }
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let settlement = Pin {
        file: fs::canonicalize(settlement_file).map_err(|e| e.to_string())?,
        sha256: digest(read(settlement_file)?),
    };
    let context = source(&root, &settlement)?;
    let mut plan = reshape(
        &context.plan,
        &context.excluded,
        context.physical,
        context.reviews,
    )?;
    fs::create_dir(state).map_err(|e| e.to_string())?;
    fs::create_dir(state.join("executables")).map_err(|e| e.to_string())?;
    let copied = state.join("executables/horary-gepa");
    fs::copy(controller, &copied).map_err(|e| e.to_string())?;
    plan.controller_executable = fs::canonicalize(copied).map_err(|e| e.to_string())?;
    plan.controller_executable_sha256 = digest(read(&plan.controller_executable)?);
    let receipt = json!({"version":1,"scope":SCOPE,"source_state":root,"settlement":settlement,
        "controller":{"file":plan.controller_executable,"sha256":plan.controller_executable_sha256},
        "source_artifacts":source_artifacts(&root)?,"excluded_training_case":context.excluded,"excluded_component_score":null,"original_round_qualified":false,
        "training_pool_sha256":digest(serde_json::to_vec(&plan.training).map_err(|e|e.to_string())?),
        "development_pool_sha256":digest(serde_json::to_vec(&plan.development).map_err(|e|e.to_string())?),
        "inherited_fresh_physical_reservations":context.physical,"inherited_fresh_review_reservations":context.reviews,
        "soft_metric_cap":4,"maximum_metric_observations":6,"qualification":QUALIFICATION});
    keep(&state.join("component-curriculum.json"), &receipt)?;
    plan.component_curriculum = Some(Pin {
        file: fs::canonicalize(state.join("component-curriculum.json"))
            .map_err(|e| e.to_string())?,
        sha256: digest(read(&state.join("component-curriculum.json"))?),
    });
    plan.verify_sources()?;
    for c in &context.controls {
        if c.request["kind"] == "reading_journey" {
            selected_stage::preflight_observation(&plan, &c.response)?;
        }
    }
    keep(&state.join("plan.json"), &plan)?;
    fs::create_dir(state.join("operations")).map_err(|e| e.to_string())?;
    Ok(plan)
}

pub(crate) fn native_control(plan: &Plan, request: &Value) -> Result<Option<(Value, String)>> {
    let Some((_, context)) = receipt_context(plan)? else {
        return Ok(None);
    };
    let found = match_native_control(plan, request, &context.controls)?;
    if let Some((response, _)) = &found {
        selected_stage::verify_observation(plan, response)?;
    }
    Ok(found)
}

fn match_native_control(
    plan: &Plan,
    request: &Value,
    controls: &[Control],
) -> Result<Option<(Value, String)>> {
    if request["candidate"] != json!(plan.seed) {
        return Ok(None);
    }
    let id = request["case"]["id"]
        .as_str()
        .ok_or("No curriculum native case")?;
    if let Some(c) = controls.iter().find(|c| {
        c.request["kind"] == "reading_journey" && c.request["request"]["case"]["id"] == id
    }) {
        if c.request["request"] != *request {
            return Err(
                "Known curriculum seed request changed; no duplicate paid call permitted".into(),
            );
        }
        return Ok(Some((c.response.clone(), c.operation.clone())));
    }
    Ok(None)
}

pub(crate) fn review_control(
    plan: &Plan,
    request: &Value,
    evaluation: &Value,
) -> Result<Option<(Value, String)>> {
    let Some((_, context)) = receipt_context(plan)? else {
        return Ok(None);
    };
    match_review_control(request, evaluation, &context.controls)
}

fn match_review_control(
    request: &Value,
    evaluation: &Value,
    controls: &[Control],
) -> Result<Option<(Value, String)>> {
    for pair in controls.chunks_exact(2) {
        if pair[0].response == *evaluation {
            if pair[1].request["request"] != *request {
                return Err(
                    "Known curriculum judge request changed; no repurchase permitted".into(),
                );
            }
            return Ok(Some((pair[1].response.clone(), pair[1].operation.clone())));
        }
    }
    Ok(None)
}

pub(crate) fn import_receipt(plan: &Plan, state: &Path, operation: &str) -> Result<Value> {
    let pin = plan
        .component_curriculum
        .as_ref()
        .ok_or("No curriculum source")?;
    Ok(
        json!({"type":IMPORT,"curriculum":pin,"source_operation":operation,"new_paid_reservations":0,
        "current_plan":fs::canonicalize(state.join("plan.json")).map_err(|e|e.to_string())?,
        "current_plan_sha256":digest(read(&state.join("plan.json"))?)}),
    )
}

pub(crate) fn verify_import(receipt: &Value, request: &Value, response: &Value) -> Result<()> {
    if receipt["type"] != IMPORT || receipt["new_paid_reservations"] != 0 {
        return Err("Invalid curriculum control import".into());
    }
    let file = Path::new(
        receipt["current_plan"]
            .as_str()
            .ok_or("No imported curriculum Plan")?,
    );
    let sha = receipt["current_plan_sha256"]
        .as_str()
        .ok_or("No imported curriculum Plan hash")?;
    verify(file, sha)?;
    if request["plan_sha256"] != sha {
        return Err("Curriculum import belongs to another journal".into());
    }
    let plan: Plan = serde_json::from_value(load(file)?).map_err(|e| e.to_string())?;
    if receipt["curriculum"] != json!(plan.component_curriculum) {
        return Err("Curriculum source pin changed".into());
    }
    let (_, context) = receipt_context(&plan)?.ok_or("No verified curriculum source")?;
    let operation = receipt["source_operation"]
        .as_str()
        .ok_or("No original curriculum operation")?;
    let c = context
        .controls
        .iter()
        .find(|c| c.operation == operation)
        .ok_or("Unscored/foreign operation cannot enter curriculum cache")?;
    if request["kind"] != c.request["kind"]
        || request["request"] != c.request["request"]
        || *response != c.response
    {
        return Err("Curriculum changed exact observed request, answer or citations".into());
    }
    Ok(())
}

pub(crate) fn status(plan: &Plan) -> Result<Value> {
    let Some((_, context)) = receipt_context(plan)? else {
        return Ok(Value::Null);
    };
    Ok(
        json!({"source_verified":true,"scope":SCOPE,"excluded_training_case":context.excluded,"excluded_score":null,
        "controls":context.controls.len(),"new_observations_from_imports":false,"original_round_qualified":false,
        "retained_hard_caps":{"physical":plan.max_physical_generation_attempts,"reviews":plan.max_review_calls,"teacher":plan.max_teacher_calls},
        "soft_metric_cap":plan.max_metric_calls,"maximum_metric_observations":6,"promotion":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        let mut p = crate::tests::plan();
        p.version = 3;
        p.function = Function::ReadingJourney;
        p.objective = Objective::SelectedStageReliability;
        p.control_mode = ControlMode::FreshHosted;
        p.selected_stage_observation = true;
        p.target_stage = Some(crate::ReadingStage::Condition);
        p.target_method = Some("synthetic_method".into());
        p.max_metric_calls = 8;
        p.max_teacher_calls = 1;
        p.max_physical_generation_attempts = 1008;
        p.max_review_calls = 7;
        p.logical_calls_per_function = 12;
        let mut a = p.training[0].clone();
        a.id = "complete-a".into();
        a.expected = json!({"method":"synthetic_method","explicit":"unchanged authored facts"});
        let mut missing = a.clone();
        missing.id = "target-uninvoked".into();
        let mut b = a.clone();
        b.id = "complete-b".into();
        p.training = vec![a, missing, b];
        p.seed = p.seed_for_guide().unwrap();
        p
    }

    #[test]
    fn curriculum_removes_only_uninvoked_training_and_retains_paid_bounds() {
        let p = plan();
        let changed = reshape(&p, "target-uninvoked", 288, 1).unwrap();
        assert_eq!(
            changed
                .training
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            vec!["complete-a", "complete-b"]
        );
        assert_eq!(
            serde_json::to_value(&changed.training).unwrap(),
            serde_json::to_value(vec![p.training[0].clone(), p.training[2].clone()]).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&changed.development).unwrap(),
            serde_json::to_value(&p.development).unwrap()
        );
        assert_eq!(
            (
                changed.max_physical_generation_attempts,
                changed.max_review_calls,
                changed.max_teacher_calls,
                changed.max_metric_calls
            ),
            (720, 6, 1, 4)
        );
        assert_eq!(changed.seed, p.seed);
        assert_eq!(changed.native_executable_sha256, p.native_executable_sha256);
        assert_eq!(changed.guide, p.guide);
        assert_eq!(changed.reserved_ids, p.reserved_ids);
        assert!(changed.qualification.contains("unscored"));
        assert!(reshape(&p, &p.development[0].id, 288, 1).is_err());
        assert!(reshape(&p, "foreign", 288, 1).is_err());
        assert!(reshape(&p, "target-uninvoked", 1009, 1).is_err());
        assert!(reshape(&p, "target-uninvoked", 288, 8).is_err());
        let mut two_teachers = p.clone();
        two_teachers.max_teacher_calls = 2;
        assert!(reshape(&two_teachers, "target-uninvoked", 288, 1).is_err());
    }

    #[test]
    fn curriculum_authority_rejects_gold_development_controller_and_cap_drift() {
        let expected = reshape(&plan(), "target-uninvoked", 288, 1).unwrap();
        for pointer in [
            "/training/0/expected",
            "/development/0/source_request_sha256",
            "/guide_sha256",
            "/native_executable_sha256",
            "/controller_executable_sha256",
            "/seed",
            "/max_physical_generation_attempts",
            "/max_review_calls",
            "/max_teacher_calls",
            "/max_metric_calls",
        ] {
            let mut value = serde_json::to_value(&expected).unwrap();
            *value.pointer_mut(pointer).unwrap() = match pointer {
                "/seed" => json!({"foreign":"teach from another case"}),
                "/max_physical_generation_attempts" => json!(864),
                "/max_review_calls" => json!(7),
                "/max_teacher_calls" => json!(2),
                "/max_metric_calls" => json!(8),
                _ => json!("changed"),
            };
            let changed: Plan = serde_json::from_value(value).unwrap();
            assert!(same_plan(&expected, &changed).is_err(), "{pointer}");
        }
        same_plan(&expected, &expected).unwrap();
    }

    fn boundary() -> Value {
        json!({"scope":"selected_stage_function_only","execution_status":"selected_stage_unobserved",
            "selected_stage_observation":true,"full_reading":false,"target_stage":"condition","target_method":"synthetic_method",
            "target_function_invoked":false,"target_stage_authentic_inputs":false,"captured_checkpoint_reused":false,
            "fresh_initial_sha256_verified":true,"physical_generation_attempts_complete":true,
            "deadline_cancelled":false,"group_cancelled":false,"provider_stop":null,"infrastructure_error":null,"execution_error":null,
            "selected_stage":{"version":1,"state":"unobserved","target_attempts":0,"all_provider_responses_settled":true,
            "request_result_pairs_complete":true,"last_sequence":4,"reading_delivered":false,"post_boundary_conversation_executed":false}})
    }

    #[test]
    fn uninvoked_curriculum_boundary_never_coerces_uncertainty_or_observation_to_zero() {
        let p = plan();
        let original = boundary();
        let records = vec![json!({"stage":"intake"}), json!({"stage":"conversation"})];
        exclusion_header(&p, &original, &records, 4).unwrap();
        for (pointer, value) in [
            ("/selected_stage/state", json!("accepted")),
            ("/selected_stage/target_attempts", json!(1)),
            (
                "/selected_stage/all_provider_responses_settled",
                json!(false),
            ),
            (
                "/selected_stage/request_result_pairs_complete",
                json!(false),
            ),
            ("/selected_stage/reading_delivered", json!(true)),
            ("/selected_stage/last_sequence", json!(3)),
            ("/target_function_invoked", json!(true)),
            ("/target_stage_authentic_inputs", json!(true)),
            ("/deadline_cancelled", json!(true)),
            ("/provider_stop", json!("unknown provider response")),
            ("/infrastructure_error", json!("incomplete I/O")),
            ("/execution_error", json!("unsettled upstream")),
            ("/physical_generation_attempts_complete", json!(false)),
            ("/captured_checkpoint_reused", json!(true)),
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                exclusion_header(&p, &changed, &records, 4).is_err(),
                "{pointer}"
            );
        }
        assert!(exclusion_header(
            &p,
            &original,
            &[json!({"stage":"condition","accepted":false})],
            4
        )
        .is_err());
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove("provider_stop");
        assert!(exclusion_header(&p, &missing, &records, 4).is_err());
        assert_eq!(original, boundary());
        assert!(original.get("component_score").is_none());
    }

    #[test]
    fn needs_information_exclusion_binds_actual_copies_and_never_accepts_a_reading() {
        let case_dir = Path::new("/synthetic/case");
        let original = json!({"session":{"method":{"result":{"result":"needs_information","need":{"kind":"missing_source_fact"}},"records":[{"stage":"intake"}]}}});
        let outcome = boundary();
        let mut final_copy = original.clone();
        final_copy["scope"] = json!("reading_journey_function_only");
        final_copy["full_reading"] = json!(false);
        final_copy["native_evidence_directory"] = json!(case_dir);
        exclusion_state(
            &outcome,
            &final_copy,
            &original,
            &outcome["selected_stage"],
            case_dir,
        )
        .unwrap();
        let mut changed = final_copy.clone();
        changed["session"]["method"]["result"]["need"] = json!({"different":"unknown"});
        assert!(exclusion_state(
            &outcome,
            &changed,
            &original,
            &outcome["selected_stage"],
            case_dir
        )
        .is_err());
        let mut observation = outcome["selected_stage"].clone();
        observation["last_sequence"] = json!(5);
        assert!(exclusion_state(&outcome, &final_copy, &original, &observation, case_dir).is_err());
        let mut reading = original.clone();
        reading["session"]["method"]["result"] =
            json!({"result":"judgment","answer":"Unqualified answer"});
        let mut copied = final_copy;
        copied["session"] = reading["session"].clone();
        assert!(exclusion_state(
            &outcome,
            &copied,
            &reading,
            &outcome["selected_stage"],
            case_dir
        )
        .is_err());
    }

    #[test]
    fn receipt_requires_explicit_unscored_and_cannot_claim_old_round_gain() {
        let original = json!({"version":1,"scope":SCOPE,"original_round_qualified":false,"excluded_component_score":null,
            "soft_metric_cap":4,"maximum_metric_observations":6,"qualification":QUALIFICATION});
        receipt_header(&original).unwrap();
        let mut absent = original.clone();
        absent
            .as_object_mut()
            .unwrap()
            .remove("excluded_component_score");
        assert!(receipt_header(&absent).is_err());
        for (key, value) in [
            ("excluded_component_score", json!(0)),
            ("original_round_qualified", json!(true)),
            ("soft_metric_cap", json!(8)),
            ("maximum_metric_observations", json!(5)),
            ("qualification", json!("repaired full reading")),
        ] {
            let mut changed = original.clone();
            changed[key] = value;
            assert!(receipt_header(&changed).is_err(), "{key}");
        }
    }

    #[test]
    fn legacy_plan_and_student_logical_request_omit_curriculum_metadata() {
        let legacy = crate::tests::plan();
        let value = serde_json::to_value(&legacy).unwrap();
        assert!(value.get("component_curriculum").is_none());
        let restored: Plan = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), value);
        let p = plan();
        let before = native::logical_request(&p, &p.training[0], &p.seed).unwrap();
        let mut pinned = p.clone();
        pinned.component_curriculum = Some(Pin {
            file: "/synthetic/receipt.json".into(),
            sha256: digest("synthetic receipt"),
        });
        assert_eq!(
            native::logical_request(&pinned, &p.training[0], &pinned.seed).unwrap(),
            before
        );
        assert!(before.get("component_curriculum").is_none());
        let reshaped = reshape(&p, "target-uninvoked", 288, 1).unwrap();
        assert_eq!(
            native::logical_request(&reshaped, &reshaped.training[0], &reshaped.seed).unwrap(),
            before
        );
    }

    fn controls(p: &Plan) -> Vec<Control> {
        ["complete-a","development"].into_iter().enumerate().flat_map(|(i,id)| {
            let native=json!({"case":{"id":id},"candidate":p.seed,"original_question_hash":digest(id)});
            let response=json!({"outcome":{"id":id},"cache_identity":digest(native.to_string())});
            let judge=json!({"native_evaluation":response,"component_scope":"actual condition only"});
            vec![Control{operation:format!("operations/{:05}",i*2),request:json!({"kind":"reading_journey","request":native}),response},
                Control{operation:format!("operations/{:05}",i*2+1),request:json!({"kind":"codex_reading_review","request":judge}),response:json!({"score":0,"evidence":"unchanged actual source"})}]
        }).collect()
    }

    #[test]
    fn exact_imports_follow_new_sampler_order_and_never_repurchase_changed_known_controls() {
        let p = plan();
        let rows = controls(&p);
        for index in [2, 0] {
            let source = &rows[index];
            let request = &source.request["request"];
            let found = match_native_control(&p, request, &rows).unwrap().unwrap();
            assert_eq!(found, (source.response.clone(), source.operation.clone()));
            let review = match_review_control(&rows[index + 1].request["request"], &found.0, &rows)
                .unwrap()
                .unwrap();
            assert_eq!(
                review,
                (
                    rows[index + 1].response.clone(),
                    rows[index + 1].operation.clone()
                )
            );
        }
        let mut candidate = rows[0].request["request"].clone();
        candidate["candidate"] = json!({"changed":"fresh target teaching"});
        assert!(match_native_control(&p, &candidate, &rows)
            .unwrap()
            .is_none());
        let mut changed = rows[0].request["request"].clone();
        changed["original_question_hash"] = json!(digest("foreign"));
        assert!(match_native_control(&p, &changed, &rows)
            .unwrap_err()
            .contains("no duplicate paid call"));
        let mut changed_judge = rows[1].request["request"].clone();
        changed_judge["component_scope"] = json!("unqualified whole reading");
        assert!(
            match_review_control(&changed_judge, &rows[0].response, &rows)
                .unwrap_err()
                .contains("no repurchase")
        );
        let mut fresh = rows[0].response.clone();
        fresh["cache_identity"] = json!(digest("new actual measurement"));
        assert!(
            match_review_control(&rows[1].request["request"], &fresh, &rows)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn pinned_original_unsealed_artifacts_cannot_drift_or_acquire_a_fabricated_completion() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for name in ["plan.json", "owner-exit.json", "owner-observed.json"] {
            keep(&root.join(name), &json!({"fixture":name})).unwrap();
        }
        let op = root.join("operations/00004");
        fs::create_dir_all(&op).unwrap();
        keep(
            &op.join("request.json"),
            &json!({"scope":"unobserved synthetic fixture"}),
        )
        .unwrap();
        let pins = source_artifacts(root).unwrap();
        verify_artifacts(root, &pins).unwrap();
        assert!(!op.join("completed.json").exists());
        keep(&op.join("completed.json"), &json!({"fabricated":true})).unwrap();
        assert!(verify_artifacts(root, &pins).is_err());
        fs::remove_file(op.join("completed.json")).unwrap();
        fs::write(op.join("request.json"), b"changed").unwrap();
        assert!(verify_artifacts(root, &pins).is_err());
    }
}
