//! One fresh owned hosted catalogue run. Nothing here retries paid work.
//!
//! The native runner creates its own evidence directory. A fresh sibling
//! `<campaign>.owner` therefore seals submission while the future native PID
//! waits on a pipe. After release, the shell execs native in that same PID.
//! Closure mirrors the exact pre-start receipt without moving native evidence.
//! `corpus::seal` must run separately, after this owner has exited.
#![forbid(unsafe_code)]

use crate::{corpus, keep, read, verify, Result};
use horary_prompt_program::digest;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const ENTRY: &str = "elicitation_eval::real_model_catalogue_campaign";
const KIND: &str = "fresh_owned_native_hosted";
const START: &str = "horary-campaign-start-v1\n";
const GATE: &str = "IFS= read -r horary_start || exit 124\n[ \"$horary_start\" = horary-campaign-start-v1 ] || exit 125\nexec \"$@\"";
const ROOT_WITNESSES: [&str; 6] = [
    "manifest.json",
    "fixtures.json",
    "report.json",
    "completed.json",
    "reading-rubrics.json",
    "prompt-program.json",
];

type Hashes = BTreeMap<String, String>;

fn fresh_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn hash_file(path: &Path) -> Result<String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("Campaign artifacts must be ordinary files".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn tree(root: &Path) -> Result<Hashes> {
    fn visit(base: &Path, directory: &Path, hashes: &mut Hashes) -> Result<()> {
        if !fs::symlink_metadata(directory)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return Err("Campaign case trees cannot contain directory aliases".into());
        }
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if metadata.is_dir() {
                visit(base, &path, hashes)?;
            } else {
                let name = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Non-UTF8 campaign artifact")?
                    .replace('\\', "/");
                hashes.insert(name, hash_file(&path)?);
            }
        }
        Ok(())
    }
    let mut hashes = Hashes::new();
    visit(root, root, &mut hashes)?;
    Ok(hashes)
}

