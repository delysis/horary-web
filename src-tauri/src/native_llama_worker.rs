#![cfg_attr(not(feature = "native-llama"), allow(dead_code, unused_imports))]

use crate::llama::{LlamaError, LlamaLaunchArgs, LlamaResult, LlamaStatus, StartLlamaRequest};
use crate::native_llama::{
    NATIVE_LLAMA_BACKEND, NATIVE_LLAMA_BINDING_VERSION, NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

pub const NATIVE_LLAMA_RUNTIME_BACKEND: &str = NATIVE_LLAMA_BACKEND;
const READY_TIMEOUT: Duration = Duration::from_secs(300);
const RESERVED_HOT_CACHE_SEQ_ID: i32 = 0;
const FIRST_REQUEST_SEQ_ID: i32 = 1;
const DEFAULT_BATCH_TOKENS: usize = 512;
const DEFAULT_UBATCH_TOKENS: u32 = 512;

pub struct NativeLlamaState {
    running: Mutex<Option<NativeWorkerHandle>>,
}

impl Default for NativeLlamaState {
    fn default() -> Self {
        Self {
            running: Mutex::new(None),
        }
    }
}

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
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub seed: u32,
    pub prompt_cache_key: Option<String>,
    pub prompt_cache_prefix: Option<String>,
    pub token_sink: Option<mpsc::Sender<String>>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for NativeGenerateOptions {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            temperature: 0.2,
            top_p: 1.0,
            seed: 0,
            prompt_cache_key: None,
            prompt_cache_prefix: None,
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

struct NativeWorkerHandle {
    model_id: String,
    ctx_size: u32,
    parallel: u32,
    draft_model_id: Option<String>,
    command_tx: mpsc::Sender<NativeWorkerCommand>,
    stats: Arc<Mutex<NativeWorkerStats>>,
    join: Option<JoinHandle<()>>,
}

#[derive(Debug, Default)]
struct NativeWorkerStats {
    hot_cache_entries: u64,
    hot_cache_hits: u64,
    cold_cache_hits: u64,
    cold_cache_writes: u64,
    speculative_draft_tokens: u64,
    speculative_accepted_tokens: u64,
}

enum NativeWorkerCommand {
    Generate(NativeWorkerJob),
    Stop,
}

struct NativeWorkerJob {
    prompt: String,
    options: NativeGenerateOptions,
    response_tx: mpsc::Sender<LlamaResult<NativeGenerationResult>>,
}

struct NativeWorkerConfig {
    model_id: String,
    model_sha256: String,
    model_path: PathBuf,
    draft: Option<NativeDraftConfig>,
    cache_dir: PathBuf,
    launch: LlamaLaunchArgs,
}

#[derive(Debug, Clone)]
struct NativeDraftConfig {
    model_id: String,
    model_path: PathBuf,
}

#[cfg(feature = "native-llama")]
mod imp {
    use super::*;
    use crate::llama::{get_model_by_id, normalize_llama_launch_args, resolve_model_path};
    use llama_cpp_2::{
        context::{params::LlamaContextParams, LlamaContext},
        llama_backend::LlamaBackend,
        llama_batch::LlamaBatch,
        model::{params::LlamaModelParams, AddBos, LlamaModel},
        sampling::LlamaSampler,
        speculative::{LlamaSpeculativeMtp, LlamaSpeculativeMtpParams},
        token::LlamaToken,
    };
    use sha2::{Digest, Sha256};
    use std::{collections::VecDeque, num::NonZeroU32, time::Instant};

    struct ActiveGeneration {
        seq_id: i32,
        prompt: String,
        tokens: Vec<LlamaToken>,
        prompt_token_count: usize,
        cursor: usize,
        position: i32,
        sampler: LlamaSampler,
        output: Vec<u8>,
        generated_tokens: u32,
        last_logit_index: i32,
        pending_decode_token: Option<PendingDecodeToken>,
        prompt_cache_key: String,
        prompt_cache_prefix_tokens: Option<usize>,
        prompt_cache_hit: bool,
        cold_cache_bytes: Option<usize>,
        token_sink: Option<mpsc::Sender<String>>,
        cancel: Option<Arc<AtomicBool>>,
        response_tx: mpsc::Sender<LlamaResult<NativeGenerationResult>>,
        started_at: Instant,
        max_tokens: u32,
    }

    #[derive(Debug, Clone, Copy)]
    struct PendingDecodeToken {
        token: LlamaToken,
        position: i32,
    }

    struct PendingVerification {
        generation_index: usize,
        batch_start: i32,
        primary_token: LlamaToken,
        draft_tokens: Vec<LlamaToken>,
        sampled_position: i32,
    }

    enum PendingBatchItem {
        DecodeOnly { generation_index: usize },
        Verify(PendingVerification),
    }

    struct HotCacheEntry {
        key: String,
        tokens: Vec<LlamaToken>,
    }

    pub fn start_native_llama_in_dir(
        app_data_dir: &Path,
        state: &NativeLlamaState,
        req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        {
            let running = state.running.lock().map_err(|_| LlamaError {
                message: "native llama state lock poisoned".to_string(),
            })?;
            if let Some(handle) = running.as_ref() {
                return Ok(handle_status(handle));
            }
        }

        let model = get_model_by_id(app_data_dir, &req.model_id)?;
        let model_path = resolve_model_path(app_data_dir, &model.filename)?;
        let draft = resolve_native_draft_model(app_data_dir, &req)?;
        let launch = normalize_llama_launch_args(&req)?;
        let cache_dir = app_data_dir
            .join("kv-cache")
            .join("native-llama")
            .join(sanitize_cache_component(&model.id));
        std::fs::create_dir_all(&cache_dir)?;

        let handle = spawn_worker(NativeWorkerConfig {
            model_id: model.id,
            model_sha256: model.sha256,
            model_path,
            draft,
            cache_dir,
            launch,
        })?;
        let status = handle_status(&handle);

        let mut running = state.running.lock().map_err(|_| LlamaError {
            message: "native llama state lock poisoned".to_string(),
        })?;
        *running = Some(handle);
        Ok(status)
    }

    #[cfg(test)]
    pub fn start_native_llama_from_path(
        state: &NativeLlamaState,
        model_id: String,
        model_sha256: String,
        model_path: PathBuf,
        cache_dir: PathBuf,
        req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        let launch = normalize_llama_launch_args(&req)?;
        let draft = resolve_test_draft_model(&req)?;
        std::fs::create_dir_all(&cache_dir)?;
        let handle = spawn_worker(NativeWorkerConfig {
            model_id,
            model_sha256,
            model_path,
            draft,
            cache_dir,
            launch,
        })?;
        let status = handle_status(&handle);
        let mut running = state.running.lock().map_err(|_| LlamaError {
            message: "native llama state lock poisoned".to_string(),
        })?;
        *running = Some(handle);
        Ok(status)
    }

    pub fn stop_native_llama(state: &NativeLlamaState) -> LlamaResult<bool> {
        let mut running = state.running.lock().map_err(|_| LlamaError {
            message: "native llama state lock poisoned".to_string(),
        })?;
        Ok(running.take().is_some())
    }

    pub fn native_llama_status(state: &NativeLlamaState) -> LlamaResult<LlamaStatus> {
        let running = state.running.lock().map_err(|_| LlamaError {
            message: "native llama state lock poisoned".to_string(),
        })?;
        Ok(running
            .as_ref()
            .map(handle_status)
            .unwrap_or_else(stopped_status))
    }

    pub fn native_llama_health(state: &NativeLlamaState) -> LlamaResult<NativeLlamaHealth> {
        let running = state.running.lock().map_err(|_| LlamaError {
            message: "native llama state lock poisoned".to_string(),
        })?;
        Ok(match running.as_ref() {
            Some(handle) => {
                let stats = handle.stats.lock().map_err(|_| LlamaError {
                    message: "native llama stats lock poisoned".to_string(),
                })?;
                NativeLlamaHealth {
                    compiled: true,
                    running: true,
                    model_id: Some(handle.model_id.clone()),
                    backend: NATIVE_LLAMA_RUNTIME_BACKEND,
                    ctx_size: Some(handle.ctx_size),
                    parallel: Some(handle.parallel),
                    speculative_decoding_supported: NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE,
                    speculative_decoding_active: handle.draft_model_id.is_some(),
                    draft_model_id: handle.draft_model_id.clone(),
                    hot_cache_entries: stats.hot_cache_entries,
                    hot_cache_hits: stats.hot_cache_hits,
                    cold_cache_hits: stats.cold_cache_hits,
                    cold_cache_writes: stats.cold_cache_writes,
                    speculative_draft_tokens: stats.speculative_draft_tokens,
                    speculative_accepted_tokens: stats.speculative_accepted_tokens,
                }
            }
            None => NativeLlamaHealth {
                compiled: true,
                running: false,
                model_id: None,
                backend: NATIVE_LLAMA_RUNTIME_BACKEND,
                ctx_size: None,
                parallel: None,
                speculative_decoding_supported: NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE,
                speculative_decoding_active: false,
                draft_model_id: None,
                hot_cache_entries: 0,
                hot_cache_hits: 0,
                cold_cache_hits: 0,
                cold_cache_writes: 0,
                speculative_draft_tokens: 0,
                speculative_accepted_tokens: 0,
            },
        })
    }

    pub fn generate_native(
        state: &NativeLlamaState,
        prompt: String,
        options: NativeGenerateOptions,
    ) -> LlamaResult<NativeGenerationResult> {
        let command_tx = {
            let running = state.running.lock().map_err(|_| LlamaError {
                message: "native llama state lock poisoned".to_string(),
            })?;
            running
                .as_ref()
                .map(|handle| handle.command_tx.clone())
                .ok_or_else(|| LlamaError {
                    message: "native llama model is not running".to_string(),
                })?
        };
        let (response_tx, response_rx) = mpsc::channel();
        command_tx
            .send(NativeWorkerCommand::Generate(NativeWorkerJob {
                prompt,
                options,
                response_tx,
            }))
            .map_err(|_| LlamaError {
                message: "native llama worker is not accepting requests".to_string(),
            })?;
        response_rx.recv().map_err(|_| LlamaError {
            message: "native llama worker stopped before returning a response".to_string(),
        })?
    }

    fn spawn_worker(config: NativeWorkerConfig) -> LlamaResult<NativeWorkerHandle> {
        let (command_tx, command_rx) = mpsc::channel::<NativeWorkerCommand>();
        let (ready_tx, ready_rx) = mpsc::channel::<LlamaResult<()>>();
        let stats = Arc::new(Mutex::new(NativeWorkerStats::default()));
        let stats_for_thread = Arc::clone(&stats);
        let model_id = config.model_id.clone();
        let ctx_size = config.launch.ctx_size;
        let parallel = effective_parallel(&config);
        let draft_model_id = config.draft.as_ref().map(|draft| draft.model_id.clone());

        let join = thread::Builder::new()
            .name("whorary-native-llama".to_string())
            .spawn(move || worker_main(config, command_rx, ready_tx, stats_for_thread))
            .map_err(|error| LlamaError {
                message: format!("failed to spawn native llama worker: {error}"),
            })?;

        let ready = ready_rx
            .recv_timeout(READY_TIMEOUT)
            .map_err(|error| LlamaError {
                message: format!("native llama worker did not become ready: {error}"),
            })?;
        ready?;

        Ok(NativeWorkerHandle {
            model_id,
            ctx_size,
            parallel,
            draft_model_id,
            command_tx,
            stats,
            join: Some(join),
        })
    }

    fn resolve_native_draft_model(
        app_data_dir: &Path,
        req: &StartLlamaRequest,
    ) -> LlamaResult<Option<NativeDraftConfig>> {
        if let Some(model_id) = req
            .draft_model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let draft = get_model_by_id(app_data_dir, model_id)?;
            let model_path = resolve_model_path(app_data_dir, &draft.filename)?;
            return Ok(Some(NativeDraftConfig {
                model_id: draft.id,
                model_path,
            }));
        }

        let assistant_id = format!("{}-assistant", req.model_id);
        match get_model_by_id(app_data_dir, &assistant_id) {
            Ok(draft) => {
                let model_path = resolve_model_path(app_data_dir, &draft.filename)?;
                Ok(Some(NativeDraftConfig {
                    model_id: draft.id,
                    model_path,
                }))
            }
            Err(error) if error.message == format!("model not found: {assistant_id}") => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    fn resolve_test_draft_model(req: &StartLlamaRequest) -> LlamaResult<Option<NativeDraftConfig>> {
        if let Some(model_id) = req
            .draft_model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let path =
                std::env::var_os("WHORARY_NATIVE_LLAMA_MTP_MODEL").ok_or_else(|| LlamaError {
                    message: format!(
                        "draftModelId '{model_id}' requires WHORARY_NATIVE_LLAMA_MTP_MODEL in native llama tests"
                    ),
                })?;
            return Ok(Some(NativeDraftConfig {
                model_id: model_id.to_string(),
                model_path: PathBuf::from(path),
            }));
        }
        Ok(None)
    }

    fn worker_main(
        config: NativeWorkerConfig,
        command_rx: mpsc::Receiver<NativeWorkerCommand>,
        ready_tx: mpsc::Sender<LlamaResult<()>>,
        stats: Arc<Mutex<NativeWorkerStats>>,
    ) {
        let startup = load_backend_and_model(&config);
        let Ok((backend, model)) = startup else {
            let error = startup.err().unwrap_or_else(|| LlamaError {
                message: "native llama worker failed to initialize".to_string(),
            });
            let _ = ready_tx.send(Err(error));
            return;
        };
        let context = create_context(&backend, &model, &config);
        let Ok(mut ctx) = context else {
            let error = context.err().unwrap_or_else(|| LlamaError {
                message: "native llama context failed to initialize".to_string(),
            });
            let _ = ready_tx.send(Err(error));
            return;
        };

        let draft_model = match config.draft.as_ref() {
            Some(draft) => match load_model(&backend, &draft.model_path, &config, "draft") {
                Ok(model) => Some(model),
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            },
            None => None,
        };
        let mut draft_context = match draft_model.as_ref() {
            Some(model) => match create_draft_context(&backend, model, &mut ctx, &config) {
                Ok(context) => Some(context),
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            },
            None => None,
        };
        let mut speculative = match draft_context.as_mut() {
            Some(draft_ctx) => match create_speculative_mtp(&mut ctx, draft_ctx, &config) {
                Ok(speculative) => Some(speculative),
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            },
            None => None,
        };
        let _ = ready_tx.send(Ok(()));

        let mut hot_cache: Option<HotCacheEntry> = None;
        while let Ok(command) = command_rx.recv() {
            let NativeWorkerCommand::Generate(first_job) = command else {
                break;
            };
            let mut jobs = vec![first_job];
            while jobs.len() < usize::try_from(effective_parallel(&config)).unwrap_or(usize::MAX) {
                match command_rx.try_recv() {
                    Ok(NativeWorkerCommand::Generate(job)) => jobs.push(job),
                    Ok(NativeWorkerCommand::Stop) => return,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }

            let results = run_generation_batch(
                &model,
                &mut ctx,
                speculative.as_mut(),
                &config,
                jobs,
                &mut hot_cache,
                &stats,
            );
            for (response_tx, result) in results {
                let _ = response_tx.send(result);
            }
        }
    }

    fn load_backend_and_model(
        config: &NativeWorkerConfig,
    ) -> LlamaResult<(LlamaBackend, LlamaModel)> {
        let mut backend = LlamaBackend::init().map_err(|error| LlamaError {
            message: format!("failed to initialize llama.cpp backend: {error}"),
        })?;
        backend.void_logs();

        let model = load_model(&backend, &config.model_path, config, "target")?;

        Ok((backend, model))
    }

    fn load_model(
        backend: &LlamaBackend,
        model_path: &Path,
        config: &NativeWorkerConfig,
        role: &str,
    ) -> LlamaResult<LlamaModel> {
        let mut model_params = LlamaModelParams::default().with_use_mmap(true);
        if config.launch.n_gpu_layers != "auto" {
            let layers = config
                .launch
                .n_gpu_layers
                .parse::<u32>()
                .map_err(|error| LlamaError {
                    message: format!("invalid native n_gpu_layers value: {error}"),
                })?;
            model_params = model_params.with_n_gpu_layers(layers);
        }

        let model =
            LlamaModel::load_from_file(backend, model_path, &model_params).map_err(|error| {
                LlamaError {
                    message: format!(
                        "failed to load native llama {role} model {}: {error}",
                        model_path.to_string_lossy()
                    ),
                }
            })?;

        Ok(model)
    }

    fn create_context<'model>(
        backend: &LlamaBackend,
        model: &'model LlamaModel,
        config: &NativeWorkerConfig,
    ) -> LlamaResult<LlamaContext<'model>> {
        let ctx_size = NonZeroU32::new(config.launch.ctx_size).ok_or_else(|| LlamaError {
            message: "native ctx_size must be non-zero".to_string(),
        })?;
        let threads = native_thread_count();
        let n_rs_seq = if config.draft.is_some() {
            config.launch.spec_draft_n_max
        } else {
            0
        };
        let _cache_controls = (
            config.launch.continuous_batching,
            config.launch.cache_ram_mb,
            config.launch.cache_idle_slots,
        );
        let context_params = LlamaContextParams::default()
            .with_n_ctx(Some(ctx_size))
            .with_n_batch(DEFAULT_BATCH_TOKENS as u32)
            .with_n_ubatch(DEFAULT_UBATCH_TOKENS)
            .with_n_seq_max(effective_parallel(config) + 1)
            .with_n_rs_seq(n_rs_seq)
            .with_n_threads(threads)
            .with_n_threads_batch(threads)
            .with_kv_unified(true)
            .with_no_perf(false);
        let ctx = model
            .new_context(backend, context_params)
            .map_err(|error| LlamaError {
                message: format!("failed to create native llama context: {error}"),
            })?;
        Ok(ctx)
    }

    fn create_draft_context<'model>(
        backend: &LlamaBackend,
        model: &'model LlamaModel,
        target_ctx: &mut LlamaContext<'_>,
        config: &NativeWorkerConfig,
    ) -> LlamaResult<LlamaContext<'model>> {
        let ctx_size = NonZeroU32::new(config.launch.ctx_size).ok_or_else(|| LlamaError {
            message: "native ctx_size must be non-zero".to_string(),
        })?;
        let threads = native_thread_count();
        let context_params = LlamaContextParams::default()
            .with_n_ctx(Some(ctx_size))
            .with_n_batch(DEFAULT_BATCH_TOKENS as u32)
            .with_n_ubatch(DEFAULT_UBATCH_TOKENS)
            .with_n_seq_max(effective_parallel(config) + 1)
            .with_context_type_mtp()
            .with_context_other(target_ctx)
            .with_n_threads(threads)
            .with_n_threads_batch(threads)
            .with_kv_unified(true)
            .with_no_perf(false);
        model
            .new_context(backend, context_params)
            .map_err(|error| LlamaError {
                message: format!("failed to create native llama MTP draft context: {error}"),
            })
    }

    fn create_speculative_mtp(
        target_ctx: &mut LlamaContext<'_>,
        draft_ctx: &mut LlamaContext<'_>,
        config: &NativeWorkerConfig,
    ) -> LlamaResult<LlamaSpeculativeMtp> {
        let params = LlamaSpeculativeMtpParams {
            max_draft_tokens: config.launch.spec_draft_n_max,
            min_draft_tokens: 0,
            min_draft_probability: 0.0,
            backend_sampling: true,
        };
        let speculative = LlamaSpeculativeMtp::new(
            target_ctx,
            draft_ctx,
            params,
            effective_parallel(config) + 1,
        )
        .map_err(|error| LlamaError {
            message: format!("failed to initialize native llama MTP speculator: {error}"),
        })?;
        if !speculative.need_embd_nextn() {
            return Err(LlamaError {
                message: "native llama MTP speculator did not request next-token embeddings"
                    .to_string(),
            });
        }
        Ok(speculative)
    }

    fn native_thread_count() -> i32 {
        std::thread::available_parallelism()
            .map(|threads| threads.get().clamp(2, 16) as i32)
            .unwrap_or(4)
    }

    fn effective_parallel(config: &NativeWorkerConfig) -> u32 {
        if config.launch.continuous_batching {
            config.launch.parallel
        } else {
            1
        }
    }

    fn run_generation_batch(
        model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        speculative: Option<&mut LlamaSpeculativeMtp>,
        config: &NativeWorkerConfig,
        jobs: Vec<NativeWorkerJob>,
        hot_cache: &mut Option<HotCacheEntry>,
        stats: &Arc<Mutex<NativeWorkerStats>>,
    ) -> Vec<(
        mpsc::Sender<LlamaResult<NativeGenerationResult>>,
        LlamaResult<NativeGenerationResult>,
    )> {
        let mut responses = Vec::new();
        let mut active = Vec::new();
        for (index, job) in jobs.into_iter().enumerate() {
            let response_tx = job.response_tx.clone();
            match prepare_active_generation(model, config, index, job) {
                Ok(active_job) => active.push(active_job),
                Err(error) => responses.push((response_tx, Err(error))),
            }
        }

        if active.is_empty() {
            return responses;
        }

        let batch_result = run_active_generations(
            model,
            ctx,
            speculative,
            config,
            &mut active,
            hot_cache,
            stats,
        );
        match batch_result {
            Ok(()) => {
                for generation in active {
                    responses.push((
                        generation.response_tx.clone(),
                        Ok(finish_generation(generation)),
                    ));
                }
            }
            Err(error) => {
                for generation in active {
                    responses.push((
                        generation.response_tx.clone(),
                        Err(clone_llama_error(&error)),
                    ));
                }
            }
        }
        responses
    }

    fn prepare_active_generation(
        model: &LlamaModel,
        config: &NativeWorkerConfig,
        index: usize,
        job: NativeWorkerJob,
    ) -> LlamaResult<ActiveGeneration> {
        let mut tokens = model
            .str_to_token(&job.prompt, AddBos::Always)
            .map_err(|error| LlamaError {
                message: format!("failed to tokenize native prompt: {error}"),
            })?;
        if tokens.is_empty() {
            tokens.push(model.token_bos());
        }
        let max_tokens = job.options.max_tokens.max(1);
        let total_budget = tokens.len() + usize::try_from(max_tokens).unwrap_or(usize::MAX);
        if total_budget >= usize::try_from(config.launch.ctx_size).unwrap_or(usize::MAX) {
            return Err(LlamaError {
                message: format!(
                    "native prompt plus max tokens ({total_budget}) exceeds ctxSize {}",
                    config.launch.ctx_size
                ),
            });
        }

        let prompt_token_count = tokens.len();
        let prompt_cache_prefix_tokens =
            prompt_cache_prefix_tokens(model, &tokens, job.options.prompt_cache_prefix.as_deref())?;
        let sampler = build_sampler(&job.options);
        Ok(ActiveGeneration {
            seq_id: FIRST_REQUEST_SEQ_ID + i32::try_from(index).unwrap_or(i32::MAX),
            prompt: job.prompt,
            tokens,
            prompt_token_count,
            cursor: 0,
            position: 0,
            sampler,
            output: Vec::new(),
            generated_tokens: 0,
            last_logit_index: -1,
            pending_decode_token: None,
            prompt_cache_key: job.options.prompt_cache_key.unwrap_or_default(),
            prompt_cache_prefix_tokens,
            prompt_cache_hit: false,
            cold_cache_bytes: None,
            token_sink: job.options.token_sink,
            cancel: job.options.cancel,
            response_tx: job.response_tx,
            started_at: Instant::now(),
            max_tokens,
        })
    }

    fn run_active_generations(
        model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        mut speculative: Option<&mut LlamaSpeculativeMtp>,
        config: &NativeWorkerConfig,
        active: &mut [ActiveGeneration],
        hot_cache: &mut Option<HotCacheEntry>,
        stats: &Arc<Mutex<NativeWorkerStats>>,
    ) -> LlamaResult<()> {
        for generation in active.iter_mut() {
            ctx.clear_kv_cache_seq(Some(generation.seq_id as u32), None, None)
                .map_err(|error| LlamaError {
                    message: format!("failed to clear native KV sequence: {error}"),
                })?;
            generation.prompt_cache_key =
                normalized_prompt_cache_key(config, generation, &generation.prompt_cache_key);
            try_restore_prompt_cache(ctx, config, generation, hot_cache, stats)?;
            if let Some(speculative) = speculative.as_mut() {
                speculative
                    .begin(generation.seq_id, &generation.tokens)
                    .map_err(|error| LlamaError {
                        message: format!("native MTP begin failed: {error}"),
                    })?;
            }
        }

        prefill_to_cache_boundary(
            model,
            ctx,
            reborrow_speculative(&mut speculative),
            config,
            active,
        )?;
        save_prompt_caches(ctx, config, active, hot_cache, stats)?;
        prefill_to_logits(
            model,
            ctx,
            reborrow_speculative(&mut speculative),
            config,
            active,
        )?;
        decode_generated_tokens(model, ctx, speculative, config, active, stats)?;
        Ok(())
    }

    fn try_restore_prompt_cache(
        ctx: &mut LlamaContext<'_>,
        config: &NativeWorkerConfig,
        generation: &mut ActiveGeneration,
        hot_cache: &Option<HotCacheEntry>,
        stats: &Arc<Mutex<NativeWorkerStats>>,
    ) -> LlamaResult<()> {
        let prefix_len = cache_prefix_len(generation);
        if prefix_len == 0 {
            return Ok(());
        }
        let prefix = &generation.tokens[..prefix_len];
        if let Some(entry) = hot_cache {
            if entry.key == generation.prompt_cache_key && entry.tokens == prefix {
                ctx.copy_kv_cache_seq(
                    RESERVED_HOT_CACHE_SEQ_ID,
                    generation.seq_id,
                    Some(0),
                    Some(prefix_len as u32),
                )
                .map_err(|error| LlamaError {
                    message: format!("failed to copy hot native KV cache: {error}"),
                })?;
                generation.cursor = prefix_len;
                generation.position = prefix_len as i32;
                generation.prompt_cache_hit = true;
                let mut stats = stats.lock().map_err(|_| LlamaError {
                    message: "native llama stats lock poisoned".to_string(),
                })?;
                stats.hot_cache_hits += 1;
                return Ok(());
            }
        }

        if !config.launch.cold_kv_cache {
            return Ok(());
        }
        let path = cold_cache_path(config, &generation.prompt_cache_key);
        if !path.is_file() {
            return Ok(());
        }
        match ctx.state_seq_load_file(&path, generation.seq_id, prefix_len) {
            Ok((loaded, bytes_read)) if loaded == prefix => {
                generation.cursor = prefix_len;
                generation.position = prefix_len as i32;
                generation.prompt_cache_hit = true;
                generation.cold_cache_bytes = Some(bytes_read);
                let mut stats = stats.lock().map_err(|_| LlamaError {
                    message: "native llama stats lock poisoned".to_string(),
                })?;
                stats.cold_cache_hits += 1;
                Ok(())
            }
            Ok(_) | Err(_) => {
                let _ = std::fs::remove_file(path);
                Ok(())
            }
        }
    }

    fn prefill_to_cache_boundary(
        model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        speculative: Option<&mut LlamaSpeculativeMtp>,
        config: &NativeWorkerConfig,
        active: &mut [ActiveGeneration],
    ) -> LlamaResult<()> {
        for generation in active.iter_mut() {
            for token in generation.tokens.iter().take(generation.cursor) {
                generation.sampler.accept(*token);
            }
        }
        decode_prompt_until(
            model,
            ctx,
            speculative,
            config,
            active,
            false,
            cache_prefix_len,
        )
    }

    fn save_prompt_caches(
        ctx: &mut LlamaContext<'_>,
        config: &NativeWorkerConfig,
        active: &mut [ActiveGeneration],
        hot_cache: &mut Option<HotCacheEntry>,
        stats: &Arc<Mutex<NativeWorkerStats>>,
    ) -> LlamaResult<()> {
        for generation in active.iter() {
            if generation.prompt_cache_hit {
                continue;
            }
            let prefix_len = cache_prefix_len(generation);
            if prefix_len == 0 || generation.cursor < prefix_len {
                continue;
            }
            ctx.clear_kv_cache_seq(Some(RESERVED_HOT_CACHE_SEQ_ID as u32), None, None)
                .map_err(|error| LlamaError {
                    message: format!("failed to clear native hot cache sequence: {error}"),
                })?;
            ctx.copy_kv_cache_seq(
                generation.seq_id,
                RESERVED_HOT_CACHE_SEQ_ID,
                Some(0),
                Some(prefix_len as u32),
            )
            .map_err(|error| LlamaError {
                message: format!("failed to update native hot KV cache: {error}"),
            })?;
            *hot_cache = Some(HotCacheEntry {
                key: generation.prompt_cache_key.clone(),
                tokens: generation.tokens[..prefix_len].to_vec(),
            });

            if config.launch.cold_kv_cache {
                let path = cold_cache_path(config, &generation.prompt_cache_key);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let bytes_written = ctx
                    .state_seq_save_file(&path, generation.seq_id, &generation.tokens[..prefix_len])
                    .map_err(|error| LlamaError {
                        message: format!("failed to save native cold KV cache: {error}"),
                    })?;
                let mut stats = stats.lock().map_err(|_| LlamaError {
                    message: "native llama stats lock poisoned".to_string(),
                })?;
                stats.hot_cache_entries = 1;
                if bytes_written > 0 {
                    stats.cold_cache_writes += 1;
                }
            }
        }
        Ok(())
    }

    fn prefill_to_logits(
        model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        speculative: Option<&mut LlamaSpeculativeMtp>,
        config: &NativeWorkerConfig,
        active: &mut [ActiveGeneration],
    ) -> LlamaResult<()> {
        decode_prompt_until(
            model,
            ctx,
            speculative,
            config,
            active,
            true,
            |generation| generation.prompt_token_count,
        )
    }

    fn decode_prompt_until<F>(
        _model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        mut speculative: Option<&mut LlamaSpeculativeMtp>,
        config: &NativeWorkerConfig,
        active: &mut [ActiveGeneration],
        logits_on_final_prompt_token: bool,
        target_len: F,
    ) -> LlamaResult<()>
    where
        F: Fn(&ActiveGeneration) -> usize,
    {
        loop {
            if active
                .iter()
                .all(|generation| generation.cursor >= target_len(generation))
            {
                return Ok(());
            }

            let mut batch =
                LlamaBatch::new(DEFAULT_BATCH_TOKENS, effective_parallel(config) as i32 + 1);
            let mut inserted = 0;
            for generation in active.iter_mut() {
                let target = target_len(generation);
                while generation.cursor < target && inserted < DEFAULT_BATCH_TOKENS {
                    let cursor = generation.cursor;
                    let logits =
                        logits_on_final_prompt_token && cursor + 1 == generation.tokens.len();
                    let batch_index = batch.n_tokens();
                    batch
                        .add(
                            generation.tokens[cursor],
                            cursor as i32,
                            &[generation.seq_id],
                            logits,
                        )
                        .map_err(|error| LlamaError {
                            message: format!("failed to add native prompt token: {error}"),
                        })?;
                    if logits {
                        generation.last_logit_index = batch_index;
                    }
                    generation.sampler.accept(generation.tokens[cursor]);
                    generation.cursor += 1;
                    generation.position = generation.cursor as i32;
                    inserted += 1;
                }
            }
            if inserted == 0 {
                return Ok(());
            }
            ctx.decode(&mut batch).map_err(|error| LlamaError {
                message: format!("native llama prompt decode failed: {error}"),
            })?;
            if let Some(speculative) = speculative.as_mut() {
                speculative.process(&batch).map_err(|error| LlamaError {
                    message: format!("native MTP prompt process failed: {error}"),
                })?;
            }
        }
    }

    fn decode_generated_tokens(
        model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        mut speculative: Option<&mut LlamaSpeculativeMtp>,
        config: &NativeWorkerConfig,
        active: &mut [ActiveGeneration],
        stats: &Arc<Mutex<NativeWorkerStats>>,
    ) -> LlamaResult<()> {
        let mut remaining: VecDeque<usize> = (0..active.len()).collect();
        while !remaining.is_empty() {
            let mut next_round = VecDeque::new();
            let mut batch =
                LlamaBatch::new(DEFAULT_BATCH_TOKENS, effective_parallel(config) as i32 + 1);
            let mut decode_count = 0;
            let mut pending_items = Vec::new();

            while let Some(index) = remaining.pop_front() {
                let generation = &mut active[index];
                if generation
                    .cancel
                    .as_ref()
                    .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
                {
                    return Err(LlamaError {
                        message: "native generation cancelled".to_string(),
                    });
                }
                if generation.generated_tokens >= generation.max_tokens {
                    continue;
                }
                if generation.last_logit_index < 0 {
                    return Err(LlamaError {
                        message: "native generation missing prompt logits".to_string(),
                    });
                }

                if let Some(pending) = generation.pending_decode_token.take() {
                    if batch_space(&batch) == 0 {
                        generation.pending_decode_token = Some(pending);
                        remaining.push_front(index);
                        break;
                    }

                    let batch_index = batch.n_tokens();
                    batch
                        .add(pending.token, pending.position, &[generation.seq_id], true)
                        .map_err(|error| LlamaError {
                            message: format!(
                                "failed to add native pending generation token: {error}"
                            ),
                        })?;
                    generation.last_logit_index = batch_index;
                    decode_count += 1;
                    pending_items.push(PendingBatchItem::DecodeOnly {
                        generation_index: index,
                    });
                    continue;
                }

                let primary_token = generation.sampler.sample(ctx, generation.last_logit_index);
                if model.is_eog_token(primary_token) {
                    continue;
                }

                let free_slots = batch_space(&batch);
                if free_slots == 0 {
                    remaining.push_front(index);
                    break;
                }

                let remaining_after_primary = generation
                    .max_tokens
                    .saturating_sub(generation.generated_tokens)
                    .saturating_sub(1);
                let draft_capacity = remaining_after_primary
                    .min(config.launch.spec_draft_n_max)
                    .min(u32::try_from(free_slots.saturating_sub(1)).unwrap_or(u32::MAX));
                let mut draft_tokens = if draft_capacity > 0 {
                    match speculative.as_mut() {
                        Some(speculative) => speculative
                            .draft(
                                generation.seq_id,
                                generation.position,
                                primary_token,
                                &generation.tokens,
                                Some(draft_capacity),
                            )
                            .map_err(|error| LlamaError {
                                message: format!("native MTP draft failed: {error}"),
                            })?,
                        None => Vec::new(),
                    }
                } else {
                    Vec::new()
                };
                draft_tokens.truncate(usize::try_from(draft_capacity).unwrap_or(usize::MAX));

                let sampled_position = generation.position;
                let batch_start = batch.n_tokens();
                batch
                    .add(primary_token, sampled_position, &[generation.seq_id], true)
                    .map_err(|error| LlamaError {
                        message: format!("failed to add native generation token: {error}"),
                    })?;
                for (offset, token) in draft_tokens.iter().enumerate() {
                    batch
                        .add(
                            *token,
                            sampled_position + i32::try_from(offset + 1).unwrap_or(i32::MAX),
                            &[generation.seq_id],
                            true,
                        )
                        .map_err(|error| LlamaError {
                            message: format!("failed to add native draft token: {error}"),
                        })?;
                }
                decode_count += 1 + draft_tokens.len();
                pending_items.push(PendingBatchItem::Verify(PendingVerification {
                    generation_index: index,
                    batch_start,
                    primary_token,
                    draft_tokens,
                    sampled_position,
                }));
            }

            if decode_count == 0 {
                return Ok(());
            }
            ctx.decode(&mut batch).map_err(|error| LlamaError {
                message: format!("native llama decode failed: {error}"),
            })?;
            if let Some(speculative) = speculative.as_mut() {
                speculative.process(&batch).map_err(|error| LlamaError {
                    message: format!("native MTP decode process failed: {error}"),
                })?;
            }

            let mut drafted_tokens = 0_u64;
            let mut accepted_draft_tokens = 0_u64;
            for item in pending_items {
                match item {
                    PendingBatchItem::DecodeOnly { generation_index } => {
                        let generation = &active[generation_index];
                        if generation.generated_tokens < generation.max_tokens {
                            next_round.push_back(generation_index);
                        }
                    }
                    PendingBatchItem::Verify(verification) => {
                        let result = verify_generated_tokens(
                            model,
                            ctx,
                            reborrow_speculative(&mut speculative),
                            active,
                            verification,
                        )?;
                        drafted_tokens += result.drafted_tokens;
                        accepted_draft_tokens += result.accepted_draft_tokens;
                        if result.continue_generation {
                            next_round.push_back(result.generation_index);
                        }
                    }
                }
            }

            if drafted_tokens > 0 || accepted_draft_tokens > 0 {
                let mut stats = stats.lock().map_err(|_| LlamaError {
                    message: "native llama stats lock poisoned".to_string(),
                })?;
                stats.speculative_draft_tokens += drafted_tokens;
                stats.speculative_accepted_tokens += accepted_draft_tokens;
            }
            next_round.extend(remaining);
            remaining = next_round;
        }
        Ok(())
    }

    struct VerificationResult {
        generation_index: usize,
        drafted_tokens: u64,
        accepted_draft_tokens: u64,
        continue_generation: bool,
    }

    fn verify_generated_tokens(
        model: &LlamaModel,
        ctx: &mut LlamaContext<'_>,
        speculative: Option<&mut LlamaSpeculativeMtp>,
        active: &mut [ActiveGeneration],
        verification: PendingVerification,
    ) -> LlamaResult<VerificationResult> {
        let generation = &mut active[verification.generation_index];
        emit_generated_token(model, generation, verification.primary_token)?;

        let mut accepted_draft_count = 0_usize;
        let mut emitted_rejection = false;
        let mut stopped_by_eog = false;
        for (draft_index, draft_token) in verification.draft_tokens.iter().enumerate() {
            if generation.generated_tokens >= generation.max_tokens {
                break;
            }
            let logits_index =
                verification.batch_start + i32::try_from(draft_index).unwrap_or(i32::MAX);
            let target_token = generation.sampler.sample(ctx, logits_index);
            if target_token == *draft_token {
                if model.is_eog_token(target_token) {
                    stopped_by_eog = true;
                    break;
                }
                emit_generated_token(model, generation, *draft_token)?;
                accepted_draft_count += 1;
                continue;
            }

            if !model.is_eog_token(target_token) {
                let position = generation.position;
                emit_generated_token(model, generation, target_token)?;
                if generation.generated_tokens < generation.max_tokens {
                    generation.pending_decode_token = Some(PendingDecodeToken {
                        token: target_token,
                        position,
                    });
                }
                emitted_rejection = true;
            } else {
                stopped_by_eog = true;
            }
            break;
        }

        let decoded_tokens = 1 + verification.draft_tokens.len();
        let accepted_decoded_tokens = 1 + accepted_draft_count;
        rollback_rejected_draft_tokens(
            ctx,
            generation,
            verification.sampled_position,
            accepted_decoded_tokens,
            decoded_tokens,
        )?;

        if !verification.draft_tokens.is_empty() {
            if let Some(speculative) = speculative {
                speculative
                    .accept(
                        generation.seq_id,
                        u16::try_from(accepted_draft_count).unwrap_or(u16::MAX),
                    )
                    .map_err(|error| LlamaError {
                        message: format!("native MTP accept failed: {error}"),
                    })?;
            }
        }

        generation.last_logit_index =
            verification.batch_start + i32::try_from(accepted_draft_count).unwrap_or(i32::MAX);
        let continue_generation = generation.generated_tokens < generation.max_tokens
            && !stopped_by_eog
            && !emitted_rejection;

        Ok(VerificationResult {
            generation_index: verification.generation_index,
            drafted_tokens: verification.draft_tokens.len() as u64,
            accepted_draft_tokens: accepted_draft_count as u64,
            continue_generation: continue_generation || generation.pending_decode_token.is_some(),
        })
    }

    fn emit_generated_token(
        model: &LlamaModel,
        generation: &mut ActiveGeneration,
        token: LlamaToken,
    ) -> LlamaResult<()> {
        append_token_piece(model, generation, token)?;
        generation.sampler.accept(token);
        generation.tokens.push(token);
        generation.generated_tokens += 1;
        generation.position += 1;
        Ok(())
    }

    fn rollback_rejected_draft_tokens(
        ctx: &mut LlamaContext<'_>,
        generation: &ActiveGeneration,
        start_position: i32,
        accepted_decoded_tokens: usize,
        decoded_tokens: usize,
    ) -> LlamaResult<()> {
        if decoded_tokens <= accepted_decoded_tokens {
            return Ok(());
        }
        let rollback_start = start_position + i32::try_from(accepted_decoded_tokens).unwrap_or(0);
        let rollback_end = start_position + i32::try_from(decoded_tokens).unwrap_or(0);
        if rollback_start < 0 || rollback_end <= rollback_start {
            return Ok(());
        }
        let removed = ctx
            .clear_kv_cache_seq(
                Some(generation.seq_id as u32),
                Some(u32::try_from(rollback_start).map_err(|error| LlamaError {
                    message: format!("native MTP rollback start position invalid: {error}"),
                })?),
                Some(u32::try_from(rollback_end).map_err(|error| LlamaError {
                    message: format!("native MTP rollback end position invalid: {error}"),
                })?),
            )
            .map_err(|error| LlamaError {
                message: format!("native MTP rollback failed: {error}"),
            })?;
        if !removed {
            return Err(LlamaError {
                message: "native MTP rollback was rejected by llama.cpp KV cache".to_string(),
            });
        }
        Ok(())
    }

    fn batch_space(batch: &LlamaBatch<'_>) -> usize {
        DEFAULT_BATCH_TOKENS.saturating_sub(usize::try_from(batch.n_tokens()).unwrap_or(usize::MAX))
    }

    fn reborrow_speculative<'a>(
        speculative: &'a mut Option<&mut LlamaSpeculativeMtp>,
    ) -> Option<&'a mut LlamaSpeculativeMtp> {
        match speculative {
            Some(value) => Some(&mut **value),
            None => None,
        }
    }

    fn finish_generation(generation: ActiveGeneration) -> NativeGenerationResult {
        let elapsed = generation.started_at.elapsed();
        let elapsed_secs = elapsed.as_secs_f64().max(0.001);
        NativeGenerationResult {
            content: String::from_utf8_lossy(&generation.output).to_string(),
            prompt_tokens: generation.prompt_token_count as u32,
            generated_tokens: generation.generated_tokens,
            elapsed_ms: elapsed.as_millis(),
            tokens_per_second: f64::from(generation.generated_tokens) / elapsed_secs,
            prompt_cache_hit: generation.prompt_cache_hit,
            cold_cache_bytes: generation.cold_cache_bytes,
        }
    }

    fn append_token_piece(
        model: &LlamaModel,
        generation: &mut ActiveGeneration,
        token: LlamaToken,
    ) -> LlamaResult<()> {
        let bytes = match model.token_to_piece_bytes(token, 32, false, None) {
            Ok(bytes) => bytes,
            Err(llama_cpp_2::TokenToStringError::InsufficientBufferSpace(size)) => model
                .token_to_piece_bytes(token, usize::try_from(-size).unwrap_or(256), false, None)
                .map_err(|error| LlamaError {
                    message: format!("failed to decode native token: {error}"),
                })?,
            Err(error) => {
                return Err(LlamaError {
                    message: format!("failed to decode native token: {error}"),
                });
            }
        };
        if !bytes.is_empty() {
            generation.output.extend_from_slice(&bytes);
            if let Some(sender) = &generation.token_sink {
                let _ = sender.send(String::from_utf8_lossy(&bytes).to_string());
            }
        }
        Ok(())
    }

    fn build_sampler(options: &NativeGenerateOptions) -> LlamaSampler {
        if options.temperature <= 0.0 {
            return LlamaSampler::greedy();
        }
        LlamaSampler::chain_simple([
            LlamaSampler::top_p(options.top_p.clamp(0.01, 1.0), 1),
            LlamaSampler::temp(options.temperature.clamp(0.01, 2.0)),
            LlamaSampler::dist(options.seed),
        ])
    }

    fn cache_prefix_len(generation: &ActiveGeneration) -> usize {
        generation
            .prompt_cache_prefix_tokens
            .unwrap_or_else(|| generation.prompt_token_count.saturating_sub(1))
            .min(generation.prompt_token_count.saturating_sub(1))
    }

    fn prompt_cache_prefix_tokens(
        model: &LlamaModel,
        prompt_tokens: &[LlamaToken],
        prefix: Option<&str>,
    ) -> LlamaResult<Option<usize>> {
        let Some(prefix) = prefix.filter(|value| !value.trim().is_empty()) else {
            return Ok(None);
        };
        let prefix_tokens = model
            .str_to_token(prefix, AddBos::Always)
            .map_err(|error| LlamaError {
                message: format!("failed to tokenize native prompt cache prefix: {error}"),
            })?;
        if prefix_tokens.is_empty() {
            return Ok(None);
        }
        let common_prefix_len = prefix_tokens
            .iter()
            .zip(prompt_tokens.iter())
            .take_while(|(left, right)| left == right)
            .count();
        let usable_prefix_len = if common_prefix_len == prefix_tokens.len() {
            common_prefix_len
        } else {
            common_prefix_len.saturating_sub(1)
        };
        if usable_prefix_len == 0 {
            return Err(LlamaError {
                message: "native prompt cache prefix must match the beginning of the prompt"
                    .to_string(),
            });
        }
        Ok(Some(
            usable_prefix_len.min(prompt_tokens.len().saturating_sub(1)),
        ))
    }

    fn normalized_prompt_cache_key(
        config: &NativeWorkerConfig,
        generation: &ActiveGeneration,
        requested: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"whorary-native-prompt-cache\0");
        hasher.update(config.model_sha256.as_bytes());
        hasher.update([0]);
        hasher.update(NATIVE_LLAMA_BINDING_VERSION.as_bytes());
        hasher.update([0]);
        hasher.update(config.launch.ctx_size.to_le_bytes());
        let requested = requested.trim();
        if let Some(prefix_len) = generation.prompt_cache_prefix_tokens {
            hasher.update(b"prefix\0");
            hasher.update(requested.as_bytes());
            hasher.update([0]);
            for token in &generation.tokens[..prefix_len] {
                hasher.update(token.0.to_le_bytes());
            }
        } else {
            hasher.update(b"prompt\0");
            hasher.update(requested.as_bytes());
            hasher.update([0]);
            hasher.update(generation.prompt.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    fn cold_cache_path(config: &NativeWorkerConfig, key: &str) -> PathBuf {
        config.cache_dir.join(format!("{key}.seqstate"))
    }
}

#[cfg(not(feature = "native-llama"))]
mod imp {
    use super::*;

    pub fn start_native_llama_in_dir(
        _app_data_dir: &Path,
        _state: &NativeLlamaState,
        _req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        Err(not_compiled_error())
    }

    pub fn start_native_llama_from_path(
        _state: &NativeLlamaState,
        _model_id: String,
        _model_sha256: String,
        _model_path: PathBuf,
        _cache_dir: PathBuf,
        _req: StartLlamaRequest,
    ) -> LlamaResult<LlamaStatus> {
        Err(not_compiled_error())
    }

    pub fn stop_native_llama(_state: &NativeLlamaState) -> LlamaResult<bool> {
        Ok(false)
    }

    pub fn native_llama_status(_state: &NativeLlamaState) -> LlamaResult<LlamaStatus> {
        Ok(stopped_status())
    }

    pub fn native_llama_health(_state: &NativeLlamaState) -> LlamaResult<NativeLlamaHealth> {
        Ok(NativeLlamaHealth {
            compiled: false,
            running: false,
            model_id: None,
            backend: NATIVE_LLAMA_RUNTIME_BACKEND,
            ctx_size: None,
            parallel: None,
            speculative_decoding_supported: NATIVE_LLAMA_MTP_RUNTIME_AVAILABLE,
            speculative_decoding_active: false,
            draft_model_id: None,
            hot_cache_entries: 0,
            hot_cache_hits: 0,
            cold_cache_hits: 0,
            cold_cache_writes: 0,
            speculative_draft_tokens: 0,
            speculative_accepted_tokens: 0,
        })
    }

    pub fn generate_native(
        _state: &NativeLlamaState,
        _prompt: String,
        _options: NativeGenerateOptions,
    ) -> LlamaResult<NativeGenerationResult> {
        Err(not_compiled_error())
    }

    fn not_compiled_error() -> LlamaError {
        LlamaError {
            message: "native llama.cpp runtime was not compiled into this build".to_string(),
        }
    }
}

#[cfg(all(test, feature = "native-llama"))]
pub use imp::start_native_llama_from_path;
pub use imp::{
    generate_native, native_llama_health, native_llama_status, start_native_llama_in_dir,
    stop_native_llama,
};

impl Drop for NativeWorkerHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(NativeWorkerCommand::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_status(handle: &NativeWorkerHandle) -> LlamaStatus {
    LlamaStatus {
        running: true,
        model_id: Some(handle.model_id.clone()),
        port: None,
        backend: Some(NATIVE_LLAMA_RUNTIME_BACKEND.to_string()),
        log_path: None,
    }
}

fn stopped_status() -> LlamaStatus {
    LlamaStatus {
        running: false,
        model_id: None,
        port: None,
        backend: None,
        log_path: None,
    }
}

fn clone_llama_error(error: &LlamaError) -> LlamaError {
    LlamaError {
        message: error.message.clone(),
    }
}

fn sanitize_cache_component(value: &str) -> String {
    let sanitized = value
        .trim()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' || char == '_' {
                char.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "cache".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_cache_components_for_file_names() {
        assert_eq!(
            sanitize_cache_component(" Horary/Prompt:V2 "),
            "horary-prompt-v2"
        );
        assert_eq!(sanitize_cache_component("///"), "cache");
    }
}

#[cfg(all(test, feature = "native-llama"))]
mod integration_tests {
    use super::*;
    use std::{fs, time::SystemTime};

    fn start_request(model_id: &str) -> StartLlamaRequest {
        StartLlamaRequest {
            model_id: model_id.to_string(),
            ctx_size: Some(512),
            n_gpu_layers: Some("auto".to_string()),
            parallel: Some(2),
            continuous_batching: Some(true),
            cache_ram_mb: Some(256),
            cache_idle_slots: Some(true),
            cold_kv_cache: Some(true),
            draft_model_id: None,
            spec_draft_n_max: Some(3),
        }
    }

    #[test]
    fn native_llama_generates_tokens_from_real_cached_gemma() {
        let Some(model_path) = std::env::var_os("WHORARY_NATIVE_LLAMA_TEST_MODEL") else {
            eprintln!("skipping real native llama gate: WHORARY_NATIVE_LLAMA_TEST_MODEL is unset");
            return;
        };
        let model_path = PathBuf::from(model_path);
        assert!(
            model_path.is_file(),
            "native llama gate model does not exist: {}",
            model_path.to_string_lossy()
        );

        let millis = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let cache_dir = std::env::temp_dir().join(format!("whorary-native-llama-gate-{millis}"));
        fs::create_dir_all(&cache_dir).unwrap();

        let state = NativeLlamaState::default();
        let mtp_enabled = std::env::var_os("WHORARY_NATIVE_LLAMA_MTP_MODEL").is_some();
        let mut req = start_request("gemma-gate");
        if mtp_enabled {
            req.draft_model_id = Some("gemma-gate-assistant".to_string());
        }
        start_native_llama_from_path(
            &state,
            "gemma-gate".to_string(),
            "test-model-sha".to_string(),
            model_path,
            cache_dir.clone(),
            req,
        )
        .unwrap();

        let prompt =
            "You are a deterministic test harness. Reply with the single word OK.\n".to_string();
        let first = generate_native(
            &state,
            prompt.clone(),
            NativeGenerateOptions {
                max_tokens: 8,
                temperature: 0.0,
                top_p: 1.0,
                seed: 1,
                prompt_cache_key: Some("native-gemma-gate".to_string()),
                prompt_cache_prefix: Some("You are a deterministic test harness.".to_string()),
                token_sink: None,
                cancel: None,
            },
        )
        .unwrap();
        let second = generate_native(
            &state,
            prompt,
            NativeGenerateOptions {
                max_tokens: 8,
                temperature: 0.0,
                top_p: 1.0,
                seed: 1,
                prompt_cache_key: Some("native-gemma-gate".to_string()),
                prompt_cache_prefix: Some("You are a deterministic test harness.".to_string()),
                token_sink: None,
                cancel: None,
            },
        )
        .unwrap();
        let health = native_llama_health(&state).unwrap();
        stop_native_llama(&state).unwrap();
        let _ = fs::remove_dir_all(cache_dir);

        assert!(first.prompt_tokens > 0);
        assert!(first.generated_tokens > 0);
        assert!(!first.content.trim().is_empty());
        assert!(first.tokens_per_second > 0.0);
        assert!(second.prompt_cache_hit);
        assert!(second.generated_tokens > 0);
        assert!(health.speculative_decoding_supported);
        assert_eq!(health.speculative_decoding_active, mtp_enabled);
        if mtp_enabled {
            assert_eq!(
                health.draft_model_id.as_deref(),
                Some("gemma-gate-assistant")
            );
            assert!(
                health.speculative_draft_tokens > 0,
                "MTP was configured but no draft tokens were produced"
            );
        }
        assert!(health.hot_cache_hits + health.cold_cache_hits > 0);
        assert!(health.cold_cache_writes > 0);
    }

    #[test]
    fn native_worker_reports_mtp_supported_when_stopped() {
        let state = NativeLlamaState::default();
        let health = native_llama_health(&state).unwrap();
        assert!(health.speculative_decoding_supported);
        assert!(!health.speculative_decoding_active);
        assert_eq!(health.speculative_draft_tokens, 0);
        assert_eq!(health.speculative_accepted_tokens, 0);
    }
}
