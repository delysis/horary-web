#![forbid(unsafe_code)]

pub mod astronomy;
pub mod book_method;
pub mod chart_input;
mod ephemeris_coefficients;
pub mod events;
pub mod review;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

pub const INTERPRETATION_SCHEMA_VERSION: &str = "2026-09-14.2";
pub const INTERPRETATION_PROMPT_VERSION: &str = "horary-interpretation-v7";
pub const INTERPRETATION_TRADITION_PROFILE: &str = "traditional-horary-textbook-v1";
pub const MAX_INTERPRETATION_TOKENS: u32 = 3072;
pub const INTERPRETATION_TEMPERATURE: f32 = 0.0;

const INTERPRETATION_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/ai/horary-interpretation.schema.json"
));
const JUDGEMENT_PIPELINE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/ai/horary-judgement-pipeline.json"
));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiError {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HoraryInterpretationRequest {
    pub question: String,
    pub chart: Value,
    pub settings: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HoraryInterpretation {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_answer: Option<String>,
    pub confidence: String,
    #[serde(default)]
    pub judgement_trace: Vec<InterpretationTraceStep>,
    pub key_factors: Vec<InterpretationKeyFactor>,
    #[serde(default)]
    pub cautions: Vec<String>,
    #[serde(default)]
    pub follow_up_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationKeyFactor {
    pub factor: String,
    pub chart_evidence: String,
    pub interpretation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationTraceStep {
    pub step_id: String,
    pub finding: String,
    pub chart_evidence: String,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationPrompt {
    pub prompt_version: &'static str,
    pub schema_version: &'static str,
    pub tradition_profile: &'static str,
    pub risk: String,
    pub response_format: Value,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

pub fn build_interpretation_prompt_json(request_json: &str) -> Result<String, AiError> {
    let request: HoraryInterpretationRequest =
        serde_json::from_str(request_json).map_err(|error| AiError {
            message: format!("invalid interpretation request JSON: {error}"),
        })?;
    let prompt = build_interpretation_prompt(&request)?;
    serde_json::to_string(&prompt).map_err(|error| AiError {
        message: format!("failed to serialize interpretation prompt: {error}"),
    })
}

pub fn parse_interpretation_content_json(content: &str) -> Result<String, AiError> {
    let interpretation = parse_interpretation_content(content)?;
    serde_json::to_string(&interpretation).map_err(|error| AiError {
        message: format!("failed to serialize normalized interpretation: {error}"),
    })
}

pub fn validate_interpretation_json(output_json: &str, risk: &str) -> Result<String, AiError> {
    let interpretation = parse_interpretation_content(output_json)?;
    validate_interpretation_for_risk(&interpretation, risk)?;
    serde_json::to_string(&interpretation).map_err(|error| AiError {
        message: format!("failed to serialize validated interpretation: {error}"),
    })
}

pub fn question_context_json(question: &str) -> Result<String, AiError> {
    serde_json::to_string(&question_context(question)).map_err(|error| AiError {
        message: format!("failed to serialize question context: {error}"),
    })
}

pub fn deterministic_assignments_json(
    question_context_json: &str,
    chart_evidence_index_json: &str,
) -> Result<String, AiError> {
    let question_context: Value =
        serde_json::from_str(question_context_json).map_err(|error| AiError {
            message: format!("invalid question context JSON: {error}"),
        })?;
    let chart_evidence_index: Value =
        serde_json::from_str(chart_evidence_index_json).map_err(|error| AiError {
            message: format!("invalid chart evidence index JSON: {error}"),
        })?;
    serde_json::to_string(&deterministic_assignments(
        &question_context,
        &chart_evidence_index,
    ))
    .map_err(|error| AiError {
        message: format!("failed to serialize deterministic assignments: {error}"),
    })
}

pub fn judgement_plan_json() -> Result<String, AiError> {
    serde_json::to_string(&judgement_plan()).map_err(|error| AiError {
        message: format!("failed to serialize judgement plan: {error}"),
    })
}

pub fn interpretation_schema_json() -> Result<String, AiError> {
    serde_json::to_string(&interpretation_schema()).map_err(|error| AiError {
        message: format!("failed to serialize interpretation schema: {error}"),
    })
}

#[cfg(feature = "wasm")]
fn wasm_error(error: AiError) -> JsValue {
    JsValue::from_str(&error.message)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = build_interpretation_prompt_json)]
pub fn wasm_build_interpretation_prompt_json(request_json: &str) -> Result<String, JsValue> {
    build_interpretation_prompt_json(request_json).map_err(wasm_error)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = parse_interpretation_content_json)]
pub fn wasm_parse_interpretation_content_json(content: &str) -> Result<String, JsValue> {
    parse_interpretation_content_json(content).map_err(wasm_error)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = validate_interpretation_json)]
pub fn wasm_validate_interpretation_json(output_json: &str, risk: &str) -> Result<String, JsValue> {
    validate_interpretation_json(output_json, risk).map_err(wasm_error)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = classify_question_risk)]
pub fn wasm_classify_question_risk(question: &str) -> String {
    classify_question_risk(question).to_string()
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = question_context_json)]
pub fn wasm_question_context_json(question: &str) -> Result<String, JsValue> {
    question_context_json(question).map_err(wasm_error)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = deterministic_assignments_json)]
pub fn wasm_deterministic_assignments_json(
    question_context: &str,
    chart_evidence_index: &str,
) -> Result<String, JsValue> {
    deterministic_assignments_json(question_context, chart_evidence_index).map_err(wasm_error)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = judgement_plan_json)]
pub fn wasm_judgement_plan_json() -> Result<String, JsValue> {
    judgement_plan_json().map_err(wasm_error)
}

#[cfg(feature = "wasm")]
#[wasm_bindgen(js_name = interpretation_schema_json)]
pub fn wasm_interpretation_schema_json() -> Result<String, JsValue> {
    interpretation_schema_json().map_err(wasm_error)
}