struct OwnedChild(Option<Child>);
impl OwnedChild {
    fn wait(&mut self, deadline: Duration) -> Result<(ExitStatus, bool)> {
        let start = Instant::now();
        loop {
            let child = self.0.as_mut().ok_or("Native child already reaped")?;
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                self.0 = None;
                return Ok((status, false));
            }
            if start.elapsed() >= deadline {
                child.kill().map_err(|e| e.to_string())?;
                let status = child.wait().map_err(|e| e.to_string())?;
                self.0 = None;
                return Ok((status, true));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Launch only the exact hosted production catalogue entry. The environment
/// supplies a credential FILE LOCATOR, never credential contents or stdin.
pub fn run(
    campaign: &Path,
    native: &Path,
    selected_ids: &[String],
    case_seconds: u64,
    max_calls: u64,
) -> Result<Value> {
    let locator = std::env::var_os("HORARY_GOOGLE_KEY_FILE")
        .map(PathBuf::from)
        .ok_or("Set the private HORARY_GOOGLE_KEY_FILE locator")?;
    run_with_locator(
        campaign,
        native,
        selected_ids,
        case_seconds,
        max_calls,
        &locator,
    )
}

fn run_with_locator(
    campaign: &Path,
    native: &Path,
    selected_ids: &[String],
    case_seconds: u64,
    max_calls: u64,
    locator: &Path,
) -> Result<Value> {
    if !cfg!(unix) {
        return Err("The owned startup pipe requires a Unix host".into());
    }
    if selected_ids.is_empty()
        || selected_ids.len() > 1024
        || selected_ids.iter().collect::<BTreeSet<_>>().len() != selected_ids.len()
        || selected_ids.iter().any(|id| {
            id.is_empty()
                || id.len() > 160
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
        || case_seconds == 0
        || max_calls == 0
    {
        return Err("A bounded campaign needs explicit unique case IDs and positive limits".into());
    }
    let physical_reservation = (selected_ids.len() as u64)
        .checked_mul(max_calls)
        .and_then(|n| n.checked_mul(12))
        .ok_or("Campaign generation reservation overflow")?;
    let wall_seconds = (selected_ids.len() as u64)
        .div_ceil(4)
        .checked_mul(case_seconds)
        .and_then(|n| n.checked_add(300))
        .ok_or("Campaign wall deadline overflow")?;
    if !campaign.is_absolute()
        || campaign.components().any(|c| {
            !matches!(
                c,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
    {
        return Err("Campaign path must be absolute without traversal".into());
    }
    let parent = campaign
        .parent()
        .ok_or("Campaign has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let name = campaign
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid campaign name")?;
    let campaign = parent.join(name);
    let owner = parent.join(format!("{name}.owner"));
    if fs::symlink_metadata(&campaign).is_ok() {
        return Err("Native campaign already exists; no replay or resubmission".into());
    }
    let native = native.canonicalize().map_err(|e| e.to_string())?;
    let native_sha = hash_file(&native)?;
    if !locator.is_absolute()
        || !fs::metadata(locator)
            .map_err(|_| "Hosted credential locator unavailable")?
            .is_file()
        || locator.starts_with(&campaign)
        || locator.starts_with(&owner)
    {
        return Err("Keep the private credential locator outside campaign evidence".into());
    }
    fs::create_dir(&owner)
        .map_err(|e| format!("Fresh campaign ownership guard blocks resubmission: {e}"))?;
    keep(
        &owner.join("configuration.json"),
        &json!({
        "entry":ENTRY,"provider":"google","model":"gemma-4-26b-a4b-it","full_reading":true,
        "case_parallelism":4,"case_seconds":case_seconds,"max_calls_per_case":max_calls,
        "physical_generation_attempt_reservation":physical_reservation,"wall_deadline_seconds":wall_seconds,
        "input_tokens_per_minute":14000,"ordinary_unconstrained_text":true,
        "response_schema":false,"local_inference":false,"automatic_resubmission":false,
        "startup_gate":"owner seals submission before releasing one pipe token"}),
    )?;
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", GATE, "horary-campaign-gate"])
        .arg(&native)
        .args([
            ENTRY,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HORARY_EVAL_PROVIDER", "google")
        .env("HORARY_EVAL_FULL", "1")
        .env("HORARY_EVAL_BATCH", "4")
        .env("HORARY_EVAL_FILTER", selected_ids.join(","))
        .env("HORARY_EVAL_CASE_SECONDS", case_seconds.to_string())
        .env("HORARY_EVAL_MAX_CALLS", max_calls.to_string())
        .env("HORARY_EVAL_EVIDENCE", &campaign)
        .env("HORARY_EVAL_PHASE", "fresh_owned_control")
        .env("HORARY_GOOGLE_KEY_FILE", locator)
        .env("HORARY_GOOGLE_INPUT_TPM", "14000")
        .env("HORARY_CAMPAIGN_OWNER_DIRECTORY", &owner)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(fresh_file(&owner.join("native.stdout.log"))?))
        .stderr(Stdio::from(fresh_file(&owner.join("native.stderr.log"))?));
    for variable in [
        "HORARY_NATIVE_LLAMA_TEST_MODEL",
        "HORARY_EVAL_PROGRAM",
        "HORARY_EVAL_MODES",
        "HORARY_NEURAL_TASK",
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
    ] {
        command.env_remove(variable);
    }
    let child = command.spawn().map_err(|e| e.to_string())?;
    let native_pid = child.id();
    let mut child = OwnedChild(Some(child));
    let submitted = json!({"version":corpus::RECEIPT_VERSION,"kind":KIND,"campaign":campaign,
        "owner_pid":std::process::id(),"native_pid":native_pid,"selected_ids":selected_ids,
        "native_executable":native,"native_executable_sha256":native_sha,"automatic_resubmission":false});
    keep(&owner.join("submitted.json"), &submitted)?;
    let submitted_bytes = read(&owner.join("submitted.json"))?;
    let submitted_sha = digest(&submitted_bytes);
    verify(&native, &native_sha)?;
    if fs::symlink_metadata(&campaign).is_ok() {
        return Err(
            "Campaign appeared before startup release; child was not authorized to run".into(),
        );
    }
    keep(
        &owner.join("start-authorized.json"),
        &json!({"submitted_sha256":submitted_sha,
        "native_pid":native_pid,"configuration_sha256":hash_file(&owner.join("configuration.json"))?,
        "native_executable_sha256":native_sha,"pre_start_submission_sealed":true}),
    )?;
    let mut gate = child
        .0
        .as_mut()
        .ok_or("Missing owned child")?
        .stdin
        .take()
        .ok_or("Missing native startup pipe")?;
    gate.write_all(START.as_bytes())
        .map_err(|e| e.to_string())?;
    drop(gate);
    let (status, deadline_cancelled) = child.wait(Duration::from_secs(wall_seconds))?;
    let code = status.code().unwrap_or(-1);
    let exit = json!({"version":corpus::RECEIPT_VERSION,"kind":KIND,"campaign":campaign,
        "owner_pid":std::process::id(),"native_pid":native_pid,"submitted_sha256":submitted_sha,
        "native_exit_code":code,"automatic_resubmission":false});
    keep(&owner.join("owner-exit.json"), &exit)?;
    keep(
        &owner.join("terminal.json"),
        &json!({"native_exit_code":status.code(),
        "exit_status":status.to_string(),"deadline_cancelled":deadline_cancelled,
        "submitted_sha256":submitted_sha,"native_pid_reaped":true,"automatic_resubmission":false,
        "qualification":"Exit 101 may be settled semantic failure. Neither exit nor closure proves successful readings; corpus sealer independently checks all native observations."}),
    )?;
    if !fs::symlink_metadata(&campaign).is_ok_and(|m| m.is_dir()) {
        return Ok(
            json!({"campaign":campaign,"owner_directory":owner,"native_exit_code":code,
            "closure":null,"native_pid_reaped":true,"automatic_resubmission":false}),
        );
    }
    let mut mirrored = fresh_file(&campaign.join("submitted.json"))?;
    mirrored
        .write_all(&submitted_bytes)
        .and_then(|_| mirrored.sync_all())
        .map_err(|e| e.to_string())?;
    verify(&campaign.join("submitted.json"), &submitted_sha)?;
    keep(&campaign.join("owner-exit.json"), &exit)?;
    verify(&native, &native_sha)?;
    let mut files = Hashes::new();
    for name in ROOT_WITNESSES {
        let path = campaign.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            files.insert(name.to_owned(), hash_file(&path)?);
        }
    }
    let mut case_files = BTreeMap::new();
    for id in selected_ids {
        let directory = campaign.join("cases").join(id);
        if fs::symlink_metadata(&directory).is_ok() {
            case_files.insert(id, tree(&directory)?);
        }
    }
    let closure = campaign.join("corpus-closure.json");
    keep(
        &closure,
        &json!({"version":corpus::CORPUS_VERSION,"campaign":campaign,
        "submitted_sha256":submitted_sha,"owner_exit_sha256":hash_file(&campaign.join("owner-exit.json"))?,
        "native_executable":native,"native_executable_sha256":native_sha,"files":files,"case_files":case_files}),
    )?;
    Ok(
        json!({"campaign":campaign,"owner_directory":owner,"native_exit_code":code,
        "closure":closure,"closure_sha256":hash_file(&closure)?,"native_pid_reaped":true,
        "automatic_resubmission":false,"case_trees":case_files.len(),
        "qualification":"Complete ordinary-file trees are pinned without editing outcomes or claiming semantic success. Seal in a later process after owner exit; unsettled/partial evidence remains rejectable."}),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::load;
    use std::os::unix::fs::PermissionsExt;

    fn fake_native(directory: &Path, code: i32) -> PathBuf {
        let native = directory.join("synthetic-native");
        fs::write(&native, format!(r#"#!/bin/sh
test -f "$HORARY_CAMPAIGN_OWNER_DIRECTORY/submitted.json" || exit 91
test -f "$HORARY_CAMPAIGN_OWNER_DIRECTORY/start-authorized.json" || exit 92
test "$HORARY_EVAL_PROVIDER" = google || exit 93
test "$HORARY_EVAL_FULL" = 1 || exit 94
test "$HORARY_EVAL_BATCH" = 4 || exit 95
test "$HORARY_EVAL_FILTER" = synthetic-explicit || exit 96
test "$HORARY_EVAL_MAX_CALLS" = 8 || exit 97
test "$HORARY_EVAL_CASE_SECONDS" = 5 || exit 98
test "$1" = elicitation_eval::real_model_catalogue_campaign || exit 99
mkdir -p "$HORARY_EVAL_EVIDENCE/cases/synthetic-explicit/calls"
printf '%s' '{{"provider":"synthetic-offline"}}' > "$HORARY_EVAL_EVIDENCE/manifest.json"
printf '%s' '[]' > "$HORARY_EVAL_EVIDENCE/fixtures.json"
printf '%s' '{{}}' > "$HORARY_EVAL_EVIDENCE/report.json"
printf '%s' '{{}}' > "$HORARY_EVAL_EVIDENCE/completed.json"
printf '%s' '[]' > "$HORARY_EVAL_EVIDENCE/reading-rubrics.json"
printf '%s' '{{"only":"synthetic"}}' > "$HORARY_EVAL_EVIDENCE/cases/synthetic-explicit/outcome.json"
printf '%s' '{{"only":"request"}}' > "$HORARY_EVAL_EVIDENCE/cases/synthetic-explicit/calls/0001-request.json"
printf '%s' "$$" > "$HORARY_EVAL_EVIDENCE/cases/synthetic-explicit/native.pid"
exit {code}
"#)).unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        native
    }
    fn locator(directory: &Path) -> PathBuf {
        let locator = directory.join("private-synthetic-locator");
        fs::write(&locator, "synthetic-offline-secret-never-read").unwrap();
        locator
    }

    #[test]
    fn pre_start_submission_same_pid_and_exact_complete_case_tree_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let native = fake_native(&root, 101);
        let campaign = root.join("fresh");
        let receipt = run_with_locator(
            &campaign,
            &native,
            &["synthetic-explicit".into()],
            5,
            8,
            &locator(&root),
        )
        .unwrap();
        assert_eq!(receipt["native_exit_code"], 101);
        let submitted = load(&campaign.join("submitted.json")).unwrap();
        assert_eq!(
            read(&campaign.join("submitted.json")).unwrap(),
            read(&root.join("fresh.owner/submitted.json")).unwrap()
        );
        let observed_pid = read(&campaign.join("cases/synthetic-explicit/native.pid")).unwrap();
        assert_eq!(
            String::from_utf8(observed_pid).unwrap(),
            submitted["native_pid"].to_string()
        );
        let closure = load(&campaign.join("corpus-closure.json")).unwrap();
        assert_eq!(
            closure["case_files"]["synthetic-explicit"],
            json!(tree(&campaign.join("cases/synthetic-explicit")).unwrap())
        );
        assert!(closure["case_files"]["synthetic-explicit"]
            .get("calls/0001-request.json")
            .is_some());
        for file in [
            campaign.join("submitted.json"),
            campaign.join("corpus-closure.json"),
            root.join("fresh.owner/configuration.json"),
            root.join("fresh.owner/start-authorized.json"),
        ] {
            let bytes = String::from_utf8(read(&file).unwrap()).unwrap();
            assert!(!bytes.contains("synthetic-offline-secret-never-read"));
            assert!(!bytes.contains("private-synthetic-locator"));
        }
        // This fake run has deliberately unsettled/wrong provider evidence;
        // neither exit 101 nor a closure turns it into a usable corpus.
        assert!(corpus::seal(&campaign, &campaign.join("corpus-closure.json")).is_err());
        assert!(run_with_locator(
            &campaign,
            &native,
            &["synthetic-explicit".into()],
            5,
            8,
            &locator(&root)
        )
        .is_err());
    }

    #[test]
    fn failed_start_is_not_replayed_when_native_creates_no_campaign() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let native = root.join("offline-failure");
        fs::write(&native, "#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
        let key = locator(&root);
        let campaign = root.join("failed");
        let receipt = run_with_locator(
            &campaign,
            &native,
            &["synthetic-explicit".into()],
            5,
            8,
            &key,
        )
        .unwrap();
        assert_eq!(receipt["native_exit_code"], 7);
        assert!(receipt["closure"].is_null());
        assert!(!campaign.exists());
        assert!(root.join("failed.owner/owner-exit.json").exists());
        assert!(run_with_locator(
            &campaign,
            &native,
            &["synthetic-explicit".into()],
            5,
            8,
            &key
        )
        .unwrap_err()
        .contains("ownership guard"));
    }

    #[test]
    fn invalid_bounds_and_file_aliases_cannot_create_valid_closures() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let native = fake_native(&root, 0);
        let key = locator(&root);
        for ids in [
            vec![],
            vec!["synthetic-explicit".into(), "synthetic-explicit".into()],
            vec!["../case".into()],
        ] {
            assert!(run_with_locator(&root.join("invalid"), &native, &ids, 5, 8, &key).is_err());
        }
        assert!(!root.join("invalid.owner").exists());
        let cases = root.join("tree");
        fs::create_dir(&cases).unwrap();
        std::os::unix::fs::symlink(&key, cases.join("foreign-private-file")).unwrap();
        assert!(tree(&cases).is_err());
        assert!(run_with_locator(
            &root.join("overflow"),
            &native,
            &["synthetic-explicit".into()],
            u64::MAX,
            8,
            &key
        )
        .is_err());
        assert!(!root.join("overflow.owner").exists());
    }

    #[test]
    fn outer_deadline_reaps_only_the_owned_child_without_claiming_success() {
        let child = Command::new("/bin/sleep").arg("5").spawn().unwrap();
        let mut child = OwnedChild(Some(child));
        let (status, cancelled) = child.wait(Duration::ZERO).unwrap();
        assert!(cancelled);
        assert!(!status.success());
        assert!(child.0.is_none());
    }
}
