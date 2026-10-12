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
                    // Synthesis can cite evidence from all of its checks. Six
                    // IDs is not a methodological limit: even a simple job
                    // reading combines roles, receptions and contacts. Retain
                    // the exact fact allowlist instead of discarding citations.
                    fields.insert(
                        "evidence".into(),
                        if ids.is_empty() {
                            evidence
                        } else {
                            list(choice(&ids), ids.len())
                        },
                    );
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
    Ok((prompt_with_guide(stage, input, schema, fixed), applied))
}

/// The model sees a lossless JSON Schema presentation. Native acceptance keeps
/// the original expanded contract, including every allowed evidence ID.
fn model_contract(stage: Stage, schema: &Value) -> Value {
    if stage != Stage::Judgment || schema.get("$defs").is_some() {
        return schema.clone();
    }
    let checks_path = if schema.get("oneOf").is_some() {
        "/oneOf/0/properties/checks/properties"
    } else {
        "/properties/checks/properties"
    };
    let Some(checks) = schema.pointer(checks_path).and_then(Value::as_object) else {
        return schema.clone();
    };
    let Some(shared) = checks.values().next() else {
        return schema.clone();
    };
    // Do not factor different limits or evidence sets into one definition.
    if checks.len() != Stage::Judgment.checks().len()
        || !checks.values().all(|check| check == shared)
    {
        return schema.clone();
    }
    let mut presented = schema.clone();
    presented["$defs"] = json!({"judgment_check":shared});
    let evidence_path = if schema.get("oneOf").is_some() {
        "/oneOf/0/properties/evidence"
    } else {
        "/properties/evidence"
    };
    let evidence = &shared["properties"]["evidence"];
    if evidence.is_object() && schema.pointer(evidence_path) == Some(evidence) {
        presented["$defs"]["judgment_evidence"] = evidence.clone();
        presented["$defs"]["judgment_check"]["properties"]["evidence"] =
            json!({"$ref":"#/$defs/judgment_evidence"});
        *presented
            .pointer_mut(evidence_path)
            .expect("The equal evidence contract exists") =
            json!({"$ref":"#/$defs/judgment_evidence"});
    }
    for check in presented
        .pointer_mut(checks_path)
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(|checks| checks.values_mut())
    {
        *check = json!({"$ref":"#/$defs/judgment_check"});
    }
    presented
}

/// Contract-derived formatting stays outside the editable system teaching.
fn native_output_protocol(schema: &Value) -> Option<String> {
    let alternatives = schema["oneOf"].as_array();
    let worksheet = alternatives
        .and_then(|branches| {
            branches
                .iter()
                .find(|branch| branch["properties"]["checks"].is_object())
        })
        .unwrap_or(schema);
    let checks = worksheet["properties"]["checks"]["properties"].as_object()?;
    if checks.is_empty()
        || !checks.values().all(|check| {
            check["type"] == "object"
                && check["properties"].as_object().is_some_and(|fields| {
                    fields.len() == 3
                        && fields
                            .get("state")
                            .is_some_and(|field| field["type"] == "string")
                        && fields
                            .get("evidence")
                            .is_some_and(|field| field["type"] == "array")
                        && fields
                            .get("finding")
                            .is_some_and(|field| field["type"] == "string")
                })
        })
    {
        return None;
    }
    let required = worksheet["required"]
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?;
    if required.is_empty() {
        return None;
    }
    let mut protocol = format!("Required:{}. Each named check directly has state,evidence,finding: evidence is an ID array; finding is one string covering role differences, never planet/role maps. Use actual input/contract values.", required.join(","));
    protocol.push_str(&format!(
        " checks contains ONLY:{}. Close checks before writing the other required fields: they are top-level siblings, never inside checks. Write each field once.",
        checks.keys().map(String::as_str).collect::<Vec<_>>().join(",")
    ));
    if alternatives.is_some_and(|branches| {
        branches
            .iter()
            .any(|branch| branch["properties"]["request_input"].is_object())
    }) {
        protocol.push_str(" Allowed request_input replaces the worksheet.");
    }
    Some(protocol)
}