pub fn build_interpretation_prompt(
    req: &HoraryInterpretationRequest,
) -> Result<InterpretationPrompt, AiError> {
    if req.question.trim().is_empty() {
        return Err(AiError {
            message: "question is required for interpretation".to_string(),
        });
    }
    if !req.chart.is_object() {
        return Err(AiError {
            message: "chart facts must be a JSON object".to_string(),
        });
    }

    let risk = classify_question_risk(&req.question).to_string();
    let chart_facts = build_chart_facts(&req.chart);
    let question_context = question_context(&req.question);
    let chart_evidence_index = chart_evidence_index(&chart_facts);
    let deterministic_assignments =
        deterministic_assignments(&question_context, &chart_evidence_index);
    let user_payload = json!({
        "promptVersion": INTERPRETATION_PROMPT_VERSION,
        "schemaVersion": INTERPRETATION_SCHEMA_VERSION,
        "traditionProfile": INTERPRETATION_TRADITION_PROFILE,
        "question": req.question,
        "risk": risk,
        "questionContext": question_context,
        "chartEvidenceIndex": chart_evidence_index,
        "deterministicAssignments": deterministic_assignments,
        "chart": chart_facts,
        "settings": req.settings.clone().unwrap_or_else(|| json!({})),
        "judgementPlan": judgement_plan(),
    });

    let system_content = [
        "You interpret horary astrology charts from supplied deterministic chart facts.".to_string(),
        "Do not calculate or recalculate planetary positions, house cusps, aspects, dignity, or timing.".to_string(),
        "Use only the chart facts provided in the user message as evidence.".to_string(),
        "Use supplied questionContext house hints to scaffold house assignment; if you depart from a hint, explain the supplied chart fact or wording that forced the departure.".to_string(),
        "The actor-to-house suggestions in deterministicAssignments are provisional keyword hints, not expert decisions. Check them against the actual question; explain departures and ask for clarification when the actor or topic is ambiguous. The supplied house rulers and body placements are facts, not choices.".to_string(),
        "For chartEvidence fields, copy or semicolon-combine short phrases from chartEvidenceIndex.canonicalFacts or deterministicAssignments.canonicalEvidence. Never cite raw JSON, a label alone, or an invented body placement.".to_string(),
        traditional_horary_doctrine().to_string(),
        "Timing: astronomical hours until an aspect are NOT the event's predicted time. If you give hours, days, weeks, months or years, include a timing_pass explaining the supplied distance to perfection, the plausible unit range, and why sign, house and volition select that unit. Otherwise omit the forecast and say timing is not established. A small display orb alone does not mean soon.".to_string(),
        "Lost objects (Frawley pp. 146-149): compare rulers of the 2nd and 4th for the querent's inanimate object; choose the better description and keep one primary location significator. For another person's object use their turned 2nd. The Moon may represent the lost object when applying to Lord 1 for recovery, or the querent when applying to the object's lord: explicitly name its role in that testimony. Do not confuse this role switch with two querent significators meeting. Detriment or fall does not automatically mean the object is damaged. Location symbols are possibilities to test against the actual surroundings, not known rooms or facts.".to_string(),
        "Follow the supplied judgementPlan step by step and include a compact judgementTrace showing the decisive steps.".to_string(),
        "The judgementTrace is an audit trail: each item must name a stepId, a finding, concrete chartEvidence, and confidence. A finding must explain what the step found in a short sentence; never repeat its stepId or merely name a stage.".to_string(),
        "Tie every key factor to a concrete chart fact.".to_string(),
        "Keep confidence humble and evidence-based.".to_string(),
        interpretation_output_guardrails().to_string(),
        format!("Prompt version: {INTERPRETATION_PROMPT_VERSION}."),
        format!("Output schema version: {INTERPRETATION_SCHEMA_VERSION}."),
        format!("Tradition profile: {INTERPRETATION_TRADITION_PROFILE}."),
        high_stakes_policy(&risk).to_string(),
        "Return only JSON matching the supplied schema.".to_string(),
    ]
    .join("\n");

    Ok(InterpretationPrompt {
        prompt_version: INTERPRETATION_PROMPT_VERSION,
        schema_version: INTERPRETATION_SCHEMA_VERSION,
        tradition_profile: INTERPRETATION_TRADITION_PROFILE,
        risk: risk.clone(),
        response_format: json!({
            "type": "json_schema",
            "json_schema": {
                "name": "horary_interpretation",
                "strict": true,
                "schema": interpretation_schema(),
            },
        }),
        messages: vec![
            ChatMessage {
                role: "system",
                content: system_content,
            },
            ChatMessage {
                role: "user",
                content: serde_json::to_string(&user_payload).map_err(|error| AiError {
                    message: format!("failed to serialize interpretation prompt: {error}"),
                })?,
            },
        ],
    })
}

pub fn native_prompt_text(prompt: &InterpretationPrompt) -> Result<String, AiError> {
    serde_json::to_string(&prompt.messages).map_err(|e| AiError {
        message: e.to_string(),
    })
}

pub fn classify_question_risk(question: &str) -> &'static str {
    let text = question.to_ascii_lowercase();
    if text.contains("urgent danger") {
        return "high_stakes";
    }

    if text
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(is_high_stakes_token)
    {
        "high_stakes"
    } else {
        "ordinary"
    }
}

pub fn parse_interpretation_content(content: &str) -> Result<HoraryInterpretation, AiError> {
    let candidate = strip_code_fence(content.trim());
    parse_interpretation_json(candidate)
        .or_else(|_| extract_json_object(content).and_then(parse_interpretation_json))
}

fn parse_interpretation_json(content: &str) -> Result<HoraryInterpretation, AiError> {
    let mut value = parse_interpretation_value(content)?;
    normalize_interpretation_value(&mut value);
    let interpretation: HoraryInterpretation =
        serde_json::from_value(value).map_err(|error| AiError {
            message: format!("local model returned invalid interpretation JSON: {error}"),
        })?;
    validate_interpretation_shape(&interpretation)?;
    Ok(interpretation)
}

fn parse_interpretation_value(content: &str) -> Result<Value, AiError> {
    match serde_json::from_str(content) {
        Ok(value) => Ok(value),
        Err(original_error) => {
            let repaired = repair_common_json_punctuation(content);
            match serde_json::from_str(&repaired) {
                Ok(value) => Ok(value),
                Err(repair_error) => extract_json_object(&repaired)
                    .and_then(|balanced| {
                        serde_json::from_str(balanced).map_err(|balanced_error| AiError {
                            message: format!(
                                "local model returned invalid interpretation JSON: {original_error}; repair failed: {repair_error}; balanced repair also failed: {balanced_error}"
                            ),
                        })
                    }),
            }
        }
    }
}

