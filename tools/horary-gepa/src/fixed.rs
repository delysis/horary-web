//! Immutable teaching for other functions, never a transplanted input record.
use crate::{read, ControlMode, Function, Plan, Result};
use horary_prompt_program::{digest, Override, Program, Signature};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, io::Write, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub file: PathBuf,
    pub sha256: String,
}

impl Source {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        if !self.file.is_absolute()
            || !fs::symlink_metadata(&self.file)
                .map_err(|e| e.to_string())?
                .is_file()
        {
            return Err("Fixed teaching must be an ordinary absolute file".into());
        }
        let bytes = read(&self.file)?;
        if digest(&bytes) != self.sha256 {
            return Err("Fixed program source changed".into());
        }
        Ok(bytes)
    }

    pub fn program(&self) -> Result<Program> {
        Program::parse(&self.bytes()?)
    }
}

// Use the existing Program validator's wildcard/overlap semantics. The probe
// never executes or enters a student prompt.
fn disjoint_from_target(program: &Program, target: Signature<'_>) -> Result<()> {
    let mut probe = program.clone();
    probe.overrides.push(Override {
        stage: target.stage.into(),
        recognition_phase: target.recognition_phase.map(str::to_owned),
        method: target.method.map(str::to_owned),
        expected_guide_sha256: digest("scope probe"),
        replacement_text: "Nonexecuting target-selector overlap validation. ".repeat(3),
        edits: vec![],
    });
    probe
        .validate()
        .map_err(|e| format!("Fixed teaching conflicts with the mutable target: {e}"))
}

pub(crate) fn validate_scope(plan: &Plan) -> Result<Option<Program>> {
    let Some(source) = &plan.fixed_program else {
        return Ok(None);
    };
    if !(plan.function == Function::ReadingJourney || plan.supplying_extractor_observation)
        || plan.control_mode != ControlMode::FreshHosted
        || plan.recovery.is_some()
    {
        return Err(
            "Fixed teaching requires a fresh reading journey, without legacy recovery".into(),
        );
    }
    let program = source.program()?;
    disjoint_from_target(&program, plan.signature())?;
    let forbidden: BTreeSet<_> = plan
        .development
        .iter()
        .map(|e| e.id.as_str())
        .chain(plan.reserved_ids.iter().map(String::as_str))
        .collect();
    if program
        .training_case_ids
        .iter()
        .any(|id| forbidden.contains(id.as_str()))
    {
        return Err("Fixed teaching trained on a current development or reserved case".into());
    }
    Ok(Some(program))
}

/// Called before native inspections as well as on each resumed source check.
/// Only frozen partition metadata is needed; no case words or gold are read.
pub(crate) fn verify_membership(plan: &Plan, split: &Value, dev: &[String]) -> Result<()> {
    let Some(program) = validate_scope(plan)? else {
        return Ok(());
    };
    let rows = split["cases"].as_array().ok_or("No frozen split cases")?;
    for (ids, partition) in [
        (&program.training_case_ids, "training"),
        (&program.holdout_case_ids, "reserved_validation"),
    ] {
        for id in ids {
            let matching: Vec<_> = rows.iter().filter(|row| row["id"] == *id).collect();
            if matching.len() != 1 || matching[0]["partition"] != partition {
                return Err(format!(
                    "Fixed case {id} has missing, duplicate or changed frozen membership"
                ));
            }
            if partition == "training" && dev.contains(id) {
                return Err("Fixed teaching trained on a current development case".into());
            }
        }
    }
    Ok(())
}

