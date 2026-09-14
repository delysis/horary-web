#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "native-llama"), allow(dead_code, unused_imports))]
use crate::llama::{LlamaError, LlamaResult, LlamaStatus, StartLlamaRequest};
use serde::Serialize;
#[cfg(test)]
use std::path::PathBuf;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
pub const NATIVE_LLAMA_RUNTIME_BACKEND: &str = "llama-native-kit";
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeLlamaHealth {
    pub compiled: bool,
    pub running: bool,
    pub model_id: Option<String>,
    pub backend: &'static str,
    pub ctx_size: Option<u32>,
    pub parallel: Option<u32>,
    pub speculative_decoding_supported: bool,
    pub speculative_decoding_active: bool,
    pub draft_model_id: Option<String>,
    pub hot_cache_entries: u64,
    pub hot_cache_hits: u64,
    pub cold_cache_hits: u64,
    pub cold_cache_writes: u64,
    pub speculative_draft_tokens: u64,
    pub speculative_accepted_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct NativeGenerateOptions {
    pub audio: Option<Vec<u8>>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub seed: u32,
    pub response_schema: Option<String>,
    pub token_sink: Option<mpsc::Sender<String>>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for NativeGenerateOptions {
    fn default() -> Self {
        Self {
            audio: None,
            max_tokens: 256,
            temperature: 0.2,
            top_p: 1.0,
            seed: 0,
            response_schema: None,
            token_sink: None,
            cancel: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeGenerationResult {
    pub content: String,
    pub prompt_tokens: u32,
    pub generated_tokens: u32,
    pub elapsed_ms: u128,
    pub tokens_per_second: f64,
    pub prompt_cache_hit: bool,
    pub cold_cache_bytes: Option<usize>,
}

#[derive(Default)]
pub struct NativeLlamaState {
    running: Mutex<Option<Arc<Loaded>>>,
}
#[cfg(feature = "native-llama")]
struct Loaded {
    host: llama_native_host::NativeHost,
    config: llama_native_types::NativeModelConfig,
    cache_hits: AtomicU64,
}
#[cfg(not(feature = "native-llama"))]
struct Loaded;
fn error(message: impl ToString) -> LlamaError {
    LlamaError {
        message: message.to_string(),
    }
}

#[cfg(feature = "native-llama")]
mod imp {
    use super::*;
    use llama_native_engine::WaitOutcome;
    use llama_native_host::{HostCachePolicy, NativeHost, NativeHostConfig};
    use llama_native_types::{
        ChatMessage, CompletionPrompt, GenerationEventKind, GenerationInput, GenerationRequest,
        GenerationState, NativeModelConfig, SamplingConfig, SpecialTokenPolicy,
    };
    static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

    pub fn start_native_llama_in_dir(
        dir: &Path,
        state: &NativeLlamaState,
        req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        let model = crate::llama::get_model_by_id(dir, &req.model_id)?;
        let path = crate::llama::resolve_model_path(dir, &model.filename)?;
        let mut config = NativeModelConfig::local(path);
        config.model_id = model.id;
        config.expected_model_sha256 = Some(model.sha256);
        if let Ok(projector) =
            crate::llama::get_model_by_id(dir, &format!("{}-projector", req.model_id))
        {
            config.mmproj_path = Some(crate::llama::resolve_model_path(dir, &projector.filename)?);
            config.expected_mmproj_sha256 = Some(projector.sha256);
        }
        start(state, config, req)
    }
    fn start(
        state: &NativeLlamaState,
        mut config: NativeModelConfig,
        req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        let mut slot = state.running.lock().map_err(error)?;
        config.context_tokens = req.ctx_size.unwrap_or(16384);
        if !(2048..=32768).contains(&config.context_tokens) {
            return Err(error("Context size must be between 2048 and 32768 tokens."));
        }
        if req.draft_model_id.is_some() {
            return Err(error(
                "This native-kit runtime does not support a speculative helper.",
            ));
        }
        let layers = crate::llama::normalize_gpu_layers(req.n_gpu_layers.as_deref())?;
        if layers != "auto" {
            config.gpu_layers = layers.parse().map_err(error)?;
        }
        config.max_sequences = 1;
        config.batch_tokens = 512;
        if let Some(old) = slot.as_ref() {
            if old.config == config {
                return Ok(status(Some(&config.model_id)));
            }
            old.host.shutdown_joined().map_err(error)?;
            *slot = None;
        }
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let memory_budget = (system.total_memory() / 4 * 3).min(20 * 1024 * 1024 * 1024);
        if memory_budget == 0 {
            return Err(error(
                "Could not determine memory available for the local model.",
            ));
        }
        let host = NativeHost::new(NativeHostConfig {
            memory_budget_bytes: memory_budget,
            max_slots: 1,
            memory_cache_bytes: 128 * 1024 * 1024,
            cache_namespace: "horary".into(),
            cache_policy: HostCachePolicy::MemoryOnly,
        });
        host.load_into_slot(0, config.clone()).map_err(error)?;
        let id = config.model_id.clone();
        *slot = Some(Arc::new(Loaded {
            host,
            config,
            cache_hits: AtomicU64::new(0),
        }));
        Ok(status(Some(&id)))
    }
    #[cfg(test)]
    pub fn start_native_llama_from_path(
        state: &NativeLlamaState,
        id: String,
        _sha: String,
        path: PathBuf,
        _cache: PathBuf,
        req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        let mut config = NativeModelConfig::local(path);
        config.model_id = id;
        config.mmproj_path = std::env::var_os("HORARY_NATIVE_LLAMA_PROJECTOR").map(PathBuf::from);
        start(state, config, req)
    }
    pub fn stop_native_llama(state: &NativeLlamaState) -> LlamaResult<bool> {
        let mut slot = state.running.lock().map_err(error)?;
        if let Some(loaded) = slot.as_ref() {
            loaded.host.shutdown_joined().map_err(error)?;
        } else {
            return Ok(false);
        }
        slot.take();
        Ok(true)
    }
    pub fn native_llama_status(state: &NativeLlamaState) -> LlamaResult<LlamaStatus> {
        let slot = state.running.lock().map_err(error)?;
        Ok(status(slot.as_ref().map(|s| s.config.model_id.as_str())))
    }
    pub fn native_llama_health(state: &NativeLlamaState) -> LlamaResult<NativeLlamaHealth> {
        let slot = state.running.lock().map_err(error)?;
        let mut health = empty_health();
        if let Some(s) = slot.as_ref() {
            health.running = true;
            health.model_id = Some(s.config.model_id.clone());
            health.ctx_size = Some(s.config.context_tokens);
            health.parallel = Some(1);
            health.hot_cache_hits = s.cache_hits.load(Ordering::Relaxed);
        }
        Ok(health)
    }
    fn constrained(
        loaded: &Loaded,
        request: GenerationRequest,
        schema: String,
        options: &NativeGenerateOptions,
    ) -> LlamaResult<Vec<llama_native_types::GenerationOutput>> {
        use llama_native_engine::ControlledGenerationSubmission;
        use llama_native_types::{
            ConstraintArtifactReference, ControlProgram, ControlledGenerationBatchRequest,
            ControlledGenerationCase, DistributionObservationPolicy, ExactTokenPrompt,
            ExtendedSamplerProgram, StructuredConstraint, TerminalSelector,
        };
        use sha2::{Digest, Sha256};
        let handle = loaded
            .host
            .load_into_slot(0, loaded.config.clone())
            .map_err(error)?;
        let identity = handle.controlled_model_identity("reader").map_err(error)?;
        let prepared = handle.prepare_input(request.input).map_err(error)?;
        let prompt = prepared
            .into_iter()
            .next()
            .ok_or_else(|| error("No prepared prompt"))?;
        let reference = ConstraintArtifactReference::new(
            "horary-reading-schema".into(),
            format!("{:x}", Sha256::digest(schema.as_bytes())),
            schema.len() as u32,
        )
        .map_err(error)?;
        let program = ControlProgram::new(
            identity,
            Vec::new(),
            Some(StructuredConstraint::JsonSchema { reference }),
            Vec::new(),
            ExtendedSamplerProgram::default(),
            if options.temperature <= 0.0 {
                TerminalSelector::Greedy
            } else {
                TerminalSelector::Distribution
            },
            DistributionObservationPolicy::default(),
            Vec::new(),
        )
        .map_err(error)?;
        let case = ControlledGenerationCase::new(
            "reading".into(),
            ExactTokenPrompt::new(prompt.token_ids).map_err(error)?,
            None,
            request.sampling,
        )
        .map_err(error)?;
        let batch = ControlledGenerationBatchRequest::new(request.request_id, vec![case], program)
            .map_err(error)?;
        let mut ticket = handle
            .generate_controlled(
                ControlledGenerationSubmission::new(batch, Some(schema)).map_err(error)?,
            )
            .map_err(error)?;
        loop {
            if options
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Acquire))
            {
                ticket.cancel_all();
            }
            for event in ticket.events.try_iter() {
                if let GenerationEventKind::Delta { text } = event.event {
                    if let Some(sink) = &options.token_sink {
                        let _ = sink.send(text);
                    }
                }
            }
            match ticket
                .wait_timeout(Duration::from_millis(30))
                .map_err(error)?
            {
                WaitOutcome::Ready(output) => {
                    return Ok(output
                        .cases()
                        .iter()
                        .map(|case| case.generation().clone())
                        .collect())
                }
                WaitOutcome::TimedOut(pending) => ticket = pending,
            }
        }
    }

    pub fn generate_native(
        state: &NativeLlamaState,
        prompt: String,
        options: NativeGenerateOptions,
    ) -> LlamaResult<NativeGenerationResult> {
        let loaded = state
            .running
            .lock()
            .map_err(error)?
            .clone()
            .ok_or_else(|| error("Set up the local model first."))?;
        let input = match serde_json::from_str::<Vec<ChatMessage>>(&prompt) {
            Ok(messages) => GenerationInput::Chat {
                messages,
                template: Default::default(),
            },
            Err(_) => GenerationInput::Completion {
                prompts: vec![CompletionPrompt::Text {
                    text: prompt,
                    special_tokens: SpecialTokenPolicy::AddBosParseSpecial,
                }],
            },
        };
        let request = GenerationRequest {
            request_id: format!("horary-{}", REQUEST_ID.fetch_add(1, Ordering::Relaxed)),
            model_id: loaded.config.model_id.clone(),
            input,
            sampling: SamplingConfig {
                max_tokens: options.max_tokens,
                temperature: options.temperature,
                top_p: options.top_p,
                seed: options.seed,
                ..Default::default()
            },
            media: options
                .audio
                .as_ref()
                .map(|bytes| {
                    use sha2::{Digest, Sha256};
                    vec![llama_native_types::MediaInput {
                        id: "spoken-question".into(),
                        kind: llama_native_types::MediaKind::Audio,
                        mime: "audio/wav".into(),
                        sha256: format!("{:x}", Sha256::digest(bytes)),
                        bytes: bytes.clone(),
                    }]
                })
                .unwrap_or_default(),
            cached_prefix: None,
        };
        if options
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Acquire))
        {
            return Err(error("Judgement cancelled."));
        }
        let outputs = if let Some(schema) = options.response_schema.clone() {
            constrained(&loaded, request, schema, &options)?
        } else {
            let mut ticket = loaded
                .host
                .generate(loaded.config.clone(), request)
                .map_err(error)?;
            let ordinary_outputs = loop {
                if options
                    .cancel
                    .as_ref()
                    .is_some_and(|c| c.load(Ordering::Acquire))
                {
                    ticket.cancel_all();
                }
                for event in ticket.events.try_iter() {
                    if let GenerationEventKind::Delta { text } = event.event {
                        if let Some(sink) = &options.token_sink {
                            let _ = sink.send(text);
                        }
                    }
                }
                match ticket
                    .wait_timeout(Duration::from_millis(30))
                    .map_err(error)?
                {
                    WaitOutcome::Ready(outputs) => break outputs,
                    WaitOutcome::TimedOut(pending) => ticket = pending,
                }
            };

            ordinary_outputs
        };
        let output = outputs
            .into_iter()
            .next()
            .ok_or_else(|| error("Native model returned no output"))?;
        if output.state == GenerationState::Cancelled {
            return Err(error("Judgement cancelled."));
        }
        if output.state != GenerationState::Completed {
            return Err(error(format!(
                "Native generation ended: {}",
                output.finish_reason
            )));
        }
        if !output.real_engine_invoked || output.fake_fixture {
            return Err(error("Reading did not come from the native model"));
        }
        let cache_hit = output.metrics.shared_prefix_tokens > 0;
        if cache_hit {
            loaded.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        Ok(NativeGenerationResult {
            content: output.text,
            prompt_tokens: output.metrics.prompt_tokens as u32,
            generated_tokens: output.metrics.completion_tokens as u32,
            elapsed_ms: output.metrics.duration_ms,
            tokens_per_second: output.metrics.tokens_per_second,
            prompt_cache_hit: cache_hit,
            cold_cache_bytes: None,
        })
    }
}
fn status(id: Option<&str>) -> LlamaStatus {
    LlamaStatus {
        running: id.is_some(),
        model_id: id.map(str::to_string),
        port: None,
        backend: id.map(|_| NATIVE_LLAMA_RUNTIME_BACKEND.to_string()),
        log_path: None,
    }
}
fn empty_health() -> NativeLlamaHealth {
    NativeLlamaHealth {
        compiled: cfg!(feature = "native-llama"),
        running: false,
        model_id: None,
        backend: NATIVE_LLAMA_RUNTIME_BACKEND,
        ctx_size: None,
        parallel: None,
        speculative_decoding_supported: false,
        speculative_decoding_active: false,
        draft_model_id: None,
        hot_cache_entries: 0,
        hot_cache_hits: 0,
        cold_cache_hits: 0,
        cold_cache_writes: 0,
        speculative_draft_tokens: 0,
        speculative_accepted_tokens: 0,
    }
}
#[cfg(not(feature = "native-llama"))]
mod imp {
    use super::*;
    pub fn start_native_llama_in_dir(
        _: &Path,
        _: &NativeLlamaState,
        _: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        Err(error("Native model support is not compiled"))
    }
    pub fn stop_native_llama(_: &NativeLlamaState) -> LlamaResult<bool> {
        Ok(false)
    }
    pub fn native_llama_status(_: &NativeLlamaState) -> LlamaResult<LlamaStatus> {
        Ok(status(None))
    }
    pub fn native_llama_health(_: &NativeLlamaState) -> LlamaResult<NativeLlamaHealth> {
        Ok(empty_health())
    }
    pub fn generate_native(
        _: &NativeLlamaState,
        _: String,
        _: NativeGenerateOptions,
    ) -> LlamaResult<NativeGenerationResult> {
        Err(error("Native model support is not compiled"))
    }
}
#[cfg(all(test, feature = "native-llama"))]
pub use imp::start_native_llama_from_path;
pub use imp::{
    generate_native, native_llama_health, native_llama_status, start_native_llama_in_dir,
    stop_native_llama,
};

#[cfg(all(test, feature = "native-llama"))]
mod integration_tests {
    use super::*;

    #[test]
    fn resident_model_constrains_streams_cancels_and_stops() {
        let Some(path) = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL") else {
            eprintln!(
                "Hardware test requires HORARY_NATIVE_LLAMA_TEST_MODEL; run check:native-llama."
            );
            return;
        };
        let state = NativeLlamaState::default();
        let req = StartLlamaRequest {
            model_id: "hardware-check".into(),
            ctx_size: Some(2048),
            n_gpu_layers: Some("auto".into()),
            parallel: None,
            continuous_batching: None,
            cache_ram_mb: None,
            cache_idle_slots: None,
            cold_kv_cache: None,
            draft_model_id: None,
            spec_draft_n_max: None,
        };
        start_native_llama_from_path(
            &state,
            req.model_id.clone(),
            String::new(),
            path.clone().into(),
            PathBuf::new(),
            req.clone(),
        )
        .unwrap();
        let resident = state.running.lock().unwrap().as_ref().unwrap().clone();
        start_native_llama_from_path(
            &state,
            req.model_id.clone(),
            String::new(),
            path.into(),
            PathBuf::new(),
            req,
        )
        .unwrap();
        assert!(Arc::ptr_eq(
            &resident,
            state.running.lock().unwrap().as_ref().unwrap()
        ));
        drop(resident);
        assert!(native_llama_status(&state).unwrap().running);
        let prompt = serde_json::json!([
            {"role":"system","content":"Return only a JSON object with answer set to ready."},
            {"role":"user","content":"Report readiness."}
        ])
        .to_string();
        let (tx, rx) = mpsc::channel();
        let result = generate_native(&state, prompt.clone(), NativeGenerateOptions {
            max_tokens: 64, temperature: 0.0,
            response_schema: Some(r#"{"type":"object","properties":{"answer":{"const":"ready"}},"required":["answer"],"additionalProperties":false}"#.into()),
            token_sink: Some(tx), ..Default::default()
        }).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.content).unwrap()["answer"],
            "ready"
        );
        assert!(result.generated_tokens > 0 && result.prompt_tokens > 0);
        assert!(
            rx.try_iter().any(|text| !text.is_empty()),
            "Native tokens must reach the UI stream"
        );

        let cancelled = Arc::new(AtomicBool::new(true));
        let error = generate_native(
            &state,
            prompt,
            NativeGenerateOptions {
                cancel: Some(cancelled),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.message.contains("cancelled"));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let live_prompt = serde_json::json!([{"role":"user","content":"Write a very long JSON string listing all numbers from one to one thousand in words."}]).to_string();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                generate_native(
                    &state,
                    live_prompt,
                    NativeGenerateOptions {
                        max_tokens: 1500,
                        temperature: 0.0,
                        response_schema: Some(r#"{"type":"string"}"#.into()),
                        token_sink: Some(tx),
                        cancel: Some(cancel.clone()),
                        ..Default::default()
                    },
                )
            });
            let first_token = rx.recv_timeout(Duration::from_secs(30));
            cancel.store(true, Ordering::Release);
            let cancelled = worker.join().unwrap();
            assert!(
                first_token.is_ok(),
                "The real generation must start before cancellation"
            );
            assert!(cancelled.unwrap_err().message.contains("cancelled"));
        });
        assert!(native_llama_health(&state).unwrap().running);
        assert!(stop_native_llama(&state).unwrap());
        assert!(!native_llama_status(&state).unwrap().running);
        assert!(!stop_native_llama(&state).unwrap());
        assert!(generate_native(&state, "Hello".into(), Default::default()).is_err());
    }
}