fn normalize_interpretation_value(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.contains_key("summary") {
        if let Some(direct_answer) = object.get("directAnswer").and_then(Value::as_str) {
            object.insert(
                "summary".to_string(),
                Value::String(direct_answer.to_string()),
            );
        }
    }
    if !object.contains_key("directAnswer") {
        if let Some(summary) = object.get("summary").and_then(Value::as_str) {
            object.insert(
                "directAnswer".to_string(),
                Value::String(summary.to_string()),
            );
        }
    }
    if let Some(trace) = object.get_mut("judgementTrace") {
        normalize_judgement_trace(trace);
        normalize_repeated_semicolon_evidence(trace, &["chartEvidence"]);
    }
    if let Some(key_factors) = object.get_mut("keyFactors") {
        normalize_key_factors(key_factors);
        normalize_repeated_semicolon_evidence(key_factors, &["factor", "chartEvidence"]);
    }
    if let Some(cautions) = object.get_mut("cautions").and_then(Value::as_array_mut) {
        cautions.truncate(3);
    }
    if let Some(follow_up_questions) = object
        .get_mut("followUpQuestions")
        .and_then(Value::as_array_mut)
    {
        follow_up_questions.truncate(3);
    }
}

fn normalize_judgement_trace(value: &mut Value) {
    let Value::Array(steps) = value else {
        return;
    };
    for step in steps {
        let Value::Object(step_object) = step else {
            continue;
        };
        if step_object
            .get("stepId")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            if let Some(step_id) = step_object.get("step").and_then(Value::as_str) {
                step_object.insert("stepId".to_string(), Value::String(step_id.to_string()));
            }
        }
    }
}

fn normalize_key_factors(value: &mut Value) {
    if let Value::Object(map) = value {
        let normalized = map
            .iter()
            .map(|(factor_name, factor_value)| {
                let mut factor = factor_value.clone();
                if let Value::Object(factor_object) = &mut factor {
                    factor_object
                        .entry("factor".to_string())
                        .or_insert_with(|| Value::String(factor_name.clone()));
                }
                factor
            })
            .collect::<Vec<_>>();
        *value = Value::Array(normalized);
    }

    let Value::Array(items) = value else {
        return;
    };
    for (index, item) in items.iter_mut().enumerate() {
        let Value::Object(factor_object) = item else {
            continue;
        };
        if factor_object
            .get("factor")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            let fallback = factor_object
                .get("chartEvidence")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("Key factor {}", index + 1));
            factor_object.insert("factor".to_string(), Value::String(fallback));
        }
    }
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| {
        if let Value::Object(factor_object) = item {
            let key = [
                factor_object
                    .get("factor")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                factor_object
                    .get("chartEvidence")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ]
            .join("\n");
            if !key.trim().is_empty() {
                return seen.insert(key);
            }
        }
        true
    });
    items.truncate(4);
}

fn normalize_repeated_semicolon_evidence(value: &mut Value, fields: &[&str]) {
    let Value::Array(items) = value else {
        return;
    };
    for item in items {
        let Value::Object(object) = item else {
            continue;
        };
        for field in fields {
            let Some(field_value) = object.get_mut(*field) else {
                continue;
            };
            let Some(text) = field_value.as_str() else {
                continue;
            };
            let normalized = dedupe_semicolon_segments(text);
            if normalized != text {
                *field_value = Value::String(normalized);
            }
        }
    }
}

fn dedupe_semicolon_segments(text: &str) -> String {
    if !text.contains(';') {
        return text.to_string();
    }
    let mut seen = std::collections::HashSet::new();
    let mut segments = Vec::new();
    for segment in text
        .split(';')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        let key = segment.to_ascii_lowercase();
        if seen.insert(key) {
            segments.push(segment.to_string());
        }
    }
    if segments.is_empty() {
        text.to_string()
    } else {
        segments.join("; ")
    }
}

fn repair_common_json_punctuation(content: &str) -> String {
    let mut content = content.replace("\"step\": \"stepId\":", "\"stepId\":");
    for field in ["keyFactors", "cautions", "followUpQuestions"] {
        content = content.replace(
            &format!("]\n  }},\n  \"{field}\""),
            &format!("],\n  \"{field}\""),
        );
        content = content.replace(
            &format!("]\n  }}\n  \"{field}\""),
            &format!("],\n  \"{field}\""),
        );
    }
    let mut repaired = String::with_capacity(content.len() + 8);
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut top_level_value_closed = false;

    for character in content.chars() {
        if !in_string && top_level_value_closed {
            if character.is_whitespace() {
                repaired.push(character);
                continue;
            }
            if depth == 1 && character == ']' {
                continue;
            }
            if character == '"' {
                repaired.push(',');
            }
            top_level_value_closed = false;
        }

        repaired.push(character);

        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 1 {
                    top_level_value_closed = true;
                }
            }
            ',' => top_level_value_closed = false,
            _ => {}
        }
    }

    repaired
}

