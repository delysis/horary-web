//! Per-invocation immutable source inventory, never a checkpoint or input patch.
use crate::{journal::Journal, load, read, ControlMode, Example, Function, Plan, Result};
use horary_prompt_program::{
    digest,
    response_memo::{native_outcome_is_settled, Index, Kind, Memo, Pin, Source, VERSION},
};
use std::{fs, path::Path};

pub(crate) fn validate(plan: &Plan) -> Result<()> {
    if plan.exact_response_memoization
        && (plan.function != Function::ReadingJourney
            || plan.control_mode != ControlMode::FreshHosted
            || plan.recovery.is_some())
    {
        return Err("Exact responses may be memoized only in a fresh ReadingJourney search".into());
    }
    Ok(())
}
fn pin(file: &Path) -> Result<Pin> {
    Ok(Pin {
        file: file.into(),
        sha256: digest(read(file)?),
    })
}

/// Availability is execution detail, so it never alters the logical GEPA
/// request on replay. The actual frozen inventory is sealed in the operation.
pub(crate) fn snapshot(plan: &Plan, journal: &Journal, example: &Example) -> Result<Index> {
    validate(plan)?;
    let wanted = serde_json::to_value(example).map_err(|e| e.to_string())?;
    if !plan
        .training
        .iter()
        .chain(&plan.development)
        .any(|e| serde_json::to_value(e).ok().as_ref() == Some(&wanted))
        || plan.reserved_ids.contains(&example.id)
    {
        return Err("Memoization cannot inspect a case outside the frozen search pools".into());
    }
    let mut sources = vec![Source {
        root: plan.campaign.clone(),
        kind: Kind::CorpusCase,
        seal: Pin {
            file: plan
                .campaign
                .join("case-origins")
                .join(format!("{}.json", example.id)),
            sha256: example.source_origin_sha256.clone(),
        },
    }];
    let mut directories = fs::read_dir(journal.root.join("operations"))
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    directories.sort();
    for directory in directories {
        if !directory.join("completed.json").is_file() {
            continue;
        }
        let request = load(&directory.join("request.json"))?;
        if request["kind"] != "reading_journey" || request["request"]["case"]["id"] != example.id {
            continue;
        }
        if request["request"]["case"]["source_fixture_sha256"] != example.source_fixture_sha256
            || request["request"]["case"]["source_initial_sha256"] != example.source_initial_sha256
        {
            return Err(
                "Completed same-case memo source has different fixture/initial provenance".into(),
            );
        }
        let response = load(&directory.join("response.json"))?;
        // Full measurement cache imports point at another sealed source and
        // have no new native tree here; their originals remain available.
        if !directory.join("native/manifest.json").is_file() {
            continue;
        }
        if !native_outcome_is_settled(&response["outcome"]) {
            continue;
        }
        sources.push(Source {
            root: directory.clone(),
            kind: Kind::NativeOperation,
            seal: pin(&directory.join("completed.json"))?,
        });
    }
    let index = Index {
        version: VERSION.into(),
        case_id: example.id.clone(),
        fixture_sha256: example.source_fixture_sha256.clone(),
        initial_sha256: example.source_initial_sha256.clone(),
        excluded_stage: plan.signature().stage.into(),
        sources,
    };
    Memo::load(index.clone())?;
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_absence_preserves_identity_and_opt_in_is_only_fresh_reading() {
        let old = crate::tests::plan();
        let encoded = serde_json::to_value(&old).unwrap();
        assert!(encoded.get("exact_response_memoization").is_none());
        let restored: Plan = serde_json::from_value(encoded.clone()).unwrap();
        assert!(!restored.exact_response_memoization);
        assert_eq!(serde_json::to_value(restored).unwrap(), encoded);
        validate(&old).unwrap();
        let mut changed = old.clone();
        changed.exact_response_memoization = true;
        assert!(validate(&changed).is_err());
        changed.function = Function::InputJourney;
        assert!(validate(&changed).is_err());
        changed.function = Function::ReadingJourney;
        assert!(validate(&changed).is_err());
        changed.control_mode = ControlMode::FreshHosted;
        validate(&changed).unwrap();
        assert!(serde_json::to_value(&changed).unwrap()["exact_response_memoization"] == true);
    }
    #[test]
    fn membership_is_exact_and_reserved_never_opens_a_cache_bank() {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = crate::tests::plan();
        plan.exact_response_memoization = true;
        plan.function = Function::ReadingJourney;
        plan.control_mode = ControlMode::FreshHosted;
        let journal = Journal::open(dir.path(), &plan).unwrap();
        let mut spoof = plan.training[0].clone();
        spoof.source_fixture_sha256 = digest("different same-id fixture");
        assert!(snapshot(&plan, &journal, &spoof)
            .unwrap_err()
            .contains("outside"));
        let mut reserved = plan.training[0].clone();
        reserved.id = plan.reserved_ids[0].clone();
        assert!(snapshot(&plan, &journal, &reserved)
            .unwrap_err()
            .contains("outside"));
        assert_eq!(
            fs::read_dir(dir.path().join("operations")).unwrap().count(),
            0
        );
    }
    #[test]
    fn snapshot_includes_authentic_completed_operation_without_invented_flag() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let campaign = root.join("corpus");
        let id = "train";
        let case = campaign.join("cases").join(id);
        fs::create_dir_all(&case).unwrap();
        let model = json!({"provider":"google_gemini_api","id":"synthetic-model",
            "endpoint":"https://example.invalid/generate","decoding":"unconstrained",
            "thinking_level":"minimal","temperature":0,"seed":0,"local_inference":false});
        crate::keep(&campaign.join("manifest.json"), &json!({"model":model})).unwrap();
        crate::keep(
            &case.join("fixture.json"),
            &json!({"id":id,"words":"Synthetic original concern"}),
        )
        .unwrap();
        crate::keep(
            &case.join("initial.json"),
            &json!({"session":{"question":"","chart":null},"clock":17}),
        )
        .unwrap();
        crate::keep(
            &case.join("outcome.json"),
            &json!({"id":id,"execution_status":"completed"}),
        )
        .unwrap();
        let origin = campaign.join("case-origins/train.json");
        fs::create_dir_all(origin.parent().unwrap()).unwrap();
        crate::keep(&origin, &json!({"case_id":id,"origin":"fresh_completed_native_campaign",
            "selected_outcome_unchanged":true,"archived_answer_replay_allowed":true,
            "source_directory":case,"source_manifest_sha256":digest(read(&campaign.join("manifest.json")).unwrap()),
            "fresh_corpus":{"native_executable_sha256":digest("synthetic-native")},
            "files":{"fixture.json":digest(read(&case.join("fixture.json")).unwrap()),
                "initial.json":digest(read(&case.join("initial.json")).unwrap()),
                "outcome.json":digest(read(&case.join("outcome.json")).unwrap())}})).unwrap();
        let mut plan = crate::tests::plan();
        plan.campaign = campaign;
        plan.exact_response_memoization = true;
        plan.function = Function::ReadingJourney;
        plan.control_mode = ControlMode::FreshHosted;
        plan.target_stage = Some(crate::ReadingStage::Condition);
        plan.training[0].source_case_directory = case.clone();
        plan.training[0].source_origin_sha256 = digest(read(&origin).unwrap());
        plan.training[0].source_fixture_sha256 = digest(read(&case.join("fixture.json")).unwrap());
        plan.training[0].source_initial_sha256 = digest(read(&case.join("initial.json")).unwrap());
        let example = plan.training[0].clone();
        let mut journal = Journal::open(&root.join("round"), &plan).unwrap();
        let response = json!({"outcome":{"id":id,"scope":"reading_journey_function_only",
            "full_reading":true,"execution_status":"completed","first_turn_execution_completed":true,
            "follow_up_execution_completed":null,"execution_error":null,
            "physical_generation_attempts_complete":true,"fresh_initial_sha256_verified":true,
            "captured_checkpoint_reused":false,"target_function_invoked":true,"target_stage_authentic_inputs":true,
            "deadline_cancelled":false,"group_cancelled":false,"infrastructure_error":null,"provider_stop":null}});
        assert!(response["outcome"].get("execution_interrupted").is_none());
        for uncertain in [false, true] {
            let crate::journal::Operation::Fresh(directory) = journal.begin("reading_journey",
                &json!({"case":example,"native_executable_sha256":digest("synthetic-native"),"trial":uncertain}),0,0).unwrap()
            else { panic!("Fresh offline operation required") };
            let native_case = directory.join("native/cases/train");
            fs::create_dir_all(&native_case).unwrap();
            for file in ["fixture.json", "initial.json"] {
                fs::copy(case.join(file), native_case.join(file)).unwrap();
            }
            crate::keep(
                &directory.join("native/manifest.json"),
                &json!({"model":model}),
            )
            .unwrap();
            let mut recorded = response.clone();
            if uncertain {
                recorded["outcome"]["infrastructure_error"] = json!("Uncertain provider request");
            }
            journal.finish(&directory, &recorded).unwrap();
        }
        let inventory = snapshot(&plan, &journal, &example).unwrap();
        assert_eq!(inventory.sources.len(), 2);
        assert!(matches!(inventory.sources[1].kind, Kind::NativeOperation));
        assert!(inventory.sources[1].root.ends_with("operations/00000"));
    }
}
