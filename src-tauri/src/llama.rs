#![cfg_attr(feature = "native-llama", allow(dead_code))]

use crate::native_llama::{
    NATIVE_LLAMA_DEFAULT_DRAFT_TOKENS, NATIVE_LLAMA_DEFAULT_PARALLEL_SEQUENCES,
    NATIVE_LLAMA_MAX_PARALLEL_SEQUENCES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

const MODELS_DIR: &str = "models";
const LOGS_DIR: &str = "logs";
const BINARIES_DIR: &str = "binaries";
const KV_CACHE_DIR: &str = "kv-cache";
const LLAMA_SERVER_BIN: &str = "llama-server";
const LLAMA_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const LLAMA_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const LLAMA_READY_CONNECT_TIMEOUT: Duration = Duration::from_millis(200);
const LLAMA_CTX_SIZE_MIN: u32 = 512;
const LLAMA_CTX_SIZE_MAX: u32 = 131_072;
const LLAMA_PARALLEL_MIN: u32 = 1;
const LLAMA_PARALLEL_MAX: u32 = NATIVE_LLAMA_MAX_PARALLEL_SEQUENCES;
const LLAMA_GPU_LAYERS_MAX: u32 = 999;
const LLAMA_CACHE_RAM_MB_MAX: u32 = 65_536;
const LLAMA_CACHE_RAM_MB_DEFAULT: u32 = 4096;
const LLAMA_SPEC_DRAFT_N_MAX_MIN: u32 = 1;
const LLAMA_SPEC_DRAFT_N_MAX_MAX: u32 = 8;
pub const LLAMA_SIDECAR_BACKEND: &str = "native sidecar";

#[derive(Debug, Serialize)]
pub struct LlamaError {
    pub message: String,
}

impl From<io::Error> for LlamaError {
    fn from(value: io::Error) -> Self {
        Self {
            message: value.to_string(),
        }
    }
}

pub type LlamaResult<T> = Result<T, LlamaError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportModelRequest {
    pub path: String,
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportLlamaSidecarRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StartLlamaRequest {
    pub model_id: String,
    pub ctx_size: Option<u32>,
    pub n_gpu_layers: Option<String>,
    pub parallel: Option<u32>,
    #[serde(default)]
    pub continuous_batching: Option<bool>,
    #[serde(default)]
    pub cache_ram_mb: Option<u32>,
    #[serde(default)]
    pub cache_idle_slots: Option<bool>,
    #[serde(default)]
    pub cold_kv_cache: Option<bool>,
    #[serde(default)]
    pub draft_model_id: Option<String>,
    #[serde(default)]
    pub spec_draft_n_max: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlamaStatus {
    pub running: bool,
    pub model_id: Option<String>,
    pub port: Option<u16>,
    pub backend: Option<String>,
    pub log_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlamaSidecarStatus {
    pub installed: bool,
    pub path: Option<String>,
    pub checked_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlamaConnection {
    pub model_id: String,
    pub port: u16,
}

pub struct LlamaState {
    running: Mutex<Option<RunningLlama>>,
}

impl Default for LlamaState {
    fn default() -> Self {
        Self {
            running: Mutex::new(None),
        }
    }
}

struct RunningLlama {
    model_id: String,
    port: u16,
    log_path: PathBuf,
    child: Child,
}

pub(crate) struct LlamaLaunchArgs {
    pub(crate) ctx_size: u32,
    pub(crate) n_gpu_layers: String,
    pub(crate) parallel: u32,
    pub(crate) continuous_batching: bool,
    pub(crate) cache_ram_mb: u32,
    pub(crate) cache_idle_slots: bool,
    pub(crate) cold_kv_cache: bool,
    pub(crate) spec_draft_n_max: u32,
}

impl Drop for RunningLlama {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn models_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(MODELS_DIR)
}

#[cfg(test)]
pub fn llama_server_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join(BINARIES_DIR)
        .join(llama_server_binary_name())
}

pub fn llama_server_binary_name() -> String {
    if cfg!(windows) {
        format!("{LLAMA_SERVER_BIN}.exe")
    } else {
        LLAMA_SERVER_BIN.to_string()
    }
}

pub fn llama_server_target_binary_name() -> String {
    let triple = if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        "aarch64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        "aarch64-unknown-linux-gnu"
    } else {
        "unknown"
    };

    if cfg!(windows) {
        format!("{LLAMA_SERVER_BIN}-{triple}.exe")
    } else {
        format!("{LLAMA_SERVER_BIN}-{triple}")
    }
}

pub fn llama_server_candidate_paths(
    app_data_dir: &Path,
    resource_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let binary = llama_server_binary_name();
    let target_binary = llama_server_target_binary_name();
    let mut paths = Vec::new();

    if let Some(resource_dir) = resource_dir {
        paths.push(resource_dir.join(BINARIES_DIR).join(&target_binary));
        paths.push(resource_dir.join(BINARIES_DIR).join(&binary));
        paths.push(resource_dir.join(&target_binary));
        paths.push(resource_dir.join(&binary));
    }

    paths.push(app_data_dir.join(BINARIES_DIR).join(&target_binary));
    paths.push(app_data_dir.join(BINARIES_DIR).join(&binary));
    dedupe_paths(paths)
}

pub fn resolve_llama_server_path(
    app_data_dir: &Path,
    resource_dir: Option<&Path>,
) -> LlamaResult<PathBuf> {
    let candidates = llama_server_candidate_paths(app_data_dir, resource_dir);
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| LlamaError {
            message: format!(
                "llama-server sidecar is not installed; checked {}",
                candidates
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
}

pub fn get_llama_sidecar_status(
    app_data_dir: &Path,
    resource_dir: Option<&Path>,
) -> LlamaSidecarStatus {
    let checked_paths = llama_server_candidate_paths(app_data_dir, resource_dir);
    let path = checked_paths.iter().find(|path| path.is_file()).cloned();
    LlamaSidecarStatus {
        installed: path.is_some(),
        path: path.map(|path| path.to_string_lossy().to_string()),
        checked_paths: checked_paths
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
    }
}

pub fn install_llama_sidecar_to_dir(
    app_data_dir: &Path,
    req: ImportLlamaSidecarRequest,
) -> LlamaResult<LlamaSidecarStatus> {
    let source = PathBuf::from(req.path);
    validate_llama_server_binary_path(&source)?;

    let target_dir = app_data_dir.join(BINARIES_DIR);
    fs::create_dir_all(&target_dir)?;
    let target = target_dir.join(llama_server_target_binary_name());
    if !same_file(&source, &target) {
        fs::copy(&source, &target)?;
    }
    make_executable(&target)?;
    Ok(get_llama_sidecar_status(app_data_dir, None))
}

pub fn import_model_to_dir(app_data_dir: &Path, req: ImportModelRequest) -> LlamaResult<ModelInfo> {
    let source = PathBuf::from(req.path);
    validate_gguf_path(&source)?;

    let model_id = sanitize_model_id(req.id.as_deref().unwrap_or_else(|| {
        source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("model")
    }))?;
    let filename = format!("{model_id}.gguf");
    let target_dir = models_dir(app_data_dir);
    fs::create_dir_all(&target_dir)?;
    let target = target_dir.join(&filename);
    fs::copy(&source, &target)?;

    model_info_from_path(
        &target,
        model_id,
        req.display_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| display_name_from_id(&filename)),
    )
}

pub fn list_models_in_dir(app_data_dir: &Path) -> LlamaResult<Vec<ModelInfo>> {
    let dir = models_dir(app_data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut models = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("gguf") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("model")
            .to_string();
        let display_name = display_name_from_id(&id);
        models.push(model_info_from_path(&path, id, display_name)?);
    }
    models.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(models)
}

pub fn get_model_by_id(app_data_dir: &Path, model_id: &str) -> LlamaResult<ModelInfo> {
    list_models_in_dir(app_data_dir)?
        .into_iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| LlamaError {
            message: format!("model not found: {model_id}"),
        })
}

pub fn get_model_status(state: &LlamaState) -> LlamaResult<LlamaStatus> {
    let mut running = state.running.lock().map_err(|_| LlamaError {
        message: "llama state lock poisoned".to_string(),
    })?;
    clear_exited_llama(&mut running)?;
    Ok(match running.as_ref() {
        Some(process) => LlamaStatus {
            running: true,
            model_id: Some(process.model_id.clone()),
            port: Some(process.port),
            backend: Some(LLAMA_SIDECAR_BACKEND.to_string()),
            log_path: Some(process.log_path.to_string_lossy().to_string()),
        },
        None => LlamaStatus {
            running: false,
            model_id: None,
            port: None,
            backend: None,
            log_path: None,
        },
    })
}

pub fn active_llama_connection(state: &LlamaState) -> LlamaResult<Option<LlamaConnection>> {
    let mut running = state.running.lock().map_err(|_| LlamaError {
        message: "llama state lock poisoned".to_string(),
    })?;
    clear_exited_llama(&mut running)?;
    Ok(running.as_ref().map(|process| LlamaConnection {
        model_id: process.model_id.clone(),
        port: process.port,
    }))
}

#[cfg(test)]
pub fn start_llama_in_dir(
    app_data_dir: &Path,
    state: &LlamaState,
    req: StartLlamaRequest,
) -> LlamaResult<LlamaStatus> {
    start_llama_with_sidecar_dir(app_data_dir, None, state, req)
}

pub fn start_llama_with_sidecar_dir(
    app_data_dir: &Path,
    resource_dir: Option<&Path>,
    state: &LlamaState,
    req: StartLlamaRequest,
) -> LlamaResult<LlamaStatus> {
    {
        let running = state.running.lock().map_err(|_| LlamaError {
            message: "llama state lock poisoned".to_string(),
        })?;
        if running.is_some() {
            drop(running);
            return get_model_status(state);
        }
    }

    let model = get_model_by_id(app_data_dir, &req.model_id)?;
    let model_path = resolve_model_path(app_data_dir, &model.filename)?;
    let draft_model_path = resolve_optional_draft_model_path(app_data_dir, &req)?;
    let binary = resolve_llama_server_path(app_data_dir, resource_dir)?;

    let port = find_free_loopback_port()?;
    let log_dir = app_data_dir.join(LOGS_DIR);
    let kv_cache_dir = app_data_dir.join(KV_CACHE_DIR).join("llama-server-slots");
    fs::create_dir_all(&log_dir)?;
    fs::create_dir_all(&kv_cache_dir)?;
    let log_path = log_dir.join("llama-server.log");
    let log_file = File::create(&log_path)?;
    let log_file_for_stderr = log_file.try_clone()?;

    let args = build_llama_server_args(
        &model_path,
        port,
        &req,
        Some(&kv_cache_dir),
        draft_model_path.as_deref(),
    )?;
    let mut child = Command::new(binary)
        .args(args)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_for_stderr))
        .spawn()?;

    if let Err(error) = wait_for_llama_ready(port, &log_path, LLAMA_STARTUP_TIMEOUT, || {
        Ok(child.try_wait()?.map(|status| status.to_string()))
    }) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let mut running = state.running.lock().map_err(|_| LlamaError {
        message: "llama state lock poisoned".to_string(),
    })?;
    *running = Some(RunningLlama {
        model_id: model.id,
        port,
        log_path,
        child,
    });
    drop(running);
    get_model_status(state)
}

pub fn stop_llama_process(state: &LlamaState) -> LlamaResult<bool> {
    let mut running = state.running.lock().map_err(|_| LlamaError {
        message: "llama state lock poisoned".to_string(),
    })?;
    Ok(running.take().is_some())
}

fn clear_exited_llama(running: &mut Option<RunningLlama>) -> LlamaResult<()> {
    let exited = match running.as_mut() {
        Some(process) => process.child.try_wait()?.is_some(),
        None => false,
    };
    if exited {
        *running = None;
    }
    Ok(())
}

pub fn build_llama_server_args(
    model_path: &Path,
    port: u16,
    req: &StartLlamaRequest,
    slot_save_path: Option<&Path>,
    draft_model_path: Option<&Path>,
) -> LlamaResult<Vec<String>> {
    let launch_args = normalize_llama_launch_args(req)?;
    let mut args = vec![
        "--model".to_string(),
        model_path.to_string_lossy().to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--ctx-size".to_string(),
        launch_args.ctx_size.to_string(),
        "--n-gpu-layers".to_string(),
        launch_args.n_gpu_layers,
        "--parallel".to_string(),
        launch_args.parallel.to_string(),
        if launch_args.continuous_batching {
            "--cont-batching".to_string()
        } else {
            "--no-cont-batching".to_string()
        },
        "--cache-prompt".to_string(),
        "--cache-ram".to_string(),
        launch_args.cache_ram_mb.to_string(),
        if launch_args.cache_idle_slots {
            "--cache-idle-slots".to_string()
        } else {
            "--no-cache-idle-slots".to_string()
        },
        "--offline".to_string(),
    ];

    if launch_args.cold_kv_cache {
        if let Some(path) = slot_save_path {
            args.push("--slot-save-path".to_string());
            args.push(path.to_string_lossy().to_string());
        }
    }

    if let Some(path) = draft_model_path {
        args.push("--model-draft".to_string());
        args.push(path.to_string_lossy().to_string());
        args.push("--spec-type".to_string());
        args.push("draft-mtp".to_string());
        args.push("--spec-draft-n-max".to_string());
        args.push(launch_args.spec_draft_n_max.to_string());
        args.push("--spec-draft-device".to_string());
        args.push("none".to_string());
    }

    Ok(args)
}

pub(crate) fn normalize_llama_launch_args(req: &StartLlamaRequest) -> LlamaResult<LlamaLaunchArgs> {
    let ctx_size = req.ctx_size.unwrap_or(8192);
    if !(LLAMA_CTX_SIZE_MIN..=LLAMA_CTX_SIZE_MAX).contains(&ctx_size) {
        return Err(LlamaError {
            message: format!(
                "ctxSize must be between {LLAMA_CTX_SIZE_MIN} and {LLAMA_CTX_SIZE_MAX}"
            ),
        });
    }

    let parallel = req
        .parallel
        .unwrap_or(NATIVE_LLAMA_DEFAULT_PARALLEL_SEQUENCES);
    if !(LLAMA_PARALLEL_MIN..=LLAMA_PARALLEL_MAX).contains(&parallel) {
        return Err(LlamaError {
            message: format!(
                "parallel must be between {LLAMA_PARALLEL_MIN} and {LLAMA_PARALLEL_MAX}"
            ),
        });
    }

    let cache_ram_mb = req.cache_ram_mb.unwrap_or(LLAMA_CACHE_RAM_MB_DEFAULT);
    if cache_ram_mb > LLAMA_CACHE_RAM_MB_MAX {
        return Err(LlamaError {
            message: format!("cacheRamMb must be no more than {LLAMA_CACHE_RAM_MB_MAX}"),
        });
    }

    let spec_draft_n_max = req
        .spec_draft_n_max
        .unwrap_or(NATIVE_LLAMA_DEFAULT_DRAFT_TOKENS);
    if !(LLAMA_SPEC_DRAFT_N_MAX_MIN..=LLAMA_SPEC_DRAFT_N_MAX_MAX).contains(&spec_draft_n_max) {
        return Err(LlamaError {
            message: format!(
                "specDraftNMax must be between {LLAMA_SPEC_DRAFT_N_MAX_MIN} and {LLAMA_SPEC_DRAFT_N_MAX_MAX}"
            ),
        });
    }

    Ok(LlamaLaunchArgs {
        ctx_size,
        n_gpu_layers: normalize_gpu_layers(req.n_gpu_layers.as_deref())?,
        parallel,
        continuous_batching: req.continuous_batching.unwrap_or(true),
        cache_ram_mb,
        cache_idle_slots: cache_ram_mb > 0 && req.cache_idle_slots.unwrap_or(true),
        cold_kv_cache: req.cold_kv_cache.unwrap_or(true),
        spec_draft_n_max,
    })
}

pub(crate) fn resolve_optional_draft_model_path(
    app_data_dir: &Path,
    req: &StartLlamaRequest,
) -> LlamaResult<Option<PathBuf>> {
    if let Some(model_id) = req
        .draft_model_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let draft = get_model_by_id(app_data_dir, model_id)?;
        return Ok(Some(resolve_model_path(app_data_dir, &draft.filename)?));
    }

    let assistant_id = format!("{}-assistant", req.model_id);
    match get_model_by_id(app_data_dir, &assistant_id) {
        Ok(draft) => Ok(Some(resolve_model_path(app_data_dir, &draft.filename)?)),
        Err(_) => Ok(None),
    }
}