/// Rebinding is execution metadata only. Original bytes and original evidence
/// remain pinned separately; it does not qualify old teaching on a new corpus.
pub(crate) fn compose(plan: &Plan, mutable: Option<Program>) -> Result<Option<Program>> {
    let Some(mut fixed) = validate_scope(plan)? else {
        return Ok(mutable);
    };
    crate::verify(&plan.split_file, &plan.split_sha256)?;
    let split = crate::load(&plan.split_file)?;
    plan.verify_split(&split)?;
    verify_membership(
        plan,
        &split,
        &plan
            .development
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
    )?;
    let source = plan.fixed_program.as_ref().ok_or("Missing fixed source")?;
    let identity = json!({"fixed_source_sha256":source.sha256,
        "original_manifest_sha256":fixed.baseline_manifest_sha256,
        "current_manifest_sha256":plan.manifest_sha256,
        "mutable_program_sha256":mutable.as_ref().map(|p|serde_json::to_vec(p).map(digest)).transpose().map_err(|e|e.to_string())?});
    let has_mutable = mutable.is_some();
    if let Some(mutable) = mutable {
        fixed.overrides.extend(mutable.overrides);
        fixed.evidence.extend(mutable.evidence);
        fixed.training_case_ids.extend(mutable.training_case_ids);
        fixed.holdout_case_ids.extend(mutable.holdout_case_ids);
        fixed.rationale = mutable.rationale;
        fixed.training_case_ids = fixed
            .training_case_ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        fixed.holdout_case_ids = fixed
            .holdout_case_ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        fixed.evidence.sort_by(|a, b| {
            (&a.case_id, &a.file, &a.json_pointer, &a.sha256).cmp(&(
                &b.case_id,
                &b.file,
                &b.json_pointer,
                &b.sha256,
            ))
        });
        fixed.evidence.dedup();
    }
    fixed.id = format!(
        "gepa-{}-{}",
        if has_mutable { "composed" } else { "fixed" },
        &digest(identity.to_string())[..32]
    );
    fixed.baseline_manifest_sha256 = plan.manifest_sha256.clone();
    fixed.validate()?;
    Ok(Some(fixed))
}

pub(crate) fn receipt(plan: &Plan, execution: &Program) -> Result<Value> {
    let source = plan.fixed_program.as_ref().ok_or("No fixed source")?;
    let original = source.program()?;
    Ok(
        json!({"version":1,"source":source,"original_program_id":original.id,
        "original_baseline_manifest_sha256":original.baseline_manifest_sha256,
        "current_corpus_manifest_sha256":plan.manifest_sha256,
        "execution_program_id":execution.id,
        "execution_program_sha256":digest(serde_json::to_vec_pretty(execution).map_err(|e|e.to_string())?),
        "fixed_overrides_sha256":digest(serde_json::to_vec(&original.overrides).map_err(|e|e.to_string())?),
        "evidence_origins":{"fixed":"Exact original Program bytes; original baseline manifest remains its evidence scope.",
            "mutable":"This round's current corpus and reflection-training IDs only."},
        "qualification":"Teaching-only composition; corpus metadata rebinding is not new semantic or reading qualification. No expected facts, accepted state or checkpoint is supplied."}),
    )
}