fn prompt_with_guide(stage: Stage, input: &Value, schema: &Value, fixed: String) -> String {
    let repair =
        input.get("original_input").is_some() && input.get("native_validation_error").is_some();
    let mut payload = json!({"input":if repair {crate::horary_step::original_input(input)} else {input},"worksheet_contract":model_contract(stage, schema)});
    if let Some(protocol) = native_output_protocol(schema) {
        payload["native_output_protocol"] = json!(protocol);
    }
    // Stable teaching and stable contract precede changing data. No whole chart
    // or conversation history is smuggled into this prefix.
    if repair {
        // Present repair as a rejected answer followed by native feedback.
        // Keeping the bad worksheet beside accepted input in one example-like
        // JSON object encouraged small models to copy the same mistake.
        // The complete contract is already in the first user message. Repeating
        // it in the correction can push a valid repair over the request budget.
        return json!([
            {"role":"system","content":fixed},
            {"role":"user","content":payload.to_string()},
            {"role":"assistant","content":input["previous_worksheet"].to_string()},
            {"role":"user","content":json!({
                "native_validation_error":input["native_validation_error"],
                "instruction":"Your preceding answer was REJECTED. None of its proposed changes were saved. Produce the complete corrected object using the original input and the output contract. Make the specific correction requested by the native validation error; do not repeat the forbidden entry. Do not change the person's question or ask them to correct your output."
            }).to_string()}
        ]).to_string();
    }
    json!([{"role":"system","content":fixed},{"role":"user","content":payload.to_string()}])
        .to_string()
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
    let method = method_for_input(original);
    let mut guide = lessons::guide_for_method(stage, matter, method)?;
    if let Some(method) = method {
        guide.push_str(&crate::reading_contracts::method_guide(method));
    }
    Ok(guide)
}

fn method_for_input(input: &Value) -> Option<crate::reading_contracts::Method> {
    let original = crate::horary_step::original_input(input);
    serde_json::from_value(original["reading_request"]["binding"]["frame"]["method"].clone())
        .ok()
        .or_else(|| {
            serde_json::from_value::<crate::reading_contracts::Consultation>(
                original["consultation"].clone(),
            )
            .ok()
            .and_then(|case| case.method())
        })
}

pub(crate) fn source_passages_for(
    stage: Stage,
    matter: Matter,
    input: &Value,
) -> Vec<&'static str> {
    lessons::source_passages(stage, matter, method_for_input(input))
}

#[cfg(test)]
mod repair_message_tests {
    use super::*;

    fn consultation_for(method: crate::reading_contracts::Method) -> Value {
        use crate::reading_contracts::{Consultation, Evidence, Facet, Frame, Observation, Slot};
        serde_json::to_value(Consultation {
            frame: Slot::Resolved {
                observation: Observation {
                    value: Frame {
                        method,
                        facet: Facet::Event,
                    },
                    evidence: Evidence::User {
                        turn: 1,
                        quote: "Authored source-selection witness".into(),
                    },
                },
            },
            ..Consultation::default()
        })
        .unwrap()
    }

    #[test]
    fn new_job_teaching_uses_original_handoff_in_every_reading_step_and_repair() {
        let original = json!({"reading_request":{"binding":{"frame":{"method":"new_job"}}}});
        let repair = json!({
            "original_input":{"original_input":original},
            "previous_worksheet":{"frame":{"method":"relationship"}},
            "native_validation_error":"Authored repair witness"
        });
        for stage in [
            Stage::Significators,
            Stage::Condition,
            Stage::Reception,
            Stage::Contacts,
            Stage::Judgment,
        ] {
            let direct = base_guide_for(stage, Matter::Work, &original).unwrap();
            assert_eq!(
                base_guide_for(stage, Matter::Relationship, &repair).unwrap(),
                direct
            );
            assert_eq!(
                source_passages_for(stage, Matter::Relationship, &repair),
                source_passages_for(stage, Matter::Work, &original)
            );
            assert!(direct.contains("id=\"new_job_"));
            assert!(!direct.contains("id=\"relationship_facets\""));
        }
    }