fn normalize_gpu_layers(value: Option<&str>) -> LlamaResult<String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok("auto".to_string());
    };
    if value.eq_ignore_ascii_case("auto") {
        return Ok("auto".to_string());
    }
    if !value.chars().all(|char| char.is_ascii_digit()) {
        return Err(LlamaError {
            message: "nGpuLayers must be auto or a non-negative integer".to_string(),
        });
    }
    let layers = value.parse::<u32>().map_err(|_| LlamaError {
        message: "nGpuLayers must be auto or a non-negative integer".to_string(),
    })?;
    if layers > LLAMA_GPU_LAYERS_MAX {
        return Err(LlamaError {
            message: format!("nGpuLayers must be auto or no more than {LLAMA_GPU_LAYERS_MAX}"),
        });
    }
    Ok(layers.to_string())
}

fn validate_gguf_path(path: &Path) -> LlamaResult<()> {
    if path.extension().and_then(|value| value.to_str()) != Some("gguf") {
        return Err(LlamaError {
            message: "model file must use the .gguf extension".to_string(),
        });
    }
    if !path.is_file() {
        return Err(LlamaError {
            message: format!("model file not found: {}", path.to_string_lossy()),
        });
    }
    Ok(())
}

fn validate_llama_server_binary_path(path: &Path) -> LlamaResult<()> {
    if !path.is_file() {
        return Err(LlamaError {
            message: format!("llama-server binary not found: {}", path.to_string_lossy()),
        });
    }

    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let plain_binary = llama_server_binary_name();
    if filename != plain_binary && !filename.starts_with(&format!("{LLAMA_SERVER_BIN}-")) {
        return Err(LlamaError {
            message: "sidecar binary filename must be llama-server or llama-server-<target>"
                .to_string(),
        });
    }
    Ok(())
}

