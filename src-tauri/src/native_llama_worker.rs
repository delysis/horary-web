#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "native-llama"), allow(dead_code, unused_imports))]
use crate::llama::{LlamaError, LlamaResult, LlamaStatus, StartLlamaRequest};
use serde::{Deserialize, Serialize};
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
    /// Cache only the fixed first system message, never the changing question.
    pub cache_lesson: bool,
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
            cache_lesson: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeGenerationResult {
    pub content: String,
    pub prompt_tokens: u32,
    pub generated_tokens: u32,
    pub elapsed_ms: u128,
    #[serde(default)]
    pub total_wall_ms: Option<u128>,
    #[serde(default)]
    pub batch_size: Option<usize>,
    #[serde(default)]
    pub lesson_bank_hit: Option<bool>,
    #[serde(default)]
    pub lesson_prepare_ms: Option<u128>,
    pub tokens_per_second: f64,
    pub prompt_cache_hit: bool,
    pub cached_prompt_tokens: u32,
    pub prefilled_prompt_tokens: u32,
    pub first_token_ms: Option<u128>,
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
    generation: Mutex<()>,
    lessons: Mutex<Vec<(String, llama_native_types::SequenceStateBlob)>>,
    lesson_budget: usize,
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
    struct PreparedLesson {
        prefix: llama_native_types::SequenceStateBlob,
        bank_hit: bool,
        prepare_ms: u128,
    }
    fn native_model_metadata(
        id: &str,
        registered: &[crate::llama::ModelInfo],
        inspect_import: impl FnOnce(&str) -> LlamaResult<crate::llama::ModelInfo>,
    ) -> LlamaResult<crate::llama::ModelInfo> {
        match registered.iter().find(|model| model.id == id) {
            Some(model) => Ok(model.clone()),
            None => inspect_import(id),
        }
    }

    #[test]
    fn registered_metadata_defers_payload_verification_to_the_native_owner() {
        let model = crate::llama::ModelInfo {
            id: "reader".into(),
            filename: "reader.gguf".into(),
            display_name: "Reader".into(),
            size_bytes: 123,
            sha256: "a".repeat(64),
        };
        let registered = [model.clone()];
        let info = native_model_metadata("reader", &registered, |_| {
            panic!("A resident registered model must not be rehashed before native start")
        })
        .unwrap();
        assert_eq!(info.sha256, model.sha256);
        let missing = native_model_metadata("imported", &registered, |id| {
            assert_eq!(id, "imported");
            Err(error("Import verification required"))
        })
        .unwrap_err();
        assert_eq!(missing.message, "Import verification required");
    }
    fn chat_template(architecture: Option<&str>) -> llama_native_types::ChatTemplateChoice {
        if architecture == Some("gemma4") {
            llama_native_types::ChatTemplateChoice::Gemma4NonThinking
        } else {
            Default::default()
        }
    }
    #[test]
    fn gemma_uses_the_explicit_native_turn_protocol_for_text_and_audio() {
        assert_eq!(
            chat_template(Some("gemma4")),
            llama_native_types::ChatTemplateChoice::Gemma4NonThinking
        );
        assert_eq!(
            chat_template(Some("other")),
            llama_native_types::ChatTemplateChoice::ModelDefault
        );
    }

    pub fn start_native_llama_in_dir(
        dir: &Path,
        state: &NativeLlamaState,
        req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        // The catalog supplies pinned expected digests, not verification.
        // Native-kit's owner hashes its opened files before initial load and
        // guards their identities. A resident start needs no second full read.
        let registered = crate::hf_cache::registered_models(dir)?;
        let lookup = |id: &str| {
            native_model_metadata(id, &registered, |id| crate::llama::get_model_by_id(dir, id))
        };
        let model = lookup(&req.model_id)?;
        let path = crate::llama::resolve_model_path(dir, &model.filename)?;
        let mut config = NativeModelConfig::local(path);
        config.model_id = model.id;
        config.expected_model_sha256 = Some(model.sha256);
        if let Ok(projector) = lookup(&format!("{}-projector", req.model_id)) {
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
        config.max_sequences = 4;
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
            generation: Mutex::new(()),
            lessons: Mutex::new(Vec::new()),
            lesson_budget: (system.total_memory() / 8).min(4 * 1024 * 1024 * 1024) as usize,
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
            health.parallel = Some(s.config.max_sequences);
            health.hot_cache_hits = s.cache_hits.load(Ordering::Relaxed);
        }
        Ok(health)
    }
    fn lesson_state(
        loaded: &Loaded,
        input: &GenerationInput,
    ) -> LlamaResult<Option<PreparedLesson>> {
        let started = std::time::Instant::now();
        use llama_native_types::{BranchRequest, ChatRole, SharedPrefixBatchRequest};
        use sha2::{Digest, Sha256};
        let GenerationInput::Chat { messages, template } = input else {
            return Ok(None);
        };
        let Some(first) = messages.first().filter(|m| m.role == ChatRole::System) else {
            return Ok(None);
        };
        let key = format!("{:x}", Sha256::digest(first.content.as_bytes()));
        let handle = loaded
            .host
            .load_into_slot(0, loaded.config.clone())
            .map_err(error)?;
        let mut bank = loaded.lessons.lock().map_err(error)?;
        let index = bank.iter().position(|(id, _)| id == &key);
        let saved = if let Some(index) = index {
            let entry = bank.remove(index);
            let state = entry.1.clone();
            bank.push(entry);
            state
        } else {
            let saved = handle
                .prefill_shared_prefix(SharedPrefixBatchRequest {
                    request_id: format!("lesson-{key}"),
                    model_id: loaded.config.model_id.clone(),
                    common_messages: vec![first.clone()],
                    chat_template: template.clone(),
                    branches: (0..2)
                        .map(|i| BranchRequest {
                            branch_id: format!("prepare-{i}"),
                            label: "Prepare fixed lesson".into(),
                            instruction: "Complete the next task.".into(),
                            sampling: SamplingConfig {
                                max_tokens: 1,
                                ..Default::default()
                            },
                            messages: Vec::new(),
                            cached_prefix: None,
                        })
                        .collect(),
                    cached_prefix: None,
                })
                .map_err(error)?;
            let bytes = saved.bytes.len();
            if bytes <= loaded.lesson_budget {
                while bank.iter().map(|(_, s)| s.bytes.len()).sum::<usize>() + bytes
                    > loaded.lesson_budget
                {
                    bank.remove(0);
                }
                bank.push((key, saved.clone()));
            }
            saved
        };
        // A chat-template boundary is not assumed to be stable: verify actual
        // tokens against the full request before granting reuse authority.
        let prepared = handle.prepare_input(input.clone()).map_err(error)?;
        if prepared
            .first()
            .is_none_or(|p| !p.token_ids.starts_with(&saved.token_ids))
        {
            return Err(error(
                "The fixed lesson is not a prefix of the live request.",
            ));
        }
        Ok(Some(PreparedLesson {
            prefix: saved,
            bank_hit: index.is_some(),
            prepare_ms: started.elapsed().as_millis(),
        }))
    }

    fn restore_lesson(
        loaded: &Loaded,
        request: &GenerationRequest,
    ) -> LlamaResult<Option<(bool, u128)>> {
        let Some(saved) = lesson_state(loaded, &request.input)? else {
            return Ok(None);
        };
        let handle = loaded
            .host
            .load_into_slot(0, loaded.config.clone())
            .map_err(error)?;
        let metadata = (saved.bank_hit, saved.prepare_ms);
        let restored = handle.restore_sequence(saved.prefix, 0).map_err(error)?;
        if restored != llama_native_types::SequenceRestoreKind::NativeState {
            return Err(error("The fixed lesson lost its live cache ownership."));
        }
        Ok(Some(metadata))
    }

    pub fn generate_native_batch(
        state: &NativeLlamaState,
        prompts: Vec<(String, u32)>,
        options: NativeGenerateOptions,
    ) -> LlamaResult<Vec<NativeGenerationResult>> {
        let wall = std::time::Instant::now();
        let count = prompts.len();
        use llama_native_types::{GenerationBatchRequest, GenerationCase};
        if prompts.is_empty() || prompts.len() > 4 {
            return Err(error("A native reading batch needs one to four tasks."));
        }
        if options.audio.is_some() || options.response_schema.is_some() {
            return Err(error(
                "Independent cached batches use text worksheets with native validation.",
            ));
        }
        let loaded = state
            .running
            .lock()
            .map_err(error)?
            .clone()
            .ok_or_else(|| error("Set up the local model first."))?;
        let _generation = loaded.generation.lock().map_err(error)?;
        let mut cases = Vec::new();
        let mut preparations = Vec::new();
        for (index, (prompt, max_tokens)) in prompts.into_iter().enumerate() {
            if options
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Acquire))
            {
                return Err(error("Judgement cancelled."));
            }
            let messages: Vec<ChatMessage> = serde_json::from_str(&prompt).map_err(error)?;
            let input = GenerationInput::Chat {
                messages,
                template: chat_template(
                    loaded
                        .host
                        .descriptors()
                        .first()
                        .map(|d| d.architecture.as_str()),
                ),
            };
            let lesson = if options.cache_lesson {
                lesson_state(&loaded, &input)?
            } else {
                None
            };
            preparations.push(lesson.as_ref().map(|p| (p.bank_hit, p.prepare_ms)));
            let cached_prefix = lesson.map(|p| p.prefix);
            cases.push(GenerationCase {
                case_id: format!("stage-{index}"),
                input,
                cached_prefix,
                sampling: SamplingConfig {
                    max_tokens,
                    temperature: options.temperature,
                    top_p: options.top_p,
                    seed: options.seed,
                    ..Default::default()
                },
            });
        }
        let mut ticket = loaded
            .host
            .generate_batch(
                loaded.config.clone(),
                GenerationBatchRequest {
                    request_id: format!(
                        "horary-batch-{}",
                        REQUEST_ID.fetch_add(1, Ordering::Relaxed)
                    ),
                    model_id: loaded.config.model_id.clone(),
                    cases,
                    media: Vec::new(),
                    first_word_choices: None,
                },
            )
            .map_err(error)?;
        let mut outputs = loop {
            if options
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Acquire))
            {
                ticket.cancel_all();
            }
            match ticket
                .wait_timeout(Duration::from_millis(30))
                .map_err(error)?
            {
                WaitOutcome::Ready(outputs) => break outputs,
                WaitOutcome::TimedOut(pending) => ticket = pending,
            }
        };
        outputs.sort_by(|a, b| a.branch_id.cmp(&b.branch_id));
        for (index, output) in outputs.iter().enumerate() {
            if output.branch_id != format!("stage-{index}") {
                return Err(error(
                    "Native batch result does not match its requested stage.",
                ));
            }
        }
        outputs
            .into_iter()
            .zip(preparations)
            .map(|(output, preparation)| {
                finish_output(&loaded, output).map(|mut result| {
                    result.total_wall_ms = Some(wall.elapsed().as_millis());
                    result.batch_size = Some(count);
                    result.lesson_bank_hit = preparation.map(|p| p.0);
                    result.lesson_prepare_ms = preparation.map(|p| p.1);
                    result
                })
            })
            .collect()
    }

    fn finish_output(
        loaded: &Loaded,
        output: llama_native_types::GenerationOutput,
    ) -> LlamaResult<NativeGenerationResult> {
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
        let reused = output.metrics.cache.resident_prefix_tokens
            + output.metrics.cache.restored_prefix_tokens;
        if reused > 0 {
            loaded.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        Ok(NativeGenerationResult {
            content: output.text,
            prompt_tokens: output.metrics.prompt_tokens as u32,
            generated_tokens: output.metrics.completion_tokens as u32,
            elapsed_ms: output.metrics.duration_ms,
            total_wall_ms: None,
            batch_size: None,
            lesson_bank_hit: None,
            lesson_prepare_ms: None,
            tokens_per_second: output.metrics.tokens_per_second,
            prompt_cache_hit: reused > 0,
            cached_prompt_tokens: reused as u32,
            prefilled_prompt_tokens: output.metrics.prompt_tokens.saturating_sub(reused) as u32,
            first_token_ms: output.metrics.first_token_ms,
            cold_cache_bytes: None,
        })
    }

    fn constrained(
        loaded: &Loaded,
        request: GenerationRequest,
        schema: String,
        options: &NativeGenerateOptions,
    ) -> LlamaResult<Vec<llama_native_types::GenerationOutput>> {
        if !request.media.is_empty() {
            return Err(error(
                "Constrained text generation cannot consume audio. Use the direct audio route.",
            ));
        }
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
        let wall = std::time::Instant::now();
        let loaded = state
            .running
            .lock()
            .map_err(error)?
            .clone()
            .ok_or_else(|| error("Set up the local model first."))?;
        // Restore plus generation is one transaction on the owned KV sequence.
        // Concurrent callers must not restore over a running stage.
        let _generation = loaded.generation.lock().map_err(error)?;
        if options
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Acquire))
        {
            return Err(error("Judgement cancelled."));
        }
        let input = match serde_json::from_str::<Vec<ChatMessage>>(&prompt) {
            Ok(messages) => GenerationInput::Chat {
                messages,
                template: chat_template(
                    loaded
                        .host
                        .descriptors()
                        .first()
                        .map(|d| d.architecture.as_str()),
                ),
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
        let preparation = if options.cache_lesson && options.audio.is_none() {
            restore_lesson(&loaded, &request)?
        } else {
            None
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
        let reused = output.metrics.cache.resident_prefix_tokens
            + output.metrics.cache.restored_prefix_tokens;
        let cache_hit = reused > 0;
        if cache_hit {
            loaded.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        Ok(NativeGenerationResult {
            content: output.text,
            prompt_tokens: output.metrics.prompt_tokens as u32,
            generated_tokens: output.metrics.completion_tokens as u32,
            elapsed_ms: output.metrics.duration_ms,
            total_wall_ms: Some(wall.elapsed().as_millis()),
            batch_size: Some(1),
            lesson_bank_hit: preparation.map(|p| p.0),
            lesson_prepare_ms: preparation.map(|p| p.1),
            tokens_per_second: output.metrics.tokens_per_second,
            prompt_cache_hit: cache_hit,
            cached_prompt_tokens: reused as u32,
            prefilled_prompt_tokens: output.metrics.prompt_tokens.saturating_sub(reused) as u32,
            first_token_ms: output.metrics.first_token_ms,
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
    pub fn generate_native_batch(
        _: &NativeLlamaState,
        _: Vec<(String, u32)>,
        _: NativeGenerateOptions,
    ) -> LlamaResult<Vec<NativeGenerationResult>> {
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
    generate_native, generate_native_batch, native_llama_health, native_llama_status,
    start_native_llama_in_dir, stop_native_llama,
};

#[cfg(all(test, feature = "native-llama"))]
mod integration_tests {
    use super::*;

    #[test]
    #[ignore = "Real model: four distinct cached prompts must produce four independent batch answers"]
    fn four_cached_lessons_decode_together_and_survive_stage_switches() {
        let state = NativeLlamaState::default();
        let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model");
        start_native_llama_from_path(
            &state,
            "four-way".into(),
            String::new(),
            path.into(),
            PathBuf::new(),
            serde_json::from_value(
                serde_json::json!({"modelId":"four-way","ctxSize":16384,"nGpuLayers":"auto"}),
            )
            .unwrap(),
        )
        .unwrap();
        let prompts = || {
            (0..4).map(|i|(serde_json::json!([{"role":"system","content":format!("{} Return exactly the one word assigned by the user. No explanation or punctuation.",format!("This is independent lesson {i}. Keep its instructions separate. ").repeat(40))},{"role":"user","content":(["amber","birch","cedar","dawn"][i])}]).to_string(),16)).collect()
        };
        let options = || NativeGenerateOptions {
            temperature: 0.,
            cache_lesson: true,
            ..Default::default()
        };
        let cold_started = std::time::Instant::now();
        let cold = generate_native_batch(&state, prompts(), options()).unwrap();
        let cold_ms = cold_started.elapsed().as_millis();
        let warm_started = std::time::Instant::now();
        let warm = generate_native_batch(&state, prompts(), options()).unwrap();
        let warm_ms = warm_started.elapsed().as_millis();
        stop_native_llama(&state).unwrap();
        for ((a, b), expected) in cold
            .iter()
            .zip(&warm)
            .zip(["amber", "birch", "cedar", "dawn"])
        {
            assert_eq!(a.content.trim(), expected);
            assert_eq!(b.content.trim(), expected);
            assert_eq!(a.lesson_bank_hit, Some(false));
            assert_eq!(
                b.lesson_bank_hit,
                Some(true),
                "A warm prefix must come from the bank, without a new fixed-lesson prefill"
            );
            assert!(
                b.cached_prompt_tokens > 500,
                "Per-case lesson prefix was not reused"
            );
        }
        eprintln!(
            "FOUR WAY CACHE: cold_wall_ms={cold_ms} warm_wall_ms={warm_ms} outputs={}",
            serde_json::to_string(&warm).unwrap()
        );
    }

    #[test]
    #[ignore = "Requires the pinned model/projector in the shared Hub cache and compatible hardware."]
    fn registered_reader_preparation_reuses_its_verified_resident() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::hf_cache::cache_root().unwrap();
        std::fs::write(
            dir.path().join("huggingface-cache.json"),
            serde_json::to_vec(&root).unwrap(),
        )
        .unwrap();
        let req: StartLlamaRequest = serde_json::from_value(
            serde_json::json!({"modelId":"gemma-4-12b-qat","ctxSize":16384,"nGpuLayers":"auto"}),
        )
        .unwrap();
        let state = NativeLlamaState::default();
        let cold = std::time::Instant::now();
        start_native_llama_in_dir(dir.path(), &state, req.clone()).unwrap();
        let cold_ms = cold.elapsed().as_millis();
        let resident = state.running.lock().unwrap().as_ref().unwrap().clone();
        let manifest = crate::model_manifest::bundled_model_manifest().unwrap();
        assert_eq!(
            resident.config.expected_model_sha256,
            manifest
                .models
                .iter()
                .find(|m| m.id == req.model_id)
                .unwrap()
                .sha256
        );
        assert!(resident.config.expected_mmproj_sha256.is_some());
        let warm = std::time::Instant::now();
        start_native_llama_in_dir(dir.path(), &state, req).unwrap();
        let warm_ms = warm.elapsed().as_millis();
        let reused = Arc::ptr_eq(&resident, state.running.lock().unwrap().as_ref().unwrap());
        drop(resident);
        stop_native_llama(&state).unwrap();
        eprintln!("REGISTERED READER PREPARATION: cold_ms={cold_ms} warm_ms={warm_ms} same_resident={reused}");
        assert!(reused, "The verified owner must stay resident");
        assert!(
            warm_ms < 1000,
            "A warm metadata lookup must not reread seven GB of payload"
        );
    }

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
        let warm = generate_native(&state, prompt.clone(), NativeGenerateOptions {
            max_tokens: 64, temperature: 0.,
            response_schema: Some(r#"{"type":"object","properties":{"answer":{"const":"ready"}},"required":["answer"],"additionalProperties":false}"#.into()),
            ..Default::default()
        }).unwrap();
        assert_eq!(warm.content, result.content);
        assert!(warm.prompt_cache_hit && warm.cached_prompt_tokens > 0);
        assert_eq!(
            warm.prefilled_prompt_tokens + warm.cached_prompt_tokens,
            warm.prompt_tokens
        );
        eprintln!("WARM INFERENCE: prompt_tokens={} cached_tokens={} prefilled_tokens={} first_token_ms={:?} elapsed_ms={}",warm.prompt_tokens,warm.cached_prompt_tokens,warm.prefilled_prompt_tokens,warm.first_token_ms,warm.elapsed_ms);
        let invalid_audio = generate_native(
            &state,
            prompt.clone(),
            NativeGenerateOptions {
                audio: Some(vec![1, 2]),
                response_schema: Some("{}".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(invalid_audio.message.contains("cannot consume audio"));
        eprintln!("READINESS INFERENCE: prompt_tokens={} output_tokens={} elapsed_ms={} tokens_per_second={:.2}", result.prompt_tokens,result.generated_tokens,result.elapsed_ms,result.tokens_per_second);
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
#[test]
#[ignore = "Real model regression: an owned restored stage prefix must be reused by constrained text."]
#[cfg(feature = "native-llama")]
fn constrained_generation_reuses_an_owned_prefix_after_another_stage() {
    let state = NativeLlamaState::default();
    let path = std::env::var_os("HORARY_NATIVE_LLAMA_TEST_MODEL").expect("model path");
    start_native_llama_from_path(
        &state,
        "stage-cache-probe".into(),
        String::new(),
        path.into(),
        std::env::temp_dir(),
        serde_json::from_value(
            serde_json::json!({"modelId":"stage-cache-probe","ctxSize":4096,"nGpuLayers":"auto"}),
        )
        .unwrap(),
    )
    .unwrap();
    let schema = r#"{"type":"object","properties":{"answer":{"const":"ready"}},"required":["answer"],"additionalProperties":false}"#;
    let input = |lesson: &str| {
        serde_json::json!([{"role":"system","content":format!("{} Return only answer=ready.",lesson.repeat(32))},{"role":"user","content":"Report readiness."}]).to_string()
    };
    let a =
        input("This is the first independent lesson. Its exact fixed prefix should be retained. ");
    let b = input("This is a different lesson, irrelevant to the first task. Keep it separate. ");
    let options = || NativeGenerateOptions {
        response_schema: Some(schema.into()),
        temperature: 0.,
        max_tokens: 32,
        ..Default::default()
    };
    let cold = generate_native(&state, a.clone(), options()).unwrap();
    let loaded = state.running.lock().unwrap().as_ref().unwrap().clone();
    let handle = loaded
        .host
        .load_into_slot(0, loaded.config.clone())
        .unwrap();
    let saved = handle.snapshot_sequence(0).unwrap();
    generate_native(&state, b, options()).unwrap();
    let restored = handle.restore_sequence(saved, 0).unwrap();
    assert_eq!(
        restored,
        llama_native_types::SequenceRestoreKind::NativeState
    );
    let warm = generate_native(&state, a, options()).unwrap();
    stop_native_llama(&state).unwrap();
    eprintln!(
        "STAGE PREFIX RESTORE: cold={} warm={} cached={} new={} first_ms={:?}",
        cold.prompt_tokens,
        warm.prompt_tokens,
        warm.cached_prompt_tokens,
        warm.prefilled_prompt_tokens,
        warm.first_token_ms
    );
    assert_eq!(warm.content, cold.content);
    assert!(
        warm.cached_prompt_tokens > cold.prompt_tokens / 2,
        "The exact live restored prefix was unnecessarily re-prefilled"
    );
}
