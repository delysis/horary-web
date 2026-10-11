//! Typed worksheet contracts and native domain validation. No scheduling or UI.
#![forbid(unsafe_code)]
use crate::{
    horary_lessons::{self as lessons, Matter, Stage},
    reading_method::{self, Fact, RoleChoice},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    pub intent: String,
    pub question: String,
    pub matter: Matter,
    pub question_kind: String,
    pub context: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<crate::horary_role_options::Person>,
    #[serde(default)]
    #[serde(skip_serializing_if = "crate::horary_role_options::Subject::is_empty")]
    pub subject: crate::horary_role_options::Subject,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub event_place: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub event_time: String,
    pub place_request: String,
    pub time_request: String,
    pub horizon: String,
    pub clarification: String,
    pub focus: String,
    pub heard: String,
    #[serde(default)]
    pub restore_revision: Option<u64>,
}

fn text(max: usize) -> Value {
    json!({"type":"string", "maxLength":max})
}
fn choice(values: &[&str]) -> Value {
    json!({"type":"string", "enum":values})
}
fn list(items: Value, max: usize) -> Value {
    json!({"type":"array", "items":items, "maxItems":max})
}
fn object(properties: Value) -> Value {
    let required: Vec<_> = properties
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k.clone())
        .collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub fn schema(stage: Stage, facts: &[Fact]) -> Value {
    let ids: Vec<_> = facts.iter().map(|f| f.id.as_str()).collect();
    let evidence = if ids.is_empty() {
        list(text(1), 0)
    } else {
        list(choice(&ids), 6)
    };
    match stage {
        Stage::Intake => crate::reading_contracts::turn_schema(None),
        Stage::Conversation => object(json!({"reply":text(1600),"ask":text(180)})),
        Stage::Place => object(
            json!({"mode":choice(&["select","ask"]),"place_id":text(100),"query":text(240),"clarification":text(180),"basis":text(240)}),
        ),
        Stage::Moment => object(
            json!({"mode":choice(&["now","keep","explicit","ask"]),"local_time":text(32),"occurrence":choice(&["","earlier","later"]),"clarification":text(180),"basis":text(240)}),
        ),
        Stage::Significators => object(
            json!({"roles":list(object(json!({"label":text(80),"house":{"type":["integer","null"],"minimum":1,"maximum":12},"natural":{"type":["string","null"],"enum":[null,"Moon","Sun","Venus"]},"reason":text(240)})),5),"owner_house":{"type":["integer","null"],"minimum":1,"maximum":12},"object_candidates":list(json!({"type":"integer","minimum":1,"maximum":12}),2),"summary":text(350),"unknowns":list(text(150),3)}),
        ),
        _ => {
            let mut checks = serde_json::Map::new();
            for key in stage.checks() {
                let check_evidence = if stage == Stage::Condition {
                    let mut contract = evidence.clone();
                    contract["maxItems"] = json!(condition_evidence_budget(key, None, facts));
                    contract
                } else {
                    evidence.clone()
                };
                checks.insert((*key).into(), object(json!({"state":choice(&["supported","contradicted","unestablished","not_relevant"]),"evidence":check_evidence,"finding":{"type":"string","minLength":1,"description":"Briefly explain the check using the relevant roles and evidence, including why it is not relevant or unestablished. Preserve necessary distinctions; an empty string cannot finish a check."}})));
            }
            let mut fields = serde_json::Map::from_iter([
                ("checks".into(), object(Value::Object(checks))),
                ("summary".into(), text(450)),
                ("unknowns".into(), list(text(150), 4)),
            ]);
            match stage {
                Stage::Contacts => {
                    fields.insert(
                        "basis".into(),
                        choice(&[
                            "direct_candidate",
                            "complex_unverified",
                            "location_or_situation",
                            "no_candidate_covered",
                        ]),
                    );
                    fields.insert("candidate_ids".into(), evidence);
                    fields.insert("candidate_signs".into(), list(object(json!({"id": if ids.is_empty(){text(1)}else{choice(&ids)}, "within_current_signs":{"type":["boolean","null"]}})),6));
                }
                Stage::Timing => {
                    fields.insert(
                        "timing_status".into(),
                        choice(&["tentative", "unestablished"]),
                    );
                    fields.insert(
                        "unit".into(),
                        choice(&["", "hours", "days", "weeks", "months", "years"]),
                    );
                    fields.insert(
                        "number".into(),
                        json!({"type":["number","null"],"minimum":0}),
                    );
                }
                Stage::Judgment => {
                    fields.insert(
                        "verdict".into(),
                        choice(&[
                            "likely_yes",
                            "likely_no",
                            "mixed",
                            "situation",
                            "location",
                            "unresolved",
                        ]),
                    );
                    fields.insert("answer".into(), text(1000));
                    fields.insert("evidence".into(), evidence);
                }
                _ => {}
            }
            object(Value::Object(fields))
        }
    }
}

pub fn schema_for(stage: Stage, matter: Matter, facts: &[Fact]) -> Value {
    let mut contract = schema(stage, facts);
    if stage == Stage::Significators && !matches!(matter, Matter::LostObject | Matter::LostAnimal) {
        if let Some(fields) = contract["properties"].as_object_mut() {
            fields.remove("owner_house");
            fields.remove("object_candidates");
        }
        if let Some(required) = contract["required"].as_array_mut() {
            required.retain(|key| key != "owner_house" && key != "object_candidates");
        }
    }
    contract
}

/// Condition evidence is a bundle of native tests per planet, not one ID per
/// planet. Its budget follows the supplied categories for the selected roles.
/// The ID allowlist and the other stages' contracts remain unchanged.
pub(crate) fn condition_schema(input: &Value, facts: &[Fact]) -> Value {
    let mut contract = schema(Stage::Condition, facts);
    let roles = input
        .get("roles")
        .or_else(|| input["question"].get("roles"))
        .and_then(Value::as_array);
    let planets: std::collections::BTreeSet<_> = roles
        .into_iter()
        .flatten()
        .filter_map(|role| role["planet"].as_str())
        .collect();
    for key in Stage::Condition.checks() {
        contract["properties"]["checks"]["properties"][*key]["properties"]["evidence"]
            ["maxItems"] = json!(condition_evidence_budget(key, Some(&planets), facts));
    }
    contract
}