fn same_file(source: &Path, target: &Path) -> bool {
    match (fs::canonicalize(source), fs::canonicalize(target)) {
        (Ok(source), Ok(target)) => source == target,
        _ => false,
    }
}

fn make_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub(crate) fn resolve_model_path(app_data_dir: &Path, filename: &str) -> LlamaResult<PathBuf> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(LlamaError {
            message: "invalid model filename".to_string(),
        });
    }
    let base = models_dir(app_data_dir);
    let model = base.join(filename);
    let canonical_base = fs::canonicalize(&base)?;
    let canonical_model = fs::canonicalize(&model)?;
    if !canonical_model.starts_with(&canonical_base) {
        return Err(LlamaError {
            message: "model path escapes model directory".to_string(),
        });
    }
    Ok(canonical_model)
}

pub(crate) fn model_info_from_path(
    path: &Path,
    id: String,
    display_name: String,
) -> LlamaResult<ModelInfo> {
    let metadata = fs::metadata(path)?;
    Ok(ModelInfo {
        id,
        display_name,
        filename: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("model.gguf")
            .to_string(),
        size_bytes: metadata.len(),
        sha256: sha256_file(path)?,
    })
}

pub(crate) fn sanitize_model_id(value: &str) -> LlamaResult<String> {
    let id = value
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

    if id.is_empty() {
        return Err(LlamaError {
            message: "model id cannot be empty".to_string(),
        });
    }
    Ok(id)
}

