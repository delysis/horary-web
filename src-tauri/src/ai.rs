use crate::inference::{active_inference_connection, local_inference_http_client};
use crate::llama::{LlamaError, LlamaState};
use crate::native_llama_worker::{generate_native, NativeGenerateOptions, NativeLlamaState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

const INTERPRETATION_SCHEMA_VERSION: &str = "2026-07-01";
const INTERPRETATION_PROMPT_VERSION: &str = "horary-interpretation-v2";
const INTERPRETATION_TRADITION_PROFILE: &str = "traditional-horary-textbook-v1";
const INTERPRETATION_SCHEMA_JSON: &str =
    include_str!("../../src/ai/horary-interpretation.schema.json");
const JUDGEMENT_PIPELINE_JSON: &str = include_str!("../../src/ai/horary-judgement-pipeline.json");
const MAX_INTERPRETATION_TOKENS: u32 = 1600;
const MAX_MODEL_OUTPUT_ERROR_CHARS: usize = 600;
pub const AI_STREAM_TOKEN_EVENT: &str = "ai-interpretation-token";
pub const AI_STREAM_COMPLETE_EVENT: &str = "ai-interpretation-complete";
pub const AI_STREAM_ERROR_EVENT: &str = "ai-interpretation-error";
pub const AI_STREAM_CANCELLED_EVENT: &str = "ai-interpretation-cancelled";

#[derive(Debug, Clone, Serialize)]
pub struct AiError {
    pub message: String,
}

impl From<LlamaError> for AiError {
    fn from(value: LlamaError) -> Self {
        Self {
            message: value.message,
        }
    }
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
pub struct InterpretationStreamStarted {
    pub generation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationStreamToken {
    pub generation_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationStreamComplete {
    pub generation_id: String,
    pub interpretation: HoraryInterpretation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InterpretationStreamMessage {
    pub generation_id: String,
    pub message: String,
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

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: Value,
}

pub struct AiGenerationState {
    active: Mutex<Option<ActiveGeneration>>,
}

impl Default for AiGenerationState {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }
}

struct ActiveGeneration {
    generation_id: String,
    cancel: Arc<AtomicBool>,
}

impl AiGenerationState {
    fn start_generation(&self, generation_id: String) -> Result<Arc<AtomicBool>, AiError> {
        let mut active = self.active.lock().map_err(|_| AiError {
            message: "AI generation state lock poisoned".to_string(),
        })?;
        if active.is_some() {
            return Err(AiError {
                message: "an interpretation is already running".to_string(),
            });
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveGeneration {
            generation_id,
            cancel: Arc::clone(&cancel),
        });
        Ok(cancel)
    }

    pub fn cancel_generation(&self, generation_id: &str) -> Result<bool, AiError> {
        let active = self.active.lock().map_err(|_| AiError {
            message: "AI generation state lock poisoned".to_string(),
        })?;
        let Some(active) = active.as_ref() else {
            return Ok(false);
        };
        if active.generation_id != generation_id {
            return Ok(false);
        }
        active.cancel.store(true, Ordering::Relaxed);
        Ok(true)
    }

    fn clear_generation(&self, generation_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if active
                .as_ref()
                .is_some_and(|active| active.generation_id == generation_id)
            {
                *active = None;
            }
        }
    }
}

pub async fn generate_interpretation_from_state(
    state: &LlamaState,
    req: HoraryInterpretationRequest,
) -> Result<HoraryInterpretation, AiError> {
    let connection = active_inference_connection(state)?.ok_or_else(|| AiError {
        message: "local model is not running".to_string(),
    })?;
    let client = local_inference_http_client()?;

    generate_interpretation_with_client(
        &client,
        &connection.chat_completions_endpoint,
        &connection.model_id,
        req,
    )
    .await
}

pub fn generate_interpretation_from_native_state(
    state: &NativeLlamaState,
    req: HoraryInterpretationRequest,
) -> Result<HoraryInterpretation, AiError> {
    let prompt = build_interpretation_prompt(&req)?;
    let risk = prompt.risk.clone();
    let result = generate_native(
        state,
        native_prompt_text(&prompt)?,
        NativeGenerateOptions {
            max_tokens: MAX_INTERPRETATION_TOKENS,
            temperature: 0.2,
            top_p: 1.0,
            seed: 0x5752_5952,
            prompt_cache_key: Some(native_interpretation_cache_key(&prompt)),
            token_sink: None,
            cancel: None,
        },
    )?;
    let interpretation = parse_interpretation_content(&result.content)?;
    validate_interpretation_for_risk(&interpretation, &risk)?;
    Ok(interpretation)
}

pub async fn start_interpretation_stream_from_state(
    app: tauri::AppHandle,
    llama_state: &LlamaState,
    generation_state: &AiGenerationState,
    req: HoraryInterpretationRequest,
) -> Result<InterpretationStreamStarted, AiError> {
    let connection = active_inference_connection(llama_state)?.ok_or_else(|| AiError {
        message: "local model is not running".to_string(),
    })?;
    let prompt = build_interpretation_prompt(&req)?;
    let risk = prompt.risk.clone();
    let endpoint = connection.chat_completions_endpoint;
    let client = local_inference_http_client()?;
    let generation_id = new_generation_id();
    let cancel = generation_state.start_generation(generation_id.clone())?;

    let app_for_task = app.clone();
    let task_generation_id = generation_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = generate_interpretation_stream_with_client(StreamGenerationContext {
            app: &app_for_task,
            client: &client,
            endpoint: &endpoint,
            model_id: &connection.model_id,
            generation_id: &task_generation_id,
            cancel,
            prompt,
            risk,
        })
        .await;
        if let Err(error) = result {
            emit_stream_error(&app_for_task, &task_generation_id, &error.message);
        }
        clear_generation_from_app(&app_for_task, &task_generation_id);
    });

    Ok(InterpretationStreamStarted { generation_id })
}

pub fn start_interpretation_stream_from_native_state(
    app: tauri::AppHandle,
    generation_state: &AiGenerationState,
    req: HoraryInterpretationRequest,
) -> Result<InterpretationStreamStarted, AiError> {
    let prompt = build_interpretation_prompt(&req)?;
    let prompt_text = native_prompt_text(&prompt)?;
    let risk = prompt.risk.clone();
    let generation_id = new_generation_id();
    let cancel = generation_state.start_generation(generation_id.clone())?;
    let cache_key = native_interpretation_cache_key(&prompt);
    let app_for_task = app.clone();
    let task_generation_id = generation_id.clone();

    thread::Builder::new()
        .name("whorary-native-ai-stream".to_string())
        .spawn(move || {
            let (token_tx, token_rx) = std::sync::mpsc::channel::<String>();
            let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<String, AiError>>();
            let app_for_generation = app_for_task.clone();
            let cancel_for_generation = Arc::clone(&cancel);
            thread::Builder::new()
                .name("whorary-native-ai-generate".to_string())
                .spawn(move || {
                    let native_state = app_for_generation.state::<NativeLlamaState>();
                    let result = generate_native(
                        &native_state,
                        prompt_text,
                        NativeGenerateOptions {
                            max_tokens: MAX_INTERPRETATION_TOKENS,
                            temperature: 0.2,
                            top_p: 1.0,
                            seed: 0x5752_5952,
                            prompt_cache_key: Some(cache_key),
                            token_sink: Some(token_tx),
                            cancel: Some(cancel_for_generation),
                        },
                    )
                    .map(|result| result.content)
                    .map_err(AiError::from);
                    let _ = done_tx.send(result);
                })
                .map_err(|error| AiError {
                    message: format!("failed to spawn native generation thread: {error}"),
                })
                .and_then(|join| {
                    drop(join);
                    stream_native_generation_events(
                        &app_for_task,
                        &task_generation_id,
                        &risk,
                        &token_rx,
                        &done_rx,
                        &cancel,
                    )
                })
                .unwrap_or_else(|error| {
                    emit_stream_error(&app_for_task, &task_generation_id, &error.message);
                });
            clear_generation_from_app(&app_for_task, &task_generation_id);
        })
        .map_err(|error| AiError {
            message: format!("failed to spawn native stream thread: {error}"),
        })?;

    Ok(InterpretationStreamStarted { generation_id })
}

pub fn cancel_interpretation_stream_from_state(
    generation_state: &AiGenerationState,
    generation_id: String,
) -> Result<bool, AiError> {
    generation_state.cancel_generation(&generation_id)
}

pub async fn generate_interpretation_with_client(
    client: &reqwest::Client,
    endpoint: &str,
    model_id: &str,
    req: HoraryInterpretationRequest,
) -> Result<HoraryInterpretation, AiError> {
    let prompt = build_interpretation_prompt(&req)?;
    let risk = prompt.risk.clone();
    let body = json!({
        "model": model_id,
        "messages": prompt.messages,
        "response_format": prompt.response_format,
        "temperature": 0.2,
        "max_tokens": MAX_INTERPRETATION_TOKENS,
        "stream": false,
    });

    let response = client
        .post(endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|error| AiError {
            message: format!("local model request failed: {error}"),
        })?;
    let status = response.status();
    let response_text = response.text().await.map_err(|error| AiError {
        message: format!("failed to read local model response: {error}"),
    })?;

    if !status.is_success() {
        return Err(AiError {
            message: format!(
                "local model request failed with HTTP {status}: {}",
                truncate_for_error(&response_text)
            ),
        });
    }

    let completion: ChatCompletionResponse =
        serde_json::from_str(&response_text).map_err(|error| AiError {
            message: format!("local model returned invalid completion JSON: {error}"),
        })?;
    let content = completion
        .choices
        .first()
        .and_then(|choice| content_as_text(&choice.message.content))
        .ok_or_else(|| AiError {
            message: "local model response did not include message content".to_string(),
        })?;

    let interpretation = parse_interpretation_content(&content)?;
    validate_interpretation_for_risk(&interpretation, &risk)?;
    Ok(interpretation)
}

struct StreamGenerationContext<'a> {
    app: &'a tauri::AppHandle,
    client: &'a reqwest::Client,
    endpoint: &'a str,
    model_id: &'a str,
    generation_id: &'a str,
    cancel: Arc<AtomicBool>,
    prompt: InterpretationPrompt,
    risk: String,
}

