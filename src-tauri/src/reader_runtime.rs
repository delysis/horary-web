//! The reading uses one local owner: native-kit, an installed sidecar, or an
//! explicitly borrowed loopback server. A selected server never falls back to a
//! second resident model on failure.
#![forbid(unsafe_code)]

use crate::{
    inference::{active_inference_connection, LocalInferenceConnection},
    llama::{LlamaError, LlamaResult, LlamaState, LlamaStatus, StartLlamaRequest},
    native_llama_worker::{
        generate_native, generate_native_batch, start_native_llama_in_dir, stop_native_llama,
        NativeGenerateOptions, NativeGenerationResult, NativeLlamaState,
    },
};
use base64::Engine;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{atomic::Ordering, OnceLock},
    time::{Duration, Instant},
};
use tauri::Manager;

pub(crate) const LOCAL_ENDPOINT_ENV: &str = "HORARY_LOCAL_LLAMA_ENDPOINT";

fn error(message: impl ToString) -> LlamaError {
    LlamaError {
        message: message.to_string(),
    }
}

/// A borrowed endpoint must use the same IPv4 loopback transport as LlamaState.
/// Reject credentials, redirects, remote hosts, and arbitrary URL path routing.
fn endpoint(value: &str) -> LlamaResult<(reqwest::Url, u16)> {
    let mut url = reqwest::Url::parse(value).map_err(error)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path().trim_end_matches('/'), "" | "/v1")
    {
        return Err(error("The local reader endpoint must be loopback HTTP, with no credentials or extra routing."));
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| error("Missing local reader port"))?;
    url.set_host(Some("127.0.0.1")).map_err(error)?;
    url.set_path("/");
    Ok((url, port))
}

fn http() -> LlamaResult<reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(300))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .cloned()
        .map_err(error)
}

async fn properties(client: &reqwest::Client, base: &reqwest::Url) -> LlamaResult<Value> {
    let response = client
        .get(base.join("props").map_err(error)?)
        .send()
        .await
        .map_err(error)?;
    if !response.status().is_success() {
        return Err(error(format!(
            "The selected local reader is unavailable (HTTP {}).",
            response.status()
        )));
    }
    response.json().await.map_err(error)
}

fn check_model(properties: &Value, expected: &Path) -> LlamaResult<()> {
    let reported = properties["model_path"]
        .as_str()
        .ok_or_else(|| error("The selected local reader did not identify its loaded model."))?;
    if std::fs::canonicalize(reported).map_err(error)?
        != std::fs::canonicalize(expected).map_err(error)?
    {
        return Err(error(
            "The selected local reader loaded a different model. The reading was not submitted.",
        ));
    }
    Ok(())
}

fn check_context(properties: &Value, requested: Option<u32>) -> LlamaResult<()> {
    if let Some(requested) = requested {
        let available = properties["default_generation_settings"]["n_ctx"]
            .as_u64()
            .ok_or_else(|| {
                error("The selected local reader did not report its context capacity.")
            })?;
        if available < u64::from(requested) {
            return Err(error("The selected local reader has too little context for the reading. No reading was submitted."));
        }
    }
    Ok(())
}

fn reading_request(owned_sidecar: bool) -> StartLlamaRequest {
    // llama-server divides its arena across slots. Keep 32k available to each
    // independent stage; generic start requests retain their original meaning.
    StartLlamaRequest {
        model_id: crate::hf_cache::RECOMMENDED_ID.into(),
        ctx_size: Some(if owned_sidecar {
            131_072
        } else {
            crate::native_llama_worker::READING_CONTEXT_TOKENS
        }),
        n_gpu_layers: Some("auto".into()),
        parallel: Some(4),
        continuous_batching: Some(true),
        cache_ram_mb: Some(4096),
        cache_idle_slots: Some(true),
        cold_kv_cache: Some(false),
        draft_model_id: None,
        spec_draft_n_max: Some(3),
    }
}

