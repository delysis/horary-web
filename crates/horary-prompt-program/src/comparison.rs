//! Promotion checks are deliberately separate from the teacher's opinion.
//! Compare the same cases on the same native program, with every receipt kept.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Default)]
pub struct Comparison {
    pub eligible: bool,
    pub blockers: Vec<String>,
    pub semantic_improvements: Vec<String>,
    pub journey_improvements: Vec<String>,
    pub conversation_improvements: Vec<String>,
    pub qualification: String,
}

/// Training must improve something measurable. Reserved validation confirms
/// that a fixed candidate generalizes without requiring an additional gain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Training,
    ReservedValidation,
}

#[derive(Debug, Deserialize)]
struct Grade {
    semantic_pass: bool,
}
#[derive(Debug, Deserialize)]
struct Outcome {
    id: String,
    grade: Grade,
    first_turn_execution_completed: bool,
    follow_up_scripted: bool,
    follow_up_execution_completed: Option<bool>,
    follow_up_pass: Option<bool>,
    infrastructure_error: Option<String>,
}
impl Outcome {
    fn first_pass(&self) -> bool {
        self.first_turn_execution_completed && self.grade.semantic_pass
    }
    fn journey_pass(&self) -> bool {
        self.first_pass()
            && (!self.follow_up_scripted
                || (self.follow_up_execution_completed == Some(true)
                    && self.follow_up_pass == Some(true)))
    }
}

fn outcomes(report: &Value) -> Result<BTreeMap<String, Outcome>, String> {
    let cases: Vec<Outcome> = serde_json::from_value(report["cases"].clone())
        .map_err(|error| format!("Unusable campaign outcomes: {error}"))?;
    let mut map = BTreeMap::new();
    for case in cases {
        if map.insert(case.id.clone(), case).is_some() {
            return Err("Duplicate campaign case id".into());
        }
    }
    if map.is_empty() {
        return Err("No tested cases".into());
    }
    Ok(map)
}

const DIMENSIONS: &[&str] = &[
    "concern_actor",
    "evidence_honesty",
    "useful_inquiry",
    "natural_phrasing",
    "continuity",
];

fn reviews(judgments: &[Value]) -> Result<BTreeMap<String, Value>, String> {
    let mut map = BTreeMap::new();
    for judgment in judgments {
        for review in judgment["reviews"]
            .as_array()
            .ok_or("Missing typed reviews")?
        {
            let id = review["case_id"].as_str().ok_or("Missing review case id")?;
            if map.insert(id.to_owned(), review.clone()).is_some() {
                return Err(format!("Duplicate review for {id}"));
            }
        }
    }
    Ok(map)
}

