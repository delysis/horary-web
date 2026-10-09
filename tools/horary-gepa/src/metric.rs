//! Frozen-target fitness, separate from permission to qualify or deploy a prompt.
//! The caller supplies an already validated TRAINING review; this module neither
//! reads evidence files nor obtains a review. Native gates alone are not semantics.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub score: f64,
    pub feedback: String,
    pub qualified: bool,
}

fn measured(score: f64, qualified: bool, feedback: String) -> Feedback {
    Feedback {
        score,
        feedback,
        qualified,
    }
}

fn zero(feedback: impl Into<String>) -> Feedback {
    measured(0.0, false, feedback.into())
}

fn completed(outcome: &Value, target: &str) -> Result<(), String> {
    if outcome["id"].as_str().is_none_or(|id| id.trim().is_empty()) {
        return Err("Missing measured case identity".into());
    }
    let classification_function = outcome["scope"] == "classification_function_only";
    if classification_function && target != "classification" {
        return Err(
            "A classification function cannot stand in for another hurdle or complete conversation"
                .into(),
        );
    }
    let observed = if classification_function {
        outcome["classification_execution_completed"] == true
    } else {
        outcome["first_turn_execution_completed"] == true
    };
    if outcome["execution_status"] != "completed"
        || !observed
        || outcome["follow_up_execution_completed"] == false
    {
        return Err("Incomplete or interrupted execution is not optimizer merit".into());
    }
    for key in ["infrastructure_error", "provider_stop"] {
        if !outcome[key].is_null() {
            return Err(format!(
                "{key}: preserve the infrastructure receipt, do not score it"
            ));
        }
    }
    for key in ["deadline_cancelled", "group_cancelled"] {
        if !outcome[key].is_null() && !outcome[key].is_boolean() {
            return Err(format!("Unknown {key} state"));
        }
        if outcome[key] == true {
            return Err(format!("{key}: cancelled execution is not optimizer merit"));
        }
    }
    if !outcome["follow_up_execution_completed"].is_null()
        && !outcome["follow_up_execution_completed"].is_boolean()
    {
        return Err("Unknown supplying-turn execution state".into());
    }
    Ok(())
}

pub fn require_completed(outcome: &Value, target: &str) -> Result<(), String> {
    completed(outcome, target)
}

fn native_gate<'a>(outcome: &'a Value, target: &str) -> Result<&'a str, String> {
    let gates = outcome
        .get("hurdles")
        .unwrap_or(&outcome["grade"]["hurdles"]);
    let status = gates[target]["status"]
        .as_str()
        .ok_or_else(|| format!("Missing authored native {target} grade"))?;
    match status {
        "pass" | "fail" | "awaiting_information" | "blocked" | "not_run" => Ok(status),
        "structure_pass_review_pending" if target == "reading" => Ok(status),
        _ => Err(format!("Unknown authored native {target} grade: {status}")),
    }
}

fn native_pass(outcome: &Value, target: &str) -> Result<bool, String> {
    Ok(matches!(
        native_gate(outcome, target)?,
        "pass" | "structure_pass_review_pending"
    ))
}

fn review_identity(outcome: &Value, review: &Value) -> Result<(), String> {
    let id = outcome["id"].as_str().ok_or("Missing case identity")?;
    if review["case_id"].as_str() != Some(id) {
        return Err("Independent review belongs to a different case".into());
    }
    if review.get("partition").is_some() && review["partition"] != "training" {
        return Err("Only supplied training reviews may enter optimizer feedback".into());
    }
    for (field, actual) in [
        ("native_semantic_pass", &outcome["grade"]["semantic_pass"]),
        ("native_journey_pass", &outcome["follow_up_pass"]),
    ] {
        if review.get(field).is_some_and(|saved| saved != actual) {
            return Err(format!("Independent review changed the recorded {field}"));
        }
    }
    Ok(())
}

