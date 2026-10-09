//! A round owns verified code and its signing metadata, not an updater's path.
use crate::{keep, read, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const NAME: &str = if cfg!(windows) { "codex.exe" } else { "codex" };
const PROVENANCE: &str = "codex-provenance.json";
const BUNDLE_EXECUTABLE: &str = "CodexCLI.app/Contents/MacOS/codex";
const BUNDLE_FILES: [&str; 4] = [
    "Contents/Info.plist",
    "Contents/embedded.provisionprofile",
    "Contents/_CodeSignature/CodeResources",
    "Contents/CodeResources",
];
const SUPPORT_LIMIT: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Support {
    pub provenance_file: PathBuf,
    pub provenance_sha256: String,
}

#[derive(Debug)]
pub struct Snapshot {
    pub executable: PathBuf,
    pub support: Support,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Artifact {
    File {
        path: String,
        sha256: String,
        byte_length: u64,
    },
    RelativeAlias {
        path: String,
        target: String,
    },
}

impl Artifact {
    fn path(&self) -> &str {
        match self {
            Self::File { path, .. } | Self::RelativeAlias { path, .. } => path,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    version: u32,
    original_path: PathBuf,
    snapshot_file: String,
    sha256: String,
    byte_length: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    support_files: Vec<Artifact>,
}

struct SourceFile {
    source: PathBuf,
    target: PathBuf,
    sha256: String,
    metadata: Metadata,
    executable: bool,
}

fn directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "Snapshot directory must be ordinary: {}",
            path.display()
        ));
    }
    Ok(())
}

fn only_entries(path: &Path, allowed: &[&str]) -> Result<()> {
    directory(path)?;
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        if !name.to_str().is_some_and(|name| allowed.contains(&name)) {
            return Err(
                "Codex bundle has unsupported artifacts; no arbitrary app tree is copied".into(),
            );
        }
    }
    Ok(())
}

fn bundle_root(source: &Path) -> Option<&Path> {
    let macos = source.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (source.file_name()? == "codex"
        && macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.file_name()? == "CodexCLI.app")
        .then_some(bundle)
}

fn verify_bundle_directories(root: &Path) -> Result<()> {
    only_entries(root, &["Contents"])?;
    only_entries(
        &root.join("Contents"),
        &[
            "Info.plist",
            "embedded.provisionprofile",
            "CodeResources",
            "MacOS",
            "_CodeSignature",
        ],
    )?;
    only_entries(&root.join("Contents/MacOS"), &["codex"])?;
    only_entries(&root.join("Contents/_CodeSignature"), &["CodeResources"])
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

fn sealed_permissions(path: &Path, executable: bool) -> Result<()> {
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        // The owning user can read/execute. No write or special permission bits.
        fs::Permissions::from_mode(if executable { 0o500 } else { 0o400 })
    };
    #[cfg(not(unix))]
    let permissions = {
        let _ = executable;
        let mut permissions = ordinary(path)?.permissions();
        permissions.set_readonly(true);
        permissions
    };
    fs::set_permissions(path, permissions).map_err(|e| e.to_string())
}

fn verify_permissions(metadata: &Metadata, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o500 } else { 0o400 };
        if metadata.permissions().mode() & 0o7777 != mode {
            return Err("Snapshot lost its private read/execute permissions".into());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = executable;
        if !metadata.permissions().readonly() {
            return Err("Snapshot lost its read-only permission".into());
        }
    }
    Ok(())
}

fn file_source(source: PathBuf, target: PathBuf, executable: bool) -> Result<SourceFile> {
    let (sha256, metadata) = fingerprint(&source)?;
    if metadata.len() == 0 || !executable && metadata.len() > SUPPORT_LIMIT {
        return Err("Executable is empty or bundle support exceeds its 1MiB file bound".into());
    }
    Ok(SourceFile {
        source,
        target,
        sha256,
        metadata,
        executable,
    })
}