pub(crate) fn start_reading(app: &tauri::AppHandle, dir: &Path) -> LlamaResult<LlamaStatus> {
    let resources = app.path().resource_dir().ok();
    let installed = crate::llama::get_llama_sidecar_status(dir, resources.as_deref()).installed;
    let native_request = reading_request(false);
    let owned_sidecar = std::env::var_os(LOCAL_ENDPOINT_ENV).is_none()
        && (!cfg!(feature = "native-llama")
            || installed
                && crate::llama::resolve_optional_draft_model_path(dir, &native_request)?
                    .is_some());
    start(app, dir, reading_request(owned_sidecar))
}

pub(crate) fn start(
    app: &tauri::AppHandle,
    dir: &Path,
    req: StartLlamaRequest,
) -> LlamaResult<LlamaStatus> {
    let native = app.state::<NativeLlamaState>();
    let sidecar = app.state::<LlamaState>();
    if let Some(value) = std::env::var_os(LOCAL_ENDPOINT_ENV) {
        let value = value
            .to_str()
            .ok_or_else(|| error("Invalid local reader endpoint"))?;
        let (base, port) = endpoint(value)?;
        // Verify the pinned payload on initial attachment. A resident attachment
        // already checked it; do not rehash gigabytes on every conversation turn.
        let resident = active_inference_connection(&sidecar)?.is_some_and(|old| {
            old.model_id == req.model_id
                && old.chat_completions_endpoint
                    == crate::inference::sidecar_chat_completions_endpoint(port)
        });
        let registered = if resident {
            crate::hf_cache::registered_models(dir)?
        } else {
            Vec::new()
        };
        let model = match registered
            .into_iter()
            .find(|model| model.id == req.model_id)
        {
            Some(model) => model,
            None => crate::llama::get_model_by_id(dir, &req.model_id)?,
        };
        let expected = crate::llama::resolve_model_path(dir, &model.filename)?;
        let properties = tauri::async_runtime::block_on(properties(&http()?, &base))?;
        check_model(&properties, &expected)?;
        check_context(&properties, req.ctx_size)?;
        let status = crate::llama::attach_llama_connection(&sidecar, req.model_id, port)?;
        stop_native_llama(&native)?;
        return Ok(status);
    }
    let resources = app.path().resource_dir().ok();
    let installed = crate::llama::get_llama_sidecar_status(dir, resources.as_deref()).installed;
    let use_sidecar = req.draft_model_id.is_some()
        || !cfg!(feature = "native-llama")
        || installed && crate::llama::resolve_optional_draft_model_path(dir, &req)?.is_some();
    if use_sidecar {
        let requested_slot = owned_slot_context(&req)?;
        stop_native_llama(&native)?;
        let status =
            crate::llama::start_llama_with_sidecar_dir(dir, resources.as_deref(), &sidecar, req)?;
        // A previously started legacy owner may have a smaller arena. Reuse
        // its weights only when its actual slot can hold the requested stage.
        if requested_slot.is_some() {
            let port = status
                .port
                .ok_or_else(|| error("Local reader has no port"))?;
            let (base, _) = endpoint(&format!("http://127.0.0.1:{port}"))?;
            let properties = tauri::async_runtime::block_on(properties(&http()?, &base))?;
            check_context(&properties, requested_slot)?;
        }
        return Ok(status);
    }
    crate::llama::stop_llama_process(&sidecar)?;
    start_native_llama_in_dir(dir, &native, req)
}

fn owned_slot_context(req: &StartLlamaRequest) -> LlamaResult<Option<u32>> {
    req.ctx_size
        .map(|_| {
            crate::llama::normalize_llama_launch_args(req)
                .map(|profile| profile.ctx_size / profile.parallel)
        })
        .transpose()
}

pub(crate) fn generate(
    app: &tauri::AppHandle,
    prompt: String,
    options: NativeGenerateOptions,
) -> LlamaResult<NativeGenerationResult> {
    match active_inference_connection(&app.state::<LlamaState>())? {
        Some(connection) => generate_server(&connection, &prompt, options),
        None => generate_native(&app.state::<NativeLlamaState>(), prompt, options),
    }
}

