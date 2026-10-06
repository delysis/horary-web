//! Explicit horary tasks. Native calculations own facts; worksheets own proposals.
#![forbid(unsafe_code)]
use crate::{
    conversation::{Message, Section, Session},
    geocode::{geocode_with_cache, GeocodeRequest, GeocodeState, LocationCandidate},
    horary_lessons::{self as lessons, Matter, Stage},
    native_llama_worker::NativeGenerationResult,
    reading_method::{self, Fact, Role, RoleChoice, Step},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    pub intent: String,
    pub question: String,
    pub matter: Matter,
    pub question_kind: String,
    pub context: String,
    pub place_request: String,
    pub time_request: String,
    pub horizon: String,
    pub clarification: String,
    pub focus: String,
    pub heard: String,
    #[serde(default)]
    pub restore_revision: Option<u64>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct MethodState {
    pub brief: Brief,
    pub records: Vec<Record>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub stage: Stage,
    pub revision: u64,
    pub guide_sha256: String,
    pub schema_sha256: String,
    pub input_sha256: String,
    pub input: Value,
    pub raw: String,
    pub worksheet: Value,
    pub generation: NativeGenerationResult,
    pub source_passages: Vec<String>,
    #[serde(default)]
    pub validation_error: Option<String>,
}

pub trait Runtime: Sync {
    fn generate(
        &self,
        stage: Stage,
        matter: Matter,
        input: &Value,
        schema: &Value,
        audio: Option<&[u8]>,
    ) -> Result<NativeGenerationResult, String>;
    fn generate_batch(
        &self,
        tasks: &[(Stage, Matter, Value, Value)],
    ) -> Result<Vec<NativeGenerationResult>, String>;
    fn publish(&self, session: &mut Session) -> Result<(), String>;
    fn check(&self) -> Result<(), String>;
    fn directory(&self) -> &Path;
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
        Stage::Intake => object(
            json!({"intent":choice(&["read","clarify","correct","new_question","explain","restore"]),"question":text(500),"matter":choice(&["relationship","lost_object","lost_animal","work","money","property","other"]),"question_kind":choice(&["event","situation","location","choice"]),"context":text(700),"place_request":text(240),"time_request":text(180),"horizon":text(100),"clarification":text(180),"focus":choice(&["roles","condition","reception","contacts","location","timing","judgment","place","moment"]),"heard":text(500),"restore_revision":{"type":["integer","null"],"minimum":1}}),
        ),
        Stage::Place => object(
            json!({"mode":choice(&["select","lookup","ask"]),"place_id":text(100),"query":text(240),"clarification":text(180),"basis":text(240)}),
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
                checks.insert((*key).into(), object(json!({"state":choice(&["supported","contradicted","unestablished","not_relevant"]),"evidence":evidence,"finding":text(220)})));
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
    let fixed = lessons::guide(stage, matter)?;
    Ok(json!([{"role":"system","content":fixed},{"role":"user","content":json!({"input":input,"worksheet_contract":schema}).to_string()}]).to_string())
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

fn validate_for(stage: Stage, matter: Matter, value: &Value, facts: &[Fact]) -> Result<(), String> {
    if !bounded_strings(value) {
        return Err("Worksheet exceeds its bounds.".into());
    }
    let contract = schema_for(stage, matter, facts);
    validate_shape(value, &contract)?;
    if stage == Stage::Intake {
        let brief: Brief = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        if brief.question.trim().is_empty() && brief.clarification.trim().is_empty() {
            return Err("Keep the actual question or ask what it is.".into());
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

fn validate_shape(value: &Value, schema: &Value) -> Result<(), String> {
    if let Some(allowed) = schema["enum"].as_array() {
        if !allowed.contains(value) {
            return Err(format!("Unexpected worksheet value {value}."));
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
        return Err("Wrong worksheet value type.".into());
    }
    if let Some(s) = value.as_str() {
        if schema["maxLength"]
            .as_u64()
            .is_some_and(|n| s.chars().count() > n as usize)
        {
            return Err("Worksheet string is too long.".into());
        }
    }
    if let Some(n) = value.as_f64() {
        if !n.is_finite()
            || schema["minimum"].as_f64().is_some_and(|min| n < min)
            || schema["maximum"].as_f64().is_some_and(|max| n > max)
        {
            return Err("Worksheet number is outside its range.".into());
        }
    }
    if let Some(a) = value.as_array() {
        if a.len() > schema["maxItems"].as_u64().unwrap_or(0) as usize {
            return Err("Too many worksheet entries.".into());
        }
        for v in a {
            validate_shape(v, &schema["items"])?;
        }
    }
    if let Some(m) = value.as_object() {
        let fields = schema["properties"]
            .as_object()
            .ok_or("Missing worksheet contract")?;
        if m.len() != fields.len() {
            return Err("Missing or additional worksheet fields.".into());
        }
        for (k, s) in fields {
            validate_shape(m.get(k).ok_or_else(|| format!("Missing {k}."))?, s)?;
        }
    }
    Ok(())
}

fn generate(
    runtime: &impl Runtime,
    stage: Stage,
    matter: Matter,
    input: Value,
    facts: &[Fact],
    audio: Option<&[u8]>,
) -> Result<(Value, Record), String> {
    runtime.check()?;
    let contract = schema_for(stage, matter, facts);
    let result = runtime.generate(stage, matter, &input, &contract, audio)?;
    let parsed = decode_json(&result.content);
    let validation_error = parsed
        .as_ref()
        .map_err(|e| e.clone())
        .and_then(|v| validate_for(stage, matter, v, facts))
        .err();
    let value = parsed.unwrap_or(Value::Null);
    let record = Record {
        stage,
        revision: 0,
        guide_sha256: lessons::digest(&lessons::guide(stage, matter)?),
        schema_sha256: lessons::digest(&contract.to_string()),
        input_sha256: lessons::digest(&input.to_string()),
        input,
        raw: result.content.clone(),
        worksheet: value.clone(),
        generation: result,
        source_passages: stage.passages(matter).iter().map(|s| (*s).into()).collect(),
        validation_error,
    };
    Ok((value, record))
}

fn keep(session: &mut Session, runtime: &impl Runtime, mut record: Record) -> Result<(), String> {
    record.revision = session.revision;
    let metric = crate::conversation::inference_measurement(&record.generation);
    if let Ok(progress) = crate::review_progress::record_with_inference(
        runtime.directory(),
        record.stage.name(),
        record.stage.activity(),
        metric.elapsed_ms,
        Some(metric),
    ) {
        session.progress.push(progress);
        if session.progress.len() > 160 {
            session.progress.remove(0);
        }
    }
    log::info!("horary stage={} revision={} guide={} prompt_tokens={} cached_tokens={} output_tokens={} elapsed_ms={} lesson_bank_hit={:?} lesson_prepare_ms={:?} valid={}",record.stage.name(),record.revision,record.guide_sha256,record.generation.prompt_tokens,record.generation.cached_prompt_tokens,record.generation.generated_tokens,record.generation.elapsed_ms,record.generation.lesson_bank_hit,record.generation.lesson_prepare_ms,record.validation_error.is_none());
    session.audit.push(json!({"event":"method_stage","stage":record.stage,"dependencies":record.stage.dependencies(),"bookOcrSha256":lessons::BOOK_OCR_SHA256,"revision":record.revision,"guideSha256":record.guide_sha256,"schemaSha256":record.schema_sha256,"inputSha256":record.input_sha256,"generation":record.generation}));
    session.method.records.push(record);
    runtime.publish(session)
}

fn task(
    session: &mut Session,
    runtime: &impl Runtime,
    stage: Stage,
    input: Value,
    facts: &[Fact],
) -> Result<Value, String> {
    session.status = stage.activity().into();
    runtime.publish(session)?;
    let mut input = input;
    let attempts = if input.get("native_validation_error").is_some() {
        1
    } else {
        2
    };
    for attempt in 0..attempts {
        let (value, record) = generate(
            runtime,
            stage,
            session.method.brief.matter,
            input.clone(),
            facts,
            None,
        )?;
        let error = record.validation_error.clone();
        keep(session, runtime, record)?;
        if let Some(error) = error {
            if attempt + 1 == attempts {
                return Err(format!("{} needs review: {error}", stage.name()));
            }
            input = json!({"original_input":input,"previous_worksheet":value,"native_validation_error":error,"instruction":"Correct only this worksheet. Do not add assumptions or change the task."});
        } else {
            return Ok(value);
        }
    }
    Err("The worksheet remains unresolved.".into())
}

fn relevant(facts: &[Fact], roles: &[Role], kinds: &[&str]) -> Vec<Fact> {
    facts
        .iter()
        .filter(|f| {
            kinds.contains(&f.kind.as_str())
                && (f.planets.is_empty()
                    || if f.kind == "condition" {
                        f.planets
                            .iter()
                            .any(|p| roles.iter().any(|r| &r.planet == p))
                    } else {
                        f.planets
                            .iter()
                            .all(|p| roles.iter().any(|r| &r.planet == p))
                    })
        })
        .cloned()
        .collect()
}

pub fn native_place(
    brief: &Brief,
    candidates: &[LocationCandidate],
    existing: Option<&LocationCandidate>,
    geocode: &GeocodeState,
) -> Result<(Option<LocationCandidate>, Vec<LocationCandidate>), String> {
    if brief.place_request.trim().is_empty() {
        return Ok((
            existing
                .cloned()
                .or_else(|| candidates.iter().find(|p| p.provider == "device").cloned()),
            Vec::new(),
        ));
    }
    let matches = geocode_with_cache(
        geocode,
        GeocodeRequest {
            query: brief.place_request.clone(),
            limit: Some(5),
        },
    )
    .map_err(|e| e.message)?;
    let chosen = (matches.len() == 1).then(|| matches[0].clone());
    Ok((chosen, matches))
}

fn resolve_moment(
    moment: &Value,
    chart: Option<&Value>,
    instant: f64,
    zone: &str,
) -> Result<f64, String> {
    match moment["mode"].as_str() {
        Some("now") => Ok(instant),
        Some("keep") => Ok(chart
            .and_then(|c| c["timestampMs"].as_f64())
            .unwrap_or(instant)),
        Some("explicit") => horary_ai_core::chart_input::resolve_chart_time(
            moment["local_time"].as_str().unwrap_or(""),
            zone,
            moment["occurrence"].as_str().unwrap_or(""),
        ),
        _ => Err("The question moment remains unresolved.".into()),
    }
}

fn restore_revision(session: &mut Session, number: u64) -> Result<(), String> {
    let old = session
        .revisions
        .iter()
        .find(|r| r.number == number)
        .cloned()
        .ok_or("Choose a listed earlier chart revision.")?;
    let next = session
        .revision
        .checked_add(1)
        .ok_or("Reading revision exhausted")?;
    session.revisions.push(crate::conversation::Revision {
        number: session.revision,
        question: session.question.clone(),
        chart: session.chart.clone(),
        sections: session.sections.clone(),
        place: session.place.clone(),
        brief: session.method.brief.clone(),
    });
    session.revision = next;
    session.question = old.question;
    session.chart = old.chart;
    session.place = old.place;
    session.sections = old.sections;
    session.method.brief = old.brief;
    session.method.brief.question = session.question.clone();
    session.chart_after_message = session.messages.len();
    for section in &mut session.sections {
        section.revision = next;
        section.after_message = session.messages.len();
    }
    session
        .audit
        .push(json!({"event":"restore_revision","restored":number,"activation_revision":next}));
    Ok(())
}

fn section(session: &mut Session, stage: Stage, value: &Value, facts: &[Fact], roles: Vec<Role>) {
    let ids: Vec<String> = if stage == Stage::Significators {
        facts
            .iter()
            .filter(|f| f.kind == "house")
            .map(|f| f.id.clone())
            .take(5)
            .collect()
    } else {
        let mut ids: Vec<_> = value["checks"]
            .as_object()
            .into_iter()
            .flatten()
            .flat_map(|(_, v)| v["evidence"].as_array().into_iter().flatten())
            .chain(value["evidence"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };
    let body = value["answer"]
        .as_str()
        .or_else(|| value["summary"].as_str())
        .unwrap_or("")
        .to_string();
    let step = match stage {
        Stage::Significators => Some(Step::Significators),
        Stage::Judgment => Some(Step::Judgment),
        _ => Some(Step::Testimony),
    };
    let title = match stage {
        Stage::Significators => "The people and the matter",
        Stage::Judgment => "An answer taking shape",
        Stage::Location => "Where to look",
        Stage::Reception => "What draws them together",
        Stage::Contacts => "What could bring it about",
        _ => stage.title(),
    };
    let rules = lessons::passages()
        .ok()
        .into_iter()
        .flatten()
        .filter(|p| {
            stage
                .passages(session.method.brief.matter)
                .contains(&p.id.as_str())
        })
        .map(|p| reading_method::BookRule {
            id: p.id.clone(),
            title: "From the textbook".into(),
            explanation: p.quote.clone(),
            pages: if p.printed_first == p.printed_last {
                p.printed_first.to_string()
            } else {
                format!("{}–{}", p.printed_first, p.printed_last)
            },
            quoted: true,
        })
        .collect();
    session.sections.push(Section {
        title: title.into(),
        body,
        evidence: ids.clone(),
        revision: session.revision,
        after_message: session.messages.len(),
        step,
        rules,
        because: String::new(),
        roles,
        facts: facts
            .iter()
            .filter(|f| ids.contains(&f.id))
            .cloned()
            .collect(),
        draft: String::new(),
        worksheet: value.clone(),
        method_stage: Some(stage),
    });
}

pub fn run(
    session: &mut Session,
    runtime: &impl Runtime,
    geocode: &GeocodeState,
    instant: f64,
    audio: Option<&[u8]>,
    previous: &Session,
) -> Result<(), String> {
    let input = json!({"retained_brief":session.method.brief,"canonical_question":session.question,"chart_exists":session.chart.is_some(),"device_place_available":session.candidates.iter().any(|p|p.provider=="device"),"native_clock_available":true,"available_revisions":session.revisions.iter().map(|r|json!({"number":r.number,"question":r.question})).collect::<Vec<_>>(),"latest_words":session.messages.last().map(|m|m.text.as_str()),"last_reader_question":session.messages.iter().rev().find(|m|m.role=="assistant").map(|m|m.text.as_str()),"spoken_input":audio.is_some()});
    session.status = Stage::Intake.activity().into();
    runtime.publish(session)?;
    let (value, record) = generate(
        runtime,
        Stage::Intake,
        session.method.brief.matter,
        input,
        &[],
        audio,
    )?;
    if let Some(error) = record.validation_error.clone() {
        keep(session, runtime, record)?;
        return Err(format!("The understood question needs review: {error}"));
    }
    let brief: Brief = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if brief.intent == "restore"
        && brief
            .restore_revision
            .is_none_or(|number| !session.revisions.iter().any(|r| r.number == number))
    {
        keep(session, runtime, record)?;
        return Err("Choose one listed earlier chart revision.".into());
    }
    if brief.intent == "new_question" {
        let last = session.messages.last().cloned();
        let sequence = session.snapshot_id;
        *session = crate::reading_store::fresh(runtime.directory(), previous, None)?;
        session.snapshot_id = sequence;
        if let Some(last) = last {
            session.messages.push(last);
        }
        session.device_context = previous.device_context.clone();
        session.candidates = previous
            .candidates
            .iter()
            .filter(|p| p.provider == "device")
            .cloned()
            .collect();
    }
    if audio.is_some() {
        if brief.heard.trim().is_empty() {
            return Err("The spoken meaning needs clarification.".into());
        }
        if let Some(last) = session.messages.last_mut() {
            last.text = format!("From your spoken words: {}", brief.heard);
        }
    }
    session.question = brief.question.clone();
    session.method.brief = brief;
    if session.chart.is_none()
        && session.candidate_moment_ms.is_none()
        && session.method.brief.clarification.is_empty()
    {
        session.candidate_moment_ms = Some(instant);
        session.audit.push(json!({"event":"question_moment_candidate","timestampMs":instant,"basis":"Receipt of the understood question; a later place-only clarification keeps this instant."}));
    }
    keep(session, runtime, record)?;
    if session.method.brief.intent == "restore" {
        restore_revision(
            session,
            session
                .method
                .brief
                .restore_revision
                .ok_or("Choose one listed revision to restore.")?,
        )?;
        return Ok(());
    }
    if !session.method.brief.clarification.is_empty() {
        session.messages.push(Message {
            role: "assistant".into(),
            text: session.method.brief.clarification.clone(),
        });
        return Ok(());
    }
    if session.method.brief.intent == "explain" && session.chart.is_some() {
        let facts = reading_method::facts(session.chart.as_ref());
        let focus = &session.method.brief.focus;
        let stage = match focus.as_str() {
            "roles" => Stage::Significators,
            "condition" => Stage::Condition,
            "reception" => Stage::Reception,
            "contacts" => Stage::Contacts,
            "location" => Stage::Location,
            "timing" => Stage::Timing,
            _ => Stage::Judgment,
        };
        let record = session
            .method
            .records
            .iter()
            .rev()
            .find(|r| r.stage == stage && r.revision == session.revision);
        let passage = session
            .sections
            .iter()
            .find(|s| s.method_stage == Some(stage))
            .or_else(|| {
                if stage == Stage::Significators {
                    session
                        .sections
                        .iter()
                        .find(|s| s.step == Some(Step::Significators))
                } else {
                    None
                }
            });
        let supplied: Vec<_> = record
            .into_iter()
            .flat_map(|r| {
                r.worksheet["checks"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .flat_map(|(_, c)| c["evidence"].as_array().into_iter().flatten())
            })
            .filter_map(Value::as_str)
            .collect();
        let selected: Vec<_> = facts
            .iter()
            .filter(|f| {
                supplied.contains(&f.id.as_str())
                    || passage.is_some_and(|s| s.evidence.contains(&f.id))
            })
            .cloned()
            .collect();
        let value = task(
            session,
            runtime,
            Stage::Explanation,
            json!({"brief":session.method.brief,"focus":focus,"prior_worksheet":record.map(|r|&r.worksheet).or_else(||passage.map(|s|&s.worksheet)),"prior_assignment":passage.map(|s|&s.roles),"facts":selected}),
            &selected,
        )?;
        session.messages.push(Message {
            role: "assistant".into(),
            text: value["summary"].as_str().unwrap_or("").into(),
        });
        return Ok(());
    }
    // Place lookup and moment sufficiency have no dependency on each other.
    // Civil-time resolution waits for the selected place's actual time zone.
    let b = &session.method.brief;
    let (place_result, moment_result) = std::thread::scope(|scope| {
        let place =
            scope.spawn(|| native_place(b, &session.candidates, session.place.as_ref(), geocode));
        let moment=scope.spawn(|| json!({"mode":if b.time_request.is_empty(){if session.chart.is_some(){"keep"}else{"now"}}else{"needs_civil_time"},"local_time":"","occurrence":"","clarification":"","basis":"The understood question uses the recorded receipt instant; the same matter keeps its chart."}));
        // Keep the two arms typed separately below; no chart mutation occurs
        // until both branches have joined successfully.
        (
            place.join().map_err(|_| "Place check interrupted"),
            moment.join().map_err(|_| "Moment check interrupted"),
        )
    });
    let (mut place, candidates) = place_result.map_err(str::to_string)??;
    let mut moment = moment_result.map_err(str::to_string)?;
    let mut place_inquiry = None;
    if place.is_none() && !candidates.is_empty() {
        let value = task(
            session,
            runtime,
            Stage::Place,
            json!({"brief":session.method.brief,"candidates":candidates}),
            &[],
        )?;
        if value["mode"] == "select" {
            place = candidates
                .iter()
                .find(|p| Some(p.id.as_str()) == value["place_id"].as_str())
                .cloned();
            if place.is_none() {
                return Err("Choose a supplied place candidate ID.".into());
            }
        } else if value["mode"] == "lookup" {
            let query = value["query"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("A place lookup needs the stated location.")?;
            let narrowed = Brief {
                place_request: query.into(),
                ..Default::default()
            };
            place = native_place(&narrowed, &[], None, geocode)?.0;
        }
        if place.is_none() {
            place_inquiry = value["clarification"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
        }
    }
    if moment["mode"] == "needs_civil_time" {
        let zone = place.as_ref().map(|p| p.timezone.as_str());
        let clock = zone
            .map(|z| horary_ai_core::chart_input::local_clock(instant, z))
            .transpose()?;
        moment = task(
            session,
            runtime,
            Stage::Moment,
            json!({"requested_question_moment":session.method.brief.time_request,"question_context":session.method.brief.context,"selected_timezone":zone,"current_local_clock":clock,"existing_chart_moment":session.chart.as_ref().map(|c|&c["timestampMs"])}),
            &[],
        )?;
    }
    let mut inquiries = Vec::new();
    if place.is_none() {
        inquiries.push(
            place_inquiry
                .as_deref()
                .unwrap_or("Where are you asking from? A city and country will do."),
        );
    }
    if moment["mode"] == "ask" {
        inquiries.push(
            moment["clarification"]
                .as_str()
                .unwrap_or("When did the question become clear to you?"),
        );
    }
    if !inquiries.is_empty() {
        session.messages.push(Message {
            role: "assistant".into(),
            text: inquiries.join(" "),
        });
        return Ok(());
    }
    let place = place.ok_or("A chart needs its reader's place")?;
    let default_instant = if session.method.brief.time_request.is_empty() {
        session.candidate_moment_ms.unwrap_or(instant)
    } else {
        instant
    };
    let timestamp = match resolve_moment(
        &moment,
        session.chart.as_ref(),
        default_instant,
        &place.timezone,
    ) {
        Ok(t) => t,
        Err(error) => {
            session
                .audit
                .push(json!({"event":"moment_validation","error":error}));
            session.messages.push(Message{role:"assistant".into(),text:if error.contains("occurs twice"){"That clock time happened twice as the clocks changed. Do you mean the earlier occurrence or the later one?"}else if error.contains("does not exist"){"The clocks skipped that time. What time before or after the change should I use?"}else{"What date and local time should this earlier question use?"}.into()});
            return Ok(());
        }
    };
    if session.chart.is_none()
        || session.method.brief.intent == "correct"
        || moment["mode"] == "explicit"
        || session
            .place
            .as_ref()
            .is_some_and(|old| old.latitude != place.latitude || old.longitude != place.longitude)
    {
        let chart = horary_ai_core::astronomy::chart(timestamp, place.latitude, place.longitude)?;
        if session.chart.is_some() {
            session.revisions.push(crate::conversation::Revision {
                number: session.revision,
                question: previous.question.clone(),
                chart: session.chart.clone(),
                sections: session.sections.clone(),
                place: session.place.clone(),
                brief: previous.method.brief.clone(),
            });
        }
        session.revision = session
            .revision
            .checked_add(1)
            .ok_or("Reading revision exhausted")?;
        session.chart = Some(chart);
        session.place = Some(place);
        session.chart_after_message = session.messages.len();
        session.sections.clear();
        session.status = "The sky at this question’s moment…".into();
        runtime.publish(session)?;
    }
    session.sections.clear();
    let chart = session.chart.as_ref().ok_or("A chart is required")?.clone();
    let all = reading_method::facts(Some(&chart));
    let houses: Vec<_> = all
        .iter()
        .filter(|f| f.kind == "house" || f.kind == "position")
        .cloned()
        .collect();
    let choices = task(
        session,
        runtime,
        Stage::Significators,
        json!({"brief":session.method.brief,"house_rulers_and_positions":houses}),
        &houses,
    )?;
    let roles = reading_method::assign(
        &chart,
        serde_json::from_value::<Vec<RoleChoice>>(choices["roles"].clone())
            .map_err(|e| e.to_string())?,
    )?;
    if session.method.brief.matter == Matter::LostObject {
        let owner = choices["owner_house"].as_u64().unwrap_or(1);
        let expected = if owner == 1 {
            vec![2, 4]
        } else {
            vec![(owner + 12) % 12 + 1]
        };
        let actual: Vec<_> = choices["object_candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        if actual != expected {
            return Err("Compare Lords 2 and 4 for the querent's object; use another owner's turned second.".into());
        }
    }
    section(
        session,
        Stage::Significators,
        &choices,
        &houses,
        roles.clone(),
    );
    runtime.publish(session)?;
    let condition = relevant(&all, &roles, &["condition", "position", "boundary"]);
    let reception = relevant(&all, &roles, &["reception", "boundary"]);
    let events = relevant(&all, &roles, &["event", "moon", "boundary"]);
    let positions = relevant(&all, &roles, &["position", "boundary"]);
    let matter = session.method.brief.matter;
    let common = json!({"brief":session.method.brief,"roles":roles});
    session.status = "Following feeling, possibility, and the paths between them…".into();
    runtime.publish(session)?;
    // Independent judgments are submitted together. The resident model owns
    // inference scheduling; it does not load duplicate copies of the weights.
    let mut tasks = vec![
        (
            Stage::Condition,
            matter,
            json!({"question":common,"facts":condition}),
            schema(Stage::Condition, &condition),
        ),
        (
            Stage::Reception,
            matter,
            json!({"question":common,"facts":reception}),
            schema(Stage::Reception, &reception),
        ),
        (
            Stage::Contacts,
            matter,
            json!({"brief":session.method.brief,"roles":roles,"facts":events}),
            schema(Stage::Contacts, &events),
        ),
    ];
    if matches!(matter, Matter::LostObject | Matter::LostAnimal) {
        tasks.push((
            Stage::Location,
            matter,
            json!({"brief":session.method.brief,"roles":roles,"facts":positions}),
            schema(Stage::Location, &positions),
        ));
    }
    let batch = runtime.generate_batch(&tasks)?;
    if batch.len() != tasks.len() {
        return Err("An independent task is missing from the batch.".into());
    }
    let mut values = Vec::new();
    for ((stage, matter, input, contract), generation) in tasks.into_iter().zip(batch) {
        let selected = match stage {
            Stage::Condition => &condition,
            Stage::Reception => &reception,
            Stage::Contacts => &events,
            _ => &positions,
        };
        let parsed = decode_json(&generation.content);
        let validation_error = parsed
            .as_ref()
            .map_err(|e| e.clone())
            .and_then(|v| validate_for(stage, matter, v, selected))
            .err();
        let mut worksheet = parsed.unwrap_or(Value::Null);
        keep(
            session,
            runtime,
            Record {
                stage,
                revision: session.revision,
                guide_sha256: lessons::digest(&lessons::guide(stage, matter)?),
                schema_sha256: lessons::digest(&contract.to_string()),
                input_sha256: lessons::digest(&input.to_string()),
                input: input.clone(),
                raw: generation.content.clone(),
                worksheet: worksheet.clone(),
                generation,
                source_passages: stage.passages(matter).iter().map(|s| (*s).into()).collect(),
                validation_error: validation_error.clone(),
            },
        )?;
        if let Some(error) = validation_error {
            worksheet = task(
                session,
                runtime,
                stage,
                json!({"original_input":input,"previous_worksheet":worksheet,"native_validation_error":error}),
                selected,
            )?;
        }
        values.push(worksheet);
    }
    let c = values[0].clone();
    let r = values[1].clone();
    let contact = values[2].clone();
    let location = values.get(3).cloned().unwrap_or(Value::Null);
    section(session, Stage::Condition, &c, &condition, Vec::new());
    section(session, Stage::Reception, &r, &reception, Vec::new());
    runtime.publish(session)?;
    section(session, Stage::Contacts, &contact, &events, Vec::new());
    if !location.is_null() {
        section(session, Stage::Location, &location, &positions, Vec::new());
    }
    // Current ephemeris supplies brackets, not the applying planet's travel
    // to exact perfection. Skip speculative timing inference entirely.
    let timing = json!({"timing_status":"unestablished","reason":"The native calculation has not supplied travel to exact perfection. Astronomical hours are not symbolic calendar timing."});
    session
        .audit
        .push(json!({"event":"native_stage","stage":"timing","result":timing}));
    let facts = relevant(
        &all,
        &roles,
        &[
            "condition",
            "reception",
            "event",
            "position",
            "moon",
            "boundary",
        ],
    );
    let judgment = task(
        session,
        runtime,
        Stage::Judgment,
        json!({"brief":session.method.brief,"roles":roles,"condition":c,"reception":r,"contacts":contact,"location":location,"timing":timing,"facts":facts}),
        &facts,
    )?;
    section(session, Stage::Judgment, &judgment, &facts, Vec::new());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_defaults_do_not_require_geocoding_or_invent_coordinates() {
        let device = LocationCandidate {
            id: "device".into(),
            label: "Here".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.65,
            longitude: -77.24,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        let geo = GeocodeState::default();
        assert_eq!(
            native_place(&Brief::default(), std::slice::from_ref(&device), None, &geo)
                .unwrap()
                .0
                .unwrap()
                .id,
            "device"
        );
        assert!(native_place(&Brief::default(), &[], None, &geo)
            .unwrap()
            .0
            .is_none());
        let explicit = Brief {
            place_request: "London, United Kingdom".into(),
            ..Default::default()
        };
        let (_, found) = native_place(&explicit, &[device], None, &geo).unwrap();
        assert!(found.iter().all(|p| p.provider != "device"));
    }
    #[test]
    fn fabricated_evidence_and_empty_answer_are_not_valid_worksheets() {
        let v = json!({"checks":{},"summary":"Fine","unknowns":[],"answer":"Yes","evidence":["invented"],"verdict":"likely_yes"});
        assert!(validate(Stage::Judgment, &v, &[]).is_err());
    }
    #[test]
    fn contact_sign_checks_cannot_change_native_event_conditions() {
        let chart = json!({"derived":{"eventSearch":{"events":[{"planet1":"Moon","planet2":"Venus","aspectName":"Sextile","withinCurrentSigns":false,"estimatedPerfectsWithinHours":123.}]}}});
        let facts = reading_method::facts(Some(&chart));
        let event = facts.iter().find(|f| f.kind == "event").unwrap();
        let checks = Stage::Contacts.checks().iter().map(|key|((*key).to_string(),json!({"state":"unestablished","evidence":[],"finding":"The contact needs contextual judgment."}))).collect::<serde_json::Map<_,_>>();
        let mut worksheet = json!({"checks":checks,"summary":"A candidate after a sign change.","unknowns":[],"basis":"direct_candidate","candidate_ids":[event.id],"candidate_signs":[{"id":event.id,"within_current_signs":true}]});
        assert!(validate(Stage::Contacts, &worksheet, &facts)
            .unwrap_err()
            .contains("sign-change"));
        worksheet["candidate_signs"][0]["within_current_signs"] = json!(false);
        validate(Stage::Contacts, &worksheet, &facts).unwrap();
        worksheet["candidate_ids"] = json!([event.id, event.id]);
        worksheet["candidate_signs"] = json!([
            worksheet["candidate_signs"][0].clone(),
            worksheet["candidate_signs"][0].clone()
        ]);
        assert!(validate(Stage::Contacts, &worksheet, &facts).is_err());
    }
    #[test]
    fn relationship_role_contract_has_no_lost_object_fields() {
        let contract = schema_for(Stage::Significators, Matter::Relationship, &[]);
        assert!(contract["properties"].get("object_candidates").is_none());
        assert!(contract["properties"].get("owner_house").is_none());
        assert!(
            schema_for(Stage::Significators, Matter::LostObject, &[])["properties"]
                .get("object_candidates")
                .is_some()
        );
    }
    #[test]
    fn scheduler_submits_contact_checks_with_condition_and_reception() {
        let requests = process_examples().unwrap();
        let examples = requests["examples"].as_array().unwrap();
        for stage in ["condition", "reception", "contacts"] {
            let task = examples.iter().find(|v| v["stage"] == stage).unwrap();
            assert_eq!(
                task["batchStages"],
                json!(["condition", "reception", "contacts"])
            );
        }
    }
    #[test]
    fn delayed_city_clarification_keeps_the_understood_question_moment() {
        let evidence = process_examples_with_place(false).unwrap();
        assert_eq!(evidence["chartMomentMs"], json!(1789387200000.));
        assert_eq!(
            evidence["canonicalQuestion"],
            "Will I get married in the next year?"
        );
        assert_eq!(evidence["horizon"], "within the next year");
        assert!(evidence["examples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["messages"][1]["content"]
                .as_str()
                .unwrap_or("")
                .contains("Woodbridge, Virginia, United States.")));
    }
    #[test]
    fn prompt_does_not_carry_unrelated_history_or_lessons() {
        let input = json!({"requested_question_moment":"2026-01-14 14:30"});
        let p = prompt(
            Stage::Moment,
            Matter::Work,
            &input,
            &schema(Stage::Moment, &[]),
        )
        .unwrap();
        assert!(p.contains("2026-01-14"));
        assert!(!p.contains("reception_triplicity"));
        assert!(!p.contains("Lord 7 signifies"));
    }
    #[test]
    fn native_moments_distinguish_receipt_existing_and_user_civil_time() {
        let old = json!({"timestampMs":42.});
        assert_eq!(
            resolve_moment(&json!({"mode":"now"}), None, 100., "America/New_York").unwrap(),
            100.
        );
        assert_eq!(
            resolve_moment(
                &json!({"mode":"keep"}),
                Some(&old),
                100.,
                "America/New_York"
            )
            .unwrap(),
            42.
        );
        let explicit = json!({"mode":"explicit","local_time":"2026-01-14T14:30","occurrence":""});
        let london = resolve_moment(&explicit, None, 100., "Europe/London").unwrap();
        let new_york = resolve_moment(&explicit, None, 100., "America/New_York").unwrap();
        assert_eq!(new_york - london, 5. * 60. * 60. * 1000.);
        assert!(resolve_moment(
            &json!({"mode":"explicit","local_time":"2026-03-08T02:30"}),
            None,
            100.,
            "America/New_York"
        )
        .is_err());
        assert!(resolve_moment(
            &json!({"mode":"explicit","local_time":"2026-11-01T01:30","occurrence":""}),
            None,
            100.,
            "America/New_York"
        )
        .is_err());
    }
    #[test]
    fn duplicate_or_unframed_junk_cannot_be_a_tool_worksheet() {
        assert!(decode_json(r#"{"mode":"now","mode":"explicit"}"#).is_err());
        assert!(decode_json("```json\n{\"mode\":\"now\"}\n```\nextra").is_err());
        assert_eq!(
            decode_json("```json\n{\"mode\":\"now\"}\n```").unwrap()["mode"],
            "now"
        );
    }
    #[test]
    fn actual_scheduler_shortcuts_defaults_and_delivers_the_answer() {
        let fixture = process_examples().unwrap();
        let calls = fixture["examples"].as_array().unwrap();
        assert!(!calls
            .iter()
            .any(|r| r["stage"] == "place" || r["stage"] == "moment"));
        assert!(calls.iter().any(|r| r["stage"] == "judgment"));
        assert!(fixture["visibleProposedAnswer"]
            .as_str()
            .unwrap()
            .contains("main document"));
    }
}

#[cfg(test)]
mod restoration_tests {
    use super::*;
    #[test]
    fn restoring_a_listed_revision_keeps_both_chart_histories() {
        let mut session = Session {
            revision: 2,
            question: "Corrected question".into(),
            chart: Some(json!({"timestampMs":20})),
            revisions: vec![crate::conversation::Revision {
                number: 1,
                question: "Earlier question".into(),
                chart: Some(json!({"timestampMs":10})),
                sections: Vec::new(),
                place: None,
                brief: Brief {
                    question: "Earlier question".into(),
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        assert!(restore_revision(&mut session, 99).is_err());
        assert_eq!(session.revision, 2);
        restore_revision(&mut session, 1).unwrap();
        assert_eq!(session.chart.as_ref().unwrap()["timestampMs"], 10);
        assert_eq!(session.question, "Earlier question");
        assert_eq!(session.revision, 3);
        assert!(session
            .revisions
            .iter()
            .any(|r| r.number == 2 && r.chart.as_ref().unwrap()["timestampMs"] == 20));
    }
}

#[cfg(all(test, feature = "native-llama"))]
mod real_reading {
    use super::*;
    use crate::native_llama_worker::{
        generate_native, generate_native_batch, start_native_llama_from_path, stop_native_llama,
        NativeGenerateOptions, NativeLlamaState,
    };
    use std::sync::Mutex;
    struct Reader {
        state: NativeLlamaState,
        dir: std::path::PathBuf,
        records: Mutex<Vec<Value>>,
    }
    impl Runtime for Reader {
        fn generate(
            &self,
            stage: Stage,
            matter: Matter,
            input: &Value,
            contract: &Value,
            audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            let prompt = prompt(stage, matter, input, contract)?;
            let result = generate_native(
                &self.state,
                prompt.clone(),
                NativeGenerateOptions {
                    max_tokens: if stage == Stage::Judgment { 1400 } else { 1000 },
                    temperature: 0.,
                    cache_lesson: audio.is_none(),
                    audio: audio.map(<[u8]>::to_vec),
                    response_schema: audio.is_none().then(|| contract.to_string()),
                    ..Default::default()
                },
            )
            .map_err(|e| e.message);
            self.records.lock().unwrap().push(json!({"stage":stage,"prompt":serde_json::from_str::<Value>(&prompt).unwrap(),"result":result.as_ref().map_err(|e|e.as_str())}));
            std::fs::write(
                self.dir.join("calls.json"),
                serde_json::to_vec_pretty(&*self.records.lock().unwrap()).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            result
        }
        fn generate_batch(
            &self,
            tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            let prompts = tasks
                .iter()
                .map(|(stage, matter, input, contract)| {
                    prompt(*stage, *matter, input, contract).map(|p| (p, 1000))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let result = generate_native_batch(
                &self.state,
                prompts,
                NativeGenerateOptions {
                    temperature: 0.,
                    cache_lesson: true,
                    ..Default::default()
                },
            )
            .map_err(|e| e.message);
            self.records.lock().unwrap().push(json!({"batchStages":tasks.iter().map(|t|t.0).collect::<Vec<_>>(),"inputs":tasks,"result":result.as_ref().map_err(|e|e.as_str())}));
            std::fs::write(
                self.dir.join("calls.json"),
                serde_json::to_vec_pretty(&*self.records.lock().unwrap()).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            result
        }
        fn publish(&self, session: &mut Session) -> Result<(), String> {
            crate::reading_store::write(&self.dir.join("reading.json"), session, false)
        }
        fn check(&self) -> Result<(), String> {
            Ok(())
        }
        fn directory(&self) -> &Path {
            &self.dir
        }
    }
    #[test]
    #[ignore = "Actual full Gemma reading over synthetic question/device data; preserves failures in HORARY_READING_EVIDENCE"]
    fn real_reader_reaches_an_interpreted_answer_with_device_defaults() {
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        let dir = std::path::PathBuf::from(
            std::env::var_os("HORARY_READING_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&dir).expect("Preserve previous attempts");
        let reader = Reader {
            state: NativeLlamaState::default(),
            dir,
            records: Mutex::new(Vec::new()),
        };
        start_native_llama_from_path(
            &reader.state,
            "reading-probe".into(),
            String::new(),
            path.into(),
            std::path::PathBuf::new(),
            serde_json::from_value(
                json!({"modelId":"reading-probe","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let question = "Will I get married in the next year?";
        let seed = Session {
            reading_id: "synthetic-reading".into(),
            messages: vec![Message {
                role: "user".into(),
                text: question.into(),
            }],
            ..Default::default()
        };
        let device = LocationCandidate {
            id: "device-location".into(),
            label: "Near Woodbridge".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        };
        // Keep the SAME native owner across both complete readings. A restarted
        // owner cannot establish a warm lesson-bank result.
        for phase in ["cold", "warm"] {
            let mut session = seed.clone();
            session.candidates.push(device.clone());
            let previous = session.clone();
            let start = std::time::Instant::now();
            let result = run(
                &mut session,
                &reader,
                &GeocodeState::default(),
                1789387200000.,
                None,
                &previous,
            );
            std::fs::write(reader.dir.join(format!("completion-{phase}.json")),serde_json::to_vec_pretty(&json!({"authorship":"Actual Gemma generation over explicitly synthetic device/question/moment; not Eileen's reading","phase":phase,"result":result,"wallMs":start.elapsed().as_millis(),"session":session})).unwrap()).unwrap();
            if result.is_err() {
                stop_native_llama(&reader.state).unwrap();
            }
            result.unwrap();
            assert!(session.chart.is_some());
            assert_eq!(session.place.as_ref().unwrap().id, "device-location");
            assert_eq!(session.question, question);
            assert!(!session
                .method
                .records
                .iter()
                .any(|r| matches!(r.stage, Stage::Place | Stage::Moment)));
            let answer = session
                .sections
                .iter()
                .find(|s| s.step == Some(Step::Judgment))
                .expect("The person must receive an interpretation");
            assert!(answer.body.len() > 70);
            assert_eq!(answer.body, answer.worksheet["answer"].as_str().unwrap());
            if phase == "warm" {
                assert!(session
                    .method
                    .records
                    .iter()
                    .all(|r| r.generation.cached_prompt_tokens > 0));
            }
            eprintln!(
                "FULL READING {phase}: wall_ms={} stages={} answer={}",
                start.elapsed().as_millis(),
                session.method.records.len(),
                answer.body
            );
        }
        stop_native_llama(&reader.state).unwrap();
    }
}

/// An authored fixture drives the real scheduler and captures its exact inputs.
/// It deliberately supplies no model judgment or qualification claim.
#[cfg(test)]
pub(crate) fn process_examples() -> Result<Value, String> {
    process_examples_with_place(true)
}

#[cfg(test)]
fn process_examples_with_place(device_available: bool) -> Result<Value, String> {
    use std::sync::Mutex;
    struct Fixture {
        dir: tempfile::TempDir,
        requests: Mutex<Vec<Value>>,
    }
    impl Runtime for Fixture {
        fn generate(
            &self,
            stage: Stage,
            matter: Matter,
            input: &Value,
            contract: &Value,
            _audio: Option<&[u8]>,
        ) -> Result<NativeGenerationResult, String> {
            self.requests.lock().unwrap().push(json!({"stage":stage,"messages":serde_json::from_str::<Value>(&prompt(stage,matter,input,contract)?).map_err(|e|e.to_string())?,"responseSchema":contract}));
            let answer = match stage {
                Stage::Intake => {
                    json!({"intent":"read","question":"Will I get married in the next year?","matter":"relationship","question_kind":"event","context":"No particular partner is named.","place_request":if input["latest_words"] == "Woodbridge, Virginia, United States." {"Woodbridge, Virginia, United States."}else{""},"time_request":"","horizon":"within the next year","clarification":"","focus":"judgment","heard":"","restore_revision":null})
                }
                Stage::Significators => {
                    json!({"roles":[{"label":"Querent","house":1,"natural":null,"reason":"The first house represents the person asking."},{"label":"Prospective partner","house":7,"natural":null,"reason":"The seventh house represents a prospective partner."},{"label":"Feelings","house":null,"natural":"Moon","reason":"The Moon has a contextual role in the question."}],"summary":"Authored fixture: identifies roles to inspect subsequent requests.","unknowns":[]})
                }
                Stage::Place => {
                    let place = input["candidates"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|p| {
                            p["latitude"]
                                .as_f64()
                                .is_some_and(|n| (38.0..39.0).contains(&n))
                        })
                        .ok_or("The authored Virginia fixture needs a real candidate")?;
                    json!({"mode":"select","place_id":place["id"],"query":"","clarification":"","basis":"Authored fixture selects the real Virginia candidate, not invented coordinates."})
                }
                _ => {
                    let mut checks = serde_json::Map::new();
                    for key in stage.checks() {
                        checks.insert((*key).into(),json!({"state":"unestablished","evidence":[],"finding":"Authored fixture, not a model assessment."}));
                    }
                    let mut answer = json!({"checks":checks,"summary":"Authored fixture; no astrology interpretation is asserted.","unknowns":[]});
                    if stage == Stage::Contacts {
                        answer["basis"] = json!("no_candidate_covered");
                        answer["candidate_ids"] = json!([]);
                        answer["candidate_signs"] = json!([]);
                    }
                    if stage == Stage::Judgment {
                        answer["verdict"] = json!("unresolved");
                        answer["answer"]=json!("This is authored fixture text showing that the proposed answer reaches the main document; it is not a model judgment.");
                        answer["evidence"] = json!([]);
                        answer["checks"]["scope_of_answer"]["evidence"] = json!([input["facts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|f| f["kind"] == "boundary")
                            .ok_or("Fixture boundary missing")?["id"]]);
                    }
                    answer
                }
            };
            Ok(NativeGenerationResult {
                content: answer.to_string(),
                prompt_tokens: 0,
                generated_tokens: 0,
                elapsed_ms: 0,
                total_wall_ms: None,
                batch_size: None,
                lesson_bank_hit: None,
                lesson_prepare_ms: None,
                tokens_per_second: 0.,
                prompt_cache_hit: false,
                cached_prompt_tokens: 0,
                prefilled_prompt_tokens: 0,
                first_token_ms: None,
                cold_cache_bytes: None,
            })
        }
        fn publish(&self, _session: &mut Session) -> Result<(), String> {
            Ok(())
        }
        fn generate_batch(
            &self,
            tasks: &[(Stage, Matter, Value, Value)],
        ) -> Result<Vec<NativeGenerationResult>, String> {
            let results = tasks
                .iter()
                .map(|(stage, matter, input, contract)| {
                    self.generate(*stage, *matter, input, contract, None)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let stages: Vec<_> = tasks.iter().map(|t| t.0).collect();
            let mut requests = self.requests.lock().map_err(|e| e.to_string())?;
            let start = requests
                .len()
                .checked_sub(tasks.len())
                .ok_or("Missing batch fixture requests")?;
            for request in &mut requests[start..] {
                request["batchStages"] = json!(stages);
            }
            Ok(results)
        }
        fn check(&self) -> Result<(), String> {
            Ok(())
        }
        fn directory(&self) -> &Path {
            self.dir.path()
        }
    }
    let runtime = Fixture {
        dir: tempfile::tempdir().map_err(|e| e.to_string())?,
        requests: Mutex::new(Vec::new()),
    };
    let mut session = Session::default();
    session.messages.push(Message {
        role: "user".into(),
        text: "Will I get married in the next year?".into(),
    });
    if device_available {
        session.candidates.push(LocationCandidate {
            id: "device-location".into(),
            label: "Near Woodbridge".into(),
            name: "Woodbridge".into(),
            country: "US".into(),
            latitude: 38.657,
            longitude: -77.249,
            timezone: "America/New_York".into(),
            provider: "device".into(),
        });
    }
    let previous = session.clone();
    run(
        &mut session,
        &runtime,
        &GeocodeState::default(),
        1789387200000.,
        None,
        &previous,
    )?;
    if !device_available {
        let previous = session.clone();
        session.messages.push(Message {
            role: "user".into(),
            text: "Woodbridge, Virginia, United States.".into(),
        });
        run(
            &mut session,
            &runtime,
            &GeocodeState::default(),
            1789387800000.,
            None,
            &previous,
        )?;
    }
    let mut requests = runtime.requests.into_inner().map_err(|e| e.to_string())?;
    requests.sort_by_key(|r| {
        Stage::ALL
            .iter()
            .position(|s| serde_json::to_value(s).unwrap() == r["stage"])
    });
    Ok(
        json!({"authorship":"Synthetic inputs and authored worksheet outputs, captured from the actual runtime scheduler; no model invoked.","examples":requests,"visibleProposedAnswer":session.sections.last().map(|s|&s.body),"chartMomentMs":session.chart.as_ref().map(|c|&c["timestampMs"]),"canonicalQuestion":session.question,"horizon":session.method.brief.horizon,"nativeDefaults":{"place":if device_available{"device coordinates"}else{"geocoded stated city"},"moment":"1789387200000, understood question receipt instant","noPlaceOrMomentModelCall":true}}),
    )
}
