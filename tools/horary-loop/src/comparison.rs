//! Full-reading promotion adds semantic gates to the unchanged elicitation
//! comparison. An accepted worksheet is not a reviewed interpretation.
#![forbid(unsafe_code)]

use horary_prompt_program::comparison as native;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub use native::Policy;

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub eligible: bool,
    pub blockers: Vec<String>,
    pub semantic_improvements: Vec<String>,
    pub journey_improvements: Vec<String>,
    pub conversation_improvements: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pipeline_improvements: Vec<String>,
    pub qualification: String,
}

impl From<native::Comparison> for Comparison {
    fn from(measured: native::Comparison) -> Self {
        Self {
            eligible: measured.eligible,
            blockers: measured.blockers,
            semantic_improvements: measured.semantic_improvements,
            journey_improvements: measured.journey_improvements,
            conversation_improvements: measured.conversation_improvements,
            pipeline_improvements: Vec::new(),
            qualification: measured.qualification,
        }
    }
}

const GATES: [&str; 4] = ["classification", "extraction", "elicitation", "reading"];

fn full_scope(report: &Value) -> bool {
    report["manifest"]["full_reading"] == true
        || report["manifest"]["entry_point"] == "horary_pipeline::run"
}

fn rows<'a>(
    value: &'a Value,
    key: &str,
    id_key: &str,
) -> Result<BTreeMap<&'a str, &'a Value>, String> {
    let mut result = BTreeMap::new();
    for row in value[key]
        .as_array()
        .ok_or_else(|| format!("Missing {key}"))?
    {
        let id = row[id_key].as_str().ok_or("Missing comparison case ID")?;
        if result.insert(id, row).is_some() {
            return Err(format!("Duplicate comparison case {id}"));
        }
    }
    Ok(result)
}

fn review_rows(judgments: &[Value]) -> Result<BTreeMap<&str, &Value>, String> {
    let mut result = BTreeMap::new();
    for judgment in judgments {
        for (id, review) in rows(judgment, "reviews", "case_id")? {
            if result.insert(id, review).is_some() {
                return Err(format!("Duplicate pipeline review {id}"));
            }
        }
    }
    Ok(result)
}

fn inquiry_required(outcome: &Value) -> bool {
    [
        "/grade/actual/needs",
        "/follow_up/grade/current_needs",
        "/follow_up/grade/native_needs",
    ]
    .iter()
    .any(|pointer| {
        outcome
            .pointer(pointer)
            .and_then(Value::as_array)
            .is_some_and(|needs| !needs.is_empty())
    })
}

/// None means a legitimate absence of elicitation work, never an unobserved
/// reading. Invalid scores return an error instead of acquiring a default 2.
fn score(review: &Value, outcome: &Value, gate: &str) -> Result<Option<u64>, &'static str> {
    let dimension = &review["pipeline"][gate];
    if dimension["state"] == "not_applicable"
        && gate == "elicitation"
        && dimension["score"].is_null()
        && !inquiry_required(outcome)
    {
        return Ok(None);
    }
    if dimension["state"] != "scored" {
        return Err("missing, unobserved or invalid independent pipeline review");
    }
    dimension["score"]
        .as_u64()
        .filter(|score| *score <= 2)
        .map(Some)
        .ok_or("invalid independent pipeline score")
}

fn final_result_kind(outcome: &Value) -> Option<&str> {
    let pointer = match outcome["follow_up_execution_completed"].as_bool() {
        Some(true) => "/follow_up/native_result/result",
        Some(false) => return None,
        None => "/grade/actual/native_result/result",
    };
    outcome.pointer(pointer).and_then(Value::as_str)
}

fn interrupted(outcome: &Value) -> bool {
    outcome["first_turn_execution_completed"] != true
        || outcome["follow_up_execution_completed"] == false
        || !outcome["infrastructure_error"].is_null()
        || !outcome["provider_stop"].is_null()
        || outcome["deadline_cancelled"] == true
        || outcome["group_cancelled"] == true
        || outcome["execution_status"]
            .as_str()
            .is_some_and(|status| status != "completed")
}