pub(crate) fn generate_batch(
    app: &tauri::AppHandle,
    prompts: Vec<(String, u32)>,
    options: NativeGenerateOptions,
) -> LlamaResult<Vec<NativeGenerationResult>> {
    let Some(connection) = active_inference_connection(&app.state::<LlamaState>())? else {
        return generate_native_batch(&app.state::<NativeLlamaState>(), prompts, options);
    };
    if prompts.is_empty() || prompts.len() > 4 || options.audio.is_some() {
        return Err(error(
            "A reading batch needs one to four independent text tasks.",
        ));
    }
    // Reuse the existing stage batch; each HTTP task owns its prompt and output
    // limit. Join every branch before returning, including a failed sibling.
    std::thread::scope(|scope| {
        let workers: Vec<_> = prompts
            .iter()
            .map(|(prompt, max)| {
                let mut task = options.clone();
                task.max_tokens = *max;
                let connection = &connection;
                scope.spawn(move || generate_server(connection, prompt, task))
            })
            .collect();
        let settled: Vec<_> = workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err(error("Local reading worker panicked")))
            })
            .collect();
        settled.into_iter().collect()
    })
}

fn request(
    model: &str,
    prompt: &str,
    options: &NativeGenerateOptions,
    audio_supported: bool,
) -> LlamaResult<Value> {
    if options.max_tokens == 0
        || options.max_tokens > 16_384
        || !options.temperature.is_finite()
        || options.temperature < 0.
        || !options.top_p.is_finite()
        || !(0. ..=1.).contains(&options.top_p)
    {
        return Err(error(
            "The local reading needs bounded output and finite sampling controls.",
        ));
    }
    let mut messages: Value = serde_json::from_str(prompt).map_err(error)?;
    let array = messages
        .as_array_mut()
        .ok_or_else(|| error("The reading prompt must be a message array."))?;
    if array.is_empty()
        || array.iter().any(|message| {
            !matches!(
                message["role"].as_str(),
                Some("system" | "user" | "assistant")
            ) || message["content"].as_str().is_none()
        })
    {
        return Err(error("Reading messages need supported roles and text."));
    }
    if let Some(audio) = &options.audio {
        if !audio_supported {
            return Err(error("The selected local reader has no audio projector. Start that reader with the matching projector before speaking directly to it."));
        }
        if audio.len() < 12 || &audio[..4] != b"RIFF" || &audio[8..12] != b"WAVE" {
            return Err(error("Direct voice needs a valid WAV recording."));
        }
        let user = array
            .last_mut()
            .filter(|message| message["role"] == "user")
            .ok_or_else(|| error("Direct audio must belong to the current final user message."))?;
        let text = user["content"].as_str().unwrap().to_owned();
        let data = format!(
            "data:audio/wav;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(audio)
        );
        user["content"] = json!([
            {"type":"text","text":text},
            {"type":"input_audio","input_audio":{"data":data,"format":"wav"}}
        ]);
    }
    let mut body = json!({
        "model":model,"messages":messages,"max_tokens":options.max_tokens,
        "temperature":options.temperature,"top_p":options.top_p,"seed":options.seed,
        "stream":false,"cache_prompt":options.cache_lesson
    });
    if model == crate::hf_cache::RECOMMENDED_ID {
        body["chat_template_kwargs"] = json!({"enable_thinking":false});
    }
    // Preserve the caller's existing constrained/native option. Ordinary
    // callers omit it; native worksheet validation/repair remains unchanged.
    if let Some(schema) = &options.response_schema {
        let schema: Value = serde_json::from_str(schema).map_err(error)?;
        body["response_format"] = json!({"type":"json_schema","json_schema":{
            "name":"horary_stage","strict":true,"schema":schema
        }});
    }
    Ok(body)
}

fn cancelled(options: &NativeGenerateOptions) -> bool {
    options
        .cancel
        .as_ref()
        .is_some_and(|cancel| cancel.load(Ordering::Acquire))
}