    #[test]
    fn new_job_teaching_resolves_consultation_without_trusting_rejected_worksheet() {
        use crate::reading_contracts::Method;
        let case_input = json!({"consultation":consultation_for(Method::NewJob)});
        assert_eq!(method_for_input(&case_input), Some(Method::NewJob));
        assert!(base_guide_for(Stage::Reception, Matter::Work, &case_input)
            .unwrap()
            .contains("id=\"new_job_employer_interest\""));
        let explicit = json!({
            "reading_request":{"binding":{"frame":{"method":"relationship"}}},
            "consultation":consultation_for(Method::NewJob)
        });
        assert_eq!(method_for_input(&explicit), Some(Method::Relationship));
        let text = base_guide_for(Stage::Reception, Matter::Work, &explicit).unwrap();
        assert!(text.contains("id=\"relationship_facets\""));
        assert!(!text.contains("id=\"new_job_"));
        let no_method = json!({"previous_worksheet":{"frame":{"method":"new_job"}}});
        assert_eq!(method_for_input(&no_method), None);
        assert!(!base_guide_for(Stage::Reception, Matter::Work, &no_method)
            .unwrap()
            .contains("id=\"new_job_"));
    }

    fn expanded_contract(presented: &Value) -> Value {
        fn expand(value: &Value, root: &Value) -> Value {
            if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
                assert!(matches!(
                    reference,
                    "#/$defs/judgment_check" | "#/$defs/judgment_evidence"
                ));
                return expand(root.pointer(&reference[1..]).unwrap(), root);
            }
            match value {
                Value::Object(properties) => Value::Object(
                    properties
                        .iter()
                        .filter(|(key, _)| key.as_str() != "$defs")
                        .map(|(key, value)| (key.clone(), expand(value, root)))
                        .collect(),
                ),
                Value::Array(values) => {
                    Value::Array(values.iter().map(|value| expand(value, root)).collect())
                }
                _ => value.clone(),
            }
        }
        expand(presented, presented)
    }

    fn captured_repair() -> (Value, Value, Value, Stage, Matter) {
        let captured = include_str!("../test-fixtures/training-judgment-repair-messages.json");
        // Historical requests remain byte-identical evidence. Current teaching
        // is checked separately, not normalized back into this paid capture.
        assert_eq!(
            lessons::digest(captured),
            "18718f26c8814e6aa7fb2e9e6f8289d8c157ecdc5dcdaff75b24f9fabe969561"
        );
        let capture: Value = serde_json::from_str(captured).unwrap();
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

    fn assert_current_method_source_guide(
        system: &Value,
        stage: Stage,
        matter: Matter,
        input: &Value,
    ) {
        let original = crate::horary_step::original_input(input);
        let method = serde_json::from_value(
            original["reading_request"]["binding"]["frame"]["method"].clone(),
        )
        .unwrap();
        let mut expected = lessons::guide_for_method(stage, matter, Some(method)).unwrap();
        expected.push_str(&crate::reading_contracts::method_guide(method));
        assert_eq!(system, &json!({"role":"system","content":expected}));
        for id in lessons::source_passages(stage, matter, Some(method)) {
            let card = lessons::passages()
                .unwrap()
                .iter()
                .find(|card| card.id == id)
                .unwrap();
            assert!(system["content"].as_str().unwrap().contains(&card.quote));
        }
    }

    #[test]
    fn captured_judgment_repair_keeps_all_evidence_with_one_complete_schema() {
        let (capture, input, schema, stage, matter) = captured_repair();
        let legacy = &capture["legacy_messages"];
        let actual: Value =
            serde_json::from_str(&prompt(stage, matter, &input, &schema).unwrap()).unwrap();
        assert_eq!(actual.as_array().unwrap().len(), 4);
        assert_current_method_source_guide(&actual[0], stage, matter, &input);
        assert_ne!(actual[0], legacy[0], "Current source-backed teaching intentionally differs from the retained historical system prompt");
        assert_eq!(actual[2], legacy[2]);
        let first: Value = serde_json::from_str(actual[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(first["input"], input["original_input"]);
        assert_eq!(expanded_contract(&first["worksheet_contract"]), schema);
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
            legacy.to_string().len() + actual[0].to_string().len()
                - legacy[0].to_string().len()
                - actual.to_string().len(),
            capture["expected_removed_serialized_prompt_utf8_bytes"]
                .as_u64()
                .unwrap() as usize
                + legacy[1].to_string().len()
                - actual[1].to_string().len()
        );
    }

    #[test]
    fn captured_original_judgment_keeps_native_data_and_lossless_contract() {
        let (capture, repair, schema, stage, matter) = captured_repair();
        let actual: Value = serde_json::from_str(
            &prompt(stage, matter, &repair["original_input"], &schema).unwrap(),
        )
        .unwrap();
        assert_current_method_source_guide(&actual[0], stage, matter, &repair["original_input"]);
        assert_ne!(actual[0], capture["legacy_messages"][0]);
        let body: Value = serde_json::from_str(actual[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(body["input"], repair["original_input"]);
        assert_eq!(expanded_contract(&body["worksheet_contract"]), schema);
        assert!(body["worksheet_contract"].to_string().len() < schema.to_string().len());
    }

    #[test]
    fn judgment_presentation_preserves_native_limits_and_all_evidence_ids() {
        let (_, _, schema, _, _) = captured_repair();
        let compact = model_contract(Stage::Judgment, &schema);
        assert_eq!(expanded_contract(&compact), schema);
        assert!(schema.to_string().len() - compact.to_string().len() > 1500);
        assert_eq!(model_contract(Stage::Contacts, &schema), schema);
        let mut different_limits = schema.clone();
        different_limits["oneOf"][0]["properties"]["checks"]["properties"]["scope_of_answer"]
            ["properties"]["evidence"]["maxItems"] = json!(1);
        assert_eq!(
            model_contract(Stage::Judgment, &different_limits),
            different_limits
        );
    }

    #[test]
    fn judgment_evidence_factoring_preserves_exact_synthetic_contract_and_allowlist() {
        let facts = (0..64)
            .map(|index| Fact {
                id: format!("synthetic-{index}"),
                kind: "position".into(),
                label: "Authored schema witness".into(),
                detail: "No interpretation asserted.".into(),
                planets: Vec::new(),
                condition_facet: None,
                event: None,
                reception: None,
            })
            .collect::<Vec<_>>();
        for wrapped in [false, true] {
            let data = schema(Stage::Judgment, &facts);
            let original = if wrapped {
                json!({"oneOf":[data,crate::horary_step::request_schema(Stage::Judgment)]})
            } else {
                data
            };
            let presented = model_contract(Stage::Judgment, &original);
            assert_eq!(expanded_contract(&presented), original);
            let path = if wrapped {
                "/oneOf/0/properties/evidence"
            } else {
                "/properties/evidence"
            };
            // The shared check is compact, while synthesis retains the larger
            // union of valid citations without conflating their array limits.
            assert!(presented["$defs"].get("judgment_evidence").is_none());
            assert_eq!(presented.pointer(path), original.pointer(path));
            assert_eq!(presented.pointer(path).unwrap()["maxItems"], 64);
            assert_eq!(
                presented["$defs"]["judgment_check"]["properties"]["evidence"]["maxItems"],
                6
            );
            assert_eq!(
                presented.pointer(path).unwrap()["items"]["enum"]
                    .as_array()
                    .unwrap()
                    .len(),
                64
            );
            assert!(presented.to_string().len() < original.to_string().len());
        }
    }

    #[test]
    fn synthesis_keeps_ten_valid_citations_but_rejects_foreign_ids() {
        let facts = (0..12)
            .map(|index| Fact {
                id: format!("e{index}"),
                kind: "position".into(),
                label: "Synthetic native evidence".into(),
                detail: "No astrological conclusion asserted".into(),
                planets: vec![],
                condition_facet: None,
                event: None,
                reception: None,
            })
            .collect::<Vec<_>>();
        let schema = schema(Stage::Judgment, &facts);
        let citations = json!((0..10).map(|index| format!("e{index}")).collect::<Vec<_>>());
        let contract = &schema["properties"]["evidence"];
        validate_shape_at(&citations, contract, "$.evidence").unwrap();
        let mut foreign = citations;
        foreign[9] = json!("not-in-this-chart");
        assert!(validate_shape_at(&foreign, contract, "$.evidence").is_err());
    }

    #[test]
    fn judgment_evidence_factoring_keeps_empty_and_different_limits_distinct() {
        let empty = schema(Stage::Judgment, &[]);
        let presented = model_contract(Stage::Judgment, &empty);
        assert_eq!(expanded_contract(&presented), empty);
        assert_eq!(presented["$defs"]["judgment_evidence"]["maxItems"], 0);
        assert_eq!(
            presented["$defs"]["judgment_evidence"]["items"],
            json!({"type":"string","maxLength":1})
        );
        let mut different = empty;
        different["properties"]["evidence"]["maxItems"] = json!(1);
        let presented = model_contract(Stage::Judgment, &different);
        assert_eq!(expanded_contract(&presented), different);
        assert!(presented["$defs"].get("judgment_evidence").is_none());
        assert_eq!(presented["properties"]["evidence"]["maxItems"], 1);
        assert_eq!(
            presented["$defs"]["judgment_check"]["properties"]["evidence"]["maxItems"],
            0
        );
    }

    #[test]
    fn judgment_output_fields_declare_every_required_top_level_field() {
        let lesson = include_str!("horary_prompts/judgment.md");
        let fields = lesson
            .split_once("<output_fields>")
            .unwrap()
            .1
            .split_once('.')
            .unwrap()
            .0;
        let declared = fields
            .split(',')
            .map(|field| field.split_whitespace().next().unwrap())
            .collect::<Vec<_>>();
        let contract = schema(Stage::Judgment, &[]);
        for field in contract["required"].as_array().unwrap() {
            assert!(declared.contains(&field.as_str().unwrap()), "{field}");
        }
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
mod protected_output_protocol_tests {
    use super::*;

    fn messages(stage: Stage, input: &Value, schema: &Value) -> Value {
        serde_json::from_str(&prompt(stage, Matter::Other, input, schema).unwrap()).unwrap()
    }

    fn first_payload(messages: &Value) -> Value {
        serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn actual_required_fields_and_direct_check_shape_are_compact_protected_context() {
        let input = json!({"source_marker":"Synthetic input remains data"});
        for stage in [
            Stage::Condition,
            Stage::Reception,
            Stage::Contacts,
            Stage::Location,
            Stage::Timing,
            Stage::Judgment,
            Stage::Explanation,
        ] {
            let schema = crate::horary_step::response_schema_for(stage, Matter::Other, &input, &[]);
            let before = schema.clone();
            let actual = messages(stage, &input, &schema);
            let payload = first_payload(&actual);
            let protocol = payload["native_output_protocol"].as_str().unwrap();
            let worksheet = if schema["oneOf"].is_array() {
                &schema["oneOf"][0]
            } else {
                &schema
            };
            let required = worksheet["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|field| field.as_str().unwrap())
                .collect::<Vec<_>>();
            assert!(protocol.starts_with(&format!("Required:{}.", required.join(","))));
            assert!(protocol.contains("state,evidence,finding"));
            assert!(protocol.contains("finding is one string covering role differences"));
            for check in worksheet["properties"]["checks"]["properties"]
                .as_object()
                .unwrap()
                .keys()
            {
                assert!(protocol.contains(check), "Missing check boundary: {check}");
            }
            assert!(protocol.contains("top-level siblings, never inside checks"));
            assert!(protocol.contains("Write each field once"));
            let control_allowed = schema["oneOf"].as_array().is_some_and(|branches| {
                branches
                    .iter()
                    .any(|branch| branch["properties"]["request_input"].is_object())
            });
            assert_eq!(
                protocol.contains("Allowed request_input replaces the worksheet"),
                control_allowed
            );
            assert!(!protocol.contains("source_marker"));
            assert!(!protocol.contains("likely_yes"));
            assert_eq!(payload["input"], input);
            assert_eq!(
                payload["worksheet_contract"],
                model_contract(stage, &schema)
            );
            assert_eq!(schema, before);
            assert_eq!(
                actual[0]["content"],
                guide_for(stage, Matter::Other, &input).unwrap()
            );
            let old_payload =
                json!({"input":input,"worksheet_contract":model_contract(stage,&schema)});
            let added_bytes = payload.to_string().len() - old_payload.to_string().len();
            assert!(
                added_bytes <= 650,
                "{}: {added_bytes} additional UTF-8 bytes",
                stage.name()
            );
            println!("{} native protocol adds {added_bytes} UTF-8 bytes; tokenizer count is not measured", stage.name());
            if stage == Stage::Judgment {
                assert!(required.contains(&"summary"));
                assert!(payload["worksheet_contract"]["$defs"].is_object());
            }
        }
    }

    #[test]
    fn whole_or_edit_based_teaching_rewrites_cannot_remove_the_native_protocol() {
        let original = json!({"source_marker":"Synthetic accepted input"});
        let schema = crate::horary_step::response_schema_for(
            Stage::Reception,
            Matter::Other,
            &original,
            &[],
        );
        let base = base_guide_for(Stage::Reception, Matter::Other, &original).unwrap();
        for edit in [false, true] {
            let mut program =
                synthetic_teaching_program(Stage::Reception, &original, "replacement");
            if edit {
                program.overrides[0].replacement_text.clear();
                program.overrides[0].edits = vec![horary_prompt_program::Edit {
                    old_text:"Return one JSON worksheet matching the supplied response schema, without fences or extra commentary.".into(),
                    new_text:"Editorial teaching without that formatting sentence.".into(),
                }];
            } else {
                program.overrides[0].replacement_text = format!(
                    "Editorial teaching without the old format/completion prose. Preserve current source facts and express only what the supplied evidence warrants.\n{}",
                    horary_prompt_program::guide_parts(&base)
                        .unwrap()
                        .quoted_source
                        .join("")
                );
            }
            for input in [
                original.clone(),
                json!({"original_input":original,
                "previous_worksheet":{"rejected":true},"native_validation_error":"Synthetic shape rejection"}),
            ] {
                let baseline = messages(Stage::Reception, &input, &schema);
                let (actual, applied) = prompt_with_program(
                    Stage::Reception,
                    Matter::Other,
                    &input,
                    &schema,
                    Some(&program),
                )
                .unwrap();
                let actual: Value = serde_json::from_str(&actual).unwrap();
                assert_ne!(actual[0], baseline[0]);
                assert_eq!(
                    &actual.as_array().unwrap()[1..],
                    &baseline.as_array().unwrap()[1..]
                );
                assert!(first_payload(&actual)["native_output_protocol"].is_string());
                let (guide, receipt) =
                    resolved_guide_for(Stage::Reception, Matter::Other, &input, Some(&program))
                        .unwrap();
                assert_eq!(applied, receipt);
                assert_eq!(actual[0]["content"], guide);
                let applied = applied.unwrap();
                assert_eq!(applied.original_guide_sha256, lessons::digest(&base));
                assert_eq!(applied.replacement_guide_sha256, lessons::digest(&guide));
                let captured = json!({"prompt":actual,"prompt_program":applied,
                    "guide_sha256":lessons::digest(&guide),"executor_guide_identity":horary_prompt_program::EFFECTIVE_GUIDE_CAPTURE});
                assert_eq!(
                    horary_prompt_program::captured_record_guide_sha256(&captured, None).unwrap(),
                    lessons::digest(&guide)
                );
            }
        }
    }

    #[test]
    fn fresh_and_repair_requests_keep_one_schema_and_protocol_with_exact_original_material() {
        let original = json!({"source_marker":"Unchanged synthetic original input"});
        let rejected = json!({"checks":{"direction":{"unwanted_child":{"state":"unestablished"}}}});
        let error = "Synthetic direct-check shape rejection";
        let repair = json!({"original_input":original,"previous_worksheet":rejected,"native_validation_error":error});
        let schema = crate::horary_step::response_schema_for(
            Stage::Reception,
            Matter::Other,
            &original,
            &[],
        );
        let fresh = messages(Stage::Reception, &original, &schema);
        let actual = messages(Stage::Reception, &repair, &schema);
        assert_eq!(actual.as_array().unwrap().len(), 4);
        assert_eq!(actual[0], fresh[0]);
        assert_eq!(actual[1], fresh[1]);
        assert_eq!(actual[2]["role"], "assistant");
        assert_eq!(actual[2]["content"], rejected.to_string());
        let correction: Value =
            serde_json::from_str(actual[3]["content"].as_str().unwrap()).unwrap();
        assert_eq!(correction["native_validation_error"], error);
        assert_eq!(correction.as_object().unwrap().len(), 2);
        assert!(correction.get("native_output_protocol").is_none());
        assert!(correction.get("worksheet_contract").is_none());
        let user_objects = actual
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "user")
            .map(|m| serde_json::from_str::<Value>(m["content"].as_str().unwrap()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            user_objects
                .iter()
                .filter(|v| v.get("worksheet_contract").is_some())
                .count(),
            1
        );
        assert_eq!(
            user_objects
                .iter()
                .filter(|v| v.get("native_output_protocol").is_some())
                .count(),
            1
        );
    }

    #[test]
    fn noncheck_requests_keep_the_existing_first_user_and_system_bytes() {
        let original =
            json!({"recognition_phase":"classify_question","source_marker":"Unchanged input"});
        for stage in [
            Stage::Intake,
            Stage::Conversation,
            Stage::Place,
            Stage::Moment,
            Stage::Significators,
        ] {
            let schema = schema_for(stage, Matter::Other, &[]);
            let expected = json!({"input":original,"worksheet_contract":schema}).to_string();
            for input in [
                original.clone(),
                json!({"original_input":original,
                "previous_worksheet":{"rejected":true},"native_validation_error":"Synthetic rejection"}),
            ] {
                let actual = messages(stage, &input, &schema);
                assert_eq!(
                    actual[0],
                    json!({"role":"system","content":guide_for(stage,Matter::Other,&input).unwrap()})
                );
                assert_eq!(actual[1], json!({"role":"user","content":expected}));
                assert!(native_output_protocol(&schema).is_none());
            }
        }
    }

    #[test]
    fn schema_acceptance_and_allowed_control_alternatives_are_unchanged() {
        let input = json!({"source_marker":"No chart facts or verdict in this shape test"});
        let schema =
            crate::horary_step::response_schema_for(Stage::Reception, Matter::Other, &input, &[]);
        let mut checks = serde_json::Map::new();
        for key in Stage::Reception.checks() {
            checks.insert(
                (*key).into(),
                json!({"state":"not_relevant","evidence":[],"finding":"Synthetic shape only"}),
            );
        }
        let value = json!({"checks":checks,"summary":"Synthetic shape only","unknowns":[]});
        validate_shape(&value, &schema).unwrap();
        let field = schema["oneOf"][1]["properties"]["request_input"]["properties"]["field"]
            ["enum"][0]
            .clone();
        let request = json!({"request_input":{"field":field,"question":"Which circumstance matters?","reason":"Synthetic control-path shape witness"}});
        validate_shape(&request, &schema).unwrap();
        let mut invalid = value.clone();
        invalid["checks"][Stage::Reception.checks()[0]] =
            json!({"unwanted_child":value["checks"][Stage::Reception.checks()[0]]});
        let before_error = validate_shape(&invalid, &schema).unwrap_err();
        let actual = first_payload(&messages(Stage::Reception, &input, &schema));
        assert_eq!(actual["worksheet_contract"], schema);
        assert_eq!(validate_shape(&invalid, &schema).unwrap_err(), before_error);
        validate_shape(&value, &schema).unwrap();
        validate_shape(&request, &schema).unwrap();
        let plain = schema_for(Stage::Reception, Matter::Other, &[]);
        assert!(!native_output_protocol(&plain)
            .unwrap()
            .contains("request_input"));
    }

    #[test]
    fn unsupported_check_or_required_metadata_has_no_invented_format_hint() {
        let plain = schema_for(Stage::Reception, Matter::Other, &[]);
        let mut wrong_fields = plain.clone();
        let fields = wrong_fields["properties"]["checks"]["properties"]["direction"]["properties"]
            .as_object_mut()
            .unwrap();
        let state = fields.remove("state").unwrap();
        fields.insert("default".into(), state);
        let mut wrong_type = plain.clone();
        wrong_type["properties"]["checks"]["properties"]["direction"]["properties"]["finding"]
            ["type"] = json!("object");
        let mut wrong_required = plain.clone();
        wrong_required["required"] = json!(["checks", 2]);
        for schema in [wrong_fields, wrong_type, wrong_required] {
            assert!(native_output_protocol(&schema).is_none());
        }
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
                        stage,
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
