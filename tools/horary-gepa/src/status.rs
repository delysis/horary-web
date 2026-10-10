//! Read-only journal observation. A reservation is not a submitted call, and
//! an unfinished operation is not permission to retry it or a semantic zero.
use crate::{load, read, verify, Result};
use horary_prompt_program::digest;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

fn ordinary_path(base: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Status artifact path escapes its operation".into());
    }
    let mut full = base.to_path_buf();
    for part in path.components() {
        full.push(part.as_os_str());
        if fs::symlink_metadata(&full)
            .map_err(|e| e.to_string())?
            .is_symlink()
        {
            return Err("Status refuses symlinked artifacts".into());
        }
    }
    Ok(full)
}

struct Settled {
    response: Value,
    artifacts: BTreeSet<String>,
}
fn settled(directory: &Path) -> Result<Settled> {
    let receipt = load(&ordinary_path(directory, "completed.json")?)?;
    let artifacts = receipt["artifacts"]
        .as_array()
        .ok_or("Missing completion artifacts")?;
    let mut names = BTreeSet::new();
    for artifact in artifacts {
        let name = artifact["file"]
            .as_str()
            .ok_or("Missing artifact filename")?;
        if !names.insert(name.to_owned()) {
            return Err("Duplicate completion artifact".into());
        }
        verify(
            &ordinary_path(directory, name)?,
            artifact["sha256"].as_str().ok_or("Missing artifact hash")?,
        )?;
    }
    for required in ["request.json", "reservation.json", "response.json"] {
        if !names.contains(required) {
            return Err(format!("Unsealed operation {required}"));
        }
    }
    let response = ordinary_path(directory, "response.json")?;
    verify(
        &response,
        receipt["response_sha256"]
            .as_str()
            .ok_or("Missing response hash")?,
    )?;
    Ok(Settled {
        response: load(&response)?,
        artifacts: names,
    })
}

fn project(value: &Value, fields: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for field in fields {
        if let Some(value) = value.get(*field) {
            result.insert((*field).into(), value.clone());
        }
    }
    Value::Object(result)
}

fn review_scores(value: &Value) -> Value {
    let mut scores = serde_json::Map::new();
    for stage in ["classification", "elicitation", "extraction", "reading"] {
        scores.insert(
            stage.into(),
            project(&value["pipeline"][stage], &["state", "score"]),
        );
    }
    if value.get("selected_stage").is_some() {
        scores.insert(
            "selected_stage".into(),
            project(&value["selected_stage"], &["state", "score"]),
        );
    }
    Value::Object(scores)
}

fn search_result(value: &Value, plan: &Value, plan_sha: &str) -> Result<Value> {
    if value.get("plan_sha256").is_none() {
        return Err("Legacy search result is unbound to its plan; original scores remain in the evidence file, not verified in this snapshot".into());
    }
    if value["plan_sha256"] != plan_sha {
        return Err("Search result belongs to a different immutable plan".into());
    }
    if value["revision"] != plan["engine_rev"]
        || value["function"]
            != plan
                .get("function")
                .cloned()
                .unwrap_or(json!("classification"))
        || value["target_method"] != plan["target_method"]
        || plan["function"] == "reading_journey" && value["target_stage"] != plan["target_stage"]
        || value["logical_metric_evaluations"].as_u64().is_none()
        || value["development_scores"].as_array().is_none_or(|scores| {
            scores.is_empty()
                || scores.iter().any(|s| {
                    s.as_f64()
                        .is_none_or(|s| !s.is_finite() || !(0.0..=1.0).contains(&s))
                })
        })
    {
        return Err("Search result identity or score receipt is invalid".into());
    }
    Ok(project(
        value,
        &[
            "best_index",
            "development_scores",
            "logical_metric_evaluations",
            "iterations",
        ],
    ))
}