async fn generate_interpretation_stream_with_client(
    context: StreamGenerationContext<'_>,
) -> Result<(), AiError> {
    let body = json!({
        "model": context.model_id,
        "messages": context.prompt.messages,
        "response_format": context.prompt.response_format,
        "temperature": 0.2,
        "max_tokens": MAX_INTERPRETATION_TOKENS,
        "stream": true,
    });

    let mut response = context
        .client
        .post(context.endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|error| AiError {
            message: format!("local model stream request failed: {error}"),
        })?;
    let status = response.status();
    if !status.is_success() {
        let response_text = response.text().await.map_err(|error| AiError {
            message: format!("failed to read local model stream response: {error}"),
        })?;
        return Err(AiError {
            message: format!(
                "local model stream request failed with HTTP {status}: {}",
                truncate_for_error(&response_text)
            ),
        });
    }

    let mut line_buffer = String::new();
    let mut content = String::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| AiError {
        message: format!("local model stream read failed: {error}"),
    })? {
        if context.cancel.load(Ordering::Relaxed) {
            emit_stream_cancelled(context.app, context.generation_id, "generation cancelled");
            return Ok(());
        }
        line_buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(newline_index) = line_buffer.find('\n') {
            let line = line_buffer[..newline_index]
                .trim_end_matches('\r')
                .to_string();
            line_buffer = line_buffer[newline_index + 1..].to_string();
            if process_stream_line(context.app, context.generation_id, &line, &mut content)? {
                let interpretation = parse_interpretation_content(&content)?;
                validate_interpretation_for_risk(&interpretation, &context.risk)?;
                emit_stream_complete(context.app, context.generation_id, interpretation);
                return Ok(());
            }
            if context.cancel.load(Ordering::Relaxed) {
                emit_stream_cancelled(context.app, context.generation_id, "generation cancelled");
                return Ok(());
            }
        }
    }

    if !line_buffer.trim().is_empty()
        && process_stream_line(
            context.app,
            context.generation_id,
            &line_buffer,
            &mut content,
        )?
    {
        let interpretation = parse_interpretation_content(&content)?;
        validate_interpretation_for_risk(&interpretation, &context.risk)?;
        emit_stream_complete(context.app, context.generation_id, interpretation);
        return Ok(());
    }

    let interpretation = parse_interpretation_content(&content)?;
    validate_interpretation_for_risk(&interpretation, &context.risk)?;
    emit_stream_complete(context.app, context.generation_id, interpretation);
    Ok(())
}