/// The control can finish honestly with incomplete inputs. This is a measured
/// failure, not a successful old reading or an interrupted native execution.
fn recoverable_input_failure(outcome: &Value) -> bool {
    !interrupted(outcome)
        && matches!(
            final_result_kind(outcome),
            Some("needs_information" | "limited")
        )
        && GATES[..3].iter().any(|gate| {
            matches!(
                outcome["hurdles"][gate]["status"].as_str(),
                Some("fail" | "awaiting_information" | "blocked")
            )
        })
}

pub fn compare(
    baseline: &Value,
    trial: &Value,
    baseline_judgments: &[Value],
    trial_judgments: &[Value],
) -> Result<Comparison, String> {
    compare_with_policy(
        baseline,
        trial,
        baseline_judgments,
        trial_judgments,
        Policy::Training,
    )
}

pub fn compare_with_policy(
    baseline: &Value,
    trial: &Value,
    baseline_judgments: &[Value],
    trial_judgments: &[Value],
    policy: Policy,
) -> Result<Comparison, String> {
    let full = full_scope(baseline) || full_scope(trial);
    // Full training can improve a pipeline score without changing conversation.
    // The no-gain training check is reapplied below using all four score sets.
    let shared_policy = if full {
        Policy::ReservedValidation
    } else {
        policy
    };
    let mut comparison: Comparison = native::compare_with_policy(
        baseline,
        trial,
        baseline_judgments,
        trial_judgments,
        shared_policy,
    )?
    .into();
    if !full {
        return Ok(comparison);
    }
    comparison.qualification = "A paired full-reading decision on these authored cases and exact source rubrics. The candidate must pass native structure and independent classification, extraction, elicitation and source-grounded reading review. A completed control input failure stays a failure, with its absent reading unobserved; capability boundaries are not readings. Hosted inference does not qualify the on-device model. Reserved validation is not blind or astrology-SME qualification.".into();
    if !full_scope(baseline) || !full_scope(trial) {
        comparison
            .blockers
            .push("Full-reading and elicitation-only comparison scopes differ".into());
    }
    for (label, report) in [("control", baseline), ("candidate", trial)] {
        if report["manifest"]["full_reading"] == false
            || report["manifest"]["entry_point"] != "horary_pipeline::run"
        {
            comparison
                .blockers
                .push(format!("{label}: inconsistent full-reading manifest scope"));
        }
    }
    let before_rubric = &baseline["manifest"]["reading_rubric_sha256"];
    if before_rubric.as_str().is_none_or(str::is_empty)
        || before_rubric != &trial["manifest"]["reading_rubric_sha256"]
    {
        comparison
            .blockers
            .push("Paired source reading-rubric fingerprints differ or are absent".into());
    }
    let prior_cases = rows(baseline, "cases", "id")?;
    let current_cases = rows(trial, "cases", "id")?;
    let prior_reviews = review_rows(baseline_judgments)?;
    let current_reviews = review_rows(trial_judgments)?;
    for (id, before) in &prior_cases {
        let Some(after) = current_cases.get(id) else {
            continue;
        };
        for (label, outcome) in [("control", *before), ("candidate", *after)] {
            if outcome["full_reading"] != true {
                comparison
                    .blockers
                    .push(format!("{id}/{label}: absent full-reading case witness"));
            }
            if interrupted(outcome) {
                comparison.blockers.push(format!(
                    "{id}/{label}: incomplete or infrastructure-interrupted pipeline"
                ));
            }
            for gate in GATES {
                let required = if gate == "reading" {
                    "structure_pass_review_pending"
                } else {
                    "pass"
                };
                let status = outcome["hurdles"][gate]["status"].as_str();
                if label == "control" {
                    if !matches!(
                        status,
                        Some(
                            "pass"
                                | "fail"
                                | "awaiting_information"
                                | "blocked"
                                | "not_run"
                                | "structure_pass_review_pending"
                        )
                    ) {
                        comparison.blockers.push(format!(
                            "{id}/control/{gate}: missing native hurdle witness"
                        ));
                    }
                } else if outcome["hurdles"][gate]["status"] != required {
                    comparison.blockers.push(format!("{id}/{label}/{gate}: native hurdle is blocked, incomplete or failed; no reading qualification"));
                }
            }
        }
        if final_result_kind(after) != Some("judgment") {
            comparison.blockers.push(format!(
                "{id}/candidate: missing actual final judgment witness"
            ));
        }
        let (Some(prior), Some(current)) = (prior_reviews.get(id), current_reviews.get(id)) else {
            // The shared comparison already records absent paired reviews.
            continue;
        };
        let recoverable = recoverable_input_failure(before);
        if final_result_kind(before) != Some("judgment") && !recoverable {
            comparison.blockers.push(format!("{id}/control/reading: no observed interpretation or completed recoverable input failure; a method boundary is not a control reading"));
        }
        for gate in GATES {
            let old = score(prior, before, gate);
            let new = score(current, after, gate);
            let unobserved_control = recoverable
                && prior["pipeline"][gate]["state"] == "unobserved"
                && prior["pipeline"][gate]["score"].is_null()
                && (gate == "reading" || before["hurdles"][gate]["status"] != "pass");
            if recoverable && gate == "reading" && !unobserved_control {
                comparison.blockers.push(format!("{id}/control/reading: no interpretation was observed; its score must remain explicitly unobserved"));
            }
            if let Err(reason) = &old {
                if !unobserved_control {
                    comparison
                        .blockers
                        .push(format!("{id}/control/{gate}: {reason}"));
                }
            }
            if let Err(reason) = &new {
                comparison
                    .blockers
                    .push(format!("{id}/candidate/{gate}: {reason}"));
            }
            if new
                .as_ref()
                .is_ok_and(|score| score.is_some_and(|score| score < 2))
            {
                comparison.blockers.push(format!("{id}/candidate/{gate}: partial or failed semantic gate; a score of 2 is required"));
            }
            if unobserved_control {
                if gate == "reading"
                    && new == Ok(Some(2))
                    && final_result_kind(after) == Some("judgment")
                    && after["hurdles"]["reading"]["status"] == "structure_pass_review_pending"
                {
                    if !comparison
                        .journey_improvements
                        .iter()
                        .any(|case| case.as_str() == *id)
                    {
                        comparison.journey_improvements.push((*id).into());
                    }
                    comparison.pipeline_improvements.push(format!("{id}/reading: completed reviewed interpretation after a control input failure; prior reading remains unobserved"));
                }
                continue;
            }
            if let (Ok(old), Ok(new)) = (old, new) {
                if let (Some(old), Some(new)) = (old, new) {
                    if new < old {
                        comparison
                            .blockers
                            .push(format!("{id}/{gate}: pipeline regression {old}->{new}"));
                    } else if new > old {
                        comparison
                            .pipeline_improvements
                            .push(format!("{id}/{gate}: {old}->{new}"));
                    }
                } else if gate == "elicitation"
                    && recoverable
                    && old.is_some_and(|score| score < 2)
                    && new.is_none()
                {
                    comparison.pipeline_improvements.push(format!("{id}/elicitation: unnecessary tracked inquiry removed; candidate requires no inquiry"));
                } else if old.is_some() != new.is_some() {
                    comparison.blockers.push(format!("{id}/{gate}: elicitation applicability changed; paired scores cannot be compared"));
                }
            }
        }
    }
    if policy == Policy::Training
        && comparison.semantic_improvements.is_empty()
        && comparison.journey_improvements.is_empty()
        && comparison.conversation_improvements.is_empty()
        && comparison.pipeline_improvements.is_empty()
    {
        comparison.blockers.push(
            "No measured semantic, continuation, conversational or pipeline improvement".into(),
        );
    }
    comparison.eligible = comparison.blockers.is_empty();
    Ok(comparison)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(full: bool) -> Value {
        let mut manifest = serde_json::Map::new();
        for key in [
            "version",
            "sources",
            "fixture_sha256",
            "book_ocr_sha256",
            "model",
            "decoder",
            "temperature",
            "seed",
            "ctx_tokens",
        ] {
            manifest.insert(key.into(), json!("same"));
        }
        manifest.insert(
            "entry_point".into(),
            json!(if full {
                "horary_pipeline::run"
            } else {
                "horary_pipeline::run_elicitation"
            }),
        );
        if full {
            manifest.insert("full_reading".into(), json!(true));
            manifest.insert("reading_rubric_sha256".into(), json!("same-frozen-rubric"));
        }
        manifest.insert("prompt_program".into(), json!({"sha256":"candidate"}));
        let mut report = json!({"campaign_state":{"status":"completed"},"manifest":manifest,"prompt_program_applied_calls":1,
            "cases":[{"id":"case","grade":{"semantic_pass":true,"actual":{"needs":[],"native_result":{"result":"judgment"}}},"first_turn_execution_completed":true,
                "follow_up_scripted":false,"follow_up_execution_completed":null,"follow_up_pass":null,"infrastructure_error":null}]});
        if full {
            report["cases"][0]["full_reading"] = json!(true);
            report["cases"][0]["hurdles"] = json!({"classification":{"status":"pass"},"extraction":{"status":"pass"},"elicitation":{"status":"pass"},"reading":{"status":"structure_pass_review_pending"}});
        }
        report
    }

    fn review(conversation: u8, pipeline: Option<u8>) -> Value {
        let mut rubric = serde_json::Map::new();
        for key in [
            "concern_actor",
            "evidence_honesty",
            "useful_inquiry",
            "natural_phrasing",
            "continuity",
        ] {
            rubric.insert(
                key.into(),
                json!({"state":"scored","score":if key=="evidence_honesty" {2}else{conversation}}),
            );
        }
        let mut review =
            json!({"reviews":[{"case_id":"case","first_turn":rubric,"follow_up":null}]});
        if let Some(score) = pipeline {
            review["reviews"][0]["pipeline"] = json!({"classification":{"state":"scored","score":score},"extraction":{"state":"scored","score":score},
                "elicitation":{"state":"scored","score":score},"reading":{"state":"scored","score":score}});
        }
        review
    }

    fn compare_full(before: &Value, after: &Value, old: Value, new: Value) -> Comparison {
        compare(before, after, &[old], &[new]).unwrap()
    }

    #[test]
    fn elicitation_comparison_preserves_the_existing_serialized_decision() {
        let report = report(false);
        for policy in [Policy::Training, Policy::ReservedValidation] {
            let before = [review(1, None)];
            let after = [review(2, None)];
            let old =
                native::compare_with_policy(&report, &report, &before, &after, policy).unwrap();
            let new = compare_with_policy(&report, &report, &before, &after, policy).unwrap();
            assert_eq!(
                serde_json::to_value(old).unwrap(),
                serde_json::to_value(new).unwrap()
            );
        }
    }

    #[test]
    fn full_reading_cannot_qualify_from_legacy_flags_missing_or_unobserved_scores() {
        let report = report(true);
        let missing = compare_full(&report, &report, review(1, None), review(2, None));
        assert!(!missing.eligible);
        assert!(missing
            .blockers
            .iter()
            .any(|b| b.contains("independent pipeline review")));
        for gate in GATES {
            for state in ["unobserved", "not_applicable"] {
                let mut candidate = review(2, Some(2));
                candidate["reviews"][0]["pipeline"][gate] = json!({"state":state,"score":null});
                let decision = compare_full(&report, &report, review(1, Some(2)), candidate);
                assert!(!decision.eligible);
            }
        }
    }

    #[test]
    fn full_pipeline_improvement_counts_but_partial_or_regressed_gates_do_not_pass() {
        let report = report(true);
        let improved = compare_full(&report, &report, review(2, Some(1)), review(2, Some(2)));
        assert!(improved.eligible);
        assert_eq!(improved.pipeline_improvements.len(), 4);
        for gate in GATES {
            let mut candidate = review(2, Some(2));
            candidate["reviews"][0]["pipeline"][gate]["score"] = json!(1);
            let decision = compare_full(&report, &report, review(1, Some(2)), candidate);
            assert!(!decision.eligible);
            assert!(decision
                .blockers
                .iter()
                .any(|b| b.contains("pipeline regression")));
        }
        assert!(!compare_full(&report, &report, review(1, Some(1)), review(2, Some(1))).eligible);
    }

    #[test]
    fn blocked_reading_and_changed_rubric_or_scope_do_not_become_passes() {
        let before = report(true);
        for status in ["blocked", "not_run", "awaiting_information", "fail"] {
            let mut after = before.clone();
            after["cases"][0]["hurdles"]["reading"]["status"] = json!(status);
            let decision = compare_full(&before, &after, review(1, Some(2)), review(2, Some(2)));
            assert!(!decision.eligible);
            assert!(decision
                .blockers
                .iter()
                .any(|b| b.contains("no reading qualification")));
        }
        let mut changed = before.clone();
        changed["manifest"]["reading_rubric_sha256"] = json!("changed-source-rubric");
        assert!(!compare_full(&before, &changed, review(1, Some(2)), review(2, Some(2))).eligible);
        let decision = compare_full(&before, &report(false), review(1, Some(2)), review(2, None));
        assert!(!decision.eligible);
        assert!(decision
            .blockers
            .iter()
            .any(|b| b.contains("comparison scopes differ")));
    }

    fn failed_control(kind: &str, gate: &str) -> (Value, Value) {
        let mut before = report(true);
        before["cases"][0]["grade"]["semantic_pass"] = json!(false);
        before["cases"][0]["grade"]["actual"]["native_result"]["result"] = json!(kind);
        before["cases"][0]["hurdles"][gate]["status"] = json!("fail");
        before["cases"][0]["hurdles"]["reading"]["status"] = json!("blocked");
        let mut judged = review(2, Some(2));
        judged["reviews"][0]["pipeline"][gate]["score"] = json!(0);
        judged["reviews"][0]["pipeline"]["reading"] = json!({"state":"unobserved","score":null});
        (before, judged)
    }

    #[test]
    fn completed_control_input_failure_can_improve_without_inventing_a_prior_reading() {
        for kind in ["needs_information", "limited"] {
            for gate in GATES[..3].iter().copied() {
                let (before, old) = failed_control(kind, gate);
                let original = old.clone();
                let decision =
                    compare_full(&before, &report(true), old.clone(), review(2, Some(2)));
                assert!(decision.eligible, "{:?}", decision.blockers);
                assert!(decision.journey_improvements.iter().any(|id| id == "case"));
                assert!(decision
                    .pipeline_improvements
                    .iter()
                    .any(|gain| gain.contains("prior reading remains unobserved")));
                assert_eq!(old, original);
                assert_eq!(
                    old["reviews"][0]["pipeline"]["reading"]["state"],
                    "unobserved"
                );
                let mut fabricated = old;
                fabricated["reviews"][0]["pipeline"]["reading"] =
                    json!({"state":"scored","score":0});
                assert!(
                    !compare_full(&before, &report(true), fabricated, review(2, Some(2))).eligible
                );
            }
        }
    }

    #[test]
    fn recovery_does_not_excuse_interruption_missing_witnesses_or_candidate_failure() {
        let (before, old) = failed_control("needs_information", "extraction");
        for (field, value) in [
            ("deadline_cancelled", json!(true)),
            ("group_cancelled", json!(true)),
            ("infrastructure_error", json!("HTTP500")),
            ("provider_stop", json!("HTTP429")),
            ("first_turn_execution_completed", json!(false)),
            ("execution_status", json!("execution_error")),
        ] {
            let mut interrupted = before.clone();
            interrupted["cases"][0][field] = value;
            assert!(
                !compare_full(&interrupted, &report(true), old.clone(), review(2, Some(2)))
                    .eligible
            );
        }
        let mut missing = before.clone();
        missing["cases"][0]["grade"]["actual"]["native_result"] = Value::Null;
        assert!(!compare_full(&missing, &report(true), old.clone(), review(2, Some(2))).eligible);
        for status in ["blocked", "awaiting_information", "fail"] {
            let mut candidate = report(true);
            candidate["cases"][0]["hurdles"]["reading"]["status"] = json!(status);
            assert!(!compare_full(&before, &candidate, old.clone(), review(2, Some(2))).eligible);
        }
        let mut candidate = review(2, Some(2));
        candidate["reviews"][0]["pipeline"]["reading"] = json!({"state":"unobserved","score":null});
        assert!(!compare_full(&before, &report(true), old.clone(), candidate).eligible);
        let mut unsupported = before;
        for gate in GATES[..3].iter().copied() {
            unsupported["cases"][0]["hurdles"][gate]["status"] = json!("pass");
        }
        unsupported["cases"][0]["grade"]["actual"]["native_result"]["result"] = json!("limited");
        assert!(!compare_full(&unsupported, &report(true), old, review(2, Some(2))).eligible);
    }

    #[test]
    fn unvisited_control_stage_remains_unobserved_and_redundant_asks_can_be_removed() {
        let (mut before, mut old) = failed_control("needs_information", "classification");
        before["cases"][0]["hurdles"]["extraction"]["status"] = json!("not_run");
        old["reviews"][0]["pipeline"]["extraction"] = json!({"state":"unobserved","score":null});
        assert!(compare_full(&before, &report(true), old.clone(), review(2, Some(2))).eligible);
        before["cases"][0]["hurdles"]["extraction"]["status"] = json!("pass");
        assert!(!compare_full(&before, &report(true), old, review(2, Some(2))).eligible);
        let (mut before, old) = failed_control("needs_information", "elicitation");
        before["cases"][0]["grade"]["actual"]["needs"] = json!(["redundant_owner"]);
        let mut new = review(2, Some(2));
        new["reviews"][0]["pipeline"]["elicitation"] =
            json!({"state":"not_applicable","score":null});
        assert!(compare_full(&before, &report(true), old, new).eligible);
    }

    #[test]
    fn elicitation_na_is_only_allowed_when_no_tracked_inquiry_and_applicability_matches() {
        let mut native_report = report(true);
        let mut judged = review(2, Some(2));
        judged["reviews"][0]["pipeline"]["elicitation"] =
            json!({"state":"not_applicable","score":null});
        assert!(
            compare_with_policy(
                &native_report,
                &native_report,
                &[judged.clone()],
                &[judged.clone()],
                Policy::ReservedValidation
            )
            .unwrap()
            .eligible
        );
        native_report["cases"][0]["grade"]["actual"]["needs"] = json!(["owner"]);
        assert!(
            !compare_with_policy(
                &native_report,
                &native_report,
                &[judged.clone()],
                &[judged],
                Policy::ReservedValidation
            )
            .unwrap()
            .eligible
        );
        let clean = report(true);
        assert!(
            !compare_with_policy(
                &clean,
                &clean,
                &[review(2, Some(2))],
                &[review(2, Some(2))],
                Policy::Training
            )
            .unwrap()
            .eligible
        );
        assert!(
            compare_with_policy(
                &clean,
                &clean,
                &[review(2, Some(2))],
                &[review(2, Some(2))],
                Policy::ReservedValidation
            )
            .unwrap()
            .eligible
        );
    }
}