fn validate_interpretation_shape(value: &HoraryInterpretation) -> Result<(), AiError> {
    let mut errors = Vec::new();
    if value.summary.trim().is_empty() {
        errors.push("summary must be a non-empty string.");
    }
    if let Some(direct_answer) = &value.direct_answer {
        if direct_answer.trim().is_empty() {
            errors.push("directAnswer must not be empty when present.");
        }
    }
    if !matches!(value.confidence.as_str(), "low" | "medium" | "high") {
        errors.push("confidence must be low, medium, or high.");
    }
    if value.judgement_trace.is_empty() {
        errors.push("judgementTrace must be a non-empty array.");
    }
    for (index, step) in value.judgement_trace.iter().enumerate() {
        if step.step_id.trim().is_empty() {
            errors.push(match index {
                0 => "judgementTrace[0].stepId must be a non-empty string.",
                _ => "judgementTrace stepId must be a non-empty string.",
            });
        }
        if step.finding.trim().is_empty() {
            errors.push(match index {
                0 => "judgementTrace[0].finding must be a non-empty string.",
                _ => "judgementTrace finding must be a non-empty string.",
            });
        }
        if step
            .finding
            .trim()
            .eq_ignore_ascii_case(step.step_id.trim())
        {
            errors.push("judgementTrace finding must explain the step, not repeat its identifier.");
        }
        if step.chart_evidence.trim().is_empty() {
            errors.push(match index {
                0 => "judgementTrace[0].chartEvidence must be a non-empty string.",
                _ => "judgementTrace chartEvidence must be a non-empty string.",
            });
        }
        if !matches!(step.confidence.as_str(), "low" | "medium" | "high") {
            errors.push(match index {
                0 => "judgementTrace[0].confidence must be low, medium, or high.",
                _ => "judgementTrace confidence must be low, medium, or high.",
            });
        }
    }
    if value.key_factors.is_empty() {
        errors.push("keyFactors must be a non-empty array.");
    }
    if value.key_factors.len() > 4 {
        errors.push("keyFactors must contain at most four items.");
    }
    if value.cautions.len() > 3 {
        errors.push("cautions must contain at most three items.");
    }
    if value.follow_up_questions.len() > 3 {
        errors.push("followUpQuestions must contain at most three items.");
    }
    for (index, factor) in value.key_factors.iter().enumerate() {
        if factor.factor.trim().is_empty() {
            errors.push(match index {
                0 => "keyFactors[0].factor must be a non-empty string.",
                _ => "keyFactors factor must be a non-empty string.",
            });
        }
        if factor.chart_evidence.trim().is_empty() {
            errors.push(match index {
                0 => "keyFactors[0].chartEvidence must be a non-empty string.",
                _ => "keyFactors chartEvidence must be a non-empty string.",
            });
        }
        if factor.interpretation.trim().is_empty() {
            errors.push(match index {
                0 => "keyFactors[0].interpretation must be a non-empty string.",
                _ => "keyFactors interpretation must be a non-empty string.",
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(AiError {
            message: errors.join(" "),
        })
    }
}

pub fn validate_interpretation_for_risk(
    value: &HoraryInterpretation,
    risk: &str,
) -> Result<(), AiError> {
    if risk == "high_stakes"
        && value
            .cautions
            .iter()
            .all(|caution| caution.trim().is_empty())
    {
        Err(AiError {
            message: "high-stakes interpretations must include at least one caution".to_string(),
        })
    } else {
        Ok(())
    }
}

fn is_high_stakes_token(token: &str) -> bool {
    matches!(
        token,
        "cancer"
            | "diagnosis"
            | "illness"
            | "disease"
            | "surgery"
            | "miscarriage"
            | "death"
            | "die"
            | "dying"
            | "suicide"
            | "lawsuit"
            | "court"
            | "custody"
            | "criminal"
            | "legal"
            | "arrest"
            | "visa"
            | "invest"
            | "investment"
            | "stock"
            | "crypto"
            | "mortgage"
            | "loan"
            | "debt"
            | "tax"
            | "emergency"
            | "unsafe"
            | "abuse"
            | "violence"
    ) || token.starts_with("pregnan")
        || token.starts_with("deport")
        || token.starts_with("bankrupt")
}

fn traditional_horary_doctrine() -> &'static str {
    "Write in a traditional horary judgement order, not as generic natal astrology. Identify the querent and quesited from the question context and supplied house/significator facts. Prioritize classical significators, the Moon, house rulership, applying Ptolemaic aspects, reception, essential dignity, accidental strength, and perfection or its blockage. Treat void Moon, prohibition, frustration, translation, collection, combustion, under-beams, cazimi, and planetary hour/day as judgement conditions when supplied. Use outer planets only as secondary descriptive testimony unless the supplied chart facts make them unavoidable. Give a concise direct judgement first when the evidence supports one, then explain the chart testimony and uncertainty."
}

fn high_stakes_policy(risk: &str) -> &'static str {
    if risk != "high_stakes" {
        return "For ordinary questions, provide symbolic interpretation without deterministic claims.";
    }

    "This question may involve medical, legal, financial, emergency, pregnancy, death, or safety concerns. Do not present deterministic claims or professional advice. Give symbolic chart interpretation only and include a caution directing the user to qualified professionals or emergency support where appropriate."
}

fn interpretation_output_guardrails() -> &'static str {
    "Return one top-level JSON object with summary, directAnswer when the question is ordinary, confidence, judgementTrace, keyFactors, cautions, and followUpQuestions. keyFactors must be a JSON array of objects with factor, chartEvidence, and interpretation; never return keyFactors as an object keyed by planet, topic, or testimony. judgementTrace must include question_scope, house_assignment, significator_selection, and synthesis_pass; synthesis_pass chartEvidence must cite concrete supplied chart facts, not only missing evidence. Review the provisional deterministicAssignments for house_assignment and significator_selection before weighing interpretation; if significatorPlacement is null, write placement unavailable rather than inventing one. Do not confuse house rulership with body placement: a planet rules a house when the house ruler names it, and a planet is in a house only when that body fact lists that house. Each chartEvidence string must be one concise line under 30 words, must not repeat a phrase, and must not contain unescaped raw JSON quotes. If perfection or blockage evidence is absent, cite canonical absence facts such as no applying major aspects supplied or no timing perfection supplied, plus any void Moon fact; never cite empty-array labels such as antisciaContacts: []. Keep summary and directAnswer under 60 words each; keep findings and interpretations under 25 words each. Report the four required trace steps and at most two other decisive steps. Do not report every analysis pass. Copy each chartEvidence verbatim from canonicalFacts or canonicalEvidence, joining at most two facts with a semicolon. Do not use actor labels, field names, or significator names alone as chartEvidence. Use at most three keyFactors and two followUpQuestions."
}

