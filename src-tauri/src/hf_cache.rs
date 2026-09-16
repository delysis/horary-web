//! Shared Hub file cache. Only bundled, immutable artifacts can be acquired.
//! No Python, external downloader, app-local weight copy, or mutation of refs/main.
#![forbid(unsafe_code)]

use crate::llama::{LlamaError, ModelInfo};
use crate::model_manifest::{bundled_model_manifest, ModelManifestEntry, ModelManifestError};
use fs2::FileExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub const RECOMMENDED_ID: &str = "gemma-4-12b-qat";
const REGISTRATION: &str = "huggingface-cache.json";
type Result<T> = std::result::Result<T, ModelManifestError>;

fn error(message: impl Into<String>) -> ModelManifestError {
    ModelManifestError {
        message: message.into(),
    }
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionProgress {
    pub active: bool,
    pub phase: String,
    pub model_name: String,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub message: String,
    pub ready: bool,
    pub cache_path: String,
}

#[derive(Default)]
pub struct Acquisition {
    pub busy: AtomicBool,
    pub cancelled: AtomicBool,
    progress: Mutex<AcquisitionProgress>,
}
#[derive(Default)]
pub struct AcquisitionState(pub Arc<Acquisition>);

impl Acquisition {
    pub fn begin(&self) -> Result<()> {
        // Serialize starting and cancelling so a pause cannot be lost during startup.
        let mut p = self
            .progress
            .lock()
            .map_err(|_| error("Model setup state unavailable"))?;
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(error("Model setup is already running"));
        }
        self.cancelled.store(false, Ordering::SeqCst);
        *p = AcquisitionProgress {
            active: true,
            phase: "checking".into(),
            ..Default::default()
        };
        Ok(())
    }

    pub fn cancel(&self) {
        if let Ok(_guard) = self.progress.lock() {
            self.cancelled.store(true, Ordering::SeqCst);
        }
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err(error(
                "Setup paused. Resume to keep the bytes already downloaded.",
            ))
        } else {
            Ok(())
        }
    }

    fn report(&self, phase: &str, entry: &ModelManifestEntry, done: u64) {
        if let Ok(mut p) = self.progress.lock() {
            p.phase = phase.into();
            p.model_name.clone_from(&entry.display_name);
            p.completed_bytes = done;
            p.total_bytes = entry.size_bytes.unwrap_or(0);
        }
    }

    pub fn status(&self, app_data: &Path) -> Result<AcquisitionProgress> {
        let mut p = self
            .progress
            .lock()
            .map_err(|_| error("Model setup state unavailable"))?
            .clone();
        p.active = self.busy.load(Ordering::SeqCst);
        p.ready = registered_models(app_data)?.len() == bundled_model_manifest()?.models.len();
        p.cache_path = cache_root()?.to_string_lossy().into_owned();
        Ok(p)
    }

    pub fn run(&self, app_data: &Path) -> Result<()> {
        let result = (|| {
            let root = cache_root()?;
            for entry in bundled_model_manifest()?.models {
                acquire(&root, &entry, self)?;
            }
            self.check_cancelled()?;
            fs::create_dir_all(app_data)?;
            let mut temporary = tempfile::NamedTempFile::new_in(app_data)?;
            temporary.write_all(&serde_json::to_vec(&fs::canonicalize(root)?)?)?;
            temporary.as_file().sync_all()?;
            // A tiny registration records the cache location; weights stay in the Hub.
            temporary
                .persist(app_data.join(REGISTRATION))
                .map_err(|e| e.error)?;
            Ok(())
        })();
        if let Ok(mut p) = self.progress.lock() {
            p.phase = if result.is_ok() {
                "ready"
            } else if self.cancelled.load(Ordering::SeqCst) {
                "paused"
            } else {
                "error"
            }
            .into();
            p.message = result
                .as_ref()
                .err()
                .map(|e: &ModelManifestError| e.message.clone())
                .unwrap_or_default();
            p.active = false;
            self.busy.store(false, Ordering::SeqCst);
        }
        result
    }
}

