//! A round owns one verified executable, not the app updater's changing path.
use crate::{keep, read, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const NAME: &str = if cfg!(windows) { "codex.exe" } else { "codex" };
const PROVENANCE: &str = "codex-provenance.json";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    version: u32,
    original_path: PathBuf,
    snapshot_file: String,
    sha256: String,
    byte_length: u64,
}

fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn ordinary(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "Executable artifact must be an ordinary file: {}",
            path.display()
        ));
    }
    Ok(metadata)
}

fn same_source(a: &Metadata, b: &Metadata) -> bool {
    let same = a.len() == b.len()
        && a.modified().ok() == b.modified().ok()
        && a.permissions().readonly() == b.permissions().readonly();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        same && a.dev() == b.dev() && a.ino() == b.ino() && a.mode() == b.mode()
    }
    #[cfg(not(unix))]
    {
        same
    }
}

fn hash(file: &mut File) -> Result<String> {
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(format!("{:x}", digest.finalize()));
        }
        digest.update(&bytes[..count]);
    }
}

fn fingerprint(path: &Path) -> Result<(String, Metadata)> {
    let before = ordinary(path)?;
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !same_source(&before, &file.metadata().map_err(|e| e.to_string())?) {
        return Err("Executable source changed while opening".into());
    }
    let digest = hash(&mut file)?;
    if !same_source(&before, &file.metadata().map_err(|e| e.to_string())?)
        || !same_source(&before, &ordinary(path)?)
    {
        return Err("Executable source changed while hashing".into());
    }
    Ok((digest, before))
}

fn executable_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        // The owning user can read/execute. No write or special permission bits.
        fs::Permissions::from_mode(0o500)
    };
    #[cfg(not(unix))]
    let permissions = {
        let mut permissions = ordinary(path)?.permissions();
        permissions.set_readonly(true);
        permissions
    };
    fs::set_permissions(path, permissions).map_err(|e| e.to_string())
}

/// New preparation only. Existing sealed copies are checked, never replaced.
/// Only executable bytes and their provenance are copied; login/config is untouched.
pub fn snapshot_codex(source: &Path, state: &Path) -> Result<PathBuf> {
    snapshot_with(source, state, || Ok(()))
}