fn condition_evidence_budget(
    check: &str,
    selected: Option<&std::collections::BTreeSet<&str>>,
    facts: &[Fact],
) -> usize {
    use horary_ai_core::book_method::ConditionFacet;
    const PLANETS: [&str; 7] = [
        "Sun", "Moon", "Mercury", "Venus", "Mars", "Jupiter", "Saturn",
    ];
    let eligible = |fact: &&Fact| {
        let selected_planet = fact.planets.iter().any(|planet| {
            PLANETS.contains(&planet.as_str())
                && selected.is_none_or(|planets| planets.contains(planet.as_str()))
        });
        let position = fact.kind == "position";
        let facet = (fact.kind == "condition")
            .then_some(fact.condition_facet)
            .flatten();
        selected_planet
            && match check {
                "own_dignity" => position || facet == Some(ConditionFacet::Essential),
                "ability_to_act" => {
                    position
                        || matches!(
                            facet,
                            Some(
                                ConditionFacet::HouseCapacity
                                    | ConditionFacet::Solar
                                    | ConditionFacet::SolarOpposition
                                    | ConditionFacet::Retrograde
                                    | ConditionFacet::CuspAdvance
                            )
                        )
                }
                "context_exceptions" => {
                    position || facet.is_some() || fact.kind == "moon" || fact.kind == "condition"
                }
                _ => false,
            }
            || check == "context_exceptions" && fact.kind == "boundary"
    };
    let supplied = facts
        .iter()
        .filter(eligible)
        .map(|fact| fact.id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    // Each traditional body supplies one position and at most five typed
    // condition facets. Context additionally has a lunar-gap/status fact and
    // the calculation boundary. Neither repeated roles nor duplicate IDs
    // increase the budget, and missing calculations add no capacity.
    let maximum = match check {
        "own_dignity" => PLANETS.len() * 2,
        "ability_to_act" => PLANETS.len() * 5,
        "context_exceptions" => PLANETS.len() * 6 + 3,
        _ => 0,
    };
    supplied.min(maximum)
}

pub fn prompt(
    stage: Stage,
    matter: Matter,
    input: &Value,
    schema: &Value,
) -> Result<String, String> {
    prompt_with_program(stage, matter, input, schema, qualified_program()).map(|(prompt, _)| prompt)
}

// Promotion is a reviewed source change, not runtime configuration. No teaching
// has been qualified for production; the default therefore remains unchanged.
fn qualified_program() -> Option<&'static horary_prompt_program::Program> {
    None
}

pub(crate) fn prompt_with_program(
    stage: Stage,
    matter: Matter,
    input: &Value,
    schema: &Value,
    program: Option<&horary_prompt_program::Program>,
) -> Result<(String, Option<horary_prompt_program::Applied>), String> {
    let (fixed, applied) = resolved_guide_for(stage, matter, input, program)?;
    Ok((prompt_with_guide(input, schema, fixed), applied))
}

fn prompt_with_guide(input: &Value, schema: &Value, fixed: String) -> String {
    // Stable teaching and stable contract precede changing data. No whole chart
    // or conversation history is smuggled into this prefix.
    if input.get("original_input").is_some() && input.get("native_validation_error").is_some() {
        // Present repair as a rejected answer followed by native feedback.
        // Keeping the bad worksheet beside accepted input in one example-like
        // JSON object encouraged small models to copy the same mistake.
        // The complete contract is already in the first user message. Repeating
        // it in the correction can push a valid repair over the request budget.
        return json!([
            {"role":"system","content":fixed},
            {"role":"user","content":json!({"input":crate::horary_step::original_input(input),"worksheet_contract":schema}).to_string()},
            {"role":"assistant","content":input["previous_worksheet"].to_string()},
            {"role":"user","content":json!({
                "native_validation_error":input["native_validation_error"],
                "instruction":"Your preceding answer was REJECTED. None of its proposed changes were saved. Produce the complete corrected object using the original input and the output contract. Make the specific correction requested by the native validation error; do not repeat the forbidden entry. Do not change the person's question or ask them to correct your output."
            }).to_string()}
        ]).to_string();
    }
    json!([{"role":"system","content":fixed},{"role":"user","content":json!({"input":input,"worksheet_contract":schema}).to_string()}]).to_string()
}

pub fn guide_for(stage: Stage, matter: Matter, input: &Value) -> Result<String, String> {
    resolved_guide_for(stage, matter, input, qualified_program()).map(|(guide, _)| guide)
}

pub(crate) fn resolved_guide_for(
    stage: Stage,
    matter: Matter,
    input: &Value,
    program: Option<&horary_prompt_program::Program>,
) -> Result<(String, Option<horary_prompt_program::Applied>), String> {
    let guide = base_guide_for(stage, matter, input)?;
    let Some(program) = program else {
        return Ok((guide, None));
    };
    program.validate()?;
    match program.apply(
        horary_prompt_program::signature(stage.name(), input),
        &guide,
    )? {
        Some((guide, applied)) => Ok((guide, Some(applied))),
        None => Ok((guide, None)),
    }
}

fn base_guide_for(stage: Stage, matter: Matter, input: &Value) -> Result<String, String> {
    let original = crate::horary_step::original_input(input);
    if stage == Stage::Intake {
        let case = serde_json::from_value::<crate::reading_contracts::Consultation>(
            original["consultation"].clone(),
        )
        .ok();
        return Ok(crate::reading_contracts::recognition_guide_for(
            case.as_ref(),
            original["recognition_phase"]
                .as_str()
                .unwrap_or("update_selected_program"),
        ));
    }
    let mut guide = lessons::guide(stage, matter)?;
    if let Ok(method) = serde_json::from_value::<crate::reading_contracts::Method>(
        original["reading_request"]["binding"]["frame"]["method"].clone(),
    ) {
        guide.push_str(&crate::reading_contracts::method_guide(method));
    }
    Ok(guide)
}

#[cfg(test)]
mod repair_message_tests {
    use super::*;

    fn captured_repair() -> (Value, Value, Value, Stage, Matter) {
        let capture: Value = serde_json::from_str(include_str!(
            "../test-fixtures/training-judgment-repair-messages.json"
        ))
        .unwrap();
        let messages = &capture["legacy_messages"];
        let first: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
        let feedback: Value =
            serde_json::from_str(messages[3]["content"].as_str().unwrap()).unwrap();
        let rejected: Value =
            serde_json::from_str(messages[2]["content"].as_str().unwrap()).unwrap();
        let input = json!({"original_input":first["input"],"previous_worksheet":rejected,
            "native_validation_error":feedback["native_validation_error"],
            "instruction":capture["native_repair_instruction"]});
        let schema = first["worksheet_contract"].clone();
        let stage = serde_json::from_value(capture["stage"].clone()).unwrap();
        let matter = serde_json::from_value(capture["matter"].clone()).unwrap();
        (capture, input, schema, stage, matter)
    }