/// Match the Hub's environment-variable precedence on every supported OS.
pub fn cache_root() -> Result<PathBuf> {
    cache_root_with(
        |name| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        },
        dirs::home_dir(),
    )
}
fn cache_root_with(
    env: impl Fn(&str) -> Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = env("HF_HUB_CACHE").or_else(|| env("HUGGINGFACE_HUB_CACHE")) {
        return Ok(path);
    }
    if let Some(path) = env("HF_HOME") {
        return Ok(path.join("hub"));
    }
    let base = env("XDG_CACHE_HOME")
        .or_else(|| home.map(|p| p.join(".cache")))
        .ok_or_else(|| {
            error("Cannot find your home directory. Set HF_HUB_CACHE to a writable directory.")
        })?;
    Ok(base.join("huggingface/hub"))
}

struct Paths {
    blob: PathBuf,
    partial: PathBuf,
    snapshot: PathBuf,
    lock: PathBuf,
}
fn paths(root: &Path, entry: &ModelManifestEntry) -> Result<Paths> {
    let source = entry
        .source
        .as_ref()
        .ok_or_else(|| error("Missing Hub source"))?;
    let repo = source
        .repo
        .as_deref()
        .ok_or_else(|| error("Missing Hub repository"))?;
    let revision = source
        .revision
        .as_deref()
        .ok_or_else(|| error("Missing pinned revision"))?;
    let filename = source
        .filename
        .as_deref()
        .ok_or_else(|| error("Missing Hub filename"))?;
    let sha = entry
        .sha256
        .as_deref()
        .ok_or_else(|| error("Missing checksum"))?;
    if revision.len() != 40
        || !revision.bytes().all(|c| c.is_ascii_hexdigit())
        || sha.len() != 64
        || !sha.bytes().all(|c| c.is_ascii_hexdigit())
        || repo.split('/').count() != 2
        || repo.split('/').any(|s| !safe_component(s))
        || !safe_component(filename)
    {
        return Err(error("Invalid pinned Hub artifact"));
    }
    let repo_dir = format!("models--{}", repo.replace('/', "--"));
    let base = root.join(&repo_dir);
    Ok(Paths {
        blob: base.join("blobs").join(sha),
        partial: base.join("blobs").join(format!("{sha}.incomplete")),
        snapshot: base.join("snapshots").join(revision).join(filename),
        lock: root
            .join(".locks")
            .join(repo_dir)
            .join(format!("{sha}.lock")),
    })
}
fn safe_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

fn verify(path: &Path, entry: &ModelManifestEntry, state: &Acquisition) -> Result<()> {
    let mut file = File::open(path)?;
    if Some(file.metadata()?.len()) != entry.size_bytes {
        return Err(error("Cached model has the wrong size"));
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut read = 0;
    loop {
        state.check_cancelled()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        read += count as u64;
        state.report("verifying", entry, read);
    }
    if Some(format!("{:x}", hash.finalize())).as_ref() != entry.sha256.as_ref() {
        return Err(error("Model checksum mismatch. The shared cache was not replaced; remove the corrupt artifact before retrying."));
    }
    Ok(())
}

fn acquire(root: &Path, entry: &ModelManifestEntry, state: &Acquisition) -> Result<PathBuf> {
    let p = paths(root, entry)?;
    fs::create_dir_all(p.lock.parent().unwrap())?;
    fs::create_dir_all(p.blob.parent().unwrap())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&p.lock)?;
    state.report("waiting", entry, 0);
    loop {
        state.check_cancelled()?;
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e.into()),
        }
    }
    // The file handle holds the same per-blob lock used by other Hub clients.
    // A cache hit is entirely offline, including the integrity check.
    if p.blob.is_file() {
        verify(&p.blob, entry, state)?;
    } else if p.snapshot.is_file() {
        // Some clients cache a regular snapshot file (not a symlink), notably Windows.
        verify(&p.snapshot, entry, state)?;
        fs::hard_link(&p.snapshot, &p.blob)?;
    } else {
        download(&p.partial, entry, state)?;
        if let Err(e) = verify(&p.partial, entry, state) {
            if !state.cancelled.load(Ordering::SeqCst) {
                fs::remove_file(&p.partial)?;
            }
            return Err(e);
        }
        state.check_cancelled()?;
        fs::rename(&p.partial, &p.blob)?;
    }
    fs::create_dir_all(p.snapshot.parent().unwrap())?;
    if fs::symlink_metadata(&p.snapshot).is_err() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            PathBuf::from("../../blobs").join(entry.sha256.as_ref().unwrap()),
            &p.snapshot,
        )?;
        #[cfg(not(unix))]
        fs::hard_link(&p.blob, &p.snapshot)?;
    }
    Ok(p.blob)
}