fn snapshot_with(
    source: &Path,
    state: &Path,
    after_copy: impl FnOnce() -> Result<()>,
) -> Result<PathBuf> {
    if present(&state.join("plan.json"))? {
        return Err(
            "Executable snapshots apply only to new preparations; preserve the existing plan"
                .into(),
        );
    }
    ordinary(source)?;
    let original_path = source.canonicalize().map_err(|e| e.to_string())?;
    fs::create_dir_all(state).map_err(|e| e.to_string())?;
    let directory = state
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join("executables");
    if present(&directory)? {
        if !fs::symlink_metadata(&directory)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return Err("Executable snapshot directory must be an ordinary directory".into());
        }
    } else {
        fs::create_dir(&directory).map_err(|e| e.to_string())?;
    }
    let target = directory.join(NAME);
    let provenance_path = directory.join(PROVENANCE);
    if present(&target)? || present(&provenance_path)? {
        ordinary(&provenance_path)?;
        let saved: Provenance =
            serde_json::from_slice(&read(&provenance_path)?).map_err(|e| e.to_string())?;
        let (digest, metadata) = fingerprint(&target)?;
        if saved.version != 1
            || saved.original_path != original_path
            || saved.snapshot_file != NAME
            || saved.byte_length == 0
            || saved.sha256 != digest
            || saved.byte_length != metadata.len()
        {
            return Err(
                "Existing executable snapshot or provenance differs; it will not be replaced"
                    .into(),
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o7777 != 0o500 {
                return Err(
                    "Existing executable snapshot lost its private read/execute permissions".into(),
                );
            }
        }
        #[cfg(not(unix))]
        if !metadata.permissions().readonly() {
            return Err("Existing executable snapshot lost its read-only permission".into());
        }
        return Ok(target);
    }

    let (before, metadata) = fingerprint(&original_path)?;
    if metadata.len() == 0 {
        return Err("Executable source is empty".into());
    }
    let mut input = File::open(&original_path).map_err(|e| e.to_string())?;
    if !same_source(&metadata, &input.metadata().map_err(|e| e.to_string())?) {
        return Err("Executable source changed before copying".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| format!("Executable snapshot must be new: {e}"))?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    output.sync_all().map_err(|e| e.to_string())?;
    after_copy()?;
    input.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let after_handle = hash(&mut input)?;
    let (after_path, current) = fingerprint(&original_path)?;
    let (copied, copy_metadata) = fingerprint(&target)?;
    if before != copied
        || before != after_handle
        || before != after_path
        || copy_metadata.len() != metadata.len()
        || !same_source(&metadata, &current)
        || !same_source(&metadata, &input.metadata().map_err(|e| e.to_string())?)
    {
        return Err("Executable source changed while copying; partial snapshot is retained without a provenance seal".into());
    }
    drop(output);
    executable_permissions(&target)?;
    keep(
        &provenance_path,
        &Provenance {
            version: 1,
            original_path,
            snapshot_file: NAME.into(),
            sha256: copied,
            byte_length: copy_metadata.len(),
        },
    )?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("updatable-codex");
        fs::write(&source, b"synthetic executable bytes, never run").unwrap();
        let state = root.path().join("round");
        (root, source, state)
    }

    #[test]
    fn copies_only_executable_bytes_and_records_the_source() {
        let (_root, source, state) = fixture();
        fs::write(source.parent().unwrap().join("auth.json"), b"must not copy").unwrap();
        let target = snapshot_codex(&source, &state).unwrap();
        assert_eq!(read(&target).unwrap(), read(&source).unwrap());
        let p: Provenance =
            serde_json::from_slice(&read(&state.join("executables").join(PROVENANCE)).unwrap())
                .unwrap();
        assert_eq!(p.original_path, source.canonicalize().unwrap());
        assert_eq!(p.sha256, fingerprint(&target).unwrap().0);
        assert_eq!(fs::read_dir(state.join("executables")).unwrap().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                ordinary(&target).unwrap().permissions().mode() & 0o7777,
                0o500
            );
        }
    }

    #[test]
    fn app_updates_do_not_replace_a_verified_snapshot() {
        let (_root, source, state) = fixture();
        let target = snapshot_codex(&source, &state).unwrap();
        let bytes = read(&target).unwrap();
        fs::write(&source, b"an updated app executable").unwrap();
        assert_eq!(snapshot_codex(&source, &state).unwrap(), target);
        assert_eq!(read(&target).unwrap(), bytes);
    }

    #[test]
    fn drift_during_copy_is_retained_but_never_sealed_or_replaced() {
        let (_root, source, state) = fixture();
        let error = snapshot_with(&source, &state, || {
            fs::write(&source, b"source changed during the snapshot").map_err(|e| e.to_string())
        })
        .unwrap_err();
        assert!(error.contains("changed while copying"));
        let partial = state.join("executables").join(NAME);
        let bytes = read(&partial).unwrap();
        assert!(!state.join("executables").join(PROVENANCE).exists());
        assert!(snapshot_codex(&source, &state).is_err());
        assert_eq!(read(&partial).unwrap(), bytes);
    }

    #[test]
    fn corrupt_snapshots_and_old_plans_are_never_rewritten() {
        let (_root, source, state) = fixture();
        let target = snapshot_codex(&source, &state).unwrap();
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(0o600)
        };
        #[cfg(not(unix))]
        let permissions = {
            let mut permissions = ordinary(&target).unwrap().permissions();
            permissions.set_readonly(false);
            permissions
        };
        fs::set_permissions(&target, permissions).unwrap();
        fs::write(&target, b"changed snapshot").unwrap();
        assert!(snapshot_codex(&source, &state)
            .unwrap_err()
            .contains("will not be replaced"));
        assert_eq!(read(&target).unwrap(), b"changed snapshot");
        keep(
            &state.join("plan.json"),
            &serde_json::json!({"legacy":"unchanged"}),
        )
        .unwrap();
        assert!(snapshot_codex(&source, &state)
            .unwrap_err()
            .contains("existing plan"));
        assert_eq!(read(&target).unwrap(), b"changed snapshot");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_source_and_snapshot_are_rejected() {
        let (_root, source, state) = fixture();
        let link = source.with_extension("link");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(snapshot_codex(&link, &state).is_err());
        fs::create_dir_all(state.join("executables")).unwrap();
        std::os::unix::fs::symlink(&source, state.join("executables").join(NAME)).unwrap();
        assert!(snapshot_codex(&source, &state).is_err());
        assert!(fs::symlink_metadata(state.join("executables").join(NAME))
            .unwrap()
            .is_symlink());
    }
}