    #[test]
    fn captured_judgment_repair_keeps_all_evidence_with_one_complete_schema() {
        let (capture, input, schema, stage, matter) = captured_repair();
        let legacy = &capture["legacy_messages"];
        let actual: Value =
            serde_json::from_str(&prompt(stage, matter, &input, &schema).unwrap()).unwrap();
        assert_eq!(actual.as_array().unwrap().len(), 4);
        // Exact message bytes preserve the source guide, original chart/context
        // and rejected answer; only the duplicate corrective schema is removed.
        assert_eq!(
            &actual.as_array().unwrap()[..3],
            &legacy.as_array().unwrap()[..3]
        );
        let first: Value = serde_json::from_str(actual[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(first["input"], input["original_input"]);
        assert_eq!(first["worksheet_contract"], schema);
        let rejected: Value = serde_json::from_str(actual[2]["content"].as_str().unwrap()).unwrap();
        assert_eq!(rejected, input["previous_worksheet"]);
        let correction: Value =
            serde_json::from_str(actual[3]["content"].as_str().unwrap()).unwrap();
        let mut original_correction: Value =
            serde_json::from_str(legacy[3]["content"].as_str().unwrap()).unwrap();
        assert_eq!(original_correction["worksheet_contract"], schema);
        original_correction
            .as_object_mut()
            .unwrap()
            .remove("worksheet_contract");
        assert_eq!(correction, original_correction);
        assert_eq!(
            correction["native_validation_error"],
            input["native_validation_error"]
        );
        let schema_copies = actual
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .map(|message| {
                serde_json::from_str::<Value>(message["content"].as_str().unwrap()).unwrap()
            })
            .filter(|body| body.get("worksheet_contract").is_some())
            .count();
        assert_eq!(schema_copies, 1);
        assert_eq!(
            legacy[3]["content"].as_str().unwrap().len()
                - actual[3]["content"].as_str().unwrap().len(),
            capture["expected_removed_model_text_utf8_bytes"]
                .as_u64()
                .unwrap() as usize
        );
        assert_eq!(
            legacy.to_string().len() - actual.to_string().len(),
            capture["expected_removed_serialized_prompt_utf8_bytes"]
                .as_u64()
                .unwrap() as usize
        );
    }

    #[test]
    fn captured_original_judgment_prompt_is_byte_identical_before_repair() {
        let (capture, repair, schema, stage, matter) = captured_repair();
        let actual: Value = serde_json::from_str(
            &prompt(stage, matter, &repair["original_input"], &schema).unwrap(),
        )
        .unwrap();
        assert_eq!(
            actual.as_array().unwrap(),
            &capture["legacy_messages"].as_array().unwrap()[..2]
        );
    }
}

#[cfg(test)]
pub(crate) fn synthetic_teaching_program(
    stage: Stage,
    input: &Value,
    annotation: &str,
) -> horary_prompt_program::Program {
    use horary_prompt_program::{digest, Evidence, Override, Program};
    let guide = base_guide_for(stage, Matter::Other, input).unwrap();
    Program {
        version: 1,
        id: "synthetic-effective-guide".into(),
        baseline_manifest_sha256: digest("synthetic-manifest"),
        overrides: vec![Override {
            stage: stage.name().into(), recognition_phase: None, method: None,
            expected_guide_sha256: digest(&guide),
            replacement_text: format!("Synthetic offline teaching annotation: {annotation}. This checks provenance only and makes no model-quality claim.\n{guide}"),
            edits: vec![],
        }],
        rationale: "Offline guide identity regression".into(),
        evidence: vec![Evidence { case_id: "synthetic-training".into(), file: "synthetic.json".into(), json_pointer: "".into(), sha256: digest("synthetic-source") }],
        training_case_ids: vec!["synthetic-training".into()], holdout_case_ids: vec!["synthetic-holdout".into()],
    }
}

#[cfg(test)]
mod effective_guide_tests {
    use super::*;

    #[test]
    fn inactive_production_seam_is_byte_identical_to_baseline() {
        assert!(qualified_program().is_none());
        for stage in [
            Stage::Intake,
            Stage::Place,
            Stage::Moment,
            Stage::Condition,
            Stage::Reception,
        ] {
            let original =
                json!({"recognition_phase":"classify_question","synthetic_fact":"retained"});
            for input in [
                original.clone(),
                json!({"original_input":original,"previous_worksheet":{"rejected":true},"native_validation_error":"Synthetic rejection"}),
            ] {
                let schema = schema_for(stage, Matter::Other, &[]);
                assert_eq!(
                    guide_for(stage, Matter::Other, &input).unwrap(),
                    base_guide_for(stage, Matter::Other, &input).unwrap()
                );
                assert_eq!(
                    prompt(stage, Matter::Other, &input, &schema).unwrap(),
                    prompt_with_guide(
                        &input,
                        &schema,
                        base_guide_for(stage, Matter::Other, &input).unwrap()
                    )
                );
            }
        }
    }

    #[test]
    fn shared_teaching_resolution_preserves_source_schema_facts_and_repair_messages() {
        let original = json!({"reading_request":{"binding":{"frame":{"method":"movable_deal"}}},"synthetic_fact":"retained exactly"});
        let schema = schema_for(Stage::Condition, Matter::Other, &[]);
        let program = synthetic_teaching_program(Stage::Condition, &original, "selected lesson");
        for input in [
            original.clone(),
            json!({"original_input":original,"previous_worksheet":{"wrong":"answer"},"native_validation_error":"Synthetic exact native rejection"}),
        ] {
            let baseline: Value = serde_json::from_str(
                &prompt(Stage::Condition, Matter::Other, &input, &schema).unwrap(),
            )
            .unwrap();
            let (actual, applied) = prompt_with_program(
                Stage::Condition,
                Matter::Other,
                &input,
                &schema,
                Some(&program),
            )
            .unwrap();
            let actual: Value = serde_json::from_str(&actual).unwrap();
            let (guide, same_applied) =
                resolved_guide_for(Stage::Condition, Matter::Other, &input, Some(&program))
                    .unwrap();
            assert_eq!(actual[0]["content"], guide);
            assert_eq!(applied, same_applied);
            assert_eq!(
                &actual.as_array().unwrap()[1..],
                &baseline.as_array().unwrap()[1..]
            );
            assert_eq!(
                horary_prompt_program::guide_parts(actual[0]["content"].as_str().unwrap())
                    .unwrap()
                    .quoted_source,
                horary_prompt_program::guide_parts(baseline[0]["content"].as_str().unwrap())
                    .unwrap()
                    .quoted_source
            );
            assert_eq!(
                applied.unwrap().replacement_guide_sha256,
                lessons::digest(&guide)
            );
        }
    }

    #[test]
    fn nonmatching_selectors_leave_bytes_unchanged_and_stale_teaching_is_an_error() {
        let input = json!({"consultation":{"frame":{"state":"resolved","observation":{"value":{"method":"movable_deal"}}}},"recognition_phase":"complete_selected_program"});
        let mut program = synthetic_teaching_program(Stage::Intake, &input, "scoped lesson");
        program.overrides[0].method = Some("movable_deal".into());
        program.overrides[0].recognition_phase = Some("complete_selected_program".into());
        let schema = schema_for(Stage::Intake, Matter::Other, &[]);
        for change in ["phase", "method", "stage"] {
            let mut other = input.clone();
            let stage = if change == "stage" {
                Stage::Place
            } else {
                Stage::Intake
            };
            if change == "phase" {
                other["recognition_phase"] = json!("update_selected_program");
            }
            if change == "method" {
                other["consultation"]["frame"]["observation"]["value"]["method"] = json!("rental");
            }
            let (actual, applied) =
                prompt_with_program(stage, Matter::Other, &other, &schema, Some(&program)).unwrap();
            assert_eq!(
                actual,
                prompt(stage, Matter::Other, &other, &schema).unwrap()
            );
            assert!(applied.is_none());
        }
        program.overrides[0].expected_guide_sha256 = lessons::digest("A changed base guide");
        assert!(prompt_with_program(
            Stage::Intake,
            Matter::Other,
            &input,
            &schema,
            Some(&program)
        )
        .unwrap_err()
        .contains("stale"));
    }
}

fn bounded_strings(value: &Value) -> bool {
    match value {
        Value::String(s) => {
            s.len() <= 8000 && !s.contains("<|") && !s.contains("|>") && !s.contains("<image")
        }
        Value::Array(a) => a.len() <= 40 && a.iter().all(bounded_strings),
        Value::Object(m) => m.len() <= 30 && m.values().all(bounded_strings),
        _ => true,
    }
}

pub fn decode_json(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    let data = trimmed
        .strip_prefix("```json\n")
        .or_else(|| trimmed.strip_prefix("```\n"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    let mut deserializer = serde_json::Deserializer::from_str(data);
    let result = StrictValue::deserialize(&mut deserializer)
        .map_err(|e| e.to_string())?
        .0;
    deserializer.end().map_err(|e| e.to_string())?;
    Ok(result)
}

struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate fields")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("Invalid JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(v) = seq.next_element::<StrictValue>()? {
                    values.push(v.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(serde::de::Error::custom("Duplicate worksheet field"));
                    }
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(test)]
pub fn validate(stage: Stage, value: &Value, facts: &[Fact]) -> Result<(), String> {
    validate_for(stage, Matter::Other, value, facts)
}

pub(crate) fn validate_for(
    stage: Stage,
    matter: Matter,
    value: &Value,
    facts: &[Fact],
) -> Result<(), String> {
    if !bounded_strings(value) {
        return Err("Worksheet exceeds its bounds.".into());
    }
    let contract = schema_for(stage, matter, facts);
    validate_shape(value, &contract)?;
    if stage == Stage::Intake {
        serde_json::from_value::<crate::reading_contracts::Turn>(value.clone())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    if stage == Stage::Significators {
        let choices: Vec<RoleChoice> =
            serde_json::from_value(value["roles"].clone()).map_err(|e| e.to_string())?;
        reading_method::assign_from_facts(facts, choices)?;
        if matter == Matter::LostObject {
            let owner = value["owner_house"]
                .as_u64()
                .ok_or("Identify the object's owner house")?;
            let expected = if owner == 1 {
                vec![2, 4]
            } else {
                vec![(owner + 12) % 12 + 1]
            };
            let actual: Vec<_> = value["object_candidates"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .collect();
            if actual != expected {
                return Err("Compare Lords 2 and 4 for the querent's object; use another owner's turned second.".into());
            }
        }
    }
    for key in stage.checks() {
        let c = &value["checks"][key];
        if c["finding"].as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err(format!("Explain the {key} check."));
        }
        let selected: Vec<_> = c["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|id| facts.iter().find(|f| f.id == id))
            .collect();
        if c["state"] == "supported" && selected.is_empty() && stage != Stage::Explanation {
            return Err(format!("The {key} check needs supplied evidence."));
        }
        if stage == Stage::Reception
            && c["state"] == "supported"
            && !selected.iter().any(|f| f.kind == "reception")
        {
            return Err("Reception requires a directed reception fact.".into());
        }
    }
    if stage == Stage::Judgment && value["answer"].as_str().is_none_or(|s| s.trim().len() < 30) {
        return Err("Answer the actual question in ordinary language.".into());
    }
    if stage == Stage::Judgment {
        let scope = &value["checks"]["scope_of_answer"];
        let boundary = facts.iter().find(|f| f.kind == "boundary");
        if boundary.is_some_and(|b| {
            !scope["evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v.as_str() == Some(b.id.as_str()))
        }) {
            return Err("The scope check must cite the supplied calculation boundary.".into());
        }
        if value["verdict"] == "likely_no"
            && !value["checks"]["contrary_testimony"]["evidence"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|id| {
                    facts
                        .iter()
                        .any(|f| f.id == id && matches!(f.kind.as_str(), "condition" | "reception"))
                })
        {
            return Err("A negative event proposal needs relevant contrary condition or reception; absence of a short-window contact is insufficient.".into());
        }
    }
    if stage == Stage::Timing && value["number"].is_number() {
        return Err("No native travel-to-perfection calculation is available; a numeric timing cannot be certified.".into());
    }
    if stage == Stage::Contacts {
        let ids = value["candidate_ids"]
            .as_array()
            .ok_or("Missing candidate IDs")?;
        let signs = value["candidate_signs"]
            .as_array()
            .ok_or("Missing candidate sign checks")?;
        if (value["basis"] == "direct_candidate" && ids.is_empty())
            || (value["basis"] == "no_candidate_covered" && !ids.is_empty())
        {
            return Err("The contact basis must agree with the supplied candidate IDs.".into());
        }
        if ids.len() != signs.len() {
            return Err("Check the native sign-change status of each selected candidate.".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            let id = id.as_str().ok_or("Invalid candidate ID")?;
            if !seen.insert(id) {
                return Err("Duplicate contact candidate.".into());
            }
            let event = facts
                .iter()
                .find(|f| f.id == id && f.kind == "event")
                .and_then(|f| f.event.as_ref())
                .ok_or("Select a supplied native event candidate.")?;
            let matches: Vec<_> = signs.iter().filter(|s| s["id"] == id).collect();
            if matches.len() != 1
                || matches[0]["within_current_signs"]
                    != serde_json::to_value(event.within_current_signs)
                        .map_err(|e| e.to_string())?
            {
                return Err("The selected candidate's sign-change status disagrees with the native calculation.".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_shape(value: &Value, schema: &Value) -> Result<(), String> {
    validate_shape_at(value, schema, "$")
}

/// Narrow diagnostics only; all alternatives have already been validated.
fn tagged_alternative_feedback(
    value: &Value,
    alternatives: &[Value],
    attempts: &[Result<(), String>],
    path: &str,
) -> Option<String> {
    let object = value.as_object()?;
    let first = alternatives.first()?;
    if alternatives
        .iter()
        .any(|branch| branch["type"] != "object" || !branch["properties"].is_object())
    {
        return None;
    }
    let tags = first["properties"]
        .as_object()?
        .keys()
        .filter(|key| {
            alternatives.iter().all(|branch| {
                let property = &branch["properties"][key.as_str()];
                property["type"] == "string"
                    && property["enum"].as_array().is_some_and(|values| {
                        !values.is_empty() && values.iter().all(Value::is_string)
                    })
            })
        })
        .map(|key| Some((key.as_str(), object.get(key)?.as_str()?)))
        .collect::<Option<Vec<_>>>()?;
    if tags.is_empty() {
        return None;
    }
    let mut unsupported = Vec::new();
    for (key, supplied) in &tags {
        let allowed = alternatives
            .iter()
            .flat_map(|branch| branch["properties"][*key]["enum"].as_array())
            .flatten()
            .filter_map(Value::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        if !allowed.contains(supplied) {
            unsupported.push(format!(
                "{path}.{key}: unexpected value {}; allowed values are {}.",
                json!(supplied),
                json!(allowed)
            ));
        }
    }
    if !unsupported.is_empty() {
        return Some(unsupported.join("; "));
    }
    let errors = alternatives
        .iter()
        .zip(attempts)
        .filter(|(branch, _)| {
            tags.iter().all(|(key, supplied)| {
                branch["properties"][*key]["enum"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(*supplied)))
            })
        })
        .filter_map(|(_, result)| result.as_ref().err().map(String::as_str))
        .collect::<Vec<_>>();
    (!errors.is_empty()).then(|| errors.join("; "))
}

/// Preserve the worksheet's strict acceptance rules while giving a repair
/// worker the field, index and exact constraint that rejected its proposal.
fn validate_shape_at(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(alternatives) = schema["oneOf"].as_array() {
        let attempts = alternatives
            .iter()
            .map(|branch| validate_shape_at(value, branch, path))
            .collect::<Vec<_>>();
        let matches = attempts.iter().filter(|result| result.is_ok()).count();
        if matches == 1 {
            return Ok(());
        }
        let details = (matches == 0)
            .then(|| tagged_alternative_feedback(value, alternatives, &attempts, path))
            .flatten()
            .unwrap_or_else(|| {
                attempts
                    .into_iter()
                    .filter_map(Result::err)
                    .collect::<Vec<_>>()
                    .join("; ")
            });
        return Err(format!(
            "{path}: must match exactly one response alternative; matched {matches} of {}. {}",
            alternatives.len(),
            details
        ));
    }
    if let Some(allowed) = schema["enum"].as_array() {
        if !allowed.contains(value) {
            return Err(format!(
                "{path}: unexpected value {value}; allowed values are {}.",
                Value::Array(allowed.clone())
            ));
        }
    }
    let types: Vec<_> = if let Some(t) = schema["type"].as_str() {
        vec![t]
    } else {
        schema["type"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    };
    if !types.iter().any(|t| match *t {
        "null" => value.is_null(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        _ => false,
    }) {
        let actual = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        return Err(format!(
            "{path}: wrong value type {actual}; expected {}.",
            types.join(" or ")
        ));
    }
    if let Some(s) = value.as_str() {
        let actual = s.chars().count();
        if schema["minLength"]
            .as_u64()
            .is_some_and(|n| (actual as u64) < n)
        {
            return Err(format!(
                "{path}: string has {actual} characters; minimum is {}. Supply the requested explanation even when the check is not relevant.",
                schema["minLength"]
            ));
        }
        if schema["maxLength"]
            .as_u64()
            .is_some_and(|n| actual > n as usize)
        {
            return Err(format!(
                "{path}: string has {actual} characters; maximum is {}.",
                schema["maxLength"]
            ));
        }
    }
    if let Some(n) = value.as_f64() {
        if !n.is_finite()
            || schema["minimum"].as_f64().is_some_and(|min| n < min)
            || schema["maximum"].as_f64().is_some_and(|max| n > max)
        {
            return Err(format!(
                "{path}: number {value} is outside its range; minimum is {}, maximum is {}.",
                schema["minimum"], schema["maximum"]
            ));
        }
    }
    if let Some(a) = value.as_array() {
        let maximum = schema["maxItems"].as_u64().unwrap_or(0) as usize;
        if a.len() > maximum {
            return Err(format!(
                "{path}: array has {} entries; maximum is {maximum}.{}",
                a.len(),
                if maximum == 0 {
                    " This field must be []."
                } else {
                    ""
                }
            ));
        }
        for (index, entry) in a.iter().enumerate() {
            validate_shape_at(entry, &schema["items"], &format!("{path}[{index}]"))?;
        }
    }
    if let Some(m) = value.as_object() {
        let fields = schema["properties"]
            .as_object()
            .ok_or_else(|| format!("{path}: missing worksheet contract."))?;
        let missing: Vec<_> = fields.keys().filter(|key| !m.contains_key(*key)).collect();
        let additional: Vec<_> = m.keys().filter(|key| !fields.contains_key(*key)).collect();
        if !missing.is_empty() || !additional.is_empty() {
            return Err(format!(
                "{path}: missing fields {}; additional fields {}.",
                json!(missing),
                json!(additional)
            ));
        }
        for (key, field_schema) in fields {
            validate_shape_at(&m[key], field_schema, &format!("{path}.{key}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod shape_feedback_tests {
    use super::*;

    fn production_update_schema() -> Value {
        let turn = crate::reading_contracts::turn_schema(None);
        json!({"type":"object","properties":{"updates":turn["properties"]["updates"]}})
    }

    fn all_alternative_errors(value: &Value, schema: &Value, path: &str) -> String {
        let alternatives = schema["oneOf"].as_array().unwrap();
        let attempts = alternatives
            .iter()
            .map(|branch| validate_shape_at(value, branch, path))
            .collect::<Vec<_>>();
        let matches = attempts.iter().filter(|result| result.is_ok()).count();
        format!(
            "{path}: must match exactly one response alternative; matched {matches} of {}. {}",
            alternatives.len(),
            attempts
                .into_iter()
                .filter_map(Result::err)
                .collect::<Vec<_>>()
                .join("; ")
        )
    }

    #[test]
    fn unknown_update_field_reports_the_union_once_without_relaxing_validation() {
        let schema = production_update_schema();
        let value = json!({"updates":[{"field":"subject","mode":"supply","value":"the vase","quote":"the vase"}]});
        let before = value.clone();
        let error = validate_shape(&value, &schema).unwrap_err();
        let count = schema["properties"]["updates"]["items"]["oneOf"]
            .as_array()
            .unwrap()
            .len();
        assert!(error.contains(&format!("matched 0 of {count}")), "{error}");
        assert_eq!(error.matches("$.updates[0].field").count(), 1, "{error}");
        assert!(
            error.contains("unexpected value \"subject\"; allowed values are ["),
            "{error}"
        );
        assert!(error.contains("\"deal_capacity\""), "{error}");
        assert!(!error.contains("$.updates[0].mode"), "{error}");
        assert!(crate::reading_contracts::turn_schema(None)["properties"]
            .get("subject")
            .is_some());
        assert_eq!(value, before);
        let mut unknown_mode = value;
        unknown_mode["updates"][0]["field"] = json!("deal_capacity");
        unknown_mode["updates"][0]["mode"] = json!("guess");
        let error = validate_shape(&unknown_mode, &schema).unwrap_err();
        assert_eq!(error.matches("$.updates[0].mode").count(), 1, "{error}");
        assert!(!error.contains("$.updates[0].field:"), "{error}");
    }

    #[test]
    fn known_update_tags_report_only_their_full_branch_failure() {
        let schema = production_update_schema();
        let value = json!({"updates":[{"field":"deal_capacity","mode":"supply","value":"profit","quote":"profit"}]});
        let error = validate_shape(&value, &schema).unwrap_err();
        assert_eq!(error.matches("$.updates[0].value").count(), 1, "{error}");
        assert!(
            error.contains("allowed values are [\"buy\",\"sell\",\"rent\"]"),
            "{error}"
        );
        assert!(!error.contains("$.updates[0].field:"), "{error}");
        assert!(!error.contains("$.updates[0].mode:"), "{error}");
        let mut valid = value;
        valid["updates"][0]["value"] = json!("sell");
        validate_shape(&valid, &schema).unwrap();
    }

    #[test]
    fn joint_field_and_mode_tags_keep_unavailable_reason_validation() {
        let schema = production_update_schema();
        let mut value = json!({"updates":[{"field":"deal_capacity","mode":"unavailable","value":"I do not know the transaction yet.","quote":"I do not know"}]});
        validate_shape(&value, &schema).unwrap();
        for (invalid, constraint) in [
            (json!(null), "wrong value type null; expected string"),
            (
                json!("x".repeat(701)),
                "string has 701 characters; maximum is 700",
            ),
        ] {
            value["updates"][0]["value"] = invalid;
            let error = validate_shape(&value, &schema).unwrap_err();
            assert!(
                error.contains(&format!("$.updates[0].value: {constraint}")),
                "{error}"
            );
            assert!(!error.contains("allowed values are [\"buy\""), "{error}");
            assert!(!error.contains("$.updates[0].mode:"), "{error}");
        }
    }

    #[test]
    fn missing_nonstring_and_incompatible_tags_keep_complete_feedback() {
        let schema =
            crate::reading_contracts::turn_schema(None)["properties"]["updates"]["items"].clone();
        for value in [
            json!({"field":"deal_capacity","value":"sell","quote":"sell"}),
            json!({"field":null,"mode":"supply","value":"sell","quote":"sell"}),
            json!({"field":"deal_capacity","mode":2,"value":"sell","quote":"sell"}),
        ] {
            assert_eq!(
                validate_shape(&value, &schema).unwrap_err(),
                all_alternative_errors(&value, &schema, "$")
            );
        }
        let incompatible = json!({"oneOf":[
            {"type":"object","properties":{"field":{"type":"string","enum":["a"]},"mode":{"type":"string","enum":["x"]}}},
            {"type":"object","properties":{"field":{"type":"string","enum":["b"]},"mode":{"type":"string","enum":["y"]}}}
        ]});
        let value = json!({"field":"a","mode":"y"});
        assert_eq!(
            validate_shape(&value, &incompatible).unwrap_err(),
            all_alternative_errors(&value, &incompatible, "$")
        );
    }

    #[test]
    fn mixed_or_malformed_tag_schemas_keep_complete_feedback() {
        for schema in [
            json!({"oneOf":[{"type":"null"},{"type":"object","properties":{"field":{"type":"string","enum":["a"]}}}]}),
            json!({"oneOf":[{"type":"object","properties":{"field":{"type":"string","enum":["a"]}}},{"type":"object","properties":{"field":{"type":"string","enum":[2]}}}]}),
            json!({"oneOf":[{"type":"object","properties":{"field":{"type":"string","enum":["a"]}}},{"type":"object","properties":{"field":{"type":"string","enum":[]}}}]}),
        ] {
            let value = json!({"field":"other"});
            assert_eq!(
                validate_shape(&value, &schema).unwrap_err(),
                all_alternative_errors(&value, &schema, "$")
            );
        }
    }

    #[test]
    fn tagged_feedback_preserves_full_validation_and_exactly_one_cardinality() {
        let production =
            crate::reading_contracts::turn_schema(None)["properties"]["updates"]["items"].clone();
        let branch = json!({"type":"object","properties":{
            "field":{"type":"string","enum":["a"]},
            "mode":{"type":"string","enum":["supply"]},
            "value":{"type":"string","maxLength":3}
        }});
        let overlap = json!({"oneOf":[branch.clone(),branch]});
        for (schema, values) in [
            (
                production,
                vec![
                    json!({"field":"deal_capacity","mode":"supply","value":"buy","quote":"buy"}),
                    json!({"field":"deal_capacity","mode":"unavailable","value":"unknown","quote":"unknown"}),
                    json!({"field":"deal_capacity","mode":"supply","value":"profit","quote":"profit"}),
                    json!({"field":"deal_capacity","mode":"supply","value":false,"quote":"buy"}),
                    json!({"field":"deal_capacity","mode":"supply","value":"buy"}),
                    json!({"field":"deal_capacity","mode":"supply","value":"buy","quote":"buy","extra":true}),
                    json!({"field":"subject","mode":"supply","value":"vase","quote":"vase"}),
                ],
            ),
            (
                overlap,
                vec![
                    json!({"field":"a","mode":"supply","value":"yes"}),
                    json!({"field":"a","mode":"supply","value":"long"}),
                    json!({"field":"a","mode":"supply","value":null}),
                ],
            ),
        ] {
            for value in values {
                let count = schema["oneOf"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|branch| validate_shape(&value, branch).is_ok())
                    .count();
                assert_eq!(
                    validate_shape(&value, &schema).is_ok(),
                    count == 1,
                    "{value}"
                );
                if count > 1 {
                    assert_eq!(
                        validate_shape(&value, &schema).unwrap_err(),
                        all_alternative_errors(&value, &schema, "$")
                    );
                }
            }
        }
    }

    #[test]
    fn captured_training_reception_prose_is_not_a_formatting_rejection() {
        // The closed TRAIN sale-profit proposal was rejected at this field's
        // 220-character limit. Exercise formatting only: its interpretation,
        // including the contradictory direction claim, is not certified here.
        let value = json!({"checks":{
            "direction":{"state":"unestablished","evidence":[],"finding":"No directed receptions between the relevant parties (Jupiter, Saturn, Mercury, Moon) are supplied."},
            "strength_and_quality":{"state":"supported","evidence":["e43","e47","e49","e51","e52"],"finding":"Jupiter (You) negatively regards Saturn (the bowls) by detriment; Mercury (customers) has minor term regard for Jupiter; Mercury negatively regards Moon (buyers' money) by fall; Moon (buyers' money) negatively regards Jupiter by detriment, but strongly loves Mercury (customers) by domicile and exaltation."},
            "contextual_motive":{"state":"supported","evidence":["e43","e47","e49","e51","e52"],"finding":"The buyers' money (Moon) is strongly drawn to the customers (Mercury), while you (Jupiter) appear to have a negative view of the bowls (Saturn). The customers (Mercury) show little interest in the money (Moon) they are providing."}
        },"summary":"There is significant tension: you (Jupiter) seem to regard the bowls (Saturn) negatively, and the money from customers (Moon) is strongly directed toward the customers themselves (Mercury) rather than you. While customers (Mercury) have a minor inclination toward you, they show a major negative regard for the money (Moon) involved in the transaction.",
        "unknowns":["The customers' regard for the bowls (Saturn) is not established.","The bowls' (Saturn) regard for you or the customers is not established.","The buyers' money's (Moon) regard for the bowls (Saturn) is not established."]});
        let facts = ["e43", "e47", "e49", "e51", "e52"].map(|id| Fact {
            id: id.into(),
            kind: "reception".into(),
            label: "Offline known-ID witness".into(),
            detail: "No semantic assessment in this formatting test".into(),
            planets: Vec::new(),
            condition_facet: None,
            event: None,
            reception: None,
        });
        let before = value.clone();
        assert_eq!(
            value["checks"]["strength_and_quality"]["finding"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            306
        );
        let contract = schema(Stage::Reception, &facts);
        assert_eq!(
            contract["properties"]["checks"]["properties"]["strength_and_quality"]["properties"]
                ["finding"]["type"],
            "string"
        );
        assert_eq!(
            contract["properties"]["checks"]["properties"]["strength_and_quality"]["properties"]
                ["finding"]["minLength"],
            1
        );
        assert!(
            contract["properties"]["checks"]["properties"]["strength_and_quality"]["properties"]
                ["finding"]
                .get("maxLength")
                .is_none()
        );
        validate_shape(&value, &contract).unwrap();
        assert_eq!(value, before); // No shortening or altered historical data.
    }

    fn synthetic_reception() -> (Value, Vec<Fact>) {
        let facts = vec![Fact {
            id: "e0".into(),
            kind: "reception".into(),
            label: "Venus → Mars".into(),
            detail: "Venus regards Mars positively by domicile; the reverse is not supplied."
                .into(),
            planets: vec!["Venus".into(), "Mars".into()],
            condition_facet: None,
            event: None,
            reception: None,
        }];
        let value = json!({"checks":{
            "direction":{"state":"supported","evidence":["e0"],"finding":"Venus regards Mars; this fact does not establish the reverse direction."},
            "strength_and_quality":{"state":"supported","evidence":["e0"],"finding":"The supplied domicile reception indicates strong positive regard."},
            "contextual_motive":{"state":"unestablished","evidence":[],"finding":"The fixture supplies no context for a motive assessment."}
        },"summary":"An observed directed regard is not a prediction of an event.","unknowns":[]});
        (value, facts)
    }

    #[test]
    fn longer_findings_keep_nonempty_type_and_resource_guards() {
        let (value, facts) = synthetic_reception();
        let field = "/checks/strength_and_quality/finding";
        let mut longer = value.clone();
        longer["checks"]["strength_and_quality"]["finding"] =
            json!(format!("{} {}", value.pointer(field).unwrap().as_str().unwrap(),
                "Each direction belongs to the guest's role. The native fact does not establish reciprocity, the recipient's ability, a contact or an event. The supplied domicile quality must remain distinct from the planet's own essential condition."));
        assert!(
            longer
                .pointer(field)
                .unwrap()
                .as_str()
                .unwrap()
                .chars()
                .count()
                > 220
        );
        validate_for(Stage::Reception, Matter::Other, &longer, &facts).unwrap();
        for invalid in [json!(""), json!("  \n"), json!(null), json!(2)] {
            let mut rejected = value.clone();
            rejected["checks"]["strength_and_quality"]["finding"] = invalid;
            assert!(validate_for(Stage::Reception, Matter::Other, &rejected, &facts).is_err());
        }
        for overflow in ["a".repeat(8001), "é".repeat(4001), "<image>".into()] {
            let mut rejected = value.clone();
            rejected["checks"]["strength_and_quality"]["finding"] = json!(overflow);
            assert_eq!(
                validate_for(Stage::Reception, Matter::Other, &rejected, &facts).unwrap_err(),
                "Worksheet exceeds its bounds."
            );
        }
    }

    #[test]
    fn advisory_brevity_keeps_foreign_evidence_and_native_reception_checks() {
        let (value, facts) = synthetic_reception();
        let mut foreign = value.clone();
        foreign["checks"]["direction"]["evidence"] = json!(["not-supplied"]);
        assert!(
            validate_for(Stage::Reception, Matter::Other, &foreign, &facts)
                .unwrap_err()
                .contains("allowed values")
        );
        let mut missing = value.clone();
        missing["checks"]["direction"]["evidence"] = json!([]);
        validate_shape(&missing, &schema(Stage::Reception, &facts)).unwrap();
        assert!(
            validate_for(Stage::Reception, Matter::Other, &missing, &facts)
                .unwrap_err()
                .contains("needs supplied evidence")
        );
        let mut positions = facts.clone();
        positions[0].kind = "position".into();
        validate_shape(&value, &schema(Stage::Reception, &positions)).unwrap();
        assert_eq!(
            validate_for(Stage::Reception, Matter::Other, &value, &positions).unwrap_err(),
            "Reception requires a directed reception fact."
        );
        let mut unknown_state = value.clone();
        unknown_state["checks"]["direction"]["state"] = json!("strong");
        assert!(validate_for(Stage::Reception, Matter::Other, &unknown_state, &facts).is_err());
        let mut extra = value;
        extra["checks"]["direction"]["invented"] = json!("Another field");
        assert!(validate_for(Stage::Reception, Matter::Other, &extra, &facts).is_err());
    }

    #[test]
    fn irrelevant_checks_still_require_an_explanation_in_the_public_contract() {
        let schema = schema(Stage::Condition, &[]);
        let mut value = json!({"checks":{
            "own_dignity":{"state":"unestablished","evidence":[],"finding":"No essential dignity data is supplied in this unit fixture."},
            "ability_to_act":{"state":"unestablished","evidence":[],"finding":"No native house-capacity fact is supplied here."},
            "context_exceptions":{"state":"not_relevant","evidence":[],"finding":""}
        },"summary":"No judgment from missing native facts.","unknowns":[]});
        let error = validate_shape(&value, &schema).unwrap_err();
        assert!(error.contains("$.checks.context_exceptions.finding"));
        assert!(error.contains("minimum is 1"));
        value["checks"]["context_exceptions"]["finding"] =
            json!("No relevant house or solar exception is established in the stated context.");
        validate_shape(&value, &schema).unwrap();
    }

    #[test]
    fn classifier_people_error_identifies_the_field_and_empty_array_repair() {
        let input = json!({"recognition_phase":"classify_question"});
        let schema =
            crate::horary_step::response_schema_for(Stage::Intake, Matter::Other, &input, &[]);
        let mut value = serde_json::to_value(crate::reading_contracts::control(
            crate::reading_contracts::Intent::Read,
        ))
        .unwrap();
        value["people"] = json!([{
            "id":"alex", "label":"Alex", "relationship":"partner", "source_quote":"my partner Alex"
        }]);
        let error = validate_shape(&value, &schema).unwrap_err();
        assert!(error.contains("$.people"), "{error}");
        assert!(error.contains("1 entries; maximum is 0"), "{error}");
        assert!(error.contains("must be []"), "{error}");
        value["people"] = json!([]);
        validate_shape(&value, &schema).unwrap();
    }

    #[test]
    fn canonical_theft_value_error_identifies_update_index_and_allowed_labels() {
        // Use the native field vocabulary to exercise a selected value branch.
        // This test does not change the production Turn's value schema.
        let schema = json!({"type":"object","properties":{
            "updates":{"type":"array","maxItems":1,"items":{
                "type":"object","properties":{
                    "field":{"type":"string","enum":["theft_raised"]},
                    "value":{"type":"string","enum":crate::reading_contracts::allowed_values(crate::reading_contracts::Field::TheftRaised)}
                }
            }}
        }});
        let mut value = json!({"updates":[{"field":"theft_raised","value":"false"}]});
        let error = validate_shape(&value, &schema).unwrap_err();
        assert!(error.contains("$.updates[0].value"), "{error}");
        assert!(error.contains("\"false\""), "{error}");
        assert!(
            error.contains("allowed values are [\"yes\",\"no\"]"),
            "{error}"
        );
        value["updates"][0]["value"] = json!("no");
        validate_shape(&value, &schema).unwrap();
    }

    #[test]
    fn missing_and_additional_fields_are_both_reported_even_with_equal_counts() {
        let schema = json!({"type":"object","properties":{
            "reply":{"type":"string"},"ask":{"type":"string"}
        }});
        let error =
            validate_shape(&json!({"reply":"Hello","asking":"need_0"}), &schema).unwrap_err();
        assert!(error.contains("$: missing fields [\"ask\"]"), "{error}");
        assert!(error.contains("additional fields [\"asking\"]"), "{error}");
    }

    #[test]
    fn nested_object_shape_errors_preserve_the_array_index() {
        let schema = json!({"type":"array","maxItems":1,"items":{
            "type":"object","properties":{"field":{"type":"string"},"value":{"type":"string"}}
        }});
        let error =
            validate_shape(&json!([{"field":"context","quote":"garden"}]), &schema).unwrap_err();
        assert!(
            error.contains("$[0]: missing fields [\"value\"]"),
            "{error}"
        );
        assert!(error.contains("additional fields [\"quote\"]"), "{error}");
    }

    #[test]
    fn ambiguous_response_alternatives_still_fail_with_the_match_count() {
        let schema = json!({"oneOf":[{"type":"string"},{"type":"string"}]});
        let error = validate_shape(&json!("reply"), &schema).unwrap_err();
        assert!(error.contains("$: must match exactly one"), "{error}");
        assert!(error.contains("matched 2 of 2"), "{error}");
    }
}