fn question_context(question: &str) -> Value {
    let text = question.to_ascii_lowercase();
    if contains_any(&text, &["where", "lost", "missing", "return", "home"])
        && contains_any(&text, &["cat", "dog", "kitten", "puppy", "horse", "cow"])
    {
        let house = if contains_any(&text, &["horse", "cow"]) {
            12
        } else {
            6
        };
        return context("lost_animal", vec![assignment("querent",1,"Person asking"),assignment("animal",house,"Small animals belong to house 6; large animals to house 12. Confirm ownership before turning houses.")]);
    }
    if contains_any(
        &text,
        &[
            "job",
            "career",
            "offer",
            "employer",
            "boss",
            "promotion",
            "application",
        ],
    ) {
        return context(
            "job_career",
            vec![
                assignment("querent", 1, "the asker and their capacity to act"),
                assignment(
                    "job_or_offer",
                    10,
                    "career, employer, boss, public success, and job offers",
                ),
                assignment(
                    "wages_or_benefit",
                    11,
                    "money or benefit from the job when relevant",
                ),
            ],
        );
    }
    if contains_any(
        &text,
        &[
            "ex",
            "spouse",
            "partner",
            "relationship",
            "reconcile",
            "reconciliation",
            "marry",
            "marriage",
            "dating",
            "lover",
        ],
    ) {
        return context(
            "relationship",
            vec![
                assignment("querent", 1, "the asker"),
                assignment(
                    "partner_or_ex",
                    7,
                    "partner, spouse, sweetheart, ex, or desired other person",
                ),
            ],
        );
    }
    if contains_any(
        &text,
        &[
            "lost",
            "missing",
            "misplaced",
            "ring",
            "wallet",
            "phone",
            "keys",
        ],
    ) {
        return context(
            "lost_object",
            vec![
                assignment("querent", 1, "the asker"),
                assignment(
                    "lost_movable_possession",
                    2,
                    "the querent's movable possession",
                ),
            ],
        );
    }
    if contains_any(
        &text,
        &[
            "invest",
            "investment",
            "stock",
            "crypto",
            "savings",
            "portfolio",
            "loan",
            "debt",
            "mortgage",
            "tax",
        ],
    ) {
        return context(
            "financial_high_stakes",
            vec![
                assignment("querent", 1, "the asker"),
                assignment(
                    "savings_or_own_money",
                    2,
                    "the querent's money and movable assets",
                ),
                assignment(
                    "risk_or_other_party_money",
                    8,
                    "other people's money, debt, fear, or loss exposure when relevant",
                ),
            ],
        );
    }
    context("ordinary", vec![assignment("querent", 1, "the asker")])
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|word| needles.contains(&word))
}

fn context(domain_module: &str, likely_assignments: Vec<Value>) -> Value {
    json!({
        "domainModule": domain_module,
        "likelyAssignments": likely_assignments,
        "caution": "Question-context hints are scaffolding for house assignment only; final judgement must still cite supplied chart facts.",
    })
}

fn assignment(actor: &str, house: u8, rationale: &str) -> Value {
    json!({
        "actor": actor,
        "house": house,
        "rationale": rationale,
    })
}

fn build_chart_facts(chart: &Value) -> Value {
    book_method::apply(&json!({
        "castLocalTime": chart_field(chart, "castLocalTime").unwrap_or(Value::Null),
        "castUtcTime": chart_field(chart, "castUtcTime").unwrap_or(Value::Null),
        "timezone": chart_field(chart, "timezone").unwrap_or(Value::Null),
        "location": chart_field(chart, "location").unwrap_or(Value::Null),
        "ascendant": chart_field(chart, "ascendant").unwrap_or(Value::Null),
        "midheaven": chart_field(chart, "midheaven").unwrap_or(Value::Null),
        "houses": chart_field(chart, "houses").unwrap_or_else(|| json!([])),
        "bodies": chart_field(chart, "bodies").unwrap_or_else(|| json!([])),
        "aspects": chart_field(chart, "aspects").unwrap_or_else(|| json!([])),
        "derived": chart_field(chart, "derived").unwrap_or_else(|| json!({})),
        "motionSamples": chart_field(chart, "motionSamples").unwrap_or(Value::Null),
    }))
}