fn copy_file(source: &SourceFile) -> Result<()> {
    let mut input = File::open(&source.source).map_err(|e| e.to_string())?;
    if !same_source(
        &source.metadata,
        &input.metadata().map_err(|e| e.to_string())?,
    ) {
        return Err("Executable source changed before copying".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source.target)
        .map_err(|e| format!("Executable snapshot must be new: {e}"))?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    Ok(())
}

fn verify_copy(source: &SourceFile) -> Result<()> {
    let (current, metadata) = fingerprint(&source.source)?;
    let (copied, copy_metadata) = fingerprint(&source.target)?;
    if source.sha256 != current
        || current != copied
        || source.metadata.len() != copy_metadata.len()
        || !same_source(&source.metadata, &metadata)
    {
        return Err("Executable source changed while copying; partial snapshot is retained without a provenance seal".into());
    }
    Ok(())
}

fn alias_target(path: &Path) -> Result<String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .is_symlink()
        || fs::read_link(path).map_err(|e| e.to_string())?
            != Path::new("_CodeSignature/CodeResources")
    {
        return Err("Only the signed relative CodeResources alias is supported".into());
    }
    Ok("_CodeSignature/CodeResources".into())
}

fn verify_provenance(root: &Path, provenance: &Provenance) -> Result<PathBuf> {
    let is_bundle = provenance.version == 2 && provenance.snapshot_file == BUNDLE_EXECUTABLE;
    if !matches!(provenance.version, 1 | 2)
        || provenance.snapshot_file != NAME && !is_bundle
        || provenance.byte_length == 0
        || !is_bundle && !provenance.support_files.is_empty()
    {
        return Err("Unsupported executable snapshot provenance".into());
    }
    directory(root)?;
    if is_bundle {
        verify_bundle_directories(&root.join("CodexCLI.app"))?;
        let mut paths = std::collections::BTreeSet::new();
        for artifact in &provenance.support_files {
            let path = artifact.path();
            if !BUNDLE_FILES.contains(&path) || !paths.insert(path) {
                return Err("Unlisted or duplicate bundle support artifact".into());
            }
            let target = root.join("CodexCLI.app").join(path);
            match artifact {
                Artifact::File {
                    sha256,
                    byte_length,
                    ..
                } => {
                    let (digest, metadata) = fingerprint(&target)?;
                    if *byte_length == 0
                        || *byte_length > SUPPORT_LIMIT
                        || *byte_length != metadata.len()
                        || *sha256 != digest
                    {
                        return Err("Bundle support changed".into());
                    }
                    verify_permissions(&metadata, false)?;
                }
                Artifact::RelativeAlias {
                    target: expected, ..
                } => {
                    if path != "Contents/CodeResources" || alias_target(&target)? != *expected {
                        return Err("Signed relative alias changed".into());
                    }
                }
            }
        }
        for path in [
            "Contents/Info.plist",
            "Contents/_CodeSignature/CodeResources",
        ] {
            if !paths.contains(path) {
                return Err("Bundle signing metadata is missing".into());
            }
        }
        for path in BUNDLE_FILES {
            if present(&root.join("CodexCLI.app").join(path))? != paths.contains(path) {
                return Err("Bundle support membership changed".into());
            }
        }
    }
    let target = root.join(&provenance.snapshot_file);
    let (digest, metadata) = fingerprint(&target)?;
    if digest != provenance.sha256 || metadata.len() != provenance.byte_length {
        return Err(
            "Existing executable snapshot or provenance differs; it will not be replaced".into(),
        );
    }
    verify_permissions(&metadata, true)?;
    Ok(target)
}

/// Verify the plan-bound manifest and every signing artifact without consulting the app.
pub fn verify_snapshot(executable: &Path, sha256: &str, support: &Support) -> Result<()> {
    let (manifest_sha256, _) = fingerprint(&support.provenance_file)?;
    if manifest_sha256 != support.provenance_sha256 {
        return Err("Executable provenance changed".into());
    }
    let provenance: Provenance =
        serde_json::from_slice(&read(&support.provenance_file)?).map_err(|e| e.to_string())?;
    let directory = support
        .provenance_file
        .parent()
        .ok_or("Provenance has no parent")?;
    if support
        .provenance_file
        .file_name()
        .is_none_or(|name| name != PROVENANCE)
        || verify_provenance(directory, &provenance)? != executable
        || provenance.sha256 != sha256
    {
        return Err("Executable and provenance do not belong to the same snapshot".into());
    }
    Ok(())
}

/// New preparation only. Existing sealed copies are checked, never replaced.
/// Copies executable bytes and a bounded signing allowlist; login/config is untouched.
pub fn snapshot_codex(source: &Path, state: &Path) -> Result<Snapshot> {
    snapshot_with(source, state, || Ok(()))
}