/// None means an explicitly unobserved dimension, never a positive score.
fn dimension(value: &Value, inquiry: bool) -> Result<Option<u8>, String> {
    if value["reason"].as_str().is_none_or(|s| s.trim().is_empty()) {
        return Err("Independent dimension needs its supplied reason".into());
    }
    let citations = value["evidence"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or("Independent dimension lacks source citations")?;
    for citation in citations {
        if ["case_id", "file"]
            .iter()
            .any(|key| citation[*key].as_str().is_none_or(|s| s.trim().is_empty()))
            || citation["json_pointer"].as_str().is_none()
            || citation["sha256"]
                .as_str()
                .is_none_or(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("Independent dimension contains a malformed source citation".into());
        }
    }
    match value["state"].as_str() {
        Some("scored") => value["score"]
            .as_u64()
            .filter(|score| *score <= 2)
            .map(|score| Some(score as u8))
            .ok_or_else(|| "Independent score must be 0, 1 or 2".into()),
        Some("unobserved") if value["score"].is_null() => Ok(None),
        Some("not_applicable") if inquiry && value["score"].is_null() => Ok(Some(2)),
        _ => Err("Unknown independent score or inapplicable N/A".into()),
    }
}

fn pipeline_score(outcome: &Value, review: &Value, target: &str) -> Result<Option<u8>, String> {
    let value = &review["pipeline"][target];
    if target == "elicitation" && value["state"] == "not_applicable" {
        let needs = outcome["grade"]["actual"]["needs"]
            .as_array()
            .ok_or("Cannot establish that no inquiry was needed")?;
        if !needs.is_empty() || !outcome["grade"]["actual"]["requested"].is_null() {
            return Err("Needed elicitation cannot be marked N/A".into());
        }
    }
    dimension(value, target == "elicitation")
}

fn honest(outcome: &Value, review: &Value, required: bool) -> Result<bool, String> {
    let first = review.pointer("/first_turn/evidence_honesty");
    if first.is_none() && !required {
        return Ok(true); // A pure component review need not invent a conversation.
    }
    if dimension(first.ok_or("Missing independent honesty grade")?, false)? != Some(2) {
        return Ok(false);
    }
    if outcome["follow_up_execution_completed"] == true {
        let after = review
            .pointer("/follow_up/evidence_honesty")
            .ok_or("Observed supplying reply lacks an independent honesty grade")?;
        if dimension(after, false)? != Some(2) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn supplying_complete(outcome: &Value) -> Result<bool, String> {
    let scripted = outcome["follow_up_scripted"]
        .as_bool()
        .ok_or("Unknown scripted-journey scope")?;
    if !scripted {
        if outcome["follow_up_execution_completed"] == true {
            return Err("Supplying execution disagrees with the scripted scope".into());
        }
        return Ok(true);
    }
    if outcome["follow_up_execution_completed"] != true {
        return Ok(false); // Authored words, withheld or never submitted, earn nothing.
    }
    let after = &outcome["follow_up"];
    let submitted = matches!(
        after["status"].as_str(),
        Some(
            "executed"
                | "executed after matching elicitation"
                | "executed after one eligible authored proposal"
        )
    ) && after["words"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
        && after["result"].get("Ok").is_some();
    if !submitted {
        return Err("Supplying execution lacks its actual submitted-turn receipt".into());
    }
    let grade = &after["grade"];
    for key in ["unresolved_original_needs", "current_needs", "native_needs"] {
        if !grade[key]
            .as_array()
            .ok_or("Unknown remaining journey needs")?
            .is_empty()
        {
            return Ok(false);
        }
    }
    let flag = |key: &str| {
        grade[key]
            .as_bool()
            .ok_or_else(|| format!("Unknown supplying journey grade: {key}"))
    };
    for key in [
        "explicit_new_matter",
        "accepted_authored_proposal",
        "sourced_chart_clock_correction",
    ] {
        if !grade[key].is_null() && !grade[key].is_boolean() {
            return Err(format!("Unknown native continuity witness: {key}"));
        }
    }
    let consent =
        grade["explicit_new_matter"] == true || grade["accepted_authored_proposal"] == true;
    let clock_correction = grade["sourced_chart_clock_correction"] == true;
    let passed = outcome["follow_up_pass"]
        .as_bool()
        .ok_or("Unknown supplying journey pass")?;
    let [after_pass, complete, fresh, question, moment] = [
        flag("pass")?,
        flag("input_complete")?,
        flag("fresh_assistant_reply")?,
        flag("question_preserved")?,
        flag("candidate_moment_preserved")?,
    ];
    Ok(passed
        && after_pass
        && complete
        && fresh
        && (question || consent)
        && (moment || consent || clock_correction))
}

fn judgment_observed(outcome: &Value) -> Result<bool, String> {
    if !outcome["full_reading"]
        .as_bool()
        .ok_or("Unknown reading scope")?
    {
        return Ok(false);
    }
    let result = if outcome["follow_up_execution_completed"] == true {
        &outcome["follow_up"]["native_result"]
    } else {
        &outcome["grade"]["actual"]["native_result"]
    };
    match result["result"].as_str() {
        Some("judgment") => {
            let answer = result["answer"]
                .as_str()
                .filter(|answer| !answer.trim().is_empty())
                .ok_or("Judgment flag lacks an observed answer")?;
            if result["worksheet"]["answer"].as_str() != Some(answer) {
                return Err("Judgment answer differs from its native worksheet".into());
            }
            Ok(true)
        }
        Some("limited" | "needs_information") => Ok(false),
        _ => Err("Unknown final native reading result".into()),
    }
}

/// Fitness is for one frozen target. `qualified` certifies only that target's
/// supplied evidence, not another method, model, programme or release. A missing
/// review permits a deterministic classification pilot; other semantic merit
/// requires an observed source-cited independent dimension.
pub fn grade(
    outcome: &Value,
    independent_review: Option<&Value>,
    target: &str,
) -> Result<Feedback, String> {
    if !matches!(
        target,
        "classification" | "elicitation" | "extraction" | "reading" | "journey" | "input_journey"
    ) {
        return Err(format!("Unknown frozen optimization target: {target}"));
    }
    completed(outcome, target)?;
    if target == "input_journey"
        && (outcome["scope"] != "input_journey_function_only" || outcome["full_reading"] != false)
    {
        return Err("Input journey metric requires actual input-only execution; a reading or component cannot substitute".into());
    }
    if let Some(review) = independent_review {
        review_identity(outcome, review)?;
    }
    if target == "input_journey" && outcome["target_function_invoked"] == false {
        return Ok(zero("The observed upstream route did not reach the optimized extractor; native grades are unchanged, but this target has no fitness"));
    }
    let journey = matches!(target, "journey" | "input_journey");
    if journey
        && !outcome["grade"]["semantic_pass"]
            .as_bool()
            .ok_or("Unknown first-turn semantic grade")?
    {
        return Ok(zero(
            "The first-turn authored input grade failed; a later answer cannot erase it",
        ));
    }
    let gates: &[&str] = if target == "reading" || journey {
        &["classification", "extraction", "elicitation"]
    } else {
        &[target]
    };
    for gate in gates {
        if !native_pass(outcome, gate)? {
            return Ok(zero(format!(
                "{target}: authored native {gate} is {}; valid formatting cannot compensate",
                native_gate(outcome, gate)?
            )));
        }
    }
    if (target == "reading" || journey) && !supplying_complete(outcome)? {
        return Ok(zero(format!(
            "{target}: supplying journey is withheld, incomplete or loses continuity"
        )));
    }
    let reading = target == "reading"
        || target == "journey"
            && outcome["full_reading"]
                .as_bool()
                .ok_or("Unknown journey scope")?;
    if reading && (!native_pass(outcome, "reading")? || !judgment_observed(outcome)?) {
        return Ok(zero("Reading unobserved or behind a method/capability boundary; no interpretation qualified"));
    }
    let Some(review) = independent_review else {
        return Ok(if target == "classification" {
            measured(1.0, false, "Authored native classification pass; semantic/source qualification remains unreviewed".into())
        } else {
            zero(format!(
                "{target}: no independent semantic/source review was supplied"
            ))
        });
    };
    if !honest(
        outcome,
        review,
        matches!(
            target,
            "elicitation" | "reading" | "journey" | "input_journey"
        ),
    )? {
        return Ok(zero(
            "Independent honesty is partial, failed or unobserved; other merits cannot compensate",
        ));
    }
    let mut minimum = 2;
    if target == "input_journey" {
        for turn in std::iter::once("first_turn")
            .chain((outcome["follow_up_execution_completed"] == true).then_some("follow_up"))
        {
            for key in ["concern_actor", "evidence_honesty", "continuity"] {
                if dimension(&review[turn][key], false)? != Some(2) {
                    return Ok(zero(format!("{turn}/{key}: incomplete independent safeguard; pipeline merits cannot compensate")));
                }
            }
            for (key, inquiry) in [("natural_phrasing", false), ("useful_inquiry", true)] {
                let Some(score) = dimension(&review[turn][key], inquiry)? else {
                    return Ok(zero(format!(
                        "{turn}/{key}: unobserved conversation cannot qualify an input journey"
                    )));
                };
                minimum = minimum.min(score);
            }
        }
    }
    let semantic_gates: Vec<&str> = if journey {
        gates
            .iter()
            .copied()
            .chain(reading.then_some("reading"))
            .collect()
    } else if reading {
        for gate in gates {
            if pipeline_score(outcome, review, gate)? != Some(2) {
                return Ok(zero(format!(
                    "Reading cannot compensate for incomplete independent {gate} evidence"
                )));
            }
        }
        vec!["reading"]
    } else {
        vec![target]
    };
    for gate in &semantic_gates {
        let Some(score) = pipeline_score(outcome, review, gate)? else {
            return Ok(zero(format!(
                "{gate}: independent evidence is explicitly unobserved"
            )));
        };
        minimum = minimum.min(score);
    }
    let reasons = semantic_gates
        .iter()
        .map(|gate| {
            format!(
                "{gate}: {}",
                review["pipeline"][*gate]["reason"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Ok(measured(f64::from(minimum) / 2.0, minimum == 2, reasons))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outcome() -> Value {
        json!({"id":"synthetic", "execution_status":"completed", "first_turn_execution_completed":true,
            "follow_up_execution_completed":null, "follow_up_scripted":false, "follow_up_pass":null,
            "full_reading":true, "infrastructure_error":null, "provider_stop":null,
            "deadline_cancelled":false, "group_cancelled":false,
            "grade":{"semantic_pass":true,"actual":{"needs":[],"requested":null,"native_result":{"result":"judgment","answer":"Authored offline answer","worksheet":{"answer":"Authored offline answer"}}}},
            "hurdles":{"classification":{"status":"pass"},"extraction":{"status":"pass"},
                "elicitation":{"status":"pass"},"reading":{"status":"structure_pass_review_pending"}}})
    }
    #[test]
    fn single_function_observation_does_not_claim_a_complete_conversation() {
        let function = json!({"id":"synthetic","scope":"classification_function_only","execution_status":"completed",
            "classification_execution_completed":true,"infrastructure_error":null,
            "hurdles":{"classification":{"status":"pass"}}});
        assert_eq!(grade(&function, None, "classification").unwrap().score, 1.0);
        assert!(!grade(&function, None, "classification").unwrap().qualified);
        assert!(grade(&function, None, "extraction").is_err());
        assert!(grade(&function, None, "journey").is_err());
    }
    fn dimension(score: u8) -> Value {
        json!({"state":"scored","score":score,"reason":"Authored offline source assessment",
            "evidence":[{"case_id":"synthetic","file":"synthetic-source.json","json_pointer":"/value","sha256":"0".repeat(64)}]})
    }
    fn review() -> Value {
        json!({"case_id":"synthetic","partition":"training","native_semantic_pass":true,
            "native_journey_pass":null,"first_turn":{"evidence_honesty":dimension(2)},"follow_up":null,
            "pipeline":{"classification":dimension(2),"extraction":dimension(2),"elicitation":dimension(2),"reading":dimension(2)}})
    }
    fn input_journey_fixture() -> (Value, Value) {
        let mut o = outcome();
        o["full_reading"] = json!(false);
        o["scope"] = json!("input_journey_function_only");
        o["hurdles"]["reading"]["status"] = json!("not_run");
        let mut r = review();
        r["pipeline"]["reading"] = json!({"state":"unobserved","score":null,"reason":"Input journey only","evidence":dimension(2)["evidence"]});
        for key in [
            "concern_actor",
            "continuity",
            "natural_phrasing",
            "useful_inquiry",
        ] {
            r["first_turn"][key] = dimension(2);
        }
        (o, r)
    }
    #[test]
    fn an_alternate_observed_route_cannot_earn_extractor_fitness() {
        let (mut o, r) = input_journey_fixture();
        o["target_function_invoked"] = serde_json::json!(false);
        let result = grade(&o, Some(&r), "input_journey").unwrap();
        assert_eq!(result.score, 0.0);
        assert!(!result.qualified);
        assert!(result.feedback.contains("did not reach"));
        assert_eq!(o["hurdles"]["classification"]["status"], "pass");
    }
    #[test]
    fn input_data_passes_cannot_compensate_for_actor_or_honesty_or_continuity() {
        let (o, r) = input_journey_fixture();
        assert_eq!(grade(&o, Some(&r), "input_journey").unwrap().score, 1.0);
        for key in ["concern_actor", "evidence_honesty", "continuity"] {
            let mut wrong = r.clone();
            wrong["first_turn"][key] = dimension(1);
            let g = grade(&o, Some(&wrong), "input_journey").unwrap();
            assert_eq!(g.score, 0.0);
            assert!(!g.qualified);
        }
        let mut partial = r;
        partial["first_turn"]["natural_phrasing"] = dimension(1);
        assert_eq!(
            grade(&o, Some(&partial), "input_journey").unwrap().score,
            0.5
        );
        assert!(
            !grade(&o, Some(&partial), "input_journey")
                .unwrap()
                .qualified
        );
    }
    #[test]
    fn an_input_journey_requires_the_real_supplying_exchange_and_both_turns() {
        let (mut o, mut r) = input_journey_fixture();
        o["follow_up_scripted"] = json!(true);
        assert_eq!(grade(&o, Some(&r), "input_journey").unwrap().score, 0.0);
        supplying(&mut o, &mut r);
        r["follow_up"] = r["first_turn"].clone();
        assert_eq!(grade(&o, Some(&r), "input_journey").unwrap().score, 1.0);
        r["follow_up"]["concern_actor"] = dimension(0);
        assert_eq!(grade(&o, Some(&r), "input_journey").unwrap().score, 0.0);
    }
    #[test]
    fn the_input_adapter_cannot_be_upgraded_to_an_interpretation() {
        let (mut o, r) = input_journey_fixture();
        assert!(grade(&o, Some(&r), "reading").unwrap().score == 0.0);
        o["full_reading"] = json!(true);
        assert!(grade(&o, Some(&r), "input_journey").is_err());
    }
    fn supplying(outcome: &mut Value, review: &mut Value) {
        outcome["follow_up_scripted"] = json!(true);
        outcome["follow_up_execution_completed"] = json!(true);
        outcome["follow_up_pass"] = json!(true);
        outcome["follow_up"] = json!({"status":"executed after matching elicitation","words":"An authored answer",
            "result":{"Ok":null},"native_result":{"result":"judgment","answer":"Authored offline answer","worksheet":{"answer":"Authored offline answer"}},"grade":{"pass":true,"input_complete":true,
                "fresh_assistant_reply":true,"question_preserved":true,"candidate_moment_preserved":true,
                "unresolved_original_needs":[],"current_needs":[],"native_needs":[]}});
        review["native_journey_pass"] = json!(true);
        review["follow_up"] = json!({"evidence_honesty":dimension(2)});
    }

    #[test]
    fn native_classification_pilot_is_merit_but_not_semantic_qualification() {
        let good = grade(&outcome(), None, "classification").unwrap();
        assert_eq!(good.score, 1.0);
        assert!(!good.qualified);
        let mut wrong = outcome();
        wrong["hurdles"]["classification"]["status"] = json!("fail");
        wrong["valid_json"] = json!(true);
        assert_eq!(grade(&wrong, None, "classification").unwrap().score, 0.0);
    }

    #[test]
    fn native_green_extraction_is_not_source_semantics() {
        assert_eq!(grade(&outcome(), None, "extraction").unwrap().score, 0.0);
        assert!(
            grade(&outcome(), Some(&review()), "extraction")
                .unwrap()
                .qualified
        );
        let mut wrong = review();
        wrong["pipeline"]["extraction"] = dimension(0);
        assert_eq!(
            grade(&outcome(), Some(&wrong), "extraction").unwrap().score,
            0.0
        );
        wrong["pipeline"]["extraction"]["state"] = json!("unobserved");
        wrong["pipeline"]["extraction"]["score"] = Value::Null;
        assert!(
            !grade(&outcome(), Some(&wrong), "extraction")
                .unwrap()
                .qualified
        );
    }

    #[test]
    fn interpretation_needs_source_review_and_cannot_compensate_for_bad_inputs() {
        assert_eq!(grade(&outcome(), None, "reading").unwrap().score, 0.0);
        assert!(
            grade(&outcome(), Some(&review()), "reading")
                .unwrap()
                .qualified
        );
        let mut wrong = review();
        wrong["pipeline"]["reading"] = dimension(0);
        assert_eq!(
            grade(&outcome(), Some(&wrong), "reading").unwrap().score,
            0.0
        );
        wrong["pipeline"]["reading"] = dimension(2);
        wrong["pipeline"]["extraction"] = dimension(1);
        assert_eq!(
            grade(&outcome(), Some(&wrong), "reading").unwrap().score,
            0.0
        );
    }

    #[test]
    fn a_judgment_flag_without_the_actual_answer_is_not_observation() {
        let mut o = outcome();
        o["grade"]["actual"]["native_result"] = json!({"result":"judgment"});
        assert!(grade(&o, Some(&review()), "reading").is_err());
        o = outcome();
        o["grade"]["actual"]["native_result"]["worksheet"]["answer"] = json!("A different answer");
        assert!(grade(&o, Some(&review()), "reading").is_err());
    }

    #[test]
    fn expert_boundary_and_unobserved_reading_are_never_reading_success() {
        let mut limited = outcome();
        limited["hurdles"]["reading"]["status"] = json!("blocked");
        limited["grade"]["actual"]["native_result"] =
            json!({"result":"limited","limitation":{"code":"judgment_program_needs_review"}});
        assert_eq!(
            grade(&limited, Some(&review()), "reading").unwrap().score,
            0.0
        );
        // A spurious structural pass still cannot convert Limited into Judgment.
        limited["hurdles"]["reading"]["status"] = json!("pass");
        assert!(
            !grade(&limited, Some(&review()), "reading")
                .unwrap()
                .qualified
        );
        let mut unobserved = review();
        unobserved["pipeline"]["reading"]["state"] = json!("unobserved");
        unobserved["pipeline"]["reading"]["score"] = Value::Null;
        assert_eq!(
            grade(&outcome(), Some(&unobserved), "reading")
                .unwrap()
                .score,
            0.0
        );
    }

    #[test]
    fn honesty_is_a_noncompensating_guard_in_both_observed_turns() {
        let mut o = outcome();
        let mut r = review();
        r["first_turn"]["evidence_honesty"] = dimension(1);
        for target in [
            "classification",
            "elicitation",
            "extraction",
            "reading",
            "journey",
        ] {
            assert_eq!(grade(&o, Some(&r), target).unwrap().score, 0.0);
        }
        r = review();
        supplying(&mut o, &mut r);
        r["follow_up"]["evidence_honesty"] = dimension(0);
        assert_eq!(grade(&o, Some(&r), "journey").unwrap().score, 0.0);
    }

    #[test]
    fn withheld_authored_words_are_not_a_supplying_journey() {
        let mut o = outcome();
        o["follow_up_scripted"] = json!(true);
        o["follow_up"] = json!({"status":"withheld because the intended fact was not elicited","words":"An authored answer"});
        assert_eq!(grade(&o, Some(&review()), "journey").unwrap().score, 0.0);
    }

    #[test]
    fn supplied_slot_is_not_completion_when_other_needs_or_continuity_remain() {
        let mut o = outcome();
        let mut r = review();
        supplying(&mut o, &mut r);
        assert!(grade(&o, Some(&r), "journey").unwrap().qualified);
        for field in [
            "input_complete",
            "fresh_assistant_reply",
            "question_preserved",
            "candidate_moment_preserved",
        ] {
            let mut bad = o.clone();
            bad["follow_up"]["grade"][field] = json!(false);
            assert_eq!(
                grade(&bad, Some(&r), "journey").unwrap().score,
                0.0,
                "{field}"
            );
        }
        o["follow_up"]["grade"]["current_needs"] =
            json!([{"kind":"field","id":"an_unresolved_fact"}]);
        assert_eq!(grade(&o, Some(&r), "journey").unwrap().score, 0.0);
    }

    #[test]
    fn continuity_exceptions_require_the_native_consent_or_clock_witness() {
        let mut o = outcome();
        let mut r = review();
        supplying(&mut o, &mut r);
        o["follow_up"]["grade"]["question_preserved"] = json!(false);
        o["follow_up"]["grade"]["accepted_authored_proposal"] = json!(true);
        assert!(grade(&o, Some(&r), "journey").unwrap().qualified);
        o["follow_up"]["grade"]["accepted_authored_proposal"] = json!(false);
        o["follow_up"]["grade"]["sourced_chart_clock_correction"] = json!(true);
        assert_eq!(grade(&o, Some(&r), "journey").unwrap().score, 0.0);
        o["follow_up"]["grade"]["question_preserved"] = json!(true);
        o["follow_up"]["grade"]["candidate_moment_preserved"] = json!(false);
        assert!(grade(&o, Some(&r), "journey").unwrap().qualified);
    }

    #[test]
    fn a_supplying_answer_does_not_erase_a_failed_first_turn() {
        let mut o = outcome();
        let mut r = review();
        supplying(&mut o, &mut r);
        o["grade"]["semantic_pass"] = json!(false);
        r["native_semantic_pass"] = json!(false);
        assert_eq!(grade(&o, Some(&r), "journey").unwrap().score, 0.0);
    }

    #[test]
    fn input_journey_does_not_claim_a_blocked_full_reading() {
        let mut o = outcome();
        o["hurdles"]["reading"]["status"] = json!("blocked");
        o["grade"]["actual"]["native_result"] = json!({"result":"limited"});
        assert_eq!(grade(&o, Some(&review()), "journey").unwrap().score, 0.0);
        o["full_reading"] = json!(false);
        assert!(grade(&o, Some(&review()), "journey").unwrap().qualified);
        assert!(!grade(&o, Some(&review()), "reading").unwrap().qualified);
    }

    #[test]
    fn inapplicable_inquiry_is_not_a_loophole_for_omitting_a_needed_question() {
        let mut r = review();
        r["pipeline"]["elicitation"]["state"] = json!("not_applicable");
        r["pipeline"]["elicitation"]["score"] = Value::Null;
        assert!(
            grade(&outcome(), Some(&r), "elicitation")
                .unwrap()
                .qualified
        );
        let mut o = outcome();
        o["grade"]["actual"]["needs"] = json!([{"kind":"field","id":"needed"}]);
        assert!(grade(&o, Some(&r), "elicitation").is_err());
    }

    #[test]
    fn interrupted_or_unknown_measurements_are_errors_not_failure_scores() {
        for (field, value) in [
            ("first_turn_execution_completed", json!(false)),
            ("follow_up_execution_completed", json!(false)),
            ("deadline_cancelled", json!(true)),
            ("provider_stop", json!("HTTP 500")),
            ("infrastructure_error", json!("Evidence I/O failure")),
        ] {
            let mut o = outcome();
            o[field] = value;
            assert!(grade(&o, None, "classification").is_err(), "{field}");
        }
        let mut o = outcome();
        o["hurdles"]["classification"]["status"] = json!("valid_json");
        assert!(grade(&o, None, "classification").is_err());
        assert!(grade(&outcome(), None, "formatting").is_err());
    }

    #[test]
    fn independent_review_identity_source_and_score_are_checked() {
        let mut r = review();
        r["case_id"] = json!("different");
        assert!(grade(&outcome(), Some(&r), "classification").is_err());
        r = review();
        r["partition"] = json!("reserved_validation");
        assert!(grade(&outcome(), Some(&r), "classification").is_err());
        r = review();
        r["native_semantic_pass"] = json!(false);
        assert!(grade(&outcome(), Some(&r), "classification").is_err());
        r = review();
        r["pipeline"]["extraction"]["evidence"] = json!([]);
        assert!(grade(&outcome(), Some(&r), "extraction").is_err());
        r = review();
        r["pipeline"]["extraction"]["score"] = json!(3);
        assert!(grade(&outcome(), Some(&r), "extraction").is_err());
    }
}