fn display_name_from_id(id: &str) -> String {
    id.trim_end_matches(".gguf")
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn sha256_file(path: &Path) -> LlamaResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn find_free_loopback_port() -> LlamaResult<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

fn wait_for_llama_ready<F>(
    port: u16,
    log_path: &Path,
    timeout: Duration,
    mut child_status: F,
) -> LlamaResult<()>
where
    F: FnMut() -> LlamaResult<Option<String>>,
{
    let started = Instant::now();

    loop {
        if llama_http_endpoint_ready(port, "/health")
            || llama_http_endpoint_ready(port, "/v1/models")
        {
            return Ok(());
        }

        if let Some(status) = child_status()? {
            return Err(LlamaError {
                message: format!(
                    "llama-server exited before becoming ready on 127.0.0.1:{port} ({status}); see {}",
                    log_path.to_string_lossy()
                ),
            });
        }

        if started.elapsed() >= timeout {
            return Err(LlamaError {
                message: format!(
                    "llama-server did not become ready on 127.0.0.1:{port} within {}s; see {}",
                    timeout.as_secs(),
                    log_path.to_string_lossy()
                ),
            });
        }

        thread::sleep(LLAMA_READY_POLL_INTERVAL);
    }
}

fn llama_http_endpoint_ready(port: u16, path: &str) -> bool {
    match request_llama_http_status(port, path) {
        Ok(status) => (200..300).contains(&status),
        Err(_) => false,
    }
}

fn request_llama_http_status(port: u16, path: &str) -> io::Result<u16> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, LLAMA_READY_CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(LLAMA_READY_CONNECT_TIMEOUT))?;
    stream.set_write_timeout(Some(LLAMA_READY_CONNECT_TIMEOUT))?;

    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;

    let mut buffer = [0_u8; 256];
    let read = stream.read(&mut buffer)?;
    let response = String::from_utf8_lossy(&buffer[..read]);
    parse_http_status(&response).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "llama-server readiness response did not include an HTTP status",
        )
    })
}