/// Native assertions are checked case by case before any conversational score.
/// A teacher may never vote away a regression, an interruption or an absent
/// continuation. This decision qualifies only the supplied evaluated cases.
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
    let base = outcomes(baseline)?;
    let candidate = outcomes(trial)?;
    let base_reviews = reviews(baseline_judgments)?;
    let candidate_reviews = reviews(trial_judgments)?;
    let mut comparison = Comparison {
        qualification: "A paired decision on these authored scenarios. Reserved validation is not blind qualification, and elicitation does not certify the later horary judgment.".into(),
        ..Default::default()
    };
    if baseline["campaign_state"]["status"] != "completed"
        || trial["campaign_state"]["status"] != "completed"
    {
        comparison
            .blockers
            .push("Both paired campaigns must finish; absent outcomes are not passes".into());
    }
    // Source/native guards, gold, actual decoder and sampling must be identical.
    for key in [
        "version",
        "sources",
        "fixture_sha256",
        "book_ocr_sha256",
        "model",
        "decoder",
        "entry_point",
        "temperature",
        "seed",
        "ctx_tokens",
    ] {
        let before = &baseline["manifest"][key];
        let after = &trial["manifest"][key];
        if before.is_null() || after.is_null() || before != after {
            comparison
                .blockers
                .push(format!("Paired {key} fingerprints differ or are absent"));
        }
    }
    let ids: BTreeSet<_> = base.keys().collect();
    if ids != candidate.keys().collect() {
        comparison
            .blockers
            .push("Control and candidate case sets differ".into());
    }
    if trial["manifest"]["prompt_program"].is_null() {
        comparison
            .blockers
            .push("Trial has no hashed executable prompt candidate".into());
    }
    if trial["prompt_program_applied_calls"].as_u64().unwrap_or(0) == 0 {
        comparison
            .blockers
            .push("The candidate was not applied to a generated request".into());
    }
    for (id, before) in &base {
        let Some(after) = candidate.get(id) else {
            continue;
        };
        if before.infrastructure_error.is_some()
            || after.infrastructure_error.is_some()
            || !before.first_turn_execution_completed
            || !after.first_turn_execution_completed
            || before.follow_up_execution_completed == Some(false)
            || after.follow_up_execution_completed == Some(false)
        {
            comparison.blockers.push(format!(
                "{id}: paired execution is incomplete or interrupted"
            ));
        }
        if before.first_pass() && !after.first_pass() {
            comparison
                .blockers
                .push(format!("{id}: native first-turn regression"));
        }
        if before.journey_pass() && !after.journey_pass() {
            comparison.blockers.push(format!(
                "{id}: native continuation regression or withheld script"
            ));
        }
        if !before.first_pass() && after.first_pass() {
            comparison.semantic_improvements.push(id.clone());
        }
        if !before.journey_pass() && after.journey_pass() {
            comparison.journey_improvements.push(id.clone());
        }
        let (Some(before_review), Some(after_review)) =
            (base_reviews.get(id), candidate_reviews.get(id))
        else {
            comparison
                .blockers
                .push(format!("{id}: missing paired Codex review"));
            continue;
        };
        for turn in ["first_turn", "follow_up"] {
            if turn == "follow_up" && !after.follow_up_scripted {
                continue;
            }
            // A correctly withheld script remains an unqualified journey. It
            // does not need a fictitious conversational review of nonexistent words.
            if turn == "follow_up" && after.follow_up_execution_completed.is_none() {
                continue;
            }
            for dimension in DIMENSIONS {
                let prior = &before_review[turn][dimension];
                let current = &after_review[turn][dimension];
                let score = current["score"].as_u64();
                let earlier_score = prior["score"].as_u64();
                let na = current["state"] == "not_applicable" && *dimension == "useful_inquiry";
                if !na && (current["state"] != "scored" || score.is_none_or(|s| s > 2)) {
                    comparison.blockers.push(format!(
                        "{id}/{turn}/{dimension}: unobserved or invalid review"
                    ));
                    continue;
                }
                if *dimension == "evidence_honesty" && score != Some(2) {
                    comparison.blockers.push(format!(
                        "{id}/{turn}: unsupported or misleading candidate reply"
                    ));
                }
                if let (Some(old), Some(new)) = (earlier_score, score) {
                    if new < old {
                        comparison.blockers.push(format!(
                            "{id}/{turn}/{dimension}: conversational regression"
                        ));
                    } else if new > old {
                        comparison
                            .conversation_improvements
                            .push(format!("{id}/{turn}/{dimension}: {old}->{new}"));
                    }
                }
            }
        }
    }
    let improved = !comparison.semantic_improvements.is_empty()
        || !comparison.journey_improvements.is_empty()
        || !comparison.conversation_improvements.is_empty();
    if !improved && policy == Policy::Training {
        comparison
            .blockers
            .push("No measured semantic, continuation or conversational improvement".into());
    }
    comparison.eligible = comparison.blockers.is_empty();
    Ok(comparison)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn report(pass: bool, scripted: bool, after: Option<bool>) -> Value {
        let mut manifest = serde_json::Map::new();
        for key in [
            "version",
            "sources",
            "fixture_sha256",
            "book_ocr_sha256",
            "model",
            "decoder",
            "entry_point",
            "temperature",
            "seed",
            "ctx_tokens",
        ] {
            manifest.insert(key.into(), json!("same"));
        }
        manifest.insert("prompt_program".into(), json!({"sha256":"candidate"}));
        json!({"campaign_state":{"status":"completed"},"manifest":manifest,"prompt_program_applied_calls":1,
        "cases":[{"id":"case","grade":{"semantic_pass":pass},"first_turn_execution_completed":true,"follow_up_scripted":scripted,"follow_up_execution_completed":after.map(|_|true),"follow_up_pass":after,"infrastructure_error":null}]})
    }
    fn review(score: u8) -> Value {
        let mut rubric = serde_json::Map::new();
        for key in DIMENSIONS {
            rubric.insert(
                (*key).into(),
                json!({"state":"scored","score":if *key=="evidence_honesty" {2}else{score}}),
            );
        }
        json!({"reviews":[{"case_id":"case","first_turn":rubric,"follow_up":rubric}]})
    }
    #[test]
    fn improved_prose_cannot_offset_a_native_regression() {
        let decision = compare(
            &report(true, false, None),
            &report(false, false, None),
            &[review(1)],
            &[review(2)],
        )
        .unwrap();
        assert!(!decision.eligible);
        assert!(decision
            .blockers
            .iter()
            .any(|b| b.contains("native first-turn regression")));
    }
    #[test]
    fn withheld_followup_and_changed_decoder_cannot_qualify() {
        let mut trial = report(true, true, None);
        trial["manifest"]["decoder"] = json!("different");
        let decision = compare(
            &report(true, true, Some(true)),
            &trial,
            &[review(1)],
            &[review(2)],
        )
        .unwrap();
        assert!(!decision.eligible);
        assert!(decision.blockers.iter().any(|b| b.contains("withheld")));
        assert!(decision.blockers.iter().any(|b| b.contains("decoder")));
    }
    #[test]
    fn paired_quality_improvement_without_regression_is_eligible() {
        assert!(
            compare(
                &report(true, true, Some(true)),
                &report(true, true, Some(true)),
                &[review(1)],
                &[review(2)]
            )
            .unwrap()
            .eligible
        );
    }
    #[test]
    fn unused_candidate_or_uncertain_execution_is_not_an_optimization_win() {
        let mut trial = report(true, false, None);
        trial["prompt_program_applied_calls"] = json!(0);
        trial["cases"][0]["first_turn_execution_completed"] = json!(false);
        assert!(
            !compare(
                &report(true, false, None),
                &trial,
                &[review(1)],
                &[review(2)]
            )
            .unwrap()
            .eligible
        );
    }

    #[test]
    fn unchanged_reserved_results_can_validate_a_training_improvement() {
        let same = report(true, true, Some(true));
        let judged = [review(2)];
        assert!(!compare(&same, &same, &judged, &judged).unwrap().eligible);
        assert!(
            compare_with_policy(&same, &same, &judged, &judged, Policy::ReservedValidation)
                .unwrap()
                .eligible
        );
        let regression = report(false, true, Some(true));
        assert!(
            !compare_with_policy(
                &same,
                &regression,
                &judged,
                &judged,
                Policy::ReservedValidation
            )
            .unwrap()
            .eligible
        );
    }
}