fn stream_native_generation_events(
    app: &tauri::AppHandle,
    generation_id: &str,
    risk: &str,
    token_rx: &std::sync::mpsc::Receiver<String>,
    done_rx: &std::sync::mpsc::Receiver<Result<String, AiError>>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), AiError> {
    let mut streamed_content = String::new();
    loop {
        while let Ok(token) = token_rx.try_recv() {
            streamed_content.push_str(&token);
            emit_stream_token(app, generation_id, &token);
        }

        if cancel.load(Ordering::Relaxed) {
            emit_stream_cancelled(app, generation_id, "generation cancelled");
            return Ok(());
        }

        match done_rx.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(Ok(content)) => {
                while let Ok(token) = token_rx.try_recv() {
                    streamed_content.push_str(&token);
                    emit_stream_token(app, generation_id, &token);
                }
                let final_content = if content.trim().is_empty() {
                    streamed_content
                } else {
                    content
                };
                let interpretation = parse_interpretation_content(&final_content)?;
                validate_interpretation_for_risk(&interpretation, risk)?;
                emit_stream_complete(app, generation_id, interpretation);
                return Ok(());
            }
            Ok(Err(error)) if error.message.contains("cancelled") => {
                emit_stream_cancelled(app, generation_id, "generation cancelled");
                return Ok(());
            }
            Ok(Err(error)) => return Err(error),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AiError {
                    message: "native generation thread ended without a result".to_string(),
                });
            }
        }
    }
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
    let user_payload = json!({
        "promptVersion": INTERPRETATION_PROMPT_VERSION,
        "schemaVersion": INTERPRETATION_SCHEMA_VERSION,
        "traditionProfile": INTERPRETATION_TRADITION_PROFILE,
        "question": req.question,
        "risk": risk,
        "chart": build_chart_facts(&req.chart),
        "settings": req.settings.clone().unwrap_or_else(|| json!({})),
        "judgementPlan": judgement_plan(),
    });

    let system_content = [
        "You interpret horary astrology charts from supplied deterministic chart facts.".to_string(),
        "Do not calculate or recalculate planetary positions, house cusps, aspects, dignity, or timing.".to_string(),
        "Use only the chart facts provided in the user message as evidence.".to_string(),
        traditional_horary_doctrine().to_string(),
        "Follow the supplied judgementPlan step by step and include a compact judgementTrace showing the decisive steps.".to_string(),
        "The judgementTrace is an audit trail: each item must name a stepId, a finding, concrete chartEvidence, and confidence.".to_string(),
        "Tie every key factor to a concrete chart fact.".to_string(),
        "Keep confidence humble and evidence-based.".to_string(),
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
                content: serde_json::to_string_pretty(&user_payload).map_err(|error| AiError {
                    message: format!("failed to serialize interpretation prompt: {error}"),
                })?,
            },
        ],
    })
}