fn parse_http_status(response: &str) -> Option<u16> {
    let mut parts = response.lines().next()?.split_whitespace();
    parts.next()?;
    parts.next()?.parse().ok()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in paths {
        if !result.contains(&path) {
            result.push(path);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "whorary-llama-test-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_model(path: &Path) {
        fs::write(path, b"not a real model, but enough for registry tests").unwrap();
    }

    fn spawn_http_responses(responses: Vec<&'static str>) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0_u8; 512];
                let _ = stream.read(&mut buffer);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (port, handle)
    }

    fn start_request(model_id: &str) -> StartLlamaRequest {
        StartLlamaRequest {
            model_id: model_id.to_string(),
            ctx_size: None,
            n_gpu_layers: None,
            parallel: None,
            continuous_batching: None,
            cache_ram_mb: None,
            cache_idle_slots: None,
            cold_kv_cache: None,
            draft_model_id: None,
            spec_draft_n_max: None,
        }
    }

    fn has_arg_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2)
            .any(|pair| pair[0] == flag && pair[1] == value)
    }

    #[test]
    fn imports_and_lists_gguf_models() {
        let app_dir = temp_dir("import-list");
        let source = app_dir.join("source-model.gguf");
        write_model(&source);

        let imported = import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: source.to_string_lossy().to_string(),
                id: Some("Gemma Test".to_string()),
                display_name: Some("Gemma Test Q4".to_string()),
            },
        )
        .unwrap();
        let models = list_models_in_dir(&app_dir).unwrap();

        assert_eq!(imported.id, "gemma-test");
        assert_eq!(imported.filename, "gemma-test.gguf");
        assert_eq!(imported.display_name, "Gemma Test Q4");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].sha256, imported.sha256);
    }

    #[test]
    fn rejects_non_gguf_imports() {
        let app_dir = temp_dir("reject-extension");
        let source = app_dir.join("model.bin");
        fs::write(&source, b"bad").unwrap();

        let error = import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: source.to_string_lossy().to_string(),
                id: None,
                display_name: None,
            },
        )
        .unwrap_err();

        assert_eq!(error.message, "model file must use the .gguf extension");
    }

    #[test]
    fn builds_loopback_offline_llama_args() {
        let mut req = start_request("gemma");
        req.ctx_size = Some(4096);
        req.n_gpu_layers = Some("0".to_string());
        req.parallel = Some(2);
        let args = build_llama_server_args(
            Path::new("/tmp/model.gguf"),
            32123,
            &req,
            Some(Path::new("/tmp/slots")),
            None,
        )
        .unwrap();

        assert!(args.windows(2).any(|pair| pair == ["--host", "127.0.0.1"]));
        assert!(args.windows(2).any(|pair| pair == ["--port", "32123"]));
        assert!(args.windows(2).any(|pair| pair == ["--ctx-size", "4096"]));
        assert!(args.windows(2).any(|pair| pair == ["--parallel", "2"]));
        assert!(args.contains(&"--cont-batching".to_string()));
        assert!(args.contains(&"--cache-prompt".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--cache-ram", "4096"]));
        assert!(args.contains(&"--cache-idle-slots".to_string()));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--slot-save-path", "/tmp/slots"]));
        assert!(args.contains(&"--offline".to_string()));
    }

    #[test]
    fn llama_args_emit_explicit_batch_cache_and_mtp_controls() {
        let mut req = start_request("gemma");
        req.continuous_batching = Some(false);
        req.cache_ram_mb = Some(0);
        req.cache_idle_slots = Some(true);
        req.cold_kv_cache = Some(false);
        req.spec_draft_n_max = Some(8);

        let args = build_llama_server_args(
            Path::new("/tmp/gemma.gguf"),
            32123,
            &req,
            Some(Path::new("/tmp/slots")),
            Some(Path::new("/tmp/gemma-assistant.gguf")),
        )
        .unwrap();

        assert!(args.contains(&"--no-cont-batching".to_string()));
        assert!(!args.contains(&"--cont-batching".to_string()));
        assert!(has_arg_pair(&args, "--cache-ram", "0"));
        assert!(args.contains(&"--no-cache-idle-slots".to_string()));
        assert!(!args.contains(&"--cache-idle-slots".to_string()));
        assert!(!args.contains(&"--slot-save-path".to_string()));
        assert!(has_arg_pair(&args, "--spec-draft-n-max", "8"));
    }

    #[test]
    fn builds_mtp_draft_args_when_draft_model_is_available() {
        let req = start_request("gemma");
        let args = build_llama_server_args(
            Path::new("/tmp/gemma.gguf"),
            32123,
            &req,
            Some(Path::new("/tmp/slots")),
            Some(Path::new("/tmp/gemma-assistant.gguf")),
        )
        .unwrap();

        assert!(args
            .windows(2)
            .any(|pair| pair == ["--model-draft", "/tmp/gemma-assistant.gguf"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--spec-type", "draft-mtp"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--spec-draft-n-max", "3"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--spec-draft-device", "none"]));
    }

    #[test]
    fn resolves_implicit_assistant_draft_model_when_present() {
        let app_dir = temp_dir("implicit-draft");
        let model_source = app_dir.join("gemma-source.gguf");
        let assistant_source = app_dir.join("assistant-source.gguf");
        write_model(&model_source);
        write_model(&assistant_source);
        import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: model_source.to_string_lossy().to_string(),
                id: Some("gemma".to_string()),
                display_name: None,
            },
        )
        .unwrap();
        import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: assistant_source.to_string_lossy().to_string(),
                id: Some("gemma-assistant".to_string()),
                display_name: None,
            },
        )
        .unwrap();

        let resolved = resolve_optional_draft_model_path(&app_dir, &start_request("gemma"))
            .unwrap()
            .unwrap();

        assert_eq!(
            resolved.file_name().and_then(|value| value.to_str()),
            Some("gemma-assistant.gguf")
        );
    }

    #[test]
    fn resolves_explicit_draft_model_and_rejects_missing_explicit_draft() {
        let app_dir = temp_dir("explicit-draft");
        let draft_source = app_dir.join("draft-source.gguf");
        write_model(&draft_source);
        import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: draft_source.to_string_lossy().to_string(),
                id: Some("tiny-draft".to_string()),
                display_name: None,
            },
        )
        .unwrap();

        let mut req = start_request("gemma");
        req.draft_model_id = Some("tiny-draft".to_string());
        let resolved = resolve_optional_draft_model_path(&app_dir, &req)
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved.file_name().and_then(|value| value.to_str()),
            Some("tiny-draft.gguf")
        );

        req.draft_model_id = Some("missing-draft".to_string());
        let error = resolve_optional_draft_model_path(&app_dir, &req).unwrap_err();
        assert!(error.message.contains("model not found: missing-draft"));
    }

    #[test]
    fn ignores_missing_implicit_assistant_draft_model() {
        let app_dir = temp_dir("missing-implicit-draft");

        let resolved =
            resolve_optional_draft_model_path(&app_dir, &start_request("gemma")).unwrap();

        assert!(resolved.is_none());
    }

    #[test]
    fn llama_args_default_to_bounded_safe_values() {
        let mut req = start_request("gemma");
        req.n_gpu_layers = Some("".to_string());
        let args = build_llama_server_args(
            Path::new("/tmp/model.gguf"),
            32123,
            &req,
            Some(Path::new("/tmp/slots")),
            None,
        )
        .unwrap();

        assert!(args.windows(2).any(|pair| pair == ["--ctx-size", "8192"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--n-gpu-layers", "auto"]));
        assert!(args.windows(2).any(|pair| pair == ["--parallel", "4"]));
    }

    #[test]
    fn rejects_unbounded_llama_launch_args() {
        let mut req = start_request("gemma");
        req.ctx_size = Some(256);
        assert!(
            build_llama_server_args(Path::new("/tmp/model.gguf"), 32123, &req, None, None)
                .unwrap_err()
                .message
                .contains("ctxSize")
        );

        req.ctx_size = Some(8192);
        req.parallel = Some(0);
        assert!(
            build_llama_server_args(Path::new("/tmp/model.gguf"), 32123, &req, None, None)
                .unwrap_err()
                .message
                .contains("parallel")
        );

        req.parallel = Some(1);
        req.n_gpu_layers = Some("--model".to_string());
        assert!(
            build_llama_server_args(Path::new("/tmp/model.gguf"), 32123, &req, None, None)
                .unwrap_err()
                .message
                .contains("nGpuLayers")
        );

        req.n_gpu_layers = None;
        req.cache_ram_mb = Some(LLAMA_CACHE_RAM_MB_MAX + 1);
        assert!(
            build_llama_server_args(Path::new("/tmp/model.gguf"), 32123, &req, None, None)
                .unwrap_err()
                .message
                .contains("cacheRamMb")
        );

        req.cache_ram_mb = None;
        req.spec_draft_n_max = Some(LLAMA_SPEC_DRAFT_N_MAX_MAX + 1);
        assert!(
            build_llama_server_args(Path::new("/tmp/model.gguf"), 32123, &req, None, None)
                .unwrap_err()
                .message
                .contains("specDraftNMax")
        );
    }

    #[test]
    fn start_reports_missing_sidecar_without_spawning_arbitrary_commands() {
        let app_dir = temp_dir("missing-sidecar");
        let source = app_dir.join("model.gguf");
        write_model(&source);
        import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: source.to_string_lossy().to_string(),
                id: Some("model".to_string()),
                display_name: None,
            },
        )
        .unwrap();

        let state = LlamaState::default();
        let error = start_llama_in_dir(&app_dir, &state, start_request("model")).unwrap_err();

        assert!(error
            .message
            .contains("llama-server sidecar is not installed"));
        assert!(!get_model_status(&state).unwrap().running);
    }

    #[cfg(unix)]
    #[test]
    fn start_creates_log_and_kv_cache_directories_before_sidecar_readiness() {
        let app_dir = temp_dir("fake-sidecar-startup");
        let source = app_dir.join("model.gguf");
        write_model(&source);
        import_model_to_dir(
            &app_dir,
            ImportModelRequest {
                path: source.to_string_lossy().to_string(),
                id: Some("model".to_string()),
                display_name: None,
            },
        )
        .unwrap();

        let binary_dir = app_dir.join(BINARIES_DIR);
        fs::create_dir_all(&binary_dir).unwrap();
        let binary = binary_dir.join(llama_server_target_binary_name());
        fs::write(&binary, "#!/bin/sh\nexit 7\n").unwrap();
        make_executable(&binary).unwrap();

        let state = LlamaState::default();
        let error = start_llama_in_dir(&app_dir, &state, start_request("model")).unwrap_err();

        assert!(error.message.contains("exited before becoming ready"));
        assert!(app_dir.join(LOGS_DIR).join("llama-server.log").is_file());
        assert!(app_dir
            .join(KV_CACHE_DIR)
            .join("llama-server-slots")
            .is_dir());
        assert!(!get_model_status(&state).unwrap().running);
    }

    #[test]
    fn readiness_wait_accepts_health_endpoint() {
        let (port, server) =
            spawn_http_responses(vec!["HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"]);

        wait_for_llama_ready(
            port,
            Path::new("llama-server.log"),
            Duration::from_secs(1),
            || Ok(None),
        )
        .unwrap();
        server.join().unwrap();
    }

    #[test]
    fn readiness_wait_falls_back_to_models_endpoint() {
        let (port, server) = spawn_http_responses(vec![
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}",
        ]);

        wait_for_llama_ready(
            port,
            Path::new("llama-server.log"),
            Duration::from_secs(1),
            || Ok(None),
        )
        .unwrap();
        server.join().unwrap();
    }

    #[test]
    fn readiness_wait_reports_timeout_for_closed_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let error = wait_for_llama_ready(
            port,
            Path::new("llama-server.log"),
            Duration::from_millis(60),
            || Ok(None),
        )
        .unwrap_err();

        assert!(error.message.contains("did not become ready"));
        assert!(error.message.contains("127.0.0.1"));
    }

    #[test]
    fn readiness_wait_reports_child_exit() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let error = wait_for_llama_ready(
            port,
            Path::new("llama-server.log"),
            Duration::from_secs(1),
            || Ok(Some("exit status: 1".to_string())),
        )
        .unwrap_err();

        assert!(error.message.contains("exited before becoming ready"));
        assert!(error.message.contains("exit status: 1"));
    }

    #[test]
    fn stop_without_running_process_is_false() {
        let state = LlamaState::default();
        assert!(!stop_llama_process(&state).unwrap());
    }

    #[test]
    fn active_connection_is_empty_when_model_is_not_running() {
        let state = LlamaState::default();
        assert_eq!(active_llama_connection(&state).unwrap(), None);
    }

    #[test]
    fn status_clears_exited_child_process() {
        let state = LlamaState::default();
        let app_dir = temp_dir("exited-child-status");
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        {
            let mut running = state.running.lock().unwrap();
            *running = Some(RunningLlama {
                model_id: "model".to_string(),
                port: 32123,
                log_path: app_dir.join("llama-server.log"),
                child,
            });
        }

        let mut status = get_model_status(&state).unwrap();
        for _ in 0..100 {
            if !status.running {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            status = get_model_status(&state).unwrap();
        }

        assert!(!status.running);
        assert_eq!(active_llama_connection(&state).unwrap(), None);
    }

    #[test]
    fn sidecar_resolution_prefers_bundled_resource_binary() {
        let app_dir = temp_dir("sidecar-prefer-app");
        let resource_dir = temp_dir("sidecar-prefer-resource");
        let app_sidecar = llama_server_path(&app_dir);
        let bundled_sidecar = resource_dir
            .join(BINARIES_DIR)
            .join(llama_server_target_binary_name());
        fs::create_dir_all(app_sidecar.parent().unwrap()).unwrap();
        fs::create_dir_all(bundled_sidecar.parent().unwrap()).unwrap();
        fs::write(&app_sidecar, b"app-data sidecar").unwrap();
        fs::write(&bundled_sidecar, b"bundled sidecar").unwrap();

        let resolved = resolve_llama_server_path(&app_dir, Some(&resource_dir)).unwrap();
        let status = get_llama_sidecar_status(&app_dir, Some(&resource_dir));

        assert_eq!(resolved, bundled_sidecar);
        assert!(status.installed);
        assert_eq!(status.path.unwrap(), resolved.to_string_lossy());
    }

    #[test]
    fn sidecar_resolution_falls_back_to_app_data_binary() {
        let app_dir = temp_dir("sidecar-app-data");
        let sidecar = llama_server_path(&app_dir);
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(&sidecar, b"app-data sidecar").unwrap();

        assert_eq!(resolve_llama_server_path(&app_dir, None).unwrap(), sidecar);
    }

    #[test]
    fn installs_llama_sidecar_into_app_data_binaries() {
        let app_dir = temp_dir("install-sidecar");
        let source = app_dir.join(llama_server_binary_name());
        fs::write(&source, b"native sidecar").unwrap();

        let status = install_llama_sidecar_to_dir(
            &app_dir,
            ImportLlamaSidecarRequest {
                path: source.to_string_lossy().to_string(),
            },
        )
        .unwrap();
        let target = app_dir
            .join(BINARIES_DIR)
            .join(llama_server_target_binary_name());

        assert!(status.installed);
        assert_eq!(status.path.unwrap(), target.to_string_lossy());
        assert_eq!(fs::read(&target).unwrap(), b"native sidecar");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o111,
                0
            );
        }
    }

    #[test]
    fn rejects_unrelated_sidecar_binary_names() {
        let app_dir = temp_dir("reject-sidecar");
        let source = app_dir.join("not-llama");
        fs::write(&source, b"not a sidecar").unwrap();

        let error = install_llama_sidecar_to_dir(
            &app_dir,
            ImportLlamaSidecarRequest {
                path: source.to_string_lossy().to_string(),
            },
        )
        .unwrap_err();

        assert!(error.message.contains("sidecar binary filename"));
    }

    #[test]
    fn missing_sidecar_status_lists_checked_paths() {
        let app_dir = temp_dir("sidecar-missing");
        let resource_dir = temp_dir("sidecar-missing-resource");
        let status = get_llama_sidecar_status(&app_dir, Some(&resource_dir));
        let error = resolve_llama_server_path(&app_dir, Some(&resource_dir)).unwrap_err();

        assert!(!status.installed);
        assert!(status.path.is_none());
        assert!(status.checked_paths.len() >= 2);
        assert!(error.message.contains("checked"));
    }
}