fn download(partial: &Path, entry: &ModelManifestEntry, state: &Acquisition) -> Result<()> {
    tauri::async_runtime::block_on(download_async(partial, entry, state))
}

async fn network<T>(
    future: impl std::future::Future<Output = std::result::Result<T, reqwest::Error>>,
    state: &Acquisition,
) -> Result<T> {
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => return result.map_err(Into::into),
            _ = tokio::time::sleep(Duration::from_millis(100)) => state.check_cancelled()?,
        }
    }
}

async fn download_async(
    partial: &Path,
    entry: &ModelManifestEntry,
    state: &Acquisition,
) -> Result<()> {
    let expected = entry
        .size_bytes
        .ok_or_else(|| error("Missing artifact size"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(partial)?;
    let mut done = file.metadata()?.len();
    file.seek(SeekFrom::End(0))?;
    if done > expected {
        return Err(error("Partial model exceeds its pinned size; remove the corrupt .incomplete file before retrying."));
    }
    if done == expected {
        return Ok(());
    }
    state.check_cancelled()?;
    state.report("downloading", entry, done);
    let url = entry
        .source
        .as_ref()
        .and_then(|s| s.url.as_deref())
        .ok_or_else(|| error("Missing artifact URL"))?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60 * 12))
        .build()?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if done > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={done}-"));
    }
    let mut response = network(request.send(), state).await?;
    if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|h| h.to_str().ok());
        let expected_range = format!("bytes {done}-{}/{expected}", expected - 1);
        if range != Some(expected_range.as_str()) {
            return Err(error(
                "Server returned an unexpected byte range; partial download preserved.",
            ));
        }
    } else if response.status() == reqwest::StatusCode::OK {
        // A server may ignore Range. Restart only when it sent a full response.
        // Windows append-only handles cannot truncate; use read/write access
        // and explicit offsets for both fresh downloads and resumptions.
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        done = 0;
    } else {
        return Err(error(format!(
            "Hugging Face returned HTTP {}. Resume setup when the connection is available.",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length != expected - done)
    {
        return Err(error(
            "Server returned the wrong artifact size; partial download preserved.",
        ));
    }
    loop {
        state.check_cancelled()?;
        let Some(chunk) = network(response.chunk(), state).await? else {
            break;
        };
        let count = chunk.len();
        if done + count as u64 > expected {
            return Err(error("Download exceeded the pinned artifact size"));
        }
        file.write_all(&chunk)?;
        done += count as u64;
        state.report("downloading", entry, done);
    }
    file.sync_all()?;
    if done != expected {
        return Err(error(
            "Download interrupted. Resume setup to keep the bytes already downloaded.",
        ));
    }
    Ok(())
}

fn registration(app_data: &Path) -> Result<Option<PathBuf>> {
    match fs::read(app_data.join(REGISTRATION)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn registered_path(app_data: &Path, filename: &str) -> Result<Option<PathBuf>> {
    let Some(root) = registration(app_data)? else {
        return Ok(None);
    };
    for entry in bundled_model_manifest()?.models {
        if filename == format!("{}.gguf", entry.id) {
            return Ok(Some(paths(&root, &entry)?.blob));
        }
    }
    Ok(None)
}
pub fn registered_models(app_data: &Path) -> Result<Vec<ModelInfo>> {
    let Some(root) = registration(app_data)? else {
        return Ok(Vec::new());
    };
    let mut models = Vec::new();
    for entry in bundled_model_manifest()?.models {
        let path = paths(&root, &entry)?.blob;
        if fs::metadata(path).ok().map(|m| m.len()) == entry.size_bytes {
            models.push(ModelInfo {
                filename: format!("{}.gguf", entry.id),
                id: entry.id,
                display_name: entry.display_name,
                size_bytes: entry.size_bytes.unwrap(),
                sha256: entry.sha256.unwrap(),
            });
        }
    }
    Ok(models)
}
pub fn verify_registered(app_data: &Path, id: &str) -> Result<Option<ModelInfo>> {
    let Some(root) = registration(app_data)? else {
        return Ok(None);
    };
    for entry in bundled_model_manifest()?.models {
        if entry.id == id {
            verify(&paths(&root, &entry)?.blob, &entry, &Acquisition::default())?;
            return Ok(registered_models(app_data)?
                .into_iter()
                .find(|m| m.id == id));
        }
    }
    Ok(None)
}
impl From<ModelManifestError> for LlamaError {
    fn from(value: ModelManifestError) -> Self {
        Self {
            message: value.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicU64;
    use std::thread;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "horary-hf-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn entry(bytes: &[u8], url: String) -> ModelManifestEntry {
        let mut entry = bundled_model_manifest().unwrap().models.remove(0);
        entry.size_bytes = Some(bytes.len() as u64);
        entry.sha256 = Some(format!("{:x}", Sha256::digest(bytes)));
        entry.source.as_mut().unwrap().url = Some(url);
        entry
    }
    // A real HTTP server with explicit headers exercises reqwest, Range and disk publication.
    fn server(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            stream.write_all(&response).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }
    fn response(bytes: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        response.extend(bytes);
        response
    }

    #[test]
    fn cache_environment_precedence_matches_hub() {
        let variables = [
            ("HF_HUB_CACHE", "/hub"),
            ("HF_HOME", "/hf"),
            ("XDG_CACHE_HOME", "/xdg"),
        ];
        for (skip, expected) in [
            (0, "/hub"),
            (1, "/hf/hub"),
            (2, "/xdg/huggingface/hub"),
            (3, "/home/test/.cache/huggingface/hub"),
        ] {
            let result = cache_root_with(
                |key| {
                    variables
                        .iter()
                        .skip(skip)
                        .find(|(k, _)| *k == key)
                        .map(|(_, v)| PathBuf::from(v))
                },
                Some(PathBuf::from("/home/test")),
            )
            .unwrap();
            assert_eq!(result, PathBuf::from(expected));
        }
    }

    #[test]
    fn fresh_install_publishes_shared_blob_and_reuses_it_offline() {
        let dir = Temp::new();
        let bytes = b"GGUF test artifact from a machine with an empty cache";
        let (url, server) = server(response(bytes));
        let entry = entry(bytes, url);
        let state = Acquisition::default();
        let blob = acquire(&dir.0, &entry, &state).unwrap();
        assert!(server.join().unwrap().starts_with("GET /model.gguf"));
        let paths = paths(&dir.0, &entry).unwrap();
        assert_eq!(fs::read(&blob).unwrap(), bytes);
        assert_eq!(fs::read(&paths.snapshot).unwrap(), bytes);
        assert!(!paths.partial.exists());
        assert!(!dir.0.join("models").exists());
        #[cfg(unix)]
        assert_eq!(
            fs::canonicalize(&paths.snapshot).unwrap(),
            fs::canonicalize(&blob).unwrap()
        );
        // No server remains: a network request here would fail.
        assert_eq!(acquire(&dir.0, &entry, &state).unwrap(), blob);
    }

    #[test]
    fn interrupted_download_resumes_exact_range_and_rejects_wrong_range() {
        let dir = Temp::new();
        let bytes = b"GGUF0123456789012345678901234567890123456789";
        let mut broken = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        broken.extend(&bytes[..10]);
        let (url, first) = server(broken);
        let mut entry = entry(bytes, url);
        let state = Acquisition::default();
        assert!(acquire(&dir.0, &entry, &state).is_err());
        first.join().unwrap();
        let paths = paths(&dir.0, &entry).unwrap();
        assert_eq!(fs::read(&paths.partial).unwrap(), &bytes[..10]);
        assert!(!paths.blob.exists());
        let (url, wrong) = server(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-1/2\r\nContent-Length: 2\r\n\r\nno".to_vec());
        entry.source.as_mut().unwrap().url = Some(url);
        assert!(acquire(&dir.0, &entry, &state)
            .unwrap_err()
            .message
            .contains("byte range"));
        wrong.join().unwrap();
        assert_eq!(fs::read(&paths.partial).unwrap(), &bytes[..10]);
        let mut resumed = format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 10-{}/{}\r\nContent-Length: {}\r\n\r\n", bytes.len()-1, bytes.len(), bytes.len()-10).into_bytes();
        resumed.extend(&bytes[10..]);
        let (url, second) = server(resumed);
        entry.source.as_mut().unwrap().url = Some(url);
        acquire(&dir.0, &entry, &state).unwrap();
        assert!(second
            .join()
            .unwrap()
            .to_lowercase()
            .contains("range: bytes=10-"));
        assert_eq!(fs::read(&paths.blob).unwrap(), bytes);
    }

    #[test]
    fn corrupted_payload_is_never_published() {
        let dir = Temp::new();
        let (url, server) = server(response(b"wrong"));
        let entry = entry(b"right", url);
        let paths = paths(&dir.0, &entry).unwrap();
        assert!(acquire(&dir.0, &entry, &Acquisition::default())
            .unwrap_err()
            .message
            .contains("checksum"));
        server.join().unwrap();
        assert!(!paths.blob.exists());
        assert!(!paths.snapshot.exists());
        assert!(!paths.partial.exists());
    }

    #[test]
    fn ignored_range_restarts_without_appending_a_second_full_file() {
        let dir = Temp::new();
        let bytes = b"the complete artifact";
        let (url, server) = server(response(bytes));
        let entry = entry(bytes, url);
        let paths = paths(&dir.0, &entry).unwrap();
        fs::create_dir_all(paths.partial.parent().unwrap()).unwrap();
        fs::write(&paths.partial, &bytes[..4]).unwrap();
        acquire(&dir.0, &entry, &Acquisition::default()).unwrap();
        assert!(server
            .join()
            .unwrap()
            .to_lowercase()
            .contains("range: bytes=4-"));
        assert_eq!(fs::read(paths.blob).unwrap(), bytes);
    }

    #[test]
    fn pause_interrupts_a_stalled_network_request() {
        let dir = Temp::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
        let (connected, received) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            connected.send(()).unwrap();
            let _ = held.recv_timeout(Duration::from_secs(5));
        });
        let state = Arc::new(Acquisition::default());
        let worker_state = state.clone();
        let root = dir.0.clone();
        let (finished, completion) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            finished
                .send(acquire(&root, &entry(b"data", url), &worker_state))
                .unwrap();
        });
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        state.cancel();
        assert!(completion
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap_err()
            .message
            .contains("paused"));
        release.send(()).unwrap();
        worker.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn regular_snapshot_is_reused_without_copying_or_network() {
        let dir = Temp::new();
        let entry = entry(b"existing", "http://127.0.0.1:1/unreachable".into());
        let paths = paths(&dir.0, &entry).unwrap();
        fs::create_dir_all(paths.snapshot.parent().unwrap()).unwrap();
        fs::write(&paths.snapshot, b"existing").unwrap();
        acquire(&dir.0, &entry, &Acquisition::default()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(paths.blob).unwrap().ino(),
                fs::metadata(paths.snapshot).unwrap().ino()
            );
        }
    }

    #[test]
    fn pause_releases_waiting_lock_and_preserves_partial() {
        let dir = Temp::new();
        let entry = entry(b"something", "http://127.0.0.1:1/unreachable".into());
        let paths = paths(&dir.0, &entry).unwrap();
        fs::create_dir_all(paths.lock.parent().unwrap()).unwrap();
        fs::create_dir_all(paths.partial.parent().unwrap()).unwrap();
        fs::write(&paths.partial, b"some").unwrap();
        let lock = File::create(&paths.lock).unwrap();
        lock.lock_exclusive().unwrap();
        let state = Arc::new(Acquisition::default());
        state.begin().unwrap();
        assert!(state.begin().is_err());
        let worker_state = state.clone();
        let root = dir.0.clone();
        let worker = thread::spawn(move || acquire(&root, &entry, &worker_state));
        state.cancel();
        assert!(worker
            .join()
            .unwrap()
            .unwrap_err()
            .message
            .contains("paused"));
        assert_eq!(fs::read(paths.partial).unwrap(), b"some");
    }
}