fn native_prompt_text(prompt: &InterpretationPrompt) -> Result<String, AiError> {
    let system = prompt
        .messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let user = prompt
        .messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| format!("{}:\n{}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    let schema = serde_json::to_string(&prompt.response_format).map_err(|error| AiError {
        message: format!("failed to serialize native response format: {error}"),
    })?;

    Ok(format!(
        "<start_of_turn>user\nSystem instructions:\n{system}\n\nRequest:\n{user}\n\nResponse format:\n{schema}\n<end_of_turn>\n<start_of_turn>model\n"
    ))
}

fn native_interpretation_cache_key(prompt: &InterpretationPrompt) -> String {
    [
        "horary-interpretation",
        prompt.prompt_version,
        prompt.schema_version,
        prompt.tradition_profile,
        "temperature-0.2",
        "max-tokens-1600",
    ]
    .join(":")
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
    let interpretation: HoraryInterpretation =
        serde_json::from_str(content).map_err(|error| AiError {
            message: format!("local model returned invalid interpretation JSON: {error}"),
        })?;
    validate_interpretation_shape(&interpretation)?;
    Ok(interpretation)
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

fn validate_interpretation_for_risk(
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

fn build_chart_facts(chart: &Value) -> Value {
    json!({
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
    })
}

fn chart_field(chart: &Value, field: &str) -> Option<Value> {
    chart.get(field).cloned()
}

fn interpretation_schema() -> Value {
    serde_json::from_str(INTERPRETATION_SCHEMA_JSON)
        .expect("shared horary interpretation schema must be valid JSON")
}

fn judgement_plan() -> Value {
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
                        "title": task.get("title").cloned().unwrap_or(Value::Null),
                        "phase": task.get("phase").cloned().unwrap_or(Value::Null),
                        "parallelGroup": task.get("parallelGroup").cloned().unwrap_or(Value::Null),
                        "objective": task.get("objective").cloned().unwrap_or(Value::Null),
                        "instruction": task.get("compactPrompt").cloned().unwrap_or(Value::Null),
                        "outputKeys": task.get("outputKeys").cloned().unwrap_or_else(|| json!([])),
                    })
                })
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
        "cachePolicy": pipeline.get("cachePolicy").cloned().unwrap_or_else(|| json!({})),
        "checkerOutputContract": pipeline
            .get("checkerOutputContract")
            .cloned()
            .unwrap_or_else(|| json!({})),
        "globalRules": pipeline.get("globalRules").cloned().unwrap_or_else(|| json!([])),
        "houseAssignmentRules": pipeline.get("houseAssignmentRules").cloned().unwrap_or_else(|| json!([])),
        "commonHouseMap": pipeline.get("commonHouseMap").cloned().unwrap_or_else(|| json!([])),
        "microTasks": micro_tasks,
        "domainModules": pipeline.get("domainModules").cloned().unwrap_or_else(|| json!([])),
        "futureMultiCallPipeline": pipeline
            .get("futureMultiCallPipeline")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "parallelGroups": pipeline.get("parallelGroups").cloned().unwrap_or_else(|| json!([])),
        "traceRequirement": pipeline
            .get("singleCallTraceRequirement")
            .cloned()
            .unwrap_or_else(|| json!({})),
        "verifierChecklist": pipeline.get("verifierChecklist").cloned().unwrap_or_else(|| json!([])),
    })
}

