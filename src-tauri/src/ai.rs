use crate::inference::{active_inference_connection, local_inference_http_client};
use crate::llama::{LlamaError, LlamaState};
use crate::native_llama_worker::{generate_native, NativeGenerateOptions, NativeLlamaState};
#[cfg(all(test, feature = "native-llama"))]
use horary_ai_core::judgement_plan as core_judgement_plan;
use horary_ai_core::{
    build_interpretation_prompt as core_build_interpretation_prompt,
    native_prompt_text as core_native_prompt_text,
    parse_interpretation_content as core_parse_interpretation_content,
    validate_interpretation_for_risk as core_validate_interpretation_for_risk,
    InterpretationPrompt, INTERPRETATION_TEMPERATURE, MAX_INTERPRETATION_TOKENS,
};
#[cfg(test)]
use horary_ai_core::{
    classify_question_risk, interpretation_schema as core_interpretation_schema,
    INTERPRETATION_PROMPT_VERSION, INTERPRETATION_SCHEMA_VERSION, INTERPRETATION_TRADITION_PROFILE,
};
pub use horary_ai_core::{HoraryInterpretation, HoraryInterpretationRequest};
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

impl From<horary_ai_core::AiError> for AiError {
    fn from(value: horary_ai_core::AiError) -> Self {
        Self {
            message: value.message,
        }
    }
}

fn build_interpretation_prompt(
    req: &HoraryInterpretationRequest,
) -> Result<InterpretationPrompt, AiError> {
    core_build_interpretation_prompt(req).map_err(AiError::from)
}

fn native_prompt_text(prompt: &InterpretationPrompt) -> Result<String, AiError> {
    core_native_prompt_text(prompt).map_err(AiError::from)
}

fn parse_interpretation_content(content: &str) -> Result<HoraryInterpretation, AiError> {
    core_parse_interpretation_content(content).map_err(AiError::from)
}

fn validate_interpretation_for_risk(
    value: &HoraryInterpretation,
    risk: &str,
) -> Result<(), AiError> {
    core_validate_interpretation_for_risk(value, risk).map_err(AiError::from)
}

#[cfg(test)]
fn interpretation_schema() -> Value {
    core_interpretation_schema()
}