pub(crate) fn write_source(plan: &Plan, directory: &std::path::Path) -> Result<PathBuf> {
    let bytes = plan
        .fixed_program
        .as_ref()
        .ok_or("No fixed source")?
        .bytes()?;
    let file = directory.join("fixed-program-source.json");
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .map_err(|e| e.to_string())?;
    output.write_all(&bytes).map_err(|e| e.to_string())?;
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BookContext, Objective, ReadingStage};
    use horary_prompt_program::{Evidence, Signature};

    fn setup() -> (tempfile::TempDir, Plan, Program) {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = crate::tests::plan();
        plan.version = 3;
        plan.function = Function::ReadingJourney;
        plan.objective = Objective::SelectedStageReliability;
        plan.control_mode = ControlMode::FreshHosted;
        plan.target_stage = Some(ReadingStage::Condition);
        plan.target_method = Some("movable_deal".into());
        plan.max_review_calls = 4;
        plan.review_seconds = 120;
        plan.max_physical_generation_attempts =
            plan.logical_calls_per_function * plan.function.generation_factor();
        for example in plan.training.iter_mut().chain(&mut plan.development) {
            example.source_rubric_sha256 = Some(digest("synthetic rubric"));
        }
        let book = dir.path().join("book.md");
        fs::write(
            &book,
            "<!-- page:0016 -->\n## Page 16\nSynthetic immutable source witness.\n",
        )
        .unwrap();
        plan.review_book = Some(BookContext {
            file: book.clone(),
            sha256: digest(read(&book).unwrap()),
            ocr_pages: vec![16],
        });
        plan.seed = plan.seed_for_guide().unwrap();
        let split = json!({"cases":[{"id":"train","partition":"training"},
            {"id":"development","partition":"training"},
            {"id":"prior-training","partition":"training"},
            {"id":"reserved","partition":"reserved_validation"}]});
        plan.split_file = dir.path().join("split.json");
        crate::keep(&plan.split_file, &split).unwrap();
        plan.split_sha256 = digest(read(&plan.split_file).unwrap());
        let original = Program {
            version: 1,
            id: "previously-selected-extractor".into(),
            baseline_manifest_sha256: digest("prior corpus"),
            overrides: vec![Override {
                stage: "intake".into(),
                recognition_phase: Some("complete_selected_program".into()),
                method: Some("movable_deal".into()),
                expected_guide_sha256: digest("Prior extractor source guide"),
                replacement_text: "Immutable sourced USER-fact extraction teaching. ".repeat(4),
                edits: vec![],
            }],
            rationale: "Prior stage-only evidence, never installed.".into(),
            evidence: vec![Evidence {
                case_id: "prior-training".into(),
                file: "cases/prior-training/calls/0002-result.json".into(),
                json_pointer: "/result".into(),
                sha256: digest("prior training observation"),
            }],
            training_case_ids: vec!["prior-training".into()],
            holdout_case_ids: vec!["reserved".into()],
        };
        pin(&mut plan, &original, dir.path());
        (dir, plan, original)
    }

    fn pin(plan: &mut Plan, program: &Program, dir: &std::path::Path) {
        let file = dir.join("prior-program.json");
        fs::write(&file, serde_json::to_vec_pretty(program).unwrap()).unwrap();
        plan.fixed_program = Some(Source {
            sha256: digest(read(&file).unwrap()),
            file,
        });
    }

    #[test]
    fn legacy_absence_keeps_plan_program_and_mutation_scope_unchanged() {
        let plan = crate::tests::plan();
        let encoded = serde_json::to_value(&plan).unwrap();
        assert!(encoded.get("fixed_program").is_none());
        let decoded: Plan = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), encoded);
        assert!(decoded.program(&decoded.seed).unwrap().is_none());
        let mut candidate = decoded.seed.clone();
        candidate.insert(
            decoded.region_key(0),
            "Changed target teaching only. ".repeat(5),
        );
        assert_eq!(
            decoded
                .program(&candidate)
                .unwrap()
                .unwrap()
                .overrides
                .len(),
            1
        );
    }

    #[test]
    fn fixed_seed_has_only_pinned_teaching_and_composition_is_deterministic() {
        let (dir, plan, original) = setup();
        plan.validate().unwrap();
        let seed = plan.program(&plan.seed).unwrap().unwrap();
        assert_eq!(seed.overrides, original.overrides);
        assert_eq!(seed.evidence, original.evidence);
        assert_eq!(seed.training_case_ids, original.training_case_ids);
        assert_eq!(seed.baseline_manifest_sha256, plan.manifest_sha256);
        assert!(seed.apply(plan.signature(), &plan.guide).unwrap().is_none());
        let mut changed = plan.seed.clone();
        changed.insert(
            plan.region_key(0),
            "Changed condition-stage decision teaching. ".repeat(5),
        );
        let program = plan.program(&changed).unwrap().unwrap();
        assert_eq!(program, plan.program(&changed).unwrap().unwrap());
        assert_eq!(program.overrides[..1], original.overrides);
        assert_eq!(program.overrides.len(), 2);
        assert_eq!(program.overrides[1].stage, "condition");
        assert!(program.evidence.contains(&original.evidence[0]));
        assert_eq!(program.training_case_ids, vec!["prior-training", "train"]);
        let (teaching, _) = program
            .apply(plan.signature(), &plan.guide)
            .unwrap()
            .unwrap();
        assert_eq!(
            horary_prompt_program::guide_parts(&teaching)
                .unwrap()
                .quoted_source,
            horary_prompt_program::guide_parts(&plan.guide)
                .unwrap()
                .quoted_source
        );
        let out = dir.path().join("operation");
        fs::create_dir(&out).unwrap();
        let copied = write_source(&plan, &out).unwrap();
        assert_eq!(
            read(&copied).unwrap(),
            plan.fixed_program.as_ref().unwrap().bytes().unwrap()
        );
        assert!(write_source(&plan, &out).is_err());
        let provenance = receipt(&plan, &program).unwrap();
        assert_eq!(
            provenance["original_baseline_manifest_sha256"],
            original.baseline_manifest_sha256
        );
        let actual_program = out.join("program.json");
        crate::keep(&actual_program, &program).unwrap();
        assert_eq!(
            provenance["execution_program_sha256"],
            digest(read(&actual_program).unwrap())
        );
    }

    #[test]
    fn composition_preserves_legacy_and_effective_system_capture_identities() {
        let (_dir, plan, _) = setup();
        let mut candidate = plan.seed.clone();
        candidate.insert(
            plan.region_key(0),
            "Changed condition-stage decision teaching. ".repeat(5),
        );
        let execution = plan.program(&candidate).unwrap().unwrap();
        let (effective, applied) = execution
            .apply(plan.signature(), &plan.guide)
            .unwrap()
            .unwrap();
        let effective_sha = digest(&effective);
        assert_ne!(effective_sha, plan.guide_sha256);
        let mut call = json!({"stage":"condition", "prompt":[
            {"role":"system","content":effective},
            {"role":"user","content":"Original authored input and schema"}],
            "guide_sha256":effective_sha,
            "baseline_guide_sha256":plan.guide_sha256,
            "prompt_program":applied});
        assert_eq!(
            horary_prompt_program::captured_record_guide_sha256(&call, None).unwrap(),
            plan.guide_sha256
        );
        call["executor_guide_identity"] = json!(horary_prompt_program::EFFECTIVE_GUIDE_CAPTURE);
        assert_eq!(
            horary_prompt_program::captured_record_guide_sha256(&call, None).unwrap(),
            effective_sha
        );
        call["guide_sha256"] = json!(plan.guide_sha256);
        assert!(horary_prompt_program::captured_record_guide_sha256(&call, None).is_err());
    }

    #[test]
    fn overlap_including_wildcards_and_stale_pins_fail_closed() {
        let (dir, mut plan, mut original) = setup();
        original.overrides[0].stage = "condition".into();
        original.overrides[0].recognition_phase = None;
        for method in [None, Some("movable_deal".into())] {
            original.overrides[0].method = method;
            pin(&mut plan, &original, dir.path());
            assert!(plan.program(&plan.seed).unwrap_err().contains("conflicts"));
        }
        let (_source_dir, plan, _) = setup();
        fs::write(&plan.fixed_program.as_ref().unwrap().file, "changed source").unwrap();
        assert!(plan
            .program(&plan.seed)
            .unwrap_err()
            .contains("source changed"));
    }

    #[test]
    fn frozen_membership_excludes_current_development_reserved_and_duplicates() {
        let (dir, mut plan, original) = setup();
        for id in ["development", "reserved", "not-in-frozen-split"] {
            let mut changed = original.clone();
            changed.training_case_ids = vec![id.into()];
            changed.evidence[0].case_id = id.into();
            if id == "reserved" {
                changed.holdout_case_ids = vec!["prior-training".into()];
            }
            pin(&mut plan, &changed, dir.path());
            assert!(plan.program(&plan.seed).is_err());
        }
        pin(&mut plan, &original, dir.path());
        let mut split = crate::load(&plan.split_file).unwrap();
        split["cases"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"prior-training","partition":"training"}));
        fs::write(&plan.split_file, serde_json::to_vec(&split).unwrap()).unwrap();
        plan.split_sha256 = digest(read(&plan.split_file).unwrap());
        assert!(plan.program(&plan.seed).unwrap_err().contains("duplicate"));
        split["cases"].as_array_mut().unwrap().pop();
        split["cases"][3]["partition"] = json!("training");
        assert!(verify_membership(&plan, &split, &[]).is_err());
    }

    #[test]
    fn fixed_content_cannot_become_a_component_or_inject_facts_schema_or_checkpoint() {
        let (_dir, plan, original) = setup();
        let mut escaped = plan.seed.clone();
        escaped.insert(
            "complete_selected_program.movable_deal.region.000".into(),
            "Mutation of fixed extractor".repeat(4),
        );
        assert!(plan
            .program(&escaped)
            .unwrap_err()
            .contains("component set"));
        let sample = vec![(plan.training[0].clone(), Value::Null)];
        assert!(crate::teacher::prepare(
            &plan,
            &plan.seed,
            &["complete_selected_program.movable_deal.region.000".into()],
            &sample,
        )
        .err()
        .unwrap()
        .contains("outside the mutable target"));
        for field in [
            "expected_facts",
            "accepted_consultation",
            "reading_checkpoint",
        ] {
            let mut value = serde_json::to_value(&original).unwrap();
            value[field] = json!({"owner":"invented"});
            assert!(Program::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut messages = json!([{"role":"system","content":"Prior extractor source guide"},
            {"role":"user","content":"Original authored words and immutable native schema"}]);
        let user = messages[1].clone();
        let program = plan.program(&plan.seed).unwrap().unwrap();
        horary_prompt_program::apply_messages(
            &program,
            Signature {
                stage: "intake",
                recognition_phase: Some("complete_selected_program"),
                method: Some("movable_deal"),
            },
            &mut messages,
        )
        .unwrap();
        assert_eq!(messages[1], user);
        assert_eq!(
            messages[0]["content"],
            original.overrides[0].replacement_text
        );
    }

    #[test]
    fn prerequisites_cannot_silently_change_old_function_or_recovery_modes() {
        let (_dir, mut plan, _) = setup();
        plan.function = Function::InputJourney;
        assert!(validate_scope(&plan).unwrap_err().contains("fresh reading"));
        plan.function = Function::ReadingJourney;
        plan.control_mode = ControlMode::ArchivedCapture;
        assert!(validate_scope(&plan).is_err());
    }
    #[test]
    fn supplying_fixed_initial_lesson_preserves_target_disjointness_and_split_guards() {
        let (dir, mut plan, mut original) = setup();
        plan.function = Function::InputJourney;
        plan.supplying_extractor_observation = true;
        plan.target_stage = None;
        plan.objective = Objective::ExtractorReliability;
        plan.seed = plan.seed_for_guide().unwrap();
        assert!(validate_scope(&plan).unwrap().is_some());
        let split = crate::load(&plan.split_file).unwrap();
        verify_membership(&plan, &split, &["development".into()]).unwrap();
        original.training_case_ids = vec!["development".into()];
        pin(&mut plan, &original, dir.path());
        assert!(verify_membership(&plan, &split, &["development".into()]).is_err());
        original.training_case_ids = vec!["reserved".into()];
        pin(&mut plan, &original, dir.path());
        assert!(validate_scope(&plan).is_err());
        original.training_case_ids = vec!["prior-training".into()];
        original.overrides[0].recognition_phase = Some("update_selected_program".into());
        pin(&mut plan, &original, dir.path());
        assert!(validate_scope(&plan).unwrap_err().contains("conflicts"));
        original.overrides[0].recognition_phase = Some("complete_selected_program".into());
        pin(&mut plan, &original, dir.path());
        fs::write(
            &plan.fixed_program.as_ref().unwrap().file,
            b"changed original lesson",
        )
        .unwrap();
        assert!(validate_scope(&plan).unwrap_err().contains("changed"));
    }
}