fn chart_evidence_index(chart: &Value) -> Value {
    let house_rulers = chart
        .get("houses")
        .and_then(Value::as_array)
        .map(|houses| {
            houses
                .iter()
                .map(|house| {
                    json!({
                        "house": house.get("number").cloned().unwrap_or(Value::Null),
                        "sign": house.get("sign").cloned().unwrap_or(Value::Null),
                        "ruler": house.get("ruler").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let body_placements = chart
        .get("bodies")
        .and_then(Value::as_array)
        .map(|bodies| {
            bodies
                .iter()
                .map(|body| {
                    json!({
                        "name": body.get("name").cloned().unwrap_or(Value::Null),
                        "sign": body.get("sign").cloned().unwrap_or(Value::Null),
                        "house": body.get("house").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let applying_major_aspects = chart
        .get("aspects")
        .and_then(Value::as_array)
        .map(|aspects| {
            aspects
                .iter()
                .filter(|aspect| {
                    aspect.get("applying").and_then(Value::as_bool) == Some(true)
                        && aspect.get("major").and_then(Value::as_bool) != Some(false)
                })
                .map(|aspect| {
                    json!({
                        "planet1": aspect.get("planet1").cloned().unwrap_or(Value::Null),
                        "aspectName": aspect.get("aspectName").cloned().unwrap_or(Value::Null),
                        "planet2": aspect.get("planet2").cloned().unwrap_or(Value::Null),
                        "orb": aspect.get("orb").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let timing_patterns = chart
        .get("derived")
        .and_then(|derived| derived.get("timingPatterns"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let void_of_course_moon = chart
        .get("derived")
        .and_then(|derived| derived.get("voidOfCourseMoon"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut canonical_facts = canonical_chart_facts(
        &house_rulers,
        &body_placements,
        &applying_major_aspects,
        &timing_patterns,
        &void_of_course_moon,
    );
    canonical_facts.extend(book_method::evidence(chart));

    json!({
        "houseRulers": house_rulers,
        "bodyPlacements": body_placements,
        "applyingMajorAspects": applying_major_aspects,
        "timingPatterns": timing_patterns,
        "voidOfCourseMoon": void_of_course_moon,
        "canonicalFacts": canonical_facts,
    })
}

fn deterministic_assignments(question_context: &Value, chart_evidence_index: &Value) -> Value {
    let assignments = question_context
        .get("likelyAssignments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let houses = chart_evidence_index
        .get("houseRulers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let bodies = chart_evidence_index
        .get("bodyPlacements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Value::Array(
        assignments
            .iter()
            .map(|assignment| {
                let house_number = assignment.get("house").and_then(Value::as_i64);
                let house = house_number.and_then(|number| {
                    houses
                        .iter()
                        .find(|house| house.get("house").and_then(Value::as_i64) == Some(number))
                });
                let significator = house
                    .and_then(|house| house.get("ruler"))
                    .and_then(Value::as_str);
                let placement = significator.and_then(|name| {
                    bodies
                        .iter()
                        .find(|body| body.get("name").and_then(Value::as_str) == Some(name))
                });
                let canonical_evidence = canonical_assignment_evidence(house, placement);

                json!({
                    "actor": assignment.get("actor").cloned().unwrap_or(Value::Null),
                    "house": assignment.get("house").cloned().unwrap_or(Value::Null),
                    "houseSign": house
                        .and_then(|house| house.get("sign"))
                        .cloned()
                        .unwrap_or(Value::Null),
                    "significator": significator
                        .map(|value| Value::String(value.to_string()))
                        .unwrap_or(Value::Null),
                    "significatorPlacement": placement.cloned().unwrap_or(Value::Null),
                    "canonicalEvidence": canonical_evidence,
                    "rationale": assignment.get("rationale").cloned().unwrap_or(Value::Null),
                })
            })
            .collect(),
    )
}

fn canonical_chart_facts(
    house_rulers: &[Value],
    body_placements: &[Value],
    applying_major_aspects: &[Value],
    timing_patterns: &Value,
    void_of_course_moon: &Value,
) -> Vec<String> {
    let mut facts = Vec::new();
    for house in house_rulers {
        facts.push(format!(
            "house {} {} ruler {}",
            value_label(house.get("house")),
            value_label(house.get("sign")),
            value_label(house.get("ruler"))
        ));
    }
    for body in body_placements {
        facts.push(format!(
            "{} {} house {}",
            value_label(body.get("name")),
            value_label(body.get("sign")),
            value_label(body.get("house"))
        ));
    }
    for aspect in applying_major_aspects {
        facts.push(format!(
            "{} {} {} applying orb {}",
            value_label(aspect.get("planet1")),
            value_label(aspect.get("aspectName")),
            value_label(aspect.get("planet2")),
            value_label(aspect.get("orb"))
        ));
    }
    if applying_major_aspects.is_empty() {
        facts.push("no applying major aspects supplied".to_string());
    }
    for timing in timing_patterns.as_array().into_iter().flatten() {
        facts.push(canonical_timing_pattern(timing));
    }
    if timing_patterns
        .as_array()
        .is_none_or(|timing_patterns| timing_patterns.is_empty())
    {
        facts.push("no timing perfection supplied".to_string());
    }
    if !void_of_course_moon.is_null() {
        facts.push(format!(
            "voidOfCourseMoon isVoid {}",
            value_label(void_of_course_moon.get("isVoid"))
        ));
    }
    facts
}

fn canonical_assignment_evidence(house: Option<&Value>, placement: Option<&Value>) -> String {
    let mut parts = Vec::new();
    if let Some(house) = house {
        parts.push(format!(
            "house {} {} ruler {}",
            value_label(house.get("house")),
            value_label(house.get("sign")),
            value_label(house.get("ruler"))
        ));
    }
    if let Some(placement) = placement {
        parts.push(format!(
            "{} {} house {}",
            value_label(placement.get("name")),
            value_label(placement.get("sign")),
            value_label(placement.get("house"))
        ));
    }
    parts.join("; ")
}

fn canonical_timing_pattern(pattern: &Value) -> String {
    format!(
        "timing {} {} {} {} {}h",
        value_label(pattern.get("type")),
        value_label(pattern.get("planet1")),
        value_label(pattern.get("aspectName")),
        value_label(pattern.get("planet2")),
        pattern
            .get("estimatedPerfectsWithinHours")
            .or_else(|| pattern.get("perfectsWithinHours"))
            .map(|value| value_label(Some(value)))
            .unwrap_or_else(|| "unknown".to_string())
    )
}

fn value_label(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => "unknown".to_string(),
    }
}

fn chart_field(chart: &Value, field: &str) -> Option<Value> {
    chart.get(field).cloned()
}

pub fn interpretation_schema() -> Value {
    let mut schema: Value = serde_json::from_str(INTERPRETATION_SCHEMA_JSON)
        .expect("shared horary interpretation schema must be valid JSON");
    let item = schema["properties"]["judgementTrace"]["items"].clone();
    let fixed = |id: &str| {
        let mut step = item.clone();
        step["properties"]["stepId"] = json!({"type": "string", "const": id});
        step
    };
    let mut analysis = item.clone();
    analysis["properties"]["stepId"] = json!({"type": "string", "enum": [
        "essential_dignity_pass", "accidental_strength_pass", "reception_pass",
        "perfection_pass", "blockage_pass", "moon_story_pass",
        "contextual_modifiers_pass", "timing_pass", "adversarial_checker_pass"
    ]});
    // Exact tuples are supported by the pinned upstream schema converter.
    // Keep required identification first and synthesis last, with zero to two
    // decisive analysis steps between them. A prompt alone cannot ensure this.
    let alternatives: Vec<Value> = (0..=2)
        .map(|extra| {
            let mut steps = vec![
                fixed("question_scope"),
                fixed("house_assignment"),
                fixed("significator_selection"),
            ];
            steps.extend((0..extra).map(|_| analysis.clone()));
            steps.push(fixed("synthesis_pass"));
            json!({"type": "array", "minItems": steps.len(),
                "maxItems": steps.len(), "prefixItems": steps})
        })
        .collect();
    schema["properties"]["judgementTrace"] = json!({"type": "array", "oneOf": alternatives});
    schema
}

pub fn judgement_plan() -> Value {
    let pipeline: Value = serde_json::from_str(JUDGEMENT_PIPELINE_JSON)
        .expect("shared horary judgement pipeline must be valid JSON");
    let micro_tasks = pipeline
        .get("microTasks")
        .and_then(Value::as_array)
        .map(|tasks| {
            tasks
                .iter()
                .map(|task| {
                    json!({
                        "id": task.get("id").cloned().unwrap_or(Value::Null),
                        "instruction": task.get("compactPrompt").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let house_map = pipeline
        .get("commonHouseMap")
        .and_then(Value::as_array)
        .map(|houses| {
            houses
                .iter()
                .map(|house| {
                    format!(
                        "{}: {}",
                        value_label(house.get("house")),
                        value_label(house.get("core"))
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let house_rules = pipeline
        .get("houseAssignmentRules")
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .filter_map(|rule| rule.get("rule").and_then(Value::as_str))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "profile": pipeline.get("profile").cloned().unwrap_or(Value::Null),
        "pipelineVersion": pipeline.get("pipelineVersion").cloned().unwrap_or(Value::Null),
        "executionMode": "single_call_with_required_trace",
        "sourceBasis": pipeline
            .get("sourceBasis")
            .and_then(|source| source.get("summary"))
            .cloned()
            .unwrap_or(Value::Null),
        "globalRules": pipeline.get("globalRules").cloned().unwrap_or_else(|| json!([])),
        "houseAssignmentRules": house_rules,
        "commonHouseMap": house_map,
        "microTasks": micro_tasks,
        "traceRequirement": pipeline
            .get("singleCallTraceRequirement")
            .cloned()
            .unwrap_or_else(|| json!({})),
        "verifierChecklist": pipeline.get("verifierChecklist").cloned().unwrap_or_else(|| json!([])),
    })
}

fn strip_code_fence(content: &str) -> &str {
    if !content.starts_with("```") {
        return content;
    }
    let Some(first_newline) = content.find('\n') else {
        return content;
    };
    let body = &content[first_newline + 1..];
    body.strip_suffix("```").unwrap_or(body).trim()
}

fn extract_json_object(content: &str) -> Result<&str, AiError> {
    let start = content.find('{').ok_or_else(|| AiError {
        message: "local model response did not contain a JSON object".to_string(),
    })?;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in content[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + character.len_utf8();
                    return Ok(&content[start..end]);
                }
            }
            _ => {}
        }
    }
    Err(AiError {
        message: "local model response did not contain a complete JSON object".to_string(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn constrained_trace_always_has_identification_and_final_synthesis() {
        let schema = interpretation_schema();
        for variant in schema["properties"]["judgementTrace"]["oneOf"]
            .as_array()
            .unwrap()
        {
            let steps = variant["prefixItems"].as_array().unwrap();
            assert!((4..=6).contains(&steps.len()));
            for (index, id) in [
                "question_scope",
                "house_assignment",
                "significator_selection",
            ]
            .iter()
            .enumerate()
            {
                assert_eq!(steps[index]["properties"]["stepId"]["const"], *id);
            }
            assert_eq!(
                steps.last().unwrap()["properties"]["stepId"]["const"],
                "synthesis_pass"
            );
            assert_eq!(variant["minItems"], variant["maxItems"]);
        }
    }
    #[test]
    fn recovery_prompt_preserves_moons_object_role_and_requires_timing_reasoning() {
        let prompt = build_interpretation_prompt(&sample_request("Where is my lost ring?"))
            .expect("valid chart prompt");
        let instructions = &prompt.messages[0].content;
        assert!(instructions.contains("Moon may represent the lost object"));
        assert!(instructions.contains("compare rulers of the 2nd and 4th"));
        assert!(instructions.contains("include a timing_pass"));
        assert!(instructions.contains("A small display orb alone does not mean soon"));
    }
    #[test]
    fn house_hints_match_words_not_substrings() {
        for question in [
            "What happens next?",
            "Will spring be pleasant?",
            "Can I recover from this illness?",
        ] {
            let domain = question_context(question)["domainModule"]
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(domain, "ordinary", "{question}");
        }
        assert_eq!(
            question_context("Will my ex return?")["domainModule"],
            "relationship"
        );
        assert_eq!(
            question_context("Where is my ring?")["domainModule"],
            "lost_object"
        );
    }
    use super::*;

    fn sample_request(question: &str) -> HoraryInterpretationRequest {
        HoraryInterpretationRequest {
            question: question.to_string(),
            chart: json!({
                "castLocalTime": "2026-06-30 12:00",
                "castUtcTime": "2026-06-30T16:00:00.000Z",
                "timezone": "UTC-04:00",
                "location": { "label": "New York, US", "latitude": 40.7128, "longitude": -74.006 },
                "ascendant": { "sign": "Libra", "degree": 12.3 },
                "midheaven": { "sign": "Cancer", "degree": 15.1 },
                "houses": [{ "number": 1, "sign": "Libra", "degree": 12.3, "ruler": "Venus" }],
                "bodies": [
                    { "name": "Moon", "sign": "Capricorn", "degree": 14.2, "house": 4 },
                    { "name": "Venus", "sign": "Gemini", "degree": 16.4, "house": 9 }
                ],
                "aspects": [{ "planet1": "Moon", "aspectName": "Trine", "planet2": "Venus", "orb": 2.1, "applying": true }],
                "derived": { "ascendantRuler": "Venus" },
                "ignoredField": "do not send",
            }),
            settings: Some(json!({ "tradition": "traditional" })),
        }
    }

    fn valid_interpretation_json() -> String {
        json!({
            "summary": "The chart emphasizes the contract significator.",
            "directAnswer": "Likely, with conditions.",
            "confidence": "medium",
            "judgementTrace": [{
                "stepId": "perfection_pass",
                "finding": "Moon applies to Venus.",
                "chartEvidence": "Moon trine Venus, 2.1 degree applying orb",
                "confidence": "medium"
            }],
            "keyFactors": [{
                "factor": "Moon applying to Venus",
                "chartEvidence": "Moon trine Venus, 2.1 degree applying orb",
                "interpretation": "A constructive contact is forming."
            }],
            "cautions": [],
            "followUpQuestions": []
        })
        .to_string()
    }

    #[test]
    fn findings_must_explain_the_step_and_animal_hints_use_the_book_houses() {
        let bad = valid_interpretation_json().replace("Moon applies to Venus.", "perfection_pass");
        assert!(parse_interpretation_content(&bad)
            .unwrap_err()
            .message
            .contains("not repeat"));
        assert_eq!(
            question_context("Where is my lost cat?")["likelyAssignments"][1]["house"],
            6
        );
        assert_eq!(
            question_context("Will my missing horse return?")["likelyAssignments"][1]["house"],
            12
        );
        assert_ne!(
            question_context("Where is my lost application?")["domainModule"],
            "lost_animal"
        );
    }

    #[test]
    fn prompt_uses_deterministic_chart_facts_and_schema() {
        let prompt = build_interpretation_prompt(&sample_request("Will I get the contract?"))
            .expect("prompt should build");
        let system = &prompt.messages[0].content;
        let user: Value = serde_json::from_str(&prompt.messages[1].content).unwrap();

        assert_eq!(prompt.prompt_version, INTERPRETATION_PROMPT_VERSION);
        assert_eq!(prompt.schema_version, INTERPRETATION_SCHEMA_VERSION);
        assert_eq!(prompt.tradition_profile, INTERPRETATION_TRADITION_PROFILE);
        assert_eq!(prompt.response_format["type"], "json_schema");
        assert_eq!(
            prompt.response_format["json_schema"]["schema"],
            interpretation_schema()
        );
        assert!(system.contains("Do not calculate or recalculate planetary positions"));
        assert!(system.contains("deterministicAssignments are provisional keyword hints"));
        assert!(!system.contains("deterministicAssignments as the authoritative mapping"));
        assert!(system.contains("traditional horary judgement order"));
        assert_eq!(user["risk"], "ordinary");
        assert_eq!(user["questionContext"]["domainModule"], "ordinary");
        assert_eq!(
            user["deterministicAssignments"][0]["canonicalEvidence"],
            "house 1 Libra ruler Venus; Venus Gemini house 9"
        );
        assert!(user["chartEvidenceIndex"]["canonicalFacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact == "house 1 Libra ruler Venus"));
        assert!(user["judgementPlan"]["microTasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "house_assignment"));
        assert!(user["judgementPlan"].get("domainModules").is_none());
        assert!(user["chart"].get("ignoredField").is_none());
    }

    #[test]
    fn prompt_json_export_matches_typed_prompt() {
        let request = sample_request("Will I get the contract?");
        let request_json = serde_json::to_string(&request).unwrap();
        let typed = build_interpretation_prompt(&request).unwrap();
        let typed_value = serde_json::to_value(&typed).unwrap();
        let from_json: Value =
            serde_json::from_str(&build_interpretation_prompt_json(&request_json).unwrap())
                .unwrap();

        assert_eq!(from_json, typed_value);
    }

    #[test]
    fn question_risk_flags_high_stakes_topics() {
        assert_eq!(
            classify_question_risk("Should I invest my savings in this stock?"),
            "high_stakes"
        );
        assert_eq!(
            classify_question_risk("Will I get the contract?"),
            "ordinary"
        );
    }

    #[test]
    fn json_boundary_helpers_return_core_scaffold_fragments() {
        let context: Value =
            serde_json::from_str(&question_context_json("Will my ex come back?").unwrap()).unwrap();
        let plan: Value = serde_json::from_str(&judgement_plan_json().unwrap()).unwrap();
        let schema: Value = serde_json::from_str(&interpretation_schema_json().unwrap()).unwrap();
        let assignments: Value = serde_json::from_str(
            &deterministic_assignments_json(
                &context.to_string(),
                &json!({
                    "houseRulers": [{ "house": 7, "sign": "Aries", "ruler": "Mars" }],
                    "bodyPlacements": [{ "name": "Mars", "sign": "Gemini", "house": 9 }]
                })
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(context["domainModule"], "relationship");
        assert_eq!(plan["executionMode"], "single_call_with_required_trace");
        assert_eq!(schema["required"][0], "summary");
        assert_eq!(
            assignments[1]["canonicalEvidence"],
            "house 7 Aries ruler Mars; Mars Gemini house 9"
        );
    }

    #[test]
    fn parser_accepts_valid_json_code_fences_and_repairs_key_factor_objects() {
        let direct = parse_interpretation_content(&valid_interpretation_json()).unwrap();
        let fenced = parse_interpretation_content(&format!(
            "```json
{}
```",
            valid_interpretation_json()
        ))
        .unwrap();
        let repaired = parse_interpretation_content(
            r#"{
                "summary": "The chart shows a constructive contact.",
                "confidence": "medium",
                "judgementTrace": [{
                    "stepId": "synthesis_pass",
                    "finding": "Moon applies to Venus.",
                    "chartEvidence": "Moon trine Venus, 2.1 degree applying orb",
                    "confidence": "medium"
                }],
                "keyFactors": {
                    "Moon-Venus": {
                        "chartEvidence": "Moon trine Venus, 2.1 degree applying orb",
                        "interpretation": "A constructive contact is forming."
                    }
                },
                "cautions": [],
                "followUpQuestions": []
            }
            }"#,
        )
        .unwrap();

        assert_eq!(direct.confidence, "medium");
        assert_eq!(
            fenced.key_factors[0].chart_evidence,
            "Moon trine Venus, 2.1 degree applying orb"
        );
        assert_eq!(repaired.key_factors[0].factor, "Moon-Venus");
    }

    #[test]
    fn parser_rejects_missing_evidence_and_high_stakes_without_caution() {
        let error = parse_interpretation_content(
            r#"{
                "summary": "Looks good.",
                "confidence": "certain",
                "keyFactors": [{ "factor": "Moon", "chartEvidence": "", "interpretation": "" }],
                "cautions": [],
                "followUpQuestions": []
            }"#,
        )
        .unwrap_err();
        let interpretation = parse_interpretation_content(&valid_interpretation_json()).unwrap();
        let risk_error =
            validate_interpretation_for_risk(&interpretation, "high_stakes").unwrap_err();

        assert!(error.message.contains("confidence"));
        assert!(error.message.contains("judgementTrace"));
        assert!(risk_error.message.contains("caution"));
    }
}