/// Never opens the execution lock, creates directories, launches a process,
/// obtains credentials or changes the optimizer's recovery decisions. Counts
/// cover this snapshot only; the owner may still be publishing an operation.
pub fn inspect(root: &Path) -> Result<Value> {
    let plan_file = ordinary_path(root, "plan.json")?;
    let plan = load(&plan_file)?;
    let plan_sha = digest(read(&plan_file)?);
    let mut issues = Vec::new();
    let recovery = plan.get("recovery").map(|value| {
        let recovery: crate::recovery::Recovery =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let materialized: crate::Plan =
            serde_json::from_value(plan.clone()).map_err(|e| e.to_string())?;
        recovery.verify_plan(&materialized)?;
        Ok::<_, String>(recovery)
    });
    let recovery = match recovery {
        Some(Ok(value)) => Some(value),
        Some(Err(error)) => {
            issues.push(format!("Recovery ancestry is unverified: {error}"));
            None
        }
        None => None,
    };
    let mut operations = Vec::new();
    let mut measurements = Vec::new();
    let mut reviews = Vec::new();
    let mut reservations = [Some(0u64); 3];
    let costs = [
        "teacher_calls",
        "review_calls",
        "physical_generation_attempts",
    ];
    let mut actual_attempts = Some(0u64);
    let mut unique_native = 0;
    let mut native_cache_reuse = 0;
    let mut review_cache_reuse = 0;
    let mut dirs = Vec::new();
    let operation_root = root.join("operations");
    if operation_root.exists() {
        ordinary_path(root, "operations")?;
        for entry in fs::read_dir(&operation_root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Non-UTF8 operation name")?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_dir()
                || name.parse::<usize>().is_err()
            {
                return Err("Unexpected non-directory/numbered operation entry".into());
            }
            dirs.push((name.parse::<usize>().map_err(|e| e.to_string())?, name));
        }
    }
    dirs.sort();
    for (expected, (sequence, name)) in dirs.into_iter().enumerate() {
        let directory = ordinary_path(root, &format!("operations/{name}"))?;
        let mut errors = Vec::new();
        if sequence != expected || name != format!("{sequence:05}") {
            errors.push("Operation sequence has a gap or noncanonical name".into());
        }
        let request = ordinary_path(&directory, "request.json").and_then(|p| load(&p));
        let request = match request {
            Ok(value) => {
                if value["sequence"].as_u64() != Some(sequence as u64)
                    || value["plan_sha256"] != plan_sha
                {
                    errors.push("Operation request differs from this plan or sequence".into());
                }
                value
            }
            Err(error) => {
                errors.push(error);
                Value::Null
            }
        };
        let reservation = ordinary_path(&directory, "reservation.json").and_then(|p| load(&p));
        match reservation {
            Ok(value) => {
                for (index, cost) in costs.iter().enumerate() {
                    // Review reservations were introduced after classification.
                    let count = if *cost == "review_calls" && value.get(*cost).is_none() {
                        Some(0)
                    } else {
                        value[*cost].as_u64()
                    };
                    let addition = reservations[index].zip(count);
                    reservations[index] = addition.and_then(|(used, n)| used.checked_add(n));
                    if count.is_none() {
                        errors.push(format!("Unknown {cost} reservation"));
                    }
                    if addition.is_some() && reservations[index].is_none() {
                        errors.push(format!("Overflowing {cost} reservation"));
                    }
                }
            }
            Err(error) => {
                errors.push(error);
                reservations = [None; 3];
            }
        }
        let kind = request["kind"].as_str();
        let case = request["request"]["case"]["id"]
            .as_str()
            .or(request["request"]["case_id"].as_str());
        let partition = case.map(|id| {
            if plan["training"]
                .as_array()
                .is_some_and(|v| v.iter().any(|e| e["id"] == id))
            {
                "training"
            } else if plan["development"]
                .as_array()
                .is_some_and(|v| v.iter().any(|e| e["id"] == id))
            {
                "development"
            } else {
                "unknown"
            }
        });
        let seal_exists = directory.join("completed.json").exists();
        let response = if seal_exists {
            match settled(&directory) {
                Ok(value) => Some(value),
                Err(error) => {
                    errors.push(error);
                    None
                }
            }
        } else {
            None
        };
        if let Some(settled) = &response {
            if settled.artifacts.contains("cache-source.json") {
                let source = load(&ordinary_path(&directory, "cache-source.json")?)?;
                if source["type"] == "predecessor_import" {
                    match recovery
                        .as_ref()
                        .ok_or("Missing verified recovery ancestry".to_string())
                        .and_then(|r| {
                            crate::recovery::verify_import(r, &source).map_err(|e| e.to_string())
                        }) {
                        Ok(()) => {}
                        Err(error) => errors.push(error.to_string()),
                    }
                } else if source["type"] == "sealed_baseline_control_import" {
                    if let Err(error) = crate::controls::verify_import(
                        &source,
                        &load(&directory.join("request.json"))?,
                        &settled.response,
                    ) {
                        errors.push(error);
                    }
                }
            }
        }
        let state = if !errors.is_empty() {
            "unverified"
        } else if response.is_some() {
            "settled"
        } else {
            "unsettled"
        };
        if let Some(settled) = response.filter(|_| errors.is_empty()) {
            let response = settled.response;
            let cached = settled.artifacts.contains("cache-source.json");
            if matches!(
                kind,
                Some("classification" | "input_journey" | "reading_journey")
            ) {
                native_cache_reuse += usize::from(cached);
                let outcome = &response["outcome"];
                if !cached {
                    if settled.artifacts.contains("native/outcome.json") {
                        unique_native += 1;
                        let native = load(&ordinary_path(&directory, "native/outcome.json")?)?;
                        let count = native["physical_generation_attempts"].as_u64();
                        let consistent = native["physical_generation_attempts"]
                            == outcome["physical_generation_attempts"];
                        let addition = actual_attempts.zip(count);
                        actual_attempts = addition.and_then(|(used, n)| used.checked_add(n));
                        if !consistent
                            || count.is_none()
                            || addition.is_some() && actual_attempts.is_none()
                        {
                            actual_attempts = None;
                            issues.push(format!("operations/{name}: missing, inconsistent or overflowing actual attempt count"));
                        }
                    } else {
                        actual_attempts = None;
                        issues.push(format!("operations/{name}: settled native operation has neither a sealed cache link nor a sealed native outcome; actual attempts remain unknown"));
                    }
                }
                let mut m = project(
                    outcome,
                    &[
                        "scope",
                        "full_reading",
                        "hurdles",
                        "classification_pass",
                        "classification_execution_completed",
                        "first_turn_execution_completed",
                        "follow_up_execution_completed",
                        "follow_up_pass",
                        "target_function_invoked",
                        "target_stage",
                        "target_stage_authentic_inputs",
                        "known_native_semantic_abort",
                        "execution_status",
                        "logical_calls",
                        "logical_model_calls",
                        "physical_generation_attempts",
                        "elapsed_ms",
                    ],
                );
                m["sequence"] = json!(sequence);
                m["case_id"] = json!(case);
                m["partition"] = json!(partition);
                m["cache_reuse"] = json!(cached);
                // Native hurdle objects contain only status/reasons, but keep
                // the status alone so this view cannot become a writer packet.
                if let Some(hurdles) = m["hurdles"].as_object_mut() {
                    for h in hurdles.values_mut() {
                        *h = project(h, &["status"]);
                    }
                }
                measurements.push(m);
            } else if matches!(kind, Some("codex_input_review" | "codex_reading_review")) {
                review_cache_reuse += usize::from(cached);
                reviews.push(json!({"sequence":sequence,"case_id":case,"partition":partition,"cache_reuse":cached,"scores":review_scores(&response)}));
            }
        }
        if !errors.is_empty() {
            issues.push(format!("operations/{name}: {}", errors.join("; ")));
        }
        operations.push(json!({"sequence":sequence,"kind":kind,"case_id":case,"partition":partition,"state":state,"completion_receipt_present":seal_exists,"evidence":directory}));
    }
    let mut budgets = serde_json::Map::new();
    for (index, (cost, limit)) in costs
        .iter()
        .zip([
            "max_teacher_calls",
            "max_review_calls",
            "max_physical_generation_attempts",
        ])
        .enumerate()
    {
        let bound = if limit == "max_review_calls" && plan.get(limit).is_none() {
            Some(0)
        } else {
            plan[limit].as_u64()
        };
        let remaining = reservations[index]
            .zip(bound)
            .and_then(|(used, cap)| cap.checked_sub(used));
        if reservations[index]
            .zip(bound)
            .is_some_and(|(used, cap)| used > cap)
        {
            issues.push(format!("{cost} reservations exceed the pinned bound"));
        }
        budgets.insert(
            (*cost).into(),
            json!({"reserved":reservations[index],"limit":bound,"remaining":remaining}),
        );
    }
    let result_path = root.join("search-result.json");
    let result = if result_path.exists() {
        match ordinary_path(root, "search-result.json")
            .and_then(|p| load(&p))
            .and_then(|v| search_result(&v, &plan, &plan_sha))
        {
            Ok(value) => Some(value),
            Err(error) => {
                issues.push(error);
                None
            }
        }
    } else {
        None
    };
    let exit_path = root.join("owner-exit.json");
    let owner_exit = if exit_path.exists() {
        Some(project(
            &load(&ordinary_path(root, "owner-exit.json")?)?,
            &["exit_code", "execution", "automatic_resubmission"],
        ))
    } else {
        None
    };
    let interruptions = if root.join("interruptions").exists() {
        fs::read_dir(ordinary_path(root, "interruptions")?)
            .map_err(|e| e.to_string())?
            .count()
    } else {
        0
    };
    Ok(
        json!({"state":root,"plan_sha256":plan_sha,"function":plan.get("function").cloned().unwrap_or(json!("classification")),"target_method":plan["target_method"],
        "operation_counts":{"settled":operations.iter().filter(|o|o["state"]=="settled").count(),"unsettled":operations.iter().filter(|o|o["state"]=="unsettled").count(),"unverified":operations.iter().filter(|o|o["state"]=="unverified").count()},
        "budgets":budgets,"observed_native_attempts":{"settled_unique_functions":unique_native,"physical_attempts":actual_attempts,"cache_reuses":native_cache_reuse,"excludes_unsettled_calls":true},"review_cache_reuses":review_cache_reuse,
        "operations":operations,"measurements":measurements,"independent_reviews":reviews,
        "recovery":recovery.map(|r|json!({"predecessor_state":r.predecessor_state,"inherited_reservations":r.inherited_reservations,"source_verified":true,"imports_are_cache_reuse":true})),
        "search_result_present":result_path.exists(),"search_result":result,"owner_exit":owner_exit,"interruption_records":interruptions,"integrity_issues":issues,
        "qualification":"Read-only receipt snapshot; unfinished work may still be running. Reservations are not submitted calls. No retry, promotion or interpretation qualification is authorized by this report."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        crate::keep(&root.path().join("plan.json"), &json!({"function":"input_journey","target_method":"movable_deal","engine_rev":"engine","max_teacher_calls":2,"max_review_calls":8,"max_physical_generation_attempts":100,"training":[{"id":"training"}],"development":[{"id":"development"}]})).unwrap();
        fs::create_dir(root.path().join("operations")).unwrap();
        root
    }
    fn seal(directory: &Path, files: &[&str]) {
        let artifacts: Vec<_> = files
            .iter()
            .map(|file| json!({"file":file,"sha256":digest(read(&directory.join(file)).unwrap())}))
            .collect();
        crate::keep(&directory.join("completed.json"), &json!({"response_sha256":digest(read(&directory.join("response.json")).unwrap()),"artifacts":artifacts})).unwrap();
    }
    fn operation(root: &Path, sequence: usize, finish: bool, cached: bool) -> std::path::PathBuf {
        let d = root.join("operations").join(format!("{sequence:05}"));
        fs::create_dir(&d).unwrap();
        crate::keep(&d.join("request.json"), &json!({"sequence":sequence,"plan_sha256":digest(read(&root.join("plan.json")).unwrap()),"kind":"input_journey","request":{"case":{"id":"training"}}})).unwrap();
        crate::keep(&d.join("reservation.json"), &json!({"teacher_calls":0,"review_calls":0,"physical_generation_attempts":if cached {0} else {36}})).unwrap();
        if finish {
            let response = json!({"outcome":{"id":"training","physical_generation_attempts":4,"full_reading":false,"hurdles":{"extraction":{"status":"fail","failures":["Do not export case words"]}}}});
            crate::keep(&d.join("response.json"), &response).unwrap();
            if cached {
                crate::keep(
                    &d.join("cache-source.json"),
                    &json!({"original_operation":"operations/00000"}),
                )
                .unwrap();
            } else {
                fs::create_dir(d.join("native")).unwrap();
                crate::keep(&d.join("native/outcome.json"), &response["outcome"]).unwrap();
            }
            seal(
                &d,
                &[
                    "request.json",
                    "reservation.json",
                    "response.json",
                    if cached {
                        "cache-source.json"
                    } else {
                        "native/outcome.json"
                    },
                ],
            );
        }
        d
    }
    #[test]
    fn pending_calls_remain_reserved_and_are_not_zero_score_results() {
        let root = setup();
        operation(root.path(), 0, false, false);
        let s = inspect(root.path()).unwrap();
        assert_eq!(s["operation_counts"]["unsettled"], 1);
        assert_eq!(s["budgets"]["physical_generation_attempts"]["reserved"], 36);
        assert_eq!(
            s["budgets"]["physical_generation_attempts"]["remaining"],
            64
        );
        assert_eq!(s["observed_native_attempts"]["physical_attempts"], 0);
        assert_eq!(s["measurements"], json!([]));
        assert!(!root.path().join("run.lock").exists());
        assert!(!root.path().join("search-result.json").exists());
    }
    #[test]
    fn cache_reuse_does_not_count_paid_attempts_twice_or_publish_trace_words() {
        let root = setup();
        operation(root.path(), 0, true, false);
        operation(root.path(), 1, true, true);
        let s = inspect(root.path()).unwrap();
        assert_eq!(s["operation_counts"]["settled"], 2);
        assert_eq!(s["observed_native_attempts"]["physical_attempts"], 4);
        assert_eq!(s["observed_native_attempts"]["settled_unique_functions"], 1);
        assert_eq!(s["observed_native_attempts"]["cache_reuses"], 1);
        assert_eq!(
            s["measurements"][1]["hurdles"]["extraction"]["status"],
            "fail"
        );
        assert!(!s.to_string().contains("Do not export case words"));
    }
    #[test]
    fn reviewer_cache_is_separate_from_native_cache() {
        let root = setup();
        operation(root.path(), 0, true, true);
        let d = operation(root.path(), 1, false, true);
        let mut request = load(&d.join("request.json")).unwrap();
        request["kind"] = json!("codex_input_review");
        request["request"] = json!({"case_id":"training"});
        fs::write(d.join("request.json"), request.to_string()).unwrap();
        crate::keep(
            &d.join("response.json"),
            &json!({"pipeline":{"reading":{"state":"unobserved","score":null}}}),
        )
        .unwrap();
        crate::keep(
            &d.join("cache-source.json"),
            &json!({"original_operation":"operations/00000"}),
        )
        .unwrap();
        seal(
            &d,
            &[
                "request.json",
                "reservation.json",
                "response.json",
                "cache-source.json",
            ],
        );
        let s = inspect(root.path()).unwrap();
        assert_eq!(s["observed_native_attempts"]["cache_reuses"], 1);
        assert_eq!(s["review_cache_reuses"], 1);
        assert_eq!(
            s["independent_reviews"][0]["scores"]["reading"]["state"],
            "unobserved"
        );
    }
    #[test]
    fn missing_native_witness_is_unknown_not_zero_even_with_a_completion_seal() {
        let root = setup();
        let d = operation(root.path(), 0, false, false);
        crate::keep(
            &d.join("response.json"),
            &json!({"outcome":{"physical_generation_attempts":4}}),
        )
        .unwrap();
        seal(&d, &["request.json", "reservation.json", "response.json"]);
        // Unsealed additions cannot turn missing evidence into cache reuse.
        crate::keep(
            &d.join("cache-source.json"),
            &json!({"original_operation":"operations/00000"}),
        )
        .unwrap();
        let s = inspect(root.path()).unwrap();
        assert!(s["observed_native_attempts"]["physical_attempts"].is_null());
        assert_eq!(s["observed_native_attempts"]["cache_reuses"], 0);
        assert!(!s["integrity_issues"].as_array().unwrap().is_empty());
    }
    #[test]
    fn final_scores_must_belong_to_this_plan_and_legacy_results_stay_unverified() {
        let root = setup();
        let plan = load(&root.path().join("plan.json")).unwrap();
        let sha = digest(read(&root.path().join("plan.json")).unwrap());
        let mut result = json!({"revision":"engine","function":"input_journey","target_method":"movable_deal","logical_metric_evaluations":5,"development_scores":[0.0,0.5],"plan_sha256":sha});
        assert_eq!(
            search_result(&result, &plan, &sha).unwrap()["development_scores"][1],
            0.5
        );
        result["plan_sha256"] = json!(digest("another round with the same engine/method"));
        assert!(search_result(&result, &plan, &sha)
            .unwrap_err()
            .contains("different immutable plan"));
        result.as_object_mut().unwrap().remove("plan_sha256");
        assert!(search_result(&result, &plan, &sha)
            .unwrap_err()
            .contains("Legacy"));
    }
    #[test]
    fn altered_completion_artifacts_are_unverified_not_successes() {
        let root = setup();
        let d = operation(root.path(), 0, true, false);
        fs::write(d.join("native/outcome.json"), b"{}").unwrap();
        let s = inspect(root.path()).unwrap();
        assert_eq!(s["operation_counts"]["unverified"], 1);
        assert_eq!(s["operation_counts"]["settled"], 0);
        assert_eq!(s["measurements"], json!([]));
        assert!(!s["integrity_issues"].as_array().unwrap().is_empty());
    }
    #[test]
    fn publishing_or_missing_reservations_never_become_free_calls() {
        let root = setup();
        let d = operation(root.path(), 0, false, false);
        fs::write(d.join("reservation.json"), b"{").unwrap();
        let s = inspect(root.path()).unwrap();
        assert!(s["budgets"]["physical_generation_attempts"]["reserved"].is_null());
        assert!(s["budgets"]["physical_generation_attempts"]["remaining"].is_null());
        assert_eq!(s["operation_counts"]["unverified"], 1);
    }
    #[test]
    fn result_file_existence_and_failed_owner_do_not_establish_closed_search() {
        let root = setup();
        crate::keep(&root.path().join("search-result.json"), &json!({})).unwrap();
        crate::keep(
            &root.path().join("owner-exit.json"),
            &json!({"exit_code":75,"execution":"withheld"}),
        )
        .unwrap();
        let s = inspect(root.path()).unwrap();
        assert_eq!(s["search_result_present"], true);
        assert!(s["search_result"].is_null());
        assert_eq!(s["owner_exit"]["exit_code"], 75);
        assert!(!s["integrity_issues"].as_array().unwrap().is_empty());
    }
    #[test]
    fn legacy_classification_reservations_have_no_reviewer_cost() {
        let root = setup();
        let d = operation(root.path(), 0, false, false);
        fs::write(
            d.join("reservation.json"),
            json!({"teacher_calls":0,"physical_generation_attempts":36}).to_string(),
        )
        .unwrap();
        let s = inspect(root.path()).unwrap();
        assert_eq!(s["budgets"]["review_calls"]["reserved"], 0);
        assert_eq!(s["operation_counts"]["unsettled"], 1);
    }
    #[test]
    fn completed_artifact_cannot_escape_through_a_symlink() {
        let root = setup();
        assert!(ordinary_path(root.path(), "../plan.json").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.path().join("plan.json"),
                root.path().join("linked.json"),
            )
            .unwrap();
            assert!(ordinary_path(root.path(), "linked.json").is_err());
        }
    }
}