fn generate_server(
    connection: &LocalInferenceConnection,
    prompt: &str,
    options: NativeGenerateOptions,
) -> LlamaResult<NativeGenerationResult> {
    let started = Instant::now();
    let client = http()?;
    let mut base = reqwest::Url::parse(&connection.chat_completions_endpoint).map_err(error)?;
    base.set_path("/");
    // Audio capability is checked against the actual server before attaching
    // the bytes. Do not silently transcribe or load another model on failure.
    let generation = async {
        let audio_supported = if options.audio.is_some() {
            let properties = properties(&client, &base).await?;
            properties["modalities"]["audio"] == true
        } else {
            false
        };
        let body = request(&connection.model_id, prompt, &options, audio_supported)?;
        let response = client
            .post(&connection.chat_completions_endpoint)
            .json(&body)
            .send()
            .await
            .map_err(error)?;
        if !response.status().is_success() {
            return Err(error(format!(
                "The local reader request failed (HTTP {}). No automatic retry was made.",
                response.status()
            )));
        }
        let body: Value = response.json().await.map_err(error)?;
        decode(&body, started.elapsed().as_millis())
    };
    let result = tauri::async_runtime::block_on(async {
        tokio::pin!(generation);
        loop {
            if cancelled(&options) {
                return Err(error("Reading generation cancelled."));
            }
            tokio::select! {
                result = &mut generation => return result,
                _ = tokio::time::sleep(Duration::from_millis(40)) => {}
            }
        }
    })?;
    if cancelled(&options) {
        return Err(error("Reading generation cancelled."));
    }
    if let Some(sink) = options.token_sink {
        let _ = sink.send(result.content.clone());
    }
    Ok(result)
}

