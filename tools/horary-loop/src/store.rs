//! Durable receipts are immutable; the ledger is an atomic derivative.
#![forbid(unsafe_code)]

use horary_prompt_program::digest;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("Read {}: {e}", path.display()))
}

pub fn json(path: &Path) -> Result<serde_json::Value> {
    serde_json::from_slice(&read(path)?).map_err(|e| format!("Decode {}: {e}", path.display()))
}

pub fn canonical(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| format!("Resolve {}: {e}", path.display()))
}

pub fn safe_relative(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if value.is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("Unsafe evidence path: {value:?}"));
    }
    Ok(path)
}

pub fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn atomic(path: &Path, bytes: &[u8], immutable: bool) -> Result<()> {
    let parent = path.parent().ok_or("Evidence file needs a parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("Create {}: {e}", parent.display()))?;
    let mut pending = tempfile::Builder::new()
        .prefix("partial-")
        .tempfile_in(parent)
        .map_err(|e| format!("Prepare {}: {e}", path.display()))?;
    if let Err(e) = pending
        .write_all(bytes)
        .and_then(|()| pending.as_file().sync_all())
    {
        let kept = pending
            .keep()
            .map(|(_, p)| p.display().to_string())
            .unwrap_or_else(|error| format!("partial preservation failed: {error}"));
        return Err(format!("Write {}: {e}; partial: {kept}", path.display()));
    }
    let saved = if immutable {
        pending.persist_noclobber(path)
    } else {
        pending.persist(path)
    };
    saved.map_err(|e| {
        let cause = e.error.to_string();
        let kept = e
            .file
            .keep()
            .map(|(_, p)| p.display().to_string())
            .unwrap_or_else(|error| format!("partial preservation failed: {error}"));
        format!("Commit {}: {cause}; partial: {kept}", path.display())
    })?;
    fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("Sync evidence directory {}: {e}", parent.display()))?;
    Ok(())
}

pub fn atomic_json(path: &Path, value: &impl Serialize, immutable: bool) -> Result<()> {
    atomic(
        path,
        &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
        immutable,
    )
}

pub fn snapshot(state: &Path, relative: &str, bytes: &[u8]) -> Result<FileRef> {
    safe_relative(relative)?;
    let sha256 = digest(bytes);
    let blob = state.join("blobs").join(&sha256);
    if blob.exists() {
        if read(&blob)? != bytes {
            return Err(format!("Immutable blob changed: {}", blob.display()));
        }
    } else {
        atomic(&blob, bytes, true)?;
    }
    Ok(FileRef {
        file: relative.into(),
        sha256,
        bytes: bytes.len() as u64,
    })
}

pub fn resolve_blob(state: &Path, reference: &FileRef) -> Result<serde_json::Value> {
    let bytes = read(&state.join("blobs").join(&reference.sha256))?;
    if digest(&bytes) != reference.sha256 || bytes.len() as u64 != reference.bytes {
        return Err(format!("Snapshot integrity failure for {}", reference.file));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Decode snapshot {}: {e}", reference.file))
}

pub fn collect_files(base: &Path, dir: &Path, result: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .map_err(|e| format!("List {}: {e}", dir.display()))?
        .map(|item| item.map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            return Err(format!(
                "Symlink in immutable campaign: {}",
                entry.path().display()
            ));
        }
        if ty.is_dir() {
            collect_files(base, &entry.path(), result)?;
        } else if ty.is_file() {
            let path = entry.path();
            let relative = path
                .strip_prefix(base)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("Evidence filename is not UTF-8")?
                .to_owned();
            // Rendered HTML is a derivative, not primary processing evidence.
            if path
                .extension()
                .is_some_and(|e| e == "json" || e == "jsonl")
            {
                result.push((relative, path));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_snapshot_deduplicates_and_detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let first = snapshot(dir.path(), "cases/a/outcome.json", b"{}").unwrap();
        let second = snapshot(dir.path(), "cases/b/outcome.json", b"{}").unwrap();
        assert_eq!(first.sha256, second.sha256);
        fs::write(dir.path().join("blobs").join(&first.sha256), b"changed").unwrap();
        assert!(snapshot(dir.path(), "cases/a/outcome.json", b"{}").is_err());
    }
    #[test]
    fn immutable_receipt_collision_preserves_old_and_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        atomic(&path, b"old", true).unwrap();
        assert!(atomic(&path, b"new", true)
            .unwrap_err()
            .contains("partial:"));
        assert_eq!(read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn traversal_and_absolute_evidence_paths_are_rejected() {
        for path in [
            "",
            "../outcome.json",
            "/etc/passwd",
            "cases/../../secret",
            "./outcome.json",
        ] {
            assert!(safe_relative(path).is_err(), "{path}");
        }
        assert!(safe_relative("cases/a/calls/0001-result.json").is_ok());
    }
}
