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
        // Seven selected traditional planets must fit the shared condition
        // coverage checks; other stage contracts retain their existing cap.
        list(choice(&ids), if stage == Stage::Condition { 7 } else { 6 })
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
                checks.insert((*key).into(), object(json!({"state":choice(&["supported","contradicted","unestablished","not_relevant"]),"evidence":evidence,"finding":{"type":"string","minLength":1,"maxLength":220,"description":"A brief nonempty explanation, including why a check is not relevant or unestablished. An empty string cannot finish a check."}})));
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

pub fn prompt(
    stage: Stage,
    matter: Matter,
    input: &Value,
    schema: &Value,
) -> Result<String, String> {
    // Stable teaching and stable contract precede changing data. No whole chart
    // or conversation history is smuggled into this prefix.
    let fixed = guide_for(stage, matter, input)?;
    if input.get("original_input").is_some() && input.get("native_validation_error").is_some() {
        // Present repair as a rejected answer followed by native feedback.
        // Keeping the bad worksheet beside accepted input in one example-like
        // JSON object encouraged small models to copy the same mistake.
        return Ok(json!([
            {"role":"system","content":fixed},
            {"role":"user","content":json!({"input":crate::horary_step::original_input(input),"worksheet_contract":schema}).to_string()},
            {"role":"assistant","content":input["previous_worksheet"].to_string()},
            {"role":"user","content":json!({
                "native_validation_error":input["native_validation_error"],
                "instruction":"Your preceding answer was REJECTED. None of its proposed changes were saved. Produce the complete corrected object using the original input and the output contract. Make the specific correction requested by the native validation error; do not repeat the forbidden entry. Do not change the person's question or ask them to correct your output.",
                "worksheet_contract":schema
            }).to_string()}
        ]).to_string());
    }
    Ok(json!([{"role":"system","content":fixed},{"role":"user","content":json!({"input":input,"worksheet_contract":schema}).to_string()}]).to_string())
}

pub fn guide_for(stage: Stage, matter: Matter, input: &Value) -> Result<String, String> {
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
        return Err(format!(
            "{path}: must match exactly one response alternative; matched {matches} of {}. {}",
            alternatives.len(),
            attempts
                .into_iter()
                .filter_map(Result::err)
                .collect::<Vec<_>>()
                .join("; ")
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
