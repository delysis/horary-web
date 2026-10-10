//! Local registrations refer to shared cache files; they never contain weight copies.
#![forbid(unsafe_code)]
use crate::llama::{sha256_file, LlamaError, LlamaResult, ModelInfo};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
const REGISTRY: &str = "imported-models.json";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    root: PathBuf,
    path: PathBuf,
    info: ModelInfo,
}
fn error(message: impl Into<String>) -> LlamaError {
    LlamaError {
        message: message.into(),
    }
}
fn read(app: &Path) -> LlamaResult<Vec<Registration>> {
    match fs::read(app.join(REGISTRY)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| error(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}
fn valid_path(registration: &Registration) -> LlamaResult<PathBuf> {
    let root = fs::canonicalize(&registration.root)?;
    let path = fs::canonicalize(&registration.path)?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(error("Imported model escaped its shared cache"));
    }
    Ok(path)
}
pub fn list(app: &Path) -> LlamaResult<Vec<ModelInfo>> {
    read(app)?
        .into_iter()
        .filter(|r| r.path.exists())
        .map(|r| {
            let path = valid_path(&r)?;
            if fs::metadata(path)?.len() != r.info.size_bytes {
                return Err(error("Imported model size changed"));
            }
            Ok(r.info)
        })
        .collect()
}
pub fn path(app: &Path, filename: &str) -> LlamaResult<Option<PathBuf>> {
    read(app)?
        .into_iter()
        .find(|r| r.info.filename == filename)
        .map(|r| valid_path(&r))
        .transpose()
}
pub fn get(app: &Path, id: &str) -> LlamaResult<Option<ModelInfo>> {
    let Some(r) = read(app)?.into_iter().find(|r| r.info.id == id) else {
        return Ok(None);
    };
    let path = valid_path(&r)?;
    if fs::metadata(&path)?.len() != r.info.size_bytes || sha256_file(&path)? != r.info.sha256 {
        return Err(error("Imported model failed checksum verification"));
    }
    Ok(Some(r.info))
}
fn lock(path: &Path) -> LlamaResult<File> {
    if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err(error("Model registry lock must be a regular file"));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(file)
}
fn matching_blob(root: &Path, sha: &str, size: u64) -> LlamaResult<Option<PathBuf>> {
    for entry in fs::read_dir(root)?.take(20_000) {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || !entry.file_name().to_string_lossy().starts_with("models--")
        {
            continue;
        }
        let blob = entry.path().join("blobs").join(sha);
        if !blob.is_file() {
            continue;
        }
        let canonical = blob.canonicalize()?;
        if canonical.starts_with(root)
            && fs::metadata(&canonical)?.len() == size
            && sha256_file(&canonical)? == sha
        {
            return Ok(Some(canonical));
        }
    }
    Ok(None)
}

pub fn import(
    app: &Path,
    root: &Path,
    source: &Path,
    id: String,
    display_name: String,
) -> LlamaResult<ModelInfo> {
    fs::create_dir_all(root)?;
    let root = fs::canonicalize(root)?;
    let source = fs::canonicalize(source)?;
    let size = fs::metadata(&source)?.len();
    let sha = sha256_file(&source)?;
    let path = if source.starts_with(&root) {
        source.clone()
    } else if let Some(existing) = matching_blob(&root, &sha, size)? {
        existing
    } else {
        // Local imports have no asserted Hub origin. Deduplicate them by bytes.
        let directory = root.join("local/imports").join(&sha);
        fs::create_dir_all(&directory)?;
        if !fs::canonicalize(&directory)?.starts_with(&root) {
            return Err(error("Local model cache escapes its root"));
        }
        let _blob_lock = lock(&directory.join("import.lock"))?;
        let target = directory.join("model.gguf");
        if target.exists() {
            if fs::symlink_metadata(&target)?.file_type().is_symlink()
                || fs::metadata(&target)?.len() != size
                || sha256_file(&target)? != sha
            {
                return Err(error("Existing shared model failed checksum verification"));
            }
        } else {
            let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
            let mut input = File::open(&source)?;
            std::io::copy(&mut input, staged.as_file_mut())?;
            staged.as_file().sync_all()?;
            if staged.as_file().metadata()?.len() != size || sha256_file(staged.path())? != sha {
                return Err(error("Import source changed while being copied"));
            }
            staged
                .persist_noclobber(&target)
                .map_err(|e| error(e.to_string()))?;
        }
        target
    };
    fs::create_dir_all(app)?;
    let _registry_lock = lock(&app.join("imported-models.lock"))?;
    let mut registrations = read(app)?;
    if registrations.len() >= 1024 && !registrations.iter().any(|r| r.info.id == id) {
        return Err(error("Imported model registration limit reached"));
    }
    let info = ModelInfo {
        filename: format!("{id}.gguf"),
        id,
        display_name,
        size_bytes: size,
        sha256: sha,
    };
    registrations.retain(|r| r.info.id != info.id);
    registrations.push(Registration {
        root,
        path,
        info: info.clone(),
    });
    let mut staged = tempfile::NamedTempFile::new_in(app)?;
    staged
        .write_all(&serde_json::to_vec_pretty(&registrations).map_err(|e| error(e.to_string()))?)?;
    staged.as_file().sync_all()?;
    staged
        .persist(app.join(REGISTRY))
        .map_err(|e| error(e.to_string()))?;
    Ok(info)
}

/// Bounded metadata discovery. Hashing still occurs at explicit model admission.
pub fn discovered(root: &Path) -> LlamaResult<Vec<(ModelInfo, PathBuf)>> {
    use sha2::{Digest, Sha256};
    let Ok(root) = fs::canonicalize(root) else {
        return Ok(Vec::new());
    };
    let mut pending = vec![(root.clone(), 0)];
    let mut count = 0;
    let mut found = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    while let Some((directory, depth)) = pending.pop() {
        if depth > 12 {
            continue;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        let mut entries = entries
            .take(20_000usize.saturating_sub(count))
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            count += 1;
            if count > 20_000 {
                return Ok(found);
            }
            let kind = entry.file_type()?;
            let selected = entry.path();
            if kind.is_dir() {
                pending.push((selected, depth + 1));
                continue;
            }
            if selected.extension().and_then(|s| s.to_str()) != Some("gguf") {
                continue;
            }
            let Ok(resolved) = fs::canonicalize(&selected) else {
                continue;
            };
            if !resolved.starts_with(&root) || !resolved.is_file() || !seen.insert(resolved.clone())
            {
                continue;
            }
            let stem = selected
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("model");
            let identity = format!(
                "{:x}",
                Sha256::digest(resolved.to_string_lossy().as_bytes())
            );
            let id = format!(
                "hf-{}-{}",
                crate::llama::sanitize_model_id(stem)?,
                &identity[..16]
            );
            let mut info =
                crate::llama::model_info_from_path_fast(&selected, id.clone(), stem.into())?;
            info.filename = format!("{id}.gguf");
            found.push((info, resolved));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_apps_and_ids_reuse_one_shared_weight_and_reject_tamper() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path().join("hf/hub");
        let source = t.path().join("source.gguf");
        fs::write(&source, b"GGUFfixture").unwrap();
        let a = t.path().join("app-a");
        let b = t.path().join("app-b");
        import(&a, &cache, &source, "first".into(), "First".into()).unwrap();
        import(&b, &cache, &source, "second".into(), "Second".into()).unwrap();
        let p = path(&a, "first.gguf").unwrap().unwrap();
        assert_eq!(Some(p.clone()), path(&b, "second.gguf").unwrap());
        assert!(!a.join("models").exists());
        assert!(!b.join("models").exists());
        assert_eq!(discovered(&cache).unwrap().len(), 1);
        fs::write(&p, b"GGUFtamper!").unwrap();
        assert!(get(&a, "first").is_err());
        assert!(import(&b, &cache, &source, "third".into(), "Third".into()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn existing_hub_snapshot_import_is_reference_only() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("hub");
        let blobs = root.join("models--owner--repo/blobs");
        let snapshot = root.join("models--owner--repo/snapshots/revision");
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        let blob = blobs.join("blob-hash");
        fs::write(&blob, b"GGUFfixture").unwrap();
        let selected = snapshot.join("model.gguf");
        std::os::unix::fs::symlink(&blob, &selected).unwrap();
        let app = t.path().join("app");
        import(&app, &root, &selected, "shared".into(), "Shared".into()).unwrap();
        assert_eq!(
            path(&app, "shared.gguf").unwrap(),
            Some(blob.canonicalize().unwrap())
        );
        assert!(!root.join("local").exists());
        assert!(!blobs.join("blob-hash.horary-model.json").exists());
    }
    #[test]
    fn external_import_reuses_matching_hub_blob_before_creating_local_storage() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("hub");
        let source = t.path().join("outside.gguf");
        fs::write(&source, b"GGUFfixture").unwrap();
        let blob = root
            .join("models--owner--repo/blobs")
            .join(sha256_file(&source).unwrap());
        fs::create_dir_all(blob.parent().unwrap()).unwrap();
        fs::copy(&source, &blob).unwrap();
        let app = t.path().join("app");
        import(&app, &root, &source, "shared".into(), "Shared".into()).unwrap();
        assert_eq!(
            path(&app, "shared.gguf").unwrap(),
            Some(blob.canonicalize().unwrap())
        );
        assert!(!root.join("local").exists());
    }
}