fn decode(body: &Value, wall_ms: u128) -> LlamaResult<NativeGenerationResult> {
    let choices = body["choices"]
        .as_array()
        .ok_or_else(|| error("Local reader returned no choices."))?;
    if choices.len() != 1
        || !matches!(
            choices[0]["finish_reason"].as_str(),
            Some("stop" | "length")
        )
    {
        return Err(error("Local reader did not deliver one text completion."));
    }
    let content = choices[0]["message"]["content"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| error("Local reader returned no answer."))?;
    let tokens = |value: &Value| -> LlamaResult<u32> {
        u32::try_from(value.as_u64().unwrap_or(0)).map_err(error)
    };
    let prompt_tokens = tokens(&body["usage"]["prompt_tokens"])?;
    let generated_tokens = tokens(&body["usage"]["completion_tokens"])?;
    let cached_prompt_tokens = tokens(&body["usage"]["prompt_tokens_details"]["cached_tokens"])?;
    if cached_prompt_tokens > prompt_tokens {
        return Err(error(
            "Local reader returned inconsistent cache-token counts.",
        ));
    }
    Ok(NativeGenerationResult {
        content: content.into(),
        prompt_tokens,
        generated_tokens,
        elapsed_ms: wall_ms,
        total_wall_ms: Some(wall_ms),
        batch_size: Some(1),
        lesson_bank_hit: None,
        lesson_prepare_ms: None,
        tokens_per_second: body["timings"]["predicted_per_second"]
            .as_f64()
            .unwrap_or(0.),
        prompt_cache_hit: cached_prompt_tokens > 0,
        cached_prompt_tokens,
        prefilled_prompt_tokens: prompt_tokens - cached_prompt_tokens,
        first_token_ms: None,
        cold_cache_bytes: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{atomic::AtomicBool, Arc},
    };

    fn mock_server(
        replies: Vec<(u16, Value)>,
    ) -> (
        LocalInferenceConnection,
        std::thread::JoinHandle<Vec<(String, Value)>>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut raw = Vec::new();
                let (head, size) = loop {
                    let mut buf = [0_u8; 2048];
                    let count = stream.read(&mut buf).unwrap();
                    assert!(count > 0);
                    raw.extend_from_slice(&buf[..count]);
                    if let Some(head) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..head]);
                        let size = headers
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                            .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        break (head + 4, size);
                    }
                };
                while raw.len() < head + size {
                    let mut buf = [0_u8; 2048];
                    let count = stream.read(&mut buf).unwrap();
                    assert!(count > 0);
                    raw.extend_from_slice(&buf[..count]);
                }
                let line = String::from_utf8_lossy(&raw[..head])
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                let received = if size == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&raw[head..head + size]).unwrap()
                };
                requests.push((line, received));
                let text = body.to_string();
                let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (
            LocalInferenceConnection {
                backend: crate::llama::LLAMA_SIDECAR_BACKEND,
                model_id: "reader".into(),
                chat_completions_endpoint: crate::inference::sidecar_chat_completions_endpoint(
                    port,
                ),
            },
            worker,
        )
    }

    fn answer() -> Value {
        json!({"choices":[{"finish_reason":"stop","message":{"content":"Observed answer"}}],
            "usage":{"prompt_tokens":100,"completion_tokens":2}})
    }

    #[test]
    fn explicit_endpoint_is_local_without_credentials_or_extra_routes() {
        for value in ["http://127.0.0.1:63924", "http://localhost:63924/v1/"] {
            let (base, port) = endpoint(value).unwrap();
            assert_eq!(base.as_str(), "http://127.0.0.1:63924/");
            assert_eq!(port, 63924);
        }
        for value in [
            "https://127.0.0.1:63924",
            "http://example.com",
            "http://name:secret@127.0.0.1",
            "http://127.0.0.1/other",
            "http://127.0.0.1?q=x",
            "http://127.0.0.1/#x",
            "",
        ] {
            assert!(endpoint(value).is_err(), "{value}");
        }
    }

    #[test]
    fn attached_server_must_report_the_actual_selected_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.gguf");
        let other = dir.path().join("other.gguf");
        std::fs::write(&target, b"target").unwrap();
        std::fs::write(&other, b"other").unwrap();
        check_model(&json!({"model_path":target}), &target).unwrap();
        assert!(check_model(&json!({"model_path":other}), &target).is_err());
        assert!(check_model(&json!({}), &target).is_err());
    }

    #[test]
    fn reading_slots_keep_full_context_and_missing_capacity_is_unknown() {
        let owned = reading_request(true);
        let native = reading_request(false);
        assert_eq!(owned.ctx_size.unwrap() / owned.parallel.unwrap(), 32768);
        assert_eq!(owned_slot_context(&owned).unwrap(), Some(32768));
        assert_eq!(
            native.ctx_size,
            Some(crate::native_llama_worker::READING_CONTEXT_TOKENS)
        );
        assert_eq!(owned.cold_kv_cache, Some(false));
        check_context(
            &json!({"default_generation_settings":{"n_ctx":32768}}),
            native.ctx_size,
        )
        .unwrap();
        assert!(check_context(
            &json!({"default_generation_settings":{"n_ctx":8192}}),
            native.ctx_size
        )
        .is_err());
        assert!(check_context(&json!({}), native.ctx_size).is_err());
    }

    #[test]
    fn ordinary_text_preserves_turns_limits_and_sampling_without_a_schema() {
        let prompt = json!([
            {"role":"system","content":"The actual lesson."},
            {"role":"user","content":"My original question."},
            {"role":"assistant","content":"My rejected proposal."},
            {"role":"user","content":"Repair only that proposal."}
        ]);
        let options = NativeGenerateOptions {
            max_tokens: 731,
            seed: 43,
            top_p: 0.8,
            temperature: 0.3,
            cache_lesson: true,
            ..Default::default()
        };
        let body = request("actual-reader", &prompt.to_string(), &options, false).unwrap();
        assert_eq!(body["messages"], prompt);
        assert_eq!(body["model"], "actual-reader");
        assert_eq!(body["max_tokens"], 731);
        assert_eq!(body["seed"], 43);
        assert_eq!(body["cache_prompt"], true);
        assert!(body.get("response_format").is_none());
        assert!(body.get("chat_template_kwargs").is_none());
        let gemma = request(
            crate::hf_cache::RECOMMENDED_ID,
            &prompt.to_string(),
            &options,
            false,
        )
        .unwrap();
        assert_eq!(gemma["messages"], prompt);
        assert_eq!(gemma["chat_template_kwargs"]["enable_thinking"], false);
    }

    #[test]
    fn audio_preserves_text_and_exact_wav_and_requires_a_real_projector() {
        let wav = b"RIFF\x04\0\0\0WAVE";
        let prompt = json!([{"role":"user","content":"These spoken words belong to this turn."}])
            .to_string();
        let options = NativeGenerateOptions {
            audio: Some(wav.to_vec()),
            ..Default::default()
        };
        assert!(request("reader", &prompt, &options, false)
            .unwrap_err()
            .message
            .contains("projector"));
        let body = request("reader", &prompt, &options, true).unwrap();
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            "These spoken words belong to this turn."
        );
        let encoded = body["messages"][0]["content"][1]["input_audio"]["data"]
            .as_str()
            .unwrap()
            .strip_prefix("data:audio/wav;base64,")
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            wav
        );
        let invalid = NativeGenerateOptions {
            audio: Some(b"not wav".to_vec()),
            ..Default::default()
        };
        assert!(request("reader", &prompt, &invalid, true).is_err());
    }

    #[test]
    fn requested_schema_is_explicit_and_native_callers_can_still_omit_it() {
        let schema = json!({"type":"object","required":["answer"],"properties":{"answer":{"type":"string"}}});
        let options = NativeGenerateOptions {
            response_schema: Some(schema.to_string()),
            ..Default::default()
        };
        let body = request(
            "reader",
            r#"[{"role":"user","content":"question"}]"#,
            &options,
            false,
        )
        .unwrap();
        assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
    }

    #[test]
    fn decoded_metrics_remain_observed_and_inconsistent_counts_fail() {
        let mut body = json!({"choices":[{"finish_reason":"stop","message":{"content":"The real answer."}}],
            "usage":{"prompt_tokens":700,"completion_tokens":50,"prompt_tokens_details":{"cached_tokens":500}},
            "timings":{"predicted_per_second":37.5}});
        let result = decode(&body, 1200).unwrap();
        assert_eq!(result.content, "The real answer.");
        assert_eq!(result.cached_prompt_tokens, 500);
        assert_eq!(result.prefilled_prompt_tokens, 200);
        assert_eq!(result.tokens_per_second, 37.5);
        body["usage"]["prompt_tokens_details"]["cached_tokens"] = json!(701);
        assert!(decode(&body, 1200).is_err());
    }

    #[test]
    fn actual_http_audio_checks_properties_before_sending_exact_current_turn() {
        let (connection, worker) = mock_server(vec![
            (200, json!({"modalities":{"audio":true}})),
            (200, answer()),
        ]);
        let prompt = r#"[{"role":"user","content":"Actual current question"}]"#;
        let options = NativeGenerateOptions {
            audio: Some(b"RIFF\x04\0\0\0WAVE".to_vec()),
            ..Default::default()
        };
        let result = generate_server(&connection, prompt, options).unwrap();
        assert_eq!(result.content, "Observed answer");
        let requests = worker.join().unwrap();
        assert!(requests[0].0.starts_with("GET /props "));
        assert!(requests[1].0.starts_with("POST /v1/chat/completions "));
        assert_eq!(
            requests[1].1["messages"][0]["content"][0]["text"],
            "Actual current question"
        );
        assert!(requests[1].1.get("response_format").is_none());
    }

    #[test]
    fn missing_projector_stops_before_audio_generation_without_fallback() {
        let (connection, worker) = mock_server(vec![(200, json!({"modalities":{"audio":false}}))]);
        let options = NativeGenerateOptions {
            audio: Some(b"RIFF\x04\0\0\0WAVE".to_vec()),
            ..Default::default()
        };
        let error = generate_server(
            &connection,
            r#"[{"role":"user","content":"question"}]"#,
            options,
        )
        .unwrap_err();
        assert!(error.message.contains("projector"));
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[test]
    fn complete_server_failure_is_not_silently_retried() {
        let (connection, worker) =
            mock_server(vec![(500, json!({"error":"complete service failure"}))]);
        assert!(generate_server(
            &connection,
            r#"[{"role":"user","content":"question"}]"#,
            Default::default()
        )
        .unwrap_err()
        .message
        .contains("No automatic retry"));
        assert_eq!(worker.join().unwrap().len(), 1);
    }

    #[test]
    fn cancellation_before_submission_does_not_send_a_request() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let connection = LocalInferenceConnection {
            backend: crate::llama::LLAMA_SIDECAR_BACKEND,
            model_id: "reader".into(),
            chat_completions_endpoint: crate::inference::sidecar_chat_completions_endpoint(
                listener.local_addr().unwrap().port(),
            ),
        };
        let options = NativeGenerateOptions {
            cancel: Some(Arc::new(AtomicBool::new(true))),
            ..Default::default()
        };
        assert!(generate_server(
            &connection,
            r#"[{"role":"user","content":"question"}]"#,
            options
        )
        .unwrap_err()
        .message
        .contains("cancelled"));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