fn snapshot_with(
    source: &Path,
    state: &Path,
    after_copy: impl FnOnce() -> Result<()>,
) -> Result<Snapshot> {
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
    let provenance_path = directory.join(PROVENANCE);
    if present(&provenance_path)?
        || present(&directory.join(NAME))?
        || present(&directory.join("CodexCLI.app"))?
    {
        ordinary(&provenance_path)?;
        let saved: Provenance =
            serde_json::from_slice(&read(&provenance_path)?).map_err(|e| e.to_string())?;
        let target = verify_provenance(&directory, &saved)?;
        if saved.original_path != original_path
            || (saved.snapshot_file == BUNDLE_EXECUTABLE) != bundle_root(&original_path).is_some()
        {
            return Err(
                "Existing executable snapshot or provenance differs; it will not be replaced"
                    .into(),
            );
        }
        return Ok(Snapshot {
            executable: target,
            support: Support {
                provenance_sha256: fingerprint(&provenance_path)?.0,
                provenance_file: provenance_path,
            },
        });
    }

    let bundle = bundle_root(&original_path);
    let snapshot_file = if bundle.is_some() {
        BUNDLE_EXECUTABLE
    } else {
        NAME
    };
    let target = directory.join(snapshot_file);
    let mut sources = vec![file_source(original_path.clone(), target.clone(), true)?];
    let mut support_files = Vec::new();
    let mut alias = None;
    if let Some(bundle) = bundle {
        verify_bundle_directories(bundle)?;
        for path in BUNDLE_FILES {
            let source = bundle.join(path);
            if !present(&source)? {
                if matches!(
                    path,
                    "Contents/Info.plist" | "Contents/_CodeSignature/CodeResources"
                ) {
                    return Err("Codex bundle signing metadata is missing".into());
                }
                continue;
            }
            let target = directory.join("CodexCLI.app").join(path);
            if path == "Contents/CodeResources"
                && fs::symlink_metadata(&source)
                    .map_err(|e| e.to_string())?
                    .is_symlink()
            {
                let relative = alias_target(&source)?;
                support_files.push(Artifact::RelativeAlias {
                    path: path.into(),
                    target: relative,
                });
                alias = Some((source, target));
            } else {
                let source = file_source(source, target, false)?;
                support_files.push(Artifact::File {
                    path: path.into(),
                    sha256: source.sha256.clone(),
                    byte_length: source.metadata.len(),
                });
                sources.push(source);
            }
        }
        for path in [
            "CodexCLI.app",
            "CodexCLI.app/Contents",
            "CodexCLI.app/Contents/MacOS",
            "CodexCLI.app/Contents/_CodeSignature",
        ] {
            fs::create_dir(directory.join(path)).map_err(|e| e.to_string())?;
        }
    }
    for source in &sources {
        copy_file(source)?;
    }
    if let Some((_, target)) = &alias {
        #[cfg(unix)]
        std::os::unix::fs::symlink("_CodeSignature/CodeResources", target)
            .map_err(|e| e.to_string())?;
        #[cfg(not(unix))]
        return Err(format!(
            "Relative bundle aliases are unsupported on this platform: {}",
            target.display()
        ));
    }
    after_copy()?;
    if let Some(bundle) = bundle {
        verify_bundle_directories(bundle)?;
        for path in BUNDLE_FILES {
            if present(&bundle.join(path))?
                != support_files.iter().any(|artifact| artifact.path() == path)
            {
                return Err("Bundle membership changed while copying; partial snapshot is retained without a provenance seal".into());
            }
        }
    }
    if let Some((source, target)) = alias {
        if alias_target(&source)? != alias_target(&target)? {
            return Err(
                "Bundle alias changed while copying; partial snapshot remains unsealed".into(),
            );
        }
    }
    for source in &sources {
        verify_copy(source)?;
        sealed_permissions(&source.target, source.executable)?;
    }
    keep(
        &provenance_path,
        &Provenance {
            version: 2,
            original_path,
            snapshot_file: snapshot_file.into(),
            sha256: sources[0].sha256.clone(),
            byte_length: sources[0].metadata.len(),
            support_files,
        },
    )?;
    Ok(Snapshot {
        executable: target,
        support: Support {
            provenance_sha256: fingerprint(&provenance_path)?.0,
            provenance_file: provenance_path,
        },
    })
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

    fn bundle_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("CodexCLI.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::create_dir(bundle.join("Contents/_CodeSignature")).unwrap();
        let source = bundle.join("Contents/MacOS/codex");
        fs::write(&source, b"synthetic signed executable, never run").unwrap();
        for path in BUNDLE_FILES {
            fs::write(
                bundle.join(path),
                format!("synthetic signing metadata for {path}"),
            )
            .unwrap();
        }
        let state = root.path().join("round");
        (root, source, state)
    }

    fn writable(path: &Path) {
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(0o600)
        };
        #[cfg(not(unix))]
        let permissions = {
            let mut permissions = ordinary(path).unwrap().permissions();
            permissions.set_readonly(false);
            permissions
        };
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn controller_artifacts_can_share_the_executable_directory() {
        let (_root, source, state) = bundle_fixture();
        let directory = state.join("executables");
        fs::create_dir_all(&directory).unwrap();
        let controller = directory.join("horary-gepa");
        fs::write(&controller, b"separately pinned controller").unwrap();
        let snapshot = snapshot_codex(&source, &state).unwrap();
        verify_snapshot(
            &snapshot.executable,
            &fingerprint(&snapshot.executable).unwrap().0,
            &snapshot.support,
        )
        .unwrap();
        assert_eq!(read(&controller).unwrap(), b"separately pinned controller");
    }

    #[test]
    fn signed_bundle_layout_and_public_metadata_are_preserved_and_pinned() {
        let (root, source, state) = bundle_fixture();
        fs::write(
            root.path().join("auth.json"),
            b"synthetic excluded authentication",
        )
        .unwrap();
        fs::write(
            root.path().join("config.toml"),
            b"synthetic excluded configuration",
        )
        .unwrap();
        let snapshot = snapshot_codex(&source, &state).unwrap();
        assert_eq!(
            snapshot.executable,
            state
                .canonicalize()
                .unwrap()
                .join("executables")
                .join(BUNDLE_EXECUTABLE)
        );
        for path in BUNDLE_FILES {
            assert_eq!(
                read(
                    &source
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join(path)
                )
                .unwrap(),
                read(&state.join("executables/CodexCLI.app").join(path)).unwrap()
            );
        }
        assert!(!state.join("executables/auth.json").exists());
        assert!(!state.join("executables/config.toml").exists());
        let executable_sha = fingerprint(&snapshot.executable).unwrap().0;
        verify_snapshot(&snapshot.executable, &executable_sha, &snapshot.support).unwrap();
        let metadata = state.join("executables/CodexCLI.app/Contents/Info.plist");
        writable(&metadata);
        fs::write(&metadata, b"changed signing metadata").unwrap();
        assert_eq!(fingerprint(&snapshot.executable).unwrap().0, executable_sha);
        assert!(
            verify_snapshot(&snapshot.executable, &executable_sha, &snapshot.support)
                .unwrap_err()
                .contains("Bundle support changed")
        );
        assert!(snapshot_codex(&source, &state).is_err());
        assert_eq!(read(&metadata).unwrap(), b"changed signing metadata");
    }

    #[test]
    fn source_metadata_drift_leaves_the_whole_bundle_unsealed_and_unreplaced() {
        let (_root, source, state) = bundle_fixture();
        let info = source
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Info.plist");
        let error = snapshot_with(&source, &state, || {
            fs::write(&info, b"updated signing metadata").map_err(|e| e.to_string())
        })
        .unwrap_err();
        assert!(error.contains("changed while copying"));
        assert!(!state.join("executables").join(PROVENANCE).exists());
        let copied_info = state.join("executables/CodexCLI.app/Contents/Info.plist");
        let retained = read(&copied_info).unwrap();
        assert!(snapshot_codex(&source, &state).is_err());
        assert_eq!(read(&copied_info).unwrap(), retained);
    }

    #[test]
    fn bundle_updates_do_not_consult_or_replace_sealed_support() {
        let (_root, source, state) = bundle_fixture();
        let snapshot = snapshot_codex(&source, &state).unwrap();
        let info = source
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Info.plist");
        let copied_info = state.join("executables/CodexCLI.app/Contents/Info.plist");
        let retained = read(&copied_info).unwrap();
        fs::write(&source, b"updated executable").unwrap();
        fs::write(info, b"updated info").unwrap();
        assert_eq!(
            snapshot_codex(&source, &state).unwrap().executable,
            snapshot.executable
        );
        assert_eq!(read(&copied_info).unwrap(), retained);
        verify_snapshot(
            &snapshot.executable,
            &fingerprint(&snapshot.executable).unwrap().0,
            &snapshot.support,
        )
        .unwrap();
    }

    #[test]
    fn unsupported_bundle_trees_and_missing_signing_metadata_stop_before_copying() {
        let (_root, source, state) = bundle_fixture();
        let contents = source.parent().unwrap().parent().unwrap();
        fs::write(contents.join("auth.json"), b"must remain outside snapshots").unwrap();
        assert!(snapshot_codex(&source, &state)
            .unwrap_err()
            .contains("unsupported artifacts"));
        assert!(!state.join("executables").join(BUNDLE_EXECUTABLE).exists());
        fs::remove_file(contents.join("auth.json")).unwrap();
        fs::remove_file(contents.join("Info.plist")).unwrap();
        assert!(snapshot_codex(&source, &state)
            .unwrap_err()
            .contains("signing metadata is missing"));
        assert!(!state.join("executables").join(PROVENANCE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn signed_relative_alias_is_preserved_but_escaping_aliases_are_rejected() {
        let (_root, source, state) = bundle_fixture();
        let contents = source.parent().unwrap().parent().unwrap();
        let alias = contents.join("CodeResources");
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink("_CodeSignature/CodeResources", &alias).unwrap();
        let snapshot = snapshot_codex(&source, &state).unwrap();
        let copied_alias = state.join("executables/CodexCLI.app/Contents/CodeResources");
        assert_eq!(
            fs::read_link(&copied_alias).unwrap(),
            Path::new("_CodeSignature/CodeResources")
        );
        let sha = fingerprint(&snapshot.executable).unwrap().0;
        verify_snapshot(&snapshot.executable, &sha, &snapshot.support).unwrap();
        fs::remove_file(&copied_alias).unwrap();
        std::os::unix::fs::symlink(&alias, &copied_alias).unwrap();
        assert!(verify_snapshot(&snapshot.executable, &sha, &snapshot.support).is_err());
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink("../../outside", &alias).unwrap();
        let other = state.with_file_name("other-round");
        assert!(snapshot_codex(&source, &other)
            .unwrap_err()
            .contains("relative CodeResources alias"));
        assert!(!other.join("executables").join(PROVENANCE).exists());
    }

    #[test]
    fn provenance_drift_and_legacy_standalone_copies_never_get_resealed() {
        let (_root, source, state) = fixture();
        let snapshot = snapshot_codex(&source, &state).unwrap();
        let provenance_bytes = read(&snapshot.support.provenance_file).unwrap();
        let mut provenance: serde_json::Value = serde_json::from_slice(&provenance_bytes).unwrap();
        provenance["version"] = serde_json::json!(1);
        let legacy_bytes = serde_json::to_vec_pretty(&provenance).unwrap();
        fs::write(&snapshot.support.provenance_file, &legacy_bytes).unwrap();
        assert!(verify_snapshot(
            &snapshot.executable,
            &fingerprint(&snapshot.executable).unwrap().0,
            &snapshot.support
        )
        .unwrap_err()
        .contains("provenance changed"));
        let reused = snapshot_codex(&source, &state).unwrap();
        assert_eq!(reused.executable, snapshot.executable);
        assert_eq!(read(&reused.support.provenance_file).unwrap(), legacy_bytes);
        verify_snapshot(
            &reused.executable,
            &fingerprint(&reused.executable).unwrap().0,
            &reused.support,
        )
        .unwrap();
    }

    #[test]
    fn copies_only_executable_bytes_and_records_the_source() {
        let (_root, source, state) = fixture();
        fs::write(source.parent().unwrap().join("auth.json"), b"must not copy").unwrap();
        let target = snapshot_codex(&source, &state).unwrap().executable;
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
        let target = snapshot_codex(&source, &state).unwrap().executable;
        let bytes = read(&target).unwrap();
        fs::write(&source, b"an updated app executable").unwrap();
        assert_eq!(snapshot_codex(&source, &state).unwrap().executable, target);
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
        let target = snapshot_codex(&source, &state).unwrap().executable;
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