fn content_as_text(content: &Value) -> Option<String> {
    match content {
        Value::String(value) => Some(value.clone()),
        Value::Object(_) => Some(content.to_string()),
        Value::Array(parts) => {
            let joined = parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.get("content").and_then(Value::as_str))
                })
                .collect::<Vec<_>>()
                .join("");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        _ => None,
    }
}

fn process_stream_line(
    app: &tauri::AppHandle,
    generation_id: &str,
    line: &str,
    content: &mut String,
) -> Result<bool, AiError> {
    let Some(data) = sse_data_from_line(line) else {
        return Ok(false);
    };
    match parse_stream_data(data)? {
        StreamData::Done => Ok(true),
        StreamData::Text(text) => {
            content.push_str(&text);
            emit_stream_token(app, generation_id, &text);
            Ok(false)
        }
        StreamData::Empty => Ok(false),
    }
}

#[derive(Debug, PartialEq)]
enum StreamData {
    Done,
    Text(String),
    Empty,
}

fn sse_data_from_line(line: &str) -> Option<&str> {
    line.trim_start()
        .strip_prefix("data:")
        .map(|value| value.trim())
}

fn parse_stream_data(data: &str) -> Result<StreamData, AiError> {
    if data == "[DONE]" {
        return Ok(StreamData::Done);
    }
    if data.is_empty() {
        return Ok(StreamData::Empty);
    }

    let value: Value = serde_json::from_str(data).map_err(|error| AiError {
        message: format!("local model stream returned invalid JSON chunk: {error}"),
    })?;
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    else {
        return Ok(StreamData::Empty);
    };
    if choice
        .get("finish_reason")
        .is_some_and(|finish_reason| !finish_reason.is_null())
    {
        return Ok(StreamData::Done);
    }
    if let Some(text) = choice
        .get("delta")
        .and_then(|delta| delta.get("content"))
        .and_then(content_as_text)
        .or_else(|| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(content_as_text)
        })
        .or_else(|| {
            choice
                .get("text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
    {
        if text.is_empty() {
            Ok(StreamData::Empty)
        } else {
            Ok(StreamData::Text(text))
        }
    } else {
        Ok(StreamData::Empty)
    }
}

fn emit_stream_token(app: &tauri::AppHandle, generation_id: &str, text: &str) {
    let _ = app.emit(
        AI_STREAM_TOKEN_EVENT,
        InterpretationStreamToken {
            generation_id: generation_id.to_string(),
            text: text.to_string(),
        },
    );
}

fn emit_stream_complete(
    app: &tauri::AppHandle,
    generation_id: &str,
    interpretation: HoraryInterpretation,
) {
    let _ = app.emit(
        AI_STREAM_COMPLETE_EVENT,
        InterpretationStreamComplete {
            generation_id: generation_id.to_string(),
            interpretation,
        },
    );
}

fn emit_stream_error(app: &tauri::AppHandle, generation_id: &str, message: &str) {
    let _ = app.emit(
        AI_STREAM_ERROR_EVENT,
        InterpretationStreamMessage {
            generation_id: generation_id.to_string(),
            message: message.to_string(),
        },
    );
}

fn emit_stream_cancelled(app: &tauri::AppHandle, generation_id: &str, message: &str) {
    let _ = app.emit(
        AI_STREAM_CANCELLED_EVENT,
        InterpretationStreamMessage {
            generation_id: generation_id.to_string(),
            message: message.to_string(),
        },
    );
}

fn clear_generation_from_app(app: &tauri::AppHandle, generation_id: &str) {
    app.state::<AiGenerationState>()
        .clear_generation(generation_id);
}

fn new_generation_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("ai-{millis}")
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
    let end = content.rfind('}').ok_or_else(|| AiError {
        message: "local model response did not contain a complete JSON object".to_string(),
    })?;
    if end <= start {
        return Err(AiError {
            message: "local model response did not contain a complete JSON object".to_string(),
        });
    }
    Ok(&content[start..=end])
}

fn truncate_for_error(content: &str) -> String {
    if content.chars().count() <= MAX_MODEL_OUTPUT_ERROR_CHARS {
        return content.to_string();
    }
    let mut truncated = content
        .chars()
        .take(MAX_MODEL_OUTPUT_ERROR_CHARS)
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
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
                "houses": [{ "number": 1, "sign": "Libra", "degree": 12.3 }],
                "bodies": [{ "name": "Moon", "sign": "Capricorn", "degree": 14.2, "house": 4 }],
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
        assert_eq!(
            prompt.response_format["json_schema"]["schema"]["required"][0],
            "summary"
        );
        assert!(system.contains("Do not calculate or recalculate planetary positions"));
        assert!(system.contains("traditional horary judgement order"));
        assert!(system.contains("querent and quesited"));
        assert!(system.contains(&format!("Prompt version: {INTERPRETATION_PROMPT_VERSION}")));
        assert!(system.contains(&format!(
            "Output schema version: {INTERPRETATION_SCHEMA_VERSION}"
        )));
        assert!(system.contains(&format!(
            "Tradition profile: {INTERPRETATION_TRADITION_PROFILE}"
        )));
        assert_eq!(user["promptVersion"], INTERPRETATION_PROMPT_VERSION);
        assert_eq!(user["schemaVersion"], INTERPRETATION_SCHEMA_VERSION);
        assert_eq!(user["traditionProfile"], INTERPRETATION_TRADITION_PROFILE);
        assert_eq!(user["risk"], "ordinary");
        assert_eq!(
            user["judgementPlan"]["profile"],
            INTERPRETATION_TRADITION_PROFILE
        );
        assert_eq!(
            user["judgementPlan"]["executionMode"],
            "single_call_with_required_trace"
        );
        assert!(user["judgementPlan"]["microTasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "house_assignment"));
        assert!(user["judgementPlan"]["domainModules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|module| module["id"] == "relationship"));
        assert!(user["judgementPlan"]["futureMultiCallPipeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["id"] == "reception_checker"));
        assert!(system.contains("judgementTrace"));
        assert_eq!(user["chart"]["bodies"][0]["name"], "Moon");
        assert!(user["chart"].get("ignoredField").is_none());
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
    fn high_stakes_prompt_includes_caution_policy() {
        let prompt =
            build_interpretation_prompt(&sample_request("Do I have cancer?")).expect("prompt");

        assert_eq!(prompt.risk, "high_stakes");
        assert!(prompt.messages[0]
            .content
            .contains("qualified professionals"));
        assert!(prompt.messages[0]
            .content
            .contains("Do not present deterministic claims"));
    }

    #[test]
    fn parser_accepts_valid_json_and_code_fences() {
        let direct = parse_interpretation_content(&valid_interpretation_json()).unwrap();
        let fenced =
            parse_interpretation_content(&format!("```json\n{}\n```", valid_interpretation_json()))
                .unwrap();

        assert_eq!(direct.confidence, "medium");
        assert_eq!(
            fenced.key_factors[0].chart_evidence,
            "Moon trine Venus, 2.1 degree applying orb"
        );
    }

    #[test]
    fn parser_rejects_missing_evidence_and_bad_confidence() {
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

        assert!(error.message.contains("confidence"));
        assert!(error.message.contains("judgementTrace"));
        assert!(error.message.contains("chartEvidence"));
    }

    #[test]
    fn high_stakes_answers_must_include_cautions() {
        let interpretation = parse_interpretation_content(&valid_interpretation_json()).unwrap();
        let error = validate_interpretation_for_risk(&interpretation, "high_stakes").unwrap_err();

        assert!(error.message.contains("caution"));
    }

    #[test]
    fn parses_openai_compatible_stream_chunks() {
        let data = r#"{"choices":[{"delta":{"content":"{\"summary\""},"finish_reason":null}]}"#;
        let done = r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#;

        assert_eq!(
            parse_stream_data(data).unwrap(),
            StreamData::Text("{\"summary\"".to_string())
        );
        assert_eq!(parse_stream_data("[DONE]").unwrap(), StreamData::Done);
        assert_eq!(parse_stream_data(done).unwrap(), StreamData::Done);
        assert_eq!(
            parse_stream_data(r#"{"choices":[{"delta":{}}]}"#).unwrap(),
            StreamData::Empty
        );
    }

    #[test]
    fn extracts_sse_data_lines() {
        assert_eq!(
            sse_data_from_line("data: {\"choices\":[]}"),
            Some("{\"choices\":[]}")
        );
        assert_eq!(sse_data_from_line(": keepalive"), None);
    }

    #[test]
    fn generation_state_allows_one_active_cancelled_generation() {
        let state = AiGenerationState::default();
        let cancel = state.start_generation("one".to_string()).unwrap();

        assert!(state
            .start_generation("two".to_string())
            .unwrap_err()
            .message
            .contains("already"));
        assert!(!state.cancel_generation("wrong").unwrap());
        assert!(state.cancel_generation("one").unwrap());
        assert!(cancel.load(Ordering::Relaxed));

        state.clear_generation("one");
        assert!(state.start_generation("two".to_string()).is_ok());
    }
}