#[cfg(all(test, feature = "native-llama"))]
fn judgement_plan() -> Value {
    core_judgement_plan()
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
    let prompt_text = native_prompt_text(&prompt)?;
    let result = generate_native(
        state,
        prompt_text,
        NativeGenerateOptions {
            max_tokens: MAX_INTERPRETATION_TOKENS,
            response_schema: Some(horary_ai_core::interpretation_schema().to_string()),
            temperature: INTERPRETATION_TEMPERATURE,
            top_p: 1.0,
            seed: 0x5752_5952,
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
    let app_for_task = app.clone();
    let task_generation_id = generation_id.clone();

    thread::Builder::new()
        .name("horary-native-ai-stream".to_string())
        .spawn(move || {
            let (token_tx, token_rx) = std::sync::mpsc::channel::<String>();
            let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<String, AiError>>();
            let app_for_generation = app_for_task.clone();
            let cancel_for_generation = Arc::clone(&cancel);
            thread::Builder::new()
                .name("horary-native-ai-generate".to_string())
                .spawn(move || {
                    let native_state = app_for_generation.state::<NativeLlamaState>();
                    let result = generate_native(
                        &native_state,
                        prompt_text,
                        NativeGenerateOptions {
                            max_tokens: MAX_INTERPRETATION_TOKENS,
                            response_schema: Some(
                                horary_ai_core::interpretation_schema().to_string(),
                            ),
                            temperature: INTERPRETATION_TEMPERATURE,
                            top_p: 1.0,
                            seed: 0x5752_5952,
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
        "temperature": INTERPRETATION_TEMPERATURE,
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
        "temperature": INTERPRETATION_TEMPERATURE,
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
    log::error!("judgement generation {generation_id} failed: {message}");
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

#[cfg(all(test, feature = "native-llama"))]
mod horary_reading_eval_tests {
    use super::*;
    use crate::{
        llama::StartLlamaRequest,
        native_llama_worker::{
            generate_native, native_llama_health, start_native_llama_from_path, stop_native_llama,
            NativeGenerateOptions, NativeLlamaState,
        },
    };
    use serde::Serialize;
    use serde_json::{json, Value};
    use std::{
        collections::HashSet,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone)]
    struct ReadingFixture {
        id: &'static str,
        question: &'static str,
        chart: Value,
        settings: Value,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ReadingEvalReport {
        model_path: String,
        mtp_model_path: Option<String>,
        health: Value,
        cases: Vec<Value>,
    }

    fn start_request(model_id: &str, mtp_enabled: bool) -> StartLlamaRequest {
        StartLlamaRequest {
            model_id: model_id.to_string(),
            ctx_size: Some(16_384),
            n_gpu_layers: Some("auto".to_string()),
            parallel: Some(2),
            continuous_batching: Some(true),
            cache_ram_mb: Some(4096),
            cache_idle_slots: Some(true),
            cold_kv_cache: Some(true),
            draft_model_id: mtp_enabled.then(|| "horary-eval-assistant".to_string()),
            spec_draft_n_max: Some(3),
        }
    }

    #[test]
    fn native_gemma_reads_horary_fixtures_with_reviewable_traces() {
        let Some(model_path) = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL") else {
            eprintln!("skipping horary reading eval: HORARY_NATIVE_LLAMA_TEST_MODEL is unset");
            return;
        };
        let model_path = PathBuf::from(model_path);
        assert!(
            model_path.is_file(),
            "horary reading eval model does not exist: {}",
            model_path.to_string_lossy()
        );
        let mtp_model_path = std::env::var_os("HORARY_NATIVE_LLAMA_MTP_MODEL")
            .map(PathBuf::from)
            .filter(|path| path.is_file());
        let mtp_enabled = mtp_model_path.is_some();

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let cache_dir = std::env::temp_dir().join(format!("horary-horary-eval-{millis}"));
        fs::create_dir_all(&cache_dir).unwrap();

        let state = NativeLlamaState::default();
        start_native_llama_from_path(
            &state,
            "horary-eval".to_string(),
            "horary-eval-test-model-sha".to_string(),
            model_path.clone(),
            cache_dir.clone(),
            start_request("horary-eval", mtp_enabled),
        )
        .unwrap();

        let mut case_reports = Vec::new();
        let mut failures = Vec::new();
        for fixture in reading_fixtures() {
            if std::env::var("HORARY_READING_EVAL_CASE").is_ok_and(|id| id != fixture.id) {
                continue;
            }
            let request = HoraryInterpretationRequest {
                question: fixture.question.to_string(),
                chart: fixture.chart.clone(),
                settings: Some(fixture.settings.clone()),
            };
            let prompt = build_interpretation_prompt(&request).unwrap();
            let prompt_text = native_prompt_text(&prompt).unwrap();
            let generation = generate_native(
                &state,
                prompt_text,
                NativeGenerateOptions {
                    max_tokens: MAX_INTERPRETATION_TOKENS,
                    response_schema: Some(horary_ai_core::interpretation_schema().to_string()),
                    temperature: INTERPRETATION_TEMPERATURE,
                    top_p: 1.0,
                    seed: 0x5752_5952,
                    token_sink: None,
                    cancel: None,
                },
            );

            let case_report = match generation {
                Ok(generation) => match parse_interpretation_content(&generation.content) {
                    Ok(interpretation) => {
                        let mut issues =
                            review_interpretation(&fixture, &prompt.risk, &interpretation);
                        if generation.prompt_tokens == 0 {
                            issues
                                .push("native generation reported zero prompt tokens".to_string());
                        }
                        if generation.generated_tokens == 0 {
                            issues.push(
                                "native generation reported zero generated tokens".to_string(),
                            );
                        }
                        if generation.tokens_per_second <= 0.0 {
                            issues.push(
                                "native generation reported non-positive throughput".to_string(),
                            );
                        }
                        if !issues.is_empty() {
                            failures.push(format!("{}: {}", fixture.id, issues.join("; ")));
                        }
                        json!({
                            "id": fixture.id,
                            "question": fixture.question,
                            "risk": prompt.risk,
                            "promptTokens": generation.prompt_tokens,
                            "generatedTokens": generation.generated_tokens,
                            "tokensPerSecond": generation.tokens_per_second,
                            "promptCacheHit": generation.prompt_cache_hit,
                            "coldCacheBytes": generation.cold_cache_bytes,
                            "issues": issues,
                            "interpretation": interpretation,
                        })
                    }
                    Err(error) => {
                        failures.push(format!("{}: {}", fixture.id, error.message));
                        json!({
                            "id": fixture.id,
                            "question": fixture.question,
                            "risk": prompt.risk,
                            "parseError": error.message,
                            "promptTokens": generation.prompt_tokens,
                            "generatedTokens": generation.generated_tokens,
                            "tokensPerSecond": generation.tokens_per_second,
                            "promptCacheHit": generation.prompt_cache_hit,
                            "rawOutput": generation.content,
                        })
                    }
                },
                Err(error) => {
                    failures.push(format!("{}: {}", fixture.id, error.message));
                    json!({
                        "id": fixture.id,
                        "question": fixture.question,
                        "generationError": error.message,
                    })
                }
            };
            println!(
                "horary eval case {}:\n{}",
                fixture.id,
                serde_json::to_string_pretty(&case_report).unwrap()
            );
            case_reports.push(case_report);
        }

        let health = native_llama_health(&state).unwrap();
        stop_native_llama(&state).unwrap();
        let _ = fs::remove_dir_all(&cache_dir);

        if mtp_enabled {
            assert!(
                health.speculative_decoding_active,
                "MTP draft model was present but speculative decoding is inactive"
            );
            assert!(
                health.speculative_draft_tokens > 0,
                "MTP draft model was present but produced no draft tokens"
            );
        }
        assert!(
            !case_reports.is_empty(),
            "No reading fixtures matched the requested case"
        );

        let report = ReadingEvalReport {
            model_path: model_path.to_string_lossy().to_string(),
            mtp_model_path: mtp_model_path.map(|path| path.to_string_lossy().to_string()),
            health: serde_json::to_value(&health).unwrap(),
            cases: case_reports,
        };
        if let Some(report_path) = std::env::var_os("HORARY_READING_EVAL_REPORT") {
            let report_path = PathBuf::from(report_path);
            if let Some(parent) = report_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(
                &report_path,
                serde_json::to_vec_pretty(&report).expect("report serializes"),
            )
            .unwrap();
        }

        assert!(
            failures.is_empty(),
            "horary reading eval failures:\n{}",
            failures.join("\n")
        );
    }

    fn review_interpretation(
        fixture: &ReadingFixture,
        risk: &str,
        interpretation: &HoraryInterpretation,
    ) -> Vec<String> {
        // Review the exact normalized facts supplied to the model, including
        // the book's next-cusp house adjustment, rather than the raw fixture.
        let fixture = ReadingFixture {
            chart: horary_ai_core::book_method::apply(&fixture.chart),
            ..fixture.clone()
        };
        let known_steps = known_microtask_ids();
        let minimum_steps = minimum_trace_steps();
        let mut issues = Vec::new();

        if interpretation.summary.split_whitespace().count() < 8 {
            issues.push("summary is too terse to be useful".to_string());
        }
        if risk == "ordinary" && interpretation.direct_answer.as_deref().is_none() {
            issues.push("ordinary horary omitted directAnswer".to_string());
        }
        if risk == "high_stakes" && interpretation.cautions.is_empty() {
            issues.push("high-stakes horary omitted cautions".to_string());
        }
        if interpretation.judgement_trace.len() < minimum_steps.len() {
            issues.push(format!(
                "judgementTrace has {} steps, expected at least {}",
                interpretation.judgement_trace.len(),
                minimum_steps.len()
            ));
        }

        let trace_steps = interpretation
            .judgement_trace
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<HashSet<_>>();
        for required in minimum_steps {
            if !trace_steps.contains(required.as_str()) {
                issues.push(format!("judgementTrace missing required step {required}"));
            }
        }
        for step in &interpretation.judgement_trace {
            if !known_steps.contains(&step.step_id) {
                issues.push(format!("unknown judgementTrace stepId {}", step.step_id));
            }
            if !evidence_mentions_chart_fact(&fixture.chart, &step.step_id, &step.chart_evidence) {
                issues.push(format!(
                    "trace step {} has generic or unsupported chartEvidence",
                    step.step_id
                ));
            }
        }
        for factor in &interpretation.key_factors {
            if !evidence_mentions_chart_fact(&fixture.chart, "key_factor", &factor.chart_evidence) {
                issues.push(format!(
                    "key factor '{}' has generic or unsupported chartEvidence",
                    factor.factor
                ));
            }
        }
        issues.extend(unsupported_body_placement_claims(
            &fixture.chart,
            interpretation,
        ));
        issues.extend(repeated_evidence_phrase_claims(interpretation));

        issues
    }

    fn known_microtask_ids() -> HashSet<String> {
        judgement_plan()["microTasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|task| task["id"].as_str().map(ToString::to_string))
            .collect()
    }

    fn minimum_trace_steps() -> Vec<String> {
        judgement_plan()["traceRequirement"]["minimumSteps"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|step| step.as_str().map(ToString::to_string))
            .collect()
    }

    fn evidence_mentions_chart_fact(chart: &Value, step_id: &str, evidence: &str) -> bool {
        let evidence = evidence.to_ascii_lowercase();
        if step_id == "question_scope"
            && (evidence.contains("question")
                || evidence.contains('?')
                || evidence.starts_with("will ")
                || evidence.starts_with("should "))
        {
            return true;
        }
        if step_id == "blockage_pass"
            && (evidence.contains("no explicit blockage")
                || evidence.contains("no blockage")
                || evidence.contains("no prohibition")
                || evidence.contains("no obstruction")
                || evidence.contains("obstructions: []")
                || evidence.contains("no applying major aspects supplied")
                || evidence.contains("no timing perfection supplied"))
        {
            return true;
        }
        if (step_id == "perfection_pass" || step_id == "key_factor")
            && (evidence.contains("no exact applying")
                || evidence.contains("no clear applying perfection")
                || evidence.contains("no applying perfection")
                || evidence.contains("no applying major aspects supplied")
                || evidence.contains("no timing perfection supplied"))
        {
            return true;
        }
        chart_evidence_terms(chart)
            .into_iter()
            .any(|term| evidence.contains(&term.to_ascii_lowercase()))
    }

    fn unsupported_body_placement_claims(
        chart: &Value,
        interpretation: &HoraryInterpretation,
    ) -> Vec<String> {
        let mut issues = Vec::new();
        let fields = interpretation_text_fields(interpretation);
        for body in chart["bodies"].as_array().into_iter().flatten() {
            let Some(name) = body["name"].as_str() else {
                continue;
            };
            let Some(actual_house) = body["house"].as_i64() else {
                continue;
            };
            for (field, text) in &fields {
                for claimed_house in 1..=12 {
                    if claimed_house == actual_house {
                        continue;
                    }
                    if body_house_claim_present(text, name, claimed_house) {
                        issues.push(format!(
                            "{field} claims {name} is in house {claimed_house}, but supplied body placement is house {actual_house}"
                        ));
                    }
                }
            }
        }
        issues
    }

    fn repeated_evidence_phrase_claims(interpretation: &HoraryInterpretation) -> Vec<String> {
        interpretation_text_fields(interpretation)
            .into_iter()
            .filter_map(|(field, text)| {
                let lower = text.to_ascii_lowercase();
                if lower.matches("synthesis pass").count() > 2
                    || lower.matches("chartEvidence").count() > 2
                    || repeated_colon_label(&lower)
                {
                    Some(format!("{field} repeats an evidence label or phrase"))
                } else {
                    None
                }
            })
            .collect()
    }

    fn interpretation_text_fields(
        interpretation: &HoraryInterpretation,
    ) -> Vec<(&'static str, String)> {
        let mut fields = vec![("summary", interpretation.summary.clone())];
        if let Some(direct_answer) = &interpretation.direct_answer {
            fields.push(("directAnswer", direct_answer.clone()));
        }
        for (index, step) in interpretation.judgement_trace.iter().enumerate() {
            fields.push(("judgementTrace.finding", step.finding.clone()));
            fields.push(("judgementTrace.chartEvidence", step.chart_evidence.clone()));
            if index > 16 {
                break;
            }
        }
        for factor in &interpretation.key_factors {
            fields.push(("keyFactors.chartEvidence", factor.chart_evidence.clone()));
            fields.push(("keyFactors.interpretation", factor.interpretation.clone()));
        }
        for caution in &interpretation.cautions {
            fields.push(("cautions", caution.clone()));
        }
        fields
    }

    fn body_house_claim_present(text: &str, body_name: &str, house: i64) -> bool {
        let text = text.to_ascii_lowercase();
        let body_name = body_name.to_ascii_lowercase();
        let ordinal = ordinal_house(house);
        [
            format!("{body_name} in house {house}"),
            format!("{body_name} is in house {house}"),
            format!("{body_name} in the {ordinal} house"),
            format!("{body_name} is in the {ordinal} house"),
            format!("{body_name} in {ordinal} house"),
            format!("{body_name} is in {ordinal} house"),
        ]
        .iter()
        .any(|pattern| text.contains(pattern))
    }

    fn ordinal_house(house: i64) -> String {
        match house {
            1 => "1st".to_string(),
            2 => "2nd".to_string(),
            3 => "3rd".to_string(),
            _ => format!("{house}th"),
        }
    }

    fn repeated_colon_label(text: &str) -> bool {
        let labels = [
            "synthesis pass chartEvidence:",
            "synthesis pass chartevidence:",
            "chartEvidence:",
            "chartevidence:",
        ];
        labels.iter().any(|label| text.matches(label).count() > 2)
    }

    fn chart_evidence_terms(chart: &Value) -> Vec<String> {
        let mut terms = Vec::new();
        for field in ["ascendant", "midheaven"] {
            if let Some(sign) = chart[field]["sign"].as_str() {
                terms.push(sign.to_string());
            }
        }
        for house in chart["houses"].as_array().into_iter().flatten() {
            if let Some(number) = house["number"].as_i64() {
                terms.push(format!("house {number}"));
                terms.push(format!("{number}th"));
                if number == 1 {
                    terms.push("1st".to_string());
                } else if number == 2 {
                    terms.push("2nd".to_string());
                } else if number == 3 {
                    terms.push("3rd".to_string());
                }
            }
            if let Some(sign) = house["sign"].as_str() {
                terms.push(sign.to_string());
            }
            if let Some(ruler) = house["ruler"].as_str() {
                terms.push(ruler.to_string());
            }
        }
        for body in chart["bodies"].as_array().into_iter().flatten() {
            if let Some(name) = body["name"].as_str() {
                terms.push(name.to_string());
            }
            if let Some(sign) = body["sign"].as_str() {
                terms.push(sign.to_string());
            }
        }
        for aspect in chart["aspects"].as_array().into_iter().flatten() {
            for field in ["planet1", "planet2", "aspectName"] {
                if let Some(value) = aspect[field].as_str() {
                    terms.push(value.to_string());
                }
            }
        }
        terms.push("void".to_string());
        terms.push("combust".to_string());
        terms.push("cazimi".to_string());
        terms
    }

    fn reading_fixtures() -> Vec<ReadingFixture> {
        vec![
            ReadingFixture {
                id: "full-london-chart",
                question: "Where is the lost ring?",
                chart: serde_json::from_str(include_str!(
                    "../test-fixtures/full-london-chart.json"
                ))
                .unwrap(),
                settings: eval_settings(),
            },
            ReadingFixture {
                id: "job-offer",
                question: "Will I get the job offer?",
                chart: job_offer_chart(),
                settings: eval_settings(),
            },
            ReadingFixture {
                id: "relationship-reconciliation",
                question: "Will my ex and I reconcile?",
                chart: relationship_chart(),
                settings: eval_settings(),
            },
            ReadingFixture {
                id: "lost-ring",
                question: "Will I find my lost ring?",
                chart: lost_ring_chart(),
                settings: eval_settings(),
            },
            ReadingFixture {
                id: "investment-high-stakes",
                question: "Should I invest my savings in this stock?",
                chart: investment_chart(),
                settings: eval_settings(),
            },
        ]
    }

    fn eval_settings() -> Value {
        json!({
            "tradition": "traditional",
            "traditionProfile": INTERPRETATION_TRADITION_PROFILE,
            "tone": "technical",
            "houseSystem": "regiomontanus",
            "zodiac": "tropical",
        })
    }

    fn base_chart(
        cast_utc: &str,
        asc_sign: &str,
        asc_ruler: &str,
        mc_sign: &str,
        house_10_ruler: &str,
        bodies: Value,
        aspects: Value,
        derived_extra: Value,
    ) -> Value {
        let mut derived = json!({
            "zodiac": "tropical",
            "houseSystem": "regiomontanus",
            "chartSect": "day",
            "ascendantRuler": asc_ruler,
            "voidOfCourseMoon": {
                "available": true,
                "isVoid": false,
                "checkedUntilHours": 24,
                "nextApplyingAspect": null,
                "reason": null
            },
            "receptions": [],
            "timingPatterns": [],
            "antisciaContacts": [],
            "solarConditions": [],
            "planetaryHour": {
                "available": true,
                "planetaryDayRuler": "Jupiter",
                "planetaryHourRuler": "Venus",
                "method": "fixture"
            }
        });
        merge_json(&mut derived, derived_extra);
        json!({
            "castLocalTime": cast_utc,
            "castUtcTime": cast_utc,
            "timezone": "UTC",
            "location": {
                "label": "New York, US",
                "latitude": 40.7128,
                "longitude": -74.006
            },
            "ascendant": { "sign": asc_sign, "degree": 12.0 },
            "midheaven": { "sign": mc_sign, "degree": 8.0 },
            "houses": [
                { "number": 1, "sign": asc_sign, "degree": 12.0, "ruler": asc_ruler },
                { "number": 2, "sign": "Scorpio", "degree": 12.0, "ruler": "Mars" },
                { "number": 3, "sign": "Sagittarius", "degree": 12.0, "ruler": "Jupiter" },
                { "number": 4, "sign": "Capricorn", "degree": 12.0, "ruler": "Saturn" },
                { "number": 5, "sign": "Aquarius", "degree": 12.0, "ruler": "Saturn" },
                { "number": 6, "sign": "Pisces", "degree": 12.0, "ruler": "Jupiter" },
                { "number": 7, "sign": "Aries", "degree": 12.0, "ruler": "Mars" },
                { "number": 8, "sign": "Taurus", "degree": 12.0, "ruler": "Venus" },
                { "number": 9, "sign": "Gemini", "degree": 12.0, "ruler": "Mercury" },
                { "number": 10, "sign": mc_sign, "degree": 8.0, "ruler": house_10_ruler },
                { "number": 11, "sign": "Leo", "degree": 12.0, "ruler": "Sun" },
                { "number": 12, "sign": "Virgo", "degree": 12.0, "ruler": "Mercury" }
            ],
            "bodies": bodies,
            "aspects": aspects,
            "derived": derived
        })
    }

    fn job_offer_chart() -> Value {
        base_chart(
            "2026-07-02T14:15:00.000Z",
            "Libra",
            "Venus",
            "Cancer",
            "Moon",
            json!([
                body(
                    "Venus",
                    "Taurus",
                    14.0,
                    8,
                    false,
                    1.1,
                    dignity("domicile"),
                    accidental(8, "succedent", 2, Value::Null)
                ),
                body(
                    "Moon",
                    "Virgo",
                    12.0,
                    11,
                    false,
                    12.4,
                    dignity("triplicity"),
                    accidental(11, "succedent", 2, Value::Null)
                ),
                body(
                    "Mars",
                    "Gemini",
                    9.0,
                    9,
                    false,
                    0.7,
                    dignity("peregrine"),
                    accidental(9, "cadent", -5, Value::Null)
                )
            ]),
            json!([
                aspect("Moon", "Trine", "Venus", 2.0, true, true),
                aspect("Moon", "Square", "Mars", 3.4, false, true)
            ]),
            json!({
                "receptions": [
                    { "hostPlanet": "Venus", "guestPlanet": "Moon", "dignity": "triplicity", "mutual": false }
                ],
                "timingPatterns": [
                    {
                        "type": "directPerfection",
                        "planet1": "Moon",
                        "planet2": "Venus",
                        "aspectName": "Trine",
                        "estimatedPerfectsWithinHours": 48,
                        "method": "fixture applying aspect"
                    }
                ]
            }),
        )
    }

    fn relationship_chart() -> Value {
        let mut chart = base_chart(
            "2026-07-02T18:20:00.000Z",
            "Gemini",
            "Mercury",
            "Aquarius",
            "Saturn",
            json!([
                body(
                    "Mercury",
                    "Cancer",
                    19.0,
                    1,
                    true,
                    -0.3,
                    dignity("term"),
                    accidental(1, "angular", 0, Value::Null)
                ),
                body(
                    "Jupiter",
                    "Capricorn",
                    20.2,
                    7,
                    false,
                    0.1,
                    dignity("fall"),
                    accidental(7, "angular", 5, Value::Null)
                ),
                body(
                    "Moon",
                    "Scorpio",
                    4.0,
                    5,
                    false,
                    13.0,
                    dignity("fall"),
                    accidental(5, "succedent", 2, Value::Null)
                )
            ]),
            json!([
                aspect("Mercury", "Opposition", "Jupiter", 1.2, true, true),
                aspect("Moon", "Trine", "Mercury", 2.8, true, true)
            ]),
            json!({
                "receptions": [
                    { "hostPlanet": "Moon", "guestPlanet": "Mercury", "dignity": "domicile", "mutual": false },
                    { "hostPlanet": "Saturn", "guestPlanet": "Jupiter", "dignity": "domicile", "mutual": false }
                ],
                "timingPatterns": [
                    {
                        "type": "directPerfection",
                        "planet1": "Mercury",
                        "planet2": "Jupiter",
                        "aspectName": "Opposition",
                        "estimatedPerfectsWithinHours": 30,
                        "method": "fixture applying aspect"
                    }
                ]
            }),
        );
        chart["houses"][6] =
            json!({ "number": 7, "sign": "Sagittarius", "degree": 12.0, "ruler": "Jupiter" });
        chart
    }

    fn lost_ring_chart() -> Value {
        let mut chart = base_chart(
            "2026-07-02T21:05:00.000Z",
            "Cancer",
            "Moon",
            "Pisces",
            "Jupiter",
            json!([
                body(
                    "Moon",
                    "Cancer",
                    7.0,
                    1,
                    false,
                    13.5,
                    dignity("domicile"),
                    accidental(1, "angular", 5, Value::Null)
                ),
                body(
                    "Sun",
                    "Gemini",
                    9.5,
                    12,
                    false,
                    0.9,
                    dignity("peregrine"),
                    accidental(12, "cadent", -5, Value::Null)
                ),
                body(
                    "Venus",
                    "Leo",
                    10.0,
                    2,
                    false,
                    1.0,
                    dignity("peregrine"),
                    accidental(2, "succedent", 2, Value::Null)
                )
            ]),
            json!([
                aspect("Moon", "Sextile", "Venus", 1.0, true, true),
                aspect("Moon", "Conjunction", "Ascendant", 0.5, false, false)
            ]),
            json!({
                "voidOfCourseMoon": {
                    "available": true,
                    "isVoid": false,
                    "checkedUntilHours": 24,
                    "nextApplyingAspect": {
                        "planet": "Venus",
                        "aspectName": "Sextile",
                        "perfectsWithinHours": 12
                    },
                    "reason": null
                },
                "timingPatterns": [
                    {
                        "type": "directPerfection",
                        "planet1": "Moon",
                        "planet2": "Venus",
                        "aspectName": "Sextile",
                        "estimatedPerfectsWithinHours": 12,
                        "method": "fixture applying aspect"
                    }
                ],
                "solarConditions": [
                    {
                        "planet": "Mercury",
                        "condition": "underBeams",
                        "separationDegrees": 11.0
                    }
                ]
            }),
        );
        chart["houses"][1] =
            json!({ "number": 2, "sign": "Taurus", "degree": 12.0, "ruler": "Venus" });
        chart
    }

    fn investment_chart() -> Value {
        let mut chart = base_chart(
            "2026-07-03T13:40:00.000Z",
            "Scorpio",
            "Mars",
            "Leo",
            "Sun",
            json!([
                body(
                    "Mars",
                    "Cancer",
                    22.0,
                    9,
                    false,
                    0.6,
                    dignity("fall"),
                    accidental(9, "cadent", -5, Value::Null)
                ),
                body(
                    "Jupiter",
                    "Gemini",
                    18.0,
                    8,
                    false,
                    0.2,
                    dignity("detriment"),
                    accidental(8, "succedent", 2, Value::Null)
                ),
                body(
                    "Moon",
                    "Sagittarius",
                    29.0,
                    2,
                    false,
                    13.1,
                    dignity("peregrine"),
                    accidental(2, "succedent", 2, Value::Null)
                ),
                body(
                    "Venus",
                    "Taurus",
                    5.0,
                    8,
                    false,
                    1.2,
                    dignity("domicile"),
                    accidental(8, "succedent", 2, Value::Null)
                )
            ]),
            json!([
                aspect("Moon", "Square", "Mars", 0.8, false, true),
                aspect("Jupiter", "Square", "Mars", 4.5, false, true)
            ]),
            json!({
                "voidOfCourseMoon": {
                    "available": true,
                    "isVoid": true,
                    "checkedUntilHours": 6,
                    "nextApplyingAspect": null,
                    "reason": "Moon leaves Sagittarius without another major applying aspect"
                },
                "receptions": [
                    { "hostPlanet": "Mercury", "guestPlanet": "Jupiter", "dignity": "domicile", "mutual": false }
                ]
            }),
        );
        chart["houses"][1]["sign"] = json!("Sagittarius");
        chart["houses"][1]["ruler"] = json!("Jupiter");
        chart
    }

    fn body(
        name: &str,
        sign: &str,
        degree: f64,
        house: i64,
        retrograde: bool,
        speed: f64,
        dignity: Value,
        accidental: Value,
    ) -> Value {
        json!({
            "name": name,
            "sign": sign,
            "degree": degree,
            "house": house,
            "retrograde": retrograde,
            "speed": speed,
            "dignity": dignity,
            "accidentalDignity": accidental,
        })
    }

    fn dignity(kind: &str) -> Value {
        json!({
            "domicile": kind == "domicile",
            "exaltation": kind == "exaltation",
            "triplicityRuler": kind == "triplicity",
            "termRuler": kind == "term",
            "faceRuler": kind == "face",
            "detriment": kind == "detriment",
            "fall": kind == "fall",
            "peregrine": kind == "peregrine",
            "score": match kind {
                "domicile" => 5,
                "exaltation" => 4,
                "triplicity" => 3,
                "term" => 2,
                "face" => 1,
                "detriment" => -5,
                "fall" => -4,
                _ => 0,
            }
        })
    }

    fn accidental(house: i64, angularity: &str, score: i64, solar_condition: Value) -> Value {
        json!({
            "house": house,
            "angularity": angularity,
            "retrograde": false,
            "solarCondition": solar_condition,
            "inJoy": false,
            "joyHouse": null,
            "score": score,
            "factors": [
                { "type": "angularity", "label": angularity, "score": score }
            ],
        })
    }

    fn aspect(
        planet1: &str,
        aspect_name: &str,
        planet2: &str,
        orb: f64,
        applying: bool,
        major: bool,
    ) -> Value {
        json!({
            "planet1": planet1,
            "aspectName": aspect_name,
            "planet2": planet2,
            "orb": orb,
            "applying": applying,
            "separating": !applying,
            "exact": false,
            "major": major,
        })
    }

    fn merge_json(target: &mut Value, source: Value) {
        let Some(target) = target.as_object_mut() else {
            return;
        };
        let Some(source) = source.as_object() else {
            return;
        };
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
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
        assert!(system.contains("deterministicAssignments are provisional keyword hints"));
        assert!(system.contains("chartEvidenceIndex.canonicalFacts"));
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
        assert_eq!(user["questionContext"]["domainModule"], "ordinary");
        assert_eq!(user["questionContext"]["likelyAssignments"][0]["house"], 1);
        assert_eq!(user["deterministicAssignments"][0]["actor"], "querent");
        assert_eq!(user["deterministicAssignments"][0]["house"], 1);
        assert_eq!(user["deterministicAssignments"][0]["houseSign"], "Libra");
        assert_eq!(user["deterministicAssignments"][0]["significator"], "Venus");
        assert_eq!(
            user["deterministicAssignments"][0]["significatorPlacement"]["house"],
            9
        );
        assert_eq!(
            user["deterministicAssignments"][0]["canonicalEvidence"],
            "house 1 Libra ruler Venus; Venus Gemini house 9"
        );
        assert_eq!(user["chartEvidenceIndex"]["bodyPlacements"][0]["house"], 4);
        assert!(user["chartEvidenceIndex"]["canonicalFacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact == "house 1 Libra ruler Venus"));
        assert!(user["chartEvidenceIndex"]["canonicalFacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact == "Venus Gemini house 9"));
        assert_eq!(
            user["chartEvidenceIndex"]["applyingMajorAspects"][0]["aspectName"],
            "Trine"
        );
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
        assert!(user["judgementPlan"]["commonHouseMap"]
            .as_array()
            .unwrap()
            .iter()
            .any(|house| house.as_str().unwrap_or_default().starts_with("7:")));
        assert!(user["judgementPlan"].get("domainModules").is_none());
        assert!(user["judgementPlan"]
            .get("futureMultiCallPipeline")
            .is_none());
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
    fn parser_repairs_common_small_model_json_shape_errors() {
        let repaired = parse_interpretation_content(
            r#"```json
{
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
}
```"#,
        )
        .unwrap();

        assert_eq!(repaired.key_factors[0].factor, "Moon-Venus");
        assert_eq!(
            repaired.key_factors[0].chart_evidence,
            "Moon trine Venus, 2.1 degree applying orb"
        );
    }

    #[test]
    fn parser_uses_direct_answer_as_summary_when_model_omits_summary() {
        let repaired = parse_interpretation_content(
            r#"{
                "directAnswer": "No clear perfection is supplied.",
                "confidence": "low",
                "judgementTrace": [{
                    "stepId": "synthesis_pass",
                    "finding": "No applying perfection is supplied.",
                    "chartEvidence": "Moon is void of course.",
                    "confidence": "low"
                }],
                "keyFactors": [{
                    "factor": "Void Moon",
                    "chartEvidence": "Moon is void of course.",
                    "interpretation": "The matter lacks flow."
                }],
                "cautions": [],
                "followUpQuestions": []
            }
            }"#,
        )
        .unwrap();

        assert_eq!(repaired.summary, "No clear perfection is supplied.");
    }

    #[test]
    fn parser_repairs_missing_top_level_commas_and_step_alias() {
        let repaired = parse_interpretation_content(
            r#"{
                "summary": "The chart shows a constructive contact.",
                "confidence": "medium",
                "judgementTrace": [{
                    "step": "synthesis_pass",
                    "finding": "Moon applies to Venus.",
                    "chartEvidence": "Moon trine Venus, 2.1 degree applying orb",
                    "confidence": "medium"
                }]
                "keyFactors": [{
                    "chartEvidence": "Moon trine Venus, 2.1 degree applying orb",
                    "interpretation": "A constructive contact is forming."
                }]
                ]
                "cautions": [],
                "followUpQuestions": []
            }"#,
        )
        .unwrap();

        assert_eq!(repaired.judgement_trace[0].step_id, "synthesis_pass");
        assert_eq!(
            repaired.key_factors[0].factor,
            "Moon trine Venus, 2.1 degree applying orb"
        );
        assert_eq!(
            repaired.direct_answer.as_deref(),
            Some("The chart shows a constructive contact.")
        );
    }

    #[test]
    fn parser_dedupes_repeated_evidence_segments_and_caps_key_factors() {
        let repaired = parse_interpretation_content(
            r#"{
                "summary": "The chart shows a constructive contact.",
                "confidence": "medium",
                "judgementTrace": [{
                    "stepId": "synthesis_pass",
                    "finding": "Moon applies to Venus.",
                    "chartEvidence": "Moon trine Venus; Moon trine Venus",
                    "confidence": "medium"
                }],
                "keyFactors": [
                    {
                        "factor": "Moon trine Venus; Moon trine Venus",
                        "chartEvidence": "Moon trine Venus; Moon trine Venus",
                        "interpretation": "A constructive contact is forming."
                    },
                    {
                        "factor": "Venus",
                        "chartEvidence": "Venus Taurus house 8",
                        "interpretation": "Strong dignity."
                    },
                    {
                        "factor": "Moon",
                        "chartEvidence": "Moon Virgo house 11",
                        "interpretation": "The quesited has practical capacity."
                    },
                    {
                        "factor": "Timing",
                        "chartEvidence": "timing directPerfection Moon Trine Venus 48h",
                        "interpretation": "The event is indicated."
                    },
                    {
                        "factor": "Extra",
                        "chartEvidence": "house 10 Cancer ruler Moon",
                        "interpretation": "This should be truncated."
                    }
                ],
                "cautions": [],
                "followUpQuestions": []
            }"#,
        )
        .unwrap();

        assert_eq!(
            repaired.judgement_trace[0].chart_evidence,
            "Moon trine Venus"
        );
        assert_eq!(repaired.key_factors[0].factor, "Moon trine Venus");
        assert_eq!(repaired.key_factors[0].chart_evidence, "Moon trine Venus");
        assert_eq!(repaired.key_factors.len(), 4);
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
