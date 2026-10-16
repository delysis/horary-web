//! Collect an interrupted catalogue without rewriting attempts or replaying
//! completed cases. Run the owner through the platform service manager.
#![forbid(unsafe_code)]
use crate::{campaign::FrozenExecution, store};
use fs2::FileExt;
use horary_prompt_program::digest;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
use store::Result;

const START_FREE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const ACTIVE_FREE_BYTES: u64 = 1024 * 1024 * 1024;

fn storage_floor(available: u64, minimum: u64) -> Result<()> {
    if available < minimum {
        return Err(format!("Evidence storage has {available} free bytes; {minimum} required. Preserve this attempt; no further provider calls permitted."));
    }
    Ok(())
}

fn require_storage(directory: &Path, minimum: u64) -> Result<()> {
    let available = fs2::available_space(directory)
        .map_err(|error| format!("Cannot verify evidence storage: {error}"))?;
    storage_floor(available, minimum)
}

fn review_origin_matches(parent: &Path, original: &Path, manifest_sha: &str) -> Result<()> {
    let original = store::canonical(original)?;
    let mut current = store::canonical(parent)?;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current.clone()) {
            return Err("Recovery lineage contains a cycle".into());
        }
        if digest(store::read(&current.join("manifest.json"))?) != manifest_sha {
            return Err("Paid-review recovery lineage changed its frozen manifest".into());
        }
        if current == original {
            return Ok(());
        }
        let lineage = store::json(&current.join("recovery.json"))?;
        if lineage["manifest_sha256"] != manifest_sha {
            return Err("Recovery lineage has an unbound manifest".into());
        }
        current = store::canonical(Path::new(
            lineage["parent"]
                .as_str()
                .ok_or("Missing recovery origin")?,
        ))?;
    }
}

pub struct Options {
    pub parent: PathBuf,
    pub state: PathBuf,
    pub repository: PathBuf,
    pub executable: PathBuf,
    pub expected_executable_sha256: String,
    pub review: Option<Review>,
}

pub struct Review {
    pub state: PathBuf,
    pub completed_review_state: PathBuf,
    pub fixtures: PathBuf,
    pub codex: PathBuf,
    pub distinct_case_budget: usize,
}

#[cfg(unix)]
fn require_absent_worker(parent: &Path) -> Result<()> {
    let receipt = store::json(&parent.join("orphaned-worker.json"))?;
    let pid = receipt["worker_pid"]
        .as_u64()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(rustix::process::Pid::from_raw)
        .ok_or("Missing valid orphan worker PID")?;
    match rustix::process::test_kill_process(pid) {
        Err(rustix::io::Errno::SRCH) => Ok(()),
        _ => Err("Parent worker may still exist; no replacement provider calls permitted".into()),
    }
}
#[cfg(not(unix))]
fn require_absent_worker(_parent: &Path) -> Result<()> {
    Err("Platform worker-absence verification is unavailable; no replacement started".into())
}

#[cfg(unix)]
fn link_reviews(source: &Path, destination: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, destination).map_err(|e| e.to_string())
}
#[cfg(not(unix))]
fn link_reviews(_source: &Path, _destination: &Path) -> Result<()> {
    Err("Read-only paid-review linkage is not supported on this platform".into())
}

fn ids(fixtures: &Value) -> Result<Vec<String>> {
    let ids = fixtures
        .as_array()
        .ok_or("Fixture bank must be an array")?
        .iter()
        .map(|case| {
            case["id"]
                .as_str()
                .filter(|id| store::safe_id(id))
                .map(str::to_owned)
                .ok_or_else(|| "Unsafe or missing fixture ID".into())
        })
        .collect::<Result<Vec<_>>>()?;
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err("Duplicate fixture IDs".into());
    }
    Ok(ids)
}

fn scope_matches(parent: &Value, child: &Value, selected: &[String]) -> Result<()> {
    let mut expected = parent.clone();
    expected["selected_count"] = json!(selected.len());
    expected["selection_filter"] = json!(selected.join(","));
    if expected != *child {
        return Err(
            "Recovery changed frozen source/provider/clock/decoder or selected scope".into(),
        );
    }
    Ok(())
}

// A controller may exit after the child commits an outcome but before importing
// it. Bind that child to the parent's declared scope before selecting its cases.
fn parent_child_attempt(
    parent: &Path,
    manifest: &Value,
    all_ids: &[String],
) -> Result<Option<(PathBuf, BTreeSet<String>)>> {
    let child = parent.join("fresh-attempt");
    if !child.is_dir() {
        return Ok(None);
    }
    if !child.join("manifest.json").is_file() {
        if all_ids
            .iter()
            .any(|id| child.join("cases").join(id).join("outcome.json").is_file())
        {
            return Err("Parent child has a terminal outcome without its frozen manifest".into());
        }
        return Ok(None);
    }
    let recovery = store::json(&parent.join("recovery.json"))?;
    if recovery["manifest_sha256"] != digest(store::read(&parent.join("manifest.json"))?) {
        return Err("Parent child scope is not bound to the frozen parent manifest".into());
    }
    let selected = recovery["replacement_case_ids"]
        .as_array()
        .ok_or("Parent child lacks its declared replacement scope")?
        .iter()
        .map(|id| {
            id.as_str()
                .filter(|id| {
                    store::safe_id(id) && all_ids.iter().any(|known| known.as_str() == *id)
                })
                .map(str::to_owned)
                .ok_or_else(|| "Parent child scope contains an unknown or unsafe case ID".into())
        })
        .collect::<Result<Vec<String>>>()?;
    let selected_set = selected.iter().cloned().collect::<BTreeSet<_>>();
    if selected_set.len() != selected.len() {
        return Err("Parent child scope contains duplicate case IDs".into());
    }
    scope_matches(
        manifest,
        &store::json(&child.join("manifest.json"))?,
        &selected,
    )?;
    for file in ["fixtures.json", "reading-rubrics.json"] {
        if store::read(&child.join(file))? != store::read(&parent.join(file))? {
            return Err(format!("Parent child changed the frozen {file}"));
        }
    }
    for id in all_ids {
        if !selected_set.contains(id) && child.join("cases").join(id).join("outcome.json").is_file()
        {
            return Err(format!(
                "Parent child completed case outside its declared scope: {id}"
            ));
        }
    }
    Ok(Some((child, selected_set)))
}

fn import_retained_case(
    parent: &Path,
    state: &Path,
    fixture: &Value,
    child_attempt: Option<&(PathBuf, BTreeSet<String>)>,
) -> Result<bool> {
    if import_case(parent, state, fixture, "completed_parent_attempt")? {
        return Ok(true);
    }
    if let Some((child, selected)) = child_attempt {
        let id = fixture["id"].as_str().ok_or("Missing case ID")?;
        if selected.contains(id) {
            return import_case(child, state, fixture, "completed_parent_child_attempt");
        }
    }
    Ok(false)
}

fn sources_match(repository: &Path, manifest: &Value) -> Result<()> {
    for (file, expected) in manifest["sources"]
        .as_object()
        .ok_or("Missing frozen source hashes")?
    {
        let path = repository.join(store::safe_relative(file)?);
        if digest(store::read(&path)?) != *expected {
            return Err(format!(
                "Frozen source changed: {file}; no provider work authorized"
            ));
        }
    }
    Ok(())
}

fn tree(root: &Path) -> Result<BTreeMap<String, String>> {
    fn visit(base: &Path, directory: &Path, hashes: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if kind.is_dir() {
                visit(base, &path, hashes)?;
            } else if kind.is_file() {
                let name = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Non-UTF8 evidence path")?
                    .replace('\\', "/");
                hashes.insert(name, digest(store::read(&path)?));
            } else {
                return Err("Evidence tree contains a symlink or special file".into());
            }
        }
        Ok(())
    }
    let mut hashes = BTreeMap::new();
    visit(root, root, &mut hashes)?;
    Ok(hashes)
}

fn copy_tree(source: &Path, destination: &Path, hashes: &BTreeMap<String, String>) -> Result<()> {
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    for (file, sha) in hashes {
        let bytes = store::read(&source.join(store::safe_relative(file)?))?;
        if digest(&bytes) != *sha {
            return Err("Source evidence changed during copying".into());
        }
        store::atomic(&destination.join(file), &bytes, true)?;
    }
    if tree(destination)? != *hashes || tree(source)? != *hashes {
        return Err("Evidence copy did not preserve the complete original tree".into());
    }
    Ok(())
}

fn import_case(source: &Path, state: &Path, fixture: &Value, origin: &str) -> Result<bool> {
    let id = fixture["id"].as_str().ok_or("Missing fixture ID")?;
    let directory = source.join("cases").join(id);
    if !directory.join("outcome.json").is_file() {
        return Ok(false);
    }
    if store::json(&directory.join("fixture.json"))? != *fixture {
        return Err(format!(
            "Case {id} changed its authored inputs or expectations"
        ));
    }
    if store::json(&directory.join("reading-rubric.json"))?
        != store::json(&source.join("reading-rubrics.json"))?[id]
    {
        return Err(format!("Case {id} changed its source rubric"));
    }
    let outcome = store::json(&directory.join("outcome.json"))?;
    if outcome["id"] != id || outcome["full_reading"] != true {
        return Err(format!(
            "Case {id} has conflicting identity or pipeline scope"
        ));
    }
    let hashes = tree(&directory)?;
    for file in [
        "initial.json",
        "first-turn.json",
        "final.json",
        "reading-rubric.json",
        "trace.html",
    ] {
        if !hashes.contains_key(file) {
            return Err(format!("Case {id} lacks terminal witness {file}"));
        }
    }
    let target = state.join("cases").join(id);
    let receipt = state.join("case-origins").join(format!("{id}.json"));
    if target.exists() {
        if tree(&target)? != hashes {
            return Err(format!("Conflicting or partial selected case {id}"));
        }
    } else {
        // A whole case appears atomically, after all immutable witnesses copy.
        let staging = tempfile::tempdir_in(state.join("cases")).map_err(|e| e.to_string())?;
        let staged_case = staging.path().join(id);
        copy_tree(&directory, &staged_case, &hashes)?;
        fs::rename(&staged_case, &target).map_err(|e| e.to_string())?;
    }
    let lineage = json!({"case_id":id,"origin":origin,"source_directory":directory,
        "source_manifest_sha256":digest(store::read(&source.join("manifest.json"))?),
        "files":hashes,"selected_outcome_unchanged":true});
    if receipt.exists() {
        if store::json(&receipt)? != lineage {
            return Err(format!("Case lineage changed: {id}"));
        }
    } else {
        store::atomic_json(&receipt, &lineage, true)?;
    }
    Ok(true)
}

fn outcomes(state: &Path, fixtures: &Value) -> Result<Vec<Value>> {
    fixtures
        .as_array()
        .ok_or("Missing fixtures")?
        .iter()
        .filter(|case| {
            state
                .join("cases")
                .join(case["id"].as_str().unwrap_or(""))
                .join("outcome.json")
                .is_file()
        })
        .map(|case| {
            store::json(
                &state
                    .join("cases")
                    .join(case["id"].as_str().ok_or("Missing ID")?)
                    .join("outcome.json"),
            )
        })
        .collect()
}

fn report(state: &Path, manifest: &Value, fixtures: &Value, status: &str) -> Result<()> {
    let cases = outcomes(state, fixtures)?;
    let mut hurdles = BTreeMap::<String, BTreeMap<String, usize>>::new();
    for case in &cases {
        for gate in ["classification", "elicitation", "extraction", "reading"] {
            let value = case["hurdles"][gate]["status"]
                .as_str()
                .ok_or("Missing native hurdle status")?;
            *hurdles
                .entry(gate.into())
                .or_default()
                .entry(value.into())
                .or_default() += 1;
        }
    }
    let result = json!({"manifest":manifest,"campaign_state":{"status":status,
        "collection":"explicit recovered collection; original worker was interrupted"},
        "completed":cases.len(),"hurdle_counts":hurdles,"cases":cases,
        "reading_semantic_review":"Pending independent source review; native structure is never a source-correctness pass",
        "collection_provenance":"recovery.json; each selected case has an unchanged tree and case-origins receipt",
        "prior_interrupted_attempts":"Preserved in the original collection; their missing provider results remain uncertain"});
    store::atomic_json(&state.join("report.json"), &result, false)?;
    let mut html = format!("<!doctype html><meta charset=utf-8><meta http-equiv=refresh content=30><title>Horary catalogue</title><style>body{{font:18px system-ui;max-width:1100px;margin:50px auto;padding:20px;background:#182126;color:#e9e0ca}}table{{width:100%;border-collapse:collapse}}td,th{{padding:12px;text-align:left;border-bottom:1px solid #435158}}a{{color:#d5c29a}}</style><h1>Horary catalogue</h1><p>{} / {} collected · {status}</p><p>Frozen hosted Gemma application procedure. This is a recovered collection; previous interrupted attempts remain preserved. Four hurdles are separate. A completed worksheet does not qualify an interpretation.</p><table><tr><th>Case</th><th>Classify</th><th>Elicit</th><th>Extract</th><th>Reading</th></tr>",result["completed"],manifest["selected_count"]);
    for case in result["cases"].as_array().ok_or("Missing cases")? {
        let id = case["id"].as_str().ok_or("Missing case ID")?;
        html.push_str(&format!(
            "<tr><td><a href=cases/{id}/trace.html>{id}</a></td>"
        ));
        for gate in ["classification", "elicitation", "extraction", "reading"] {
            // Only native enumerated statuses appear in this table.
            let value = case["hurdles"][gate]["status"]
                .as_str()
                .ok_or("Missing status")?;
            if !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err("Unsafe status label".into());
            }
            html.push_str(&format!("<td>{value}</td>"));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    store::atomic(&state.join("review.html"), html.as_bytes(), false)
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn run(options: Options) -> Result<()> {
    // This entry creates a new attempt collection. It never secretly resumes a
    // submitted provider call, overwrites evidence, or acquires local weights.
    let parent = store::canonical(&options.parent)?;
    let destination = store::canonical(
        options
            .state
            .parent()
            .ok_or("State needs a parent directory")?,
    )?
    .join(options.state.file_name().ok_or("State needs a name")?);
    if destination.starts_with(&parent)
        || destination.starts_with(store::canonical(&options.repository)?)
    {
        return Err(
            "Keep recovered collection outside original evidence and source checkout".into(),
        );
    }
    require_absent_worker(&options.parent)?;
    if digest(store::read(&options.executable)?) != options.expected_executable_sha256 {
        return Err("Frozen executable identity changed; no provider calls started".into());
    }
    require_storage(
        destination.parent().ok_or("Missing evidence parent")?,
        START_FREE_BYTES,
    )?;
    let origin_id = digest(format!(
        "{}:{}",
        parent.display(),
        digest(store::read(&parent.join("manifest.json"))?)
    ));
    let owner_path = parent
        .parent()
        .ok_or("Campaign needs a parent directory")?
        .join(format!("catalogue-owner-{origin_id}.lock"));
    let owner = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&owner_path)
        .map_err(|e| e.to_string())?;
    owner
        .try_lock_exclusive()
        .map_err(|_| "Another controller owns the original campaign".to_owned())?;
    let assignment = owner_path.with_extension("json");
    if assignment.exists() {
        return Err("This original campaign already has a recovery assignment; preserve that attempt and recover its latest collection explicitly".into());
    }
    store::atomic_json(
        &assignment,
        &json!({"parent":parent,"destination":destination,"owner_pid":std::process::id(),
        "native_executable_sha256":options.expected_executable_sha256,"automatic_reassignment":false}),
        true,
    )?;
    fs::create_dir(&options.state)
        .map_err(|e| format!("Keep prior collection; choose fresh state: {e}"))?;
    let lock = fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(options.state.join("collection.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock_exclusive().map_err(|e| e.to_string())?;
    let result = run_locked(&options);
    if let Err(error) = &result {
        let _ = store::atomic_json(
            &options.state.join("interrupted.json"),
            &json!({"status":"infrastructure_interrupted","reason":error,"semantic_grades_unchanged":true}),
            true,
        );
        if let (Ok(manifest), Ok(fixtures)) = (
            store::json(&options.state.join("manifest.json")),
            store::json(&options.state.join("fixtures.json")),
        ) {
            let _ = report(
                &options.state,
                &manifest,
                &fixtures,
                "infrastructure_interrupted",
            );
        }
    }
    result?;
    if let Some(review) = &options.review {
        run_review(&options, review)?;
    }
    Ok(())
}

fn run_review(options: &Options, review: &Review) -> Result<()> {
    // Paid ledgers keep their original campaign identity. A symlink permits
    // reading the verified completed jobs, never rewriting that identity.
    let grades = crate::completed_judgments(&review.completed_review_state)?;
    let proposals = crate::completed_proposals(&review.completed_review_state)?;
    if grades.is_empty() || proposals.is_empty() {
        return Err("No verified completed teacher/writer results to reuse".into());
    }
    let manifest_sha = digest(store::read(&options.state.join("manifest.json"))?);
    let ledger = store::json(&review.completed_review_state.join("ledger.json"))?;
    if ledger["config"]["manifest_sha256"] != manifest_sha {
        return Err("Completed paid review belongs to a different original campaign".into());
    }
    review_origin_matches(
        &options.parent,
        Path::new(
            ledger["config"]["campaign"]
                .as_str()
                .ok_or("Missing review origin")?,
        ),
        &manifest_sha,
    )?;
    for (id, job) in ledger["jobs"].as_object().ok_or("Missing paid jobs")? {
        if job["status"] != "complete" || !store::safe_id(id) {
            return Err("Unsettled paid jobs cannot be reused or resubmitted".into());
        }
        let packet = store::json(
            &review
                .completed_review_state
                .join("jobs")
                .join(id)
                .join("packet.json"),
        )?;
        for case in packet["cases"]
            .as_array()
            .ok_or("Missing paid packet cases")?
        {
            for reference in case["files"]
                .as_array()
                .ok_or("Missing paid file witnesses")?
            {
                let file = store::safe_relative(
                    reference["file"].as_str().ok_or("Missing paid file name")?,
                )?;
                for root in [&options.parent, &options.state] {
                    let bytes = store::read(&root.join(file))?;
                    if digest(&bytes) != reference["sha256"] || reference["bytes"] != bytes.len() {
                        return Err(
                            "Copied case no longer matches the original paid review witness".into(),
                        );
                    }
                }
            }
        }
    }
    for judgment in &grades {
        for case in judgment["reviews"]
            .as_array()
            .ok_or("Missing paid case reviews")?
        {
            let id = case["case_id"]
                .as_str()
                .filter(|id| store::safe_id(id))
                .ok_or("Unsafe review case ID")?;
            if tree(&options.parent.join("cases").join(id))?
                != tree(&options.state.join("cases").join(id))?
            {
                return Err("Recovered teacher case differs from paid original".into());
            }
        }
    }
    fs::create_dir(&review.state)
        .map_err(|e| format!("Preserve existing optimization state: {e}"))?;
    link_reviews(
        &review.completed_review_state,
        &review.state.join("training-review"),
    )?;
    store::atomic_json(
        &review.state.join("reused-review-receipt.json"),
        &json!({
        "source":review.completed_review_state,"ledger_sha256":digest(store::read(&review.completed_review_state.join("ledger.json"))?),
        "discovery":options.state,"manifest_sha256":manifest_sha,"case_trees_hash_matched":true,
        "repeat_teacher_or_writer_submission_authorized":false,"distinct_case_budget":review.distinct_case_budget}),
        true,
    )?;
    let binary = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name(if cfg!(windows) {
            "horary-optimize.exe"
        } else {
            "horary-optimize"
        });
    // The experiment owner enforces the provider cooldown before each child.
    println!("Discovery collection closed; starting the queued paired comparison.");
    let status = Command::new(binary)
        .current_dir(&options.repository)
        .arg("--discovery")
        .arg(&options.state)
        .arg("--fixtures")
        .arg(&review.fixtures)
        .arg("--native-executable")
        .arg(&options.executable)
        .arg("--state")
        .arg(&review.state)
        .arg("--codex")
        .arg(&review.codex)
        .args(["--watch", "--batch-size", "1", "--review-jobs", "1"])
        .arg("--max-native-cases")
        .arg(review.distinct_case_budget.to_string())
        .args([
            "--judge-seconds",
            "600",
            "--case-seconds",
            "1800",
            "--max-calls",
            "28",
        ])
        .stdin(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    store::atomic_json(
        &options.state.join("optimization-exit.json"),
        &json!({"code":status.code(),"round":review.state,"no_automatic_retry":true}),
        true,
    )?;
    if !status.success() {
        return Err("Paired optimization stopped; its partial and paid receipts remain intact, without retry".into());
    }
    Ok(())
}

fn run_locked(options: &Options) -> Result<()> {
    let manifest_bytes = store::read(&options.parent.join("manifest.json"))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
    let fixtures = store::json(&options.parent.join("fixtures.json"))?;
    let all_ids = ids(&fixtures)?;
    let execution = FrozenExecution::from_manifest(&manifest, None)?;
    if !execution.full_reading
        || manifest["selected_count"] != all_ids.len()
        || manifest["selection_filter"] != ""
    {
        return Err("Recovery requires a complete selected hosted reading catalogue".into());
    }
    sources_match(&options.repository, &manifest)?;
    fs::create_dir(options.state.join("cases")).map_err(|e| e.to_string())?;
    for file in ["manifest.json", "fixtures.json", "reading-rubrics.json"] {
        store::atomic(
            &options.state.join(file),
            &store::read(&options.parent.join(file))?,
            true,
        )?;
    }
    let mut reused = Vec::new();
    let mut prior_partial = Vec::new();
    let child_attempt = parent_child_attempt(&options.parent, &manifest, &all_ids)?;
    for fixture in fixtures.as_array().ok_or("Missing fixture bank")? {
        let id = fixture["id"].as_str().ok_or("Missing case ID")?;
        if import_retained_case(
            &options.parent,
            &options.state,
            fixture,
            child_attempt.as_ref(),
        )? {
            reused.push(id.to_owned());
        } else {
            for directory in [
                options.parent.join("cases").join(id),
                options.parent.join("fresh-attempt/cases").join(id),
            ] {
                if directory.is_dir() {
                    prior_partial.push(json!({"id":id,"directory":directory,
                        "files":tree(&directory)?,
                        "provider_completion":"Earlier requests and results remain unchanged; missing provider results remain uncertain",
                        "replacement":"Explicit whole-case replacement in a new child attempt; previous evidence is retained"}));
                }
            }
        }
    }
    let remaining = all_ids
        .into_iter()
        .filter(|id| !reused.contains(id))
        .collect::<Vec<_>>();
    store::atomic_json(
        &options.state.join("recovery.json"),
        &json!({"version":1,
        "parent":options.parent,"manifest_sha256":digest(&manifest_bytes),
        "native_executable":options.executable,"native_executable_sha256":digest(store::read(&options.executable)?),
        "reused_case_ids":reused,"replacement_case_ids":remaining,"prior_partial_attempts":prior_partial,
        "source_repository":options.repository,"credentials_in_receipts":false,
        "controller_storage_floor":{"before_launch_bytes":START_FREE_BYTES,"while_active_bytes":ACTIVE_FREE_BYTES},
        "review_qualification":"No completed paid review is repeated or changed by collection"}),
        true,
    )?;
    report(&options.state, &manifest, &fixtures, "running")?;
    if !remaining.is_empty() {
        let child_dir = options.state.join("fresh-attempt");
        require_storage(&options.state, START_FREE_BYTES)?;
        if digest(store::read(&options.executable)?) != options.expected_executable_sha256 {
            return Err("Frozen executable changed before submission".into());
        }
        let mut command = Command::new(&options.executable);
        execution.apply_environment(&mut command)?;
        command
            .current_dir(&options.repository)
            .args([
                "--exact",
                "elicitation_eval::real_model_catalogue_campaign",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HORARY_EVAL_EVIDENCE", &child_dir)
            .env("HORARY_EVAL_FILTER", remaining.join(","))
            .env(
                "HORARY_EVAL_CASE_SECONDS",
                manifest["case_deadline_seconds"]
                    .as_u64()
                    .ok_or("Missing deadline")?
                    .to_string(),
            )
            .env(
                "HORARY_EVAL_MAX_CALLS",
                manifest["case_max_calls"]
                    .as_u64()
                    .ok_or("Missing call budget")?
                    .to_string(),
            )
            .env(
                "HORARY_EVAL_PHASE",
                manifest["campaign_phase"].as_str().ok_or("Missing phase")?,
            )
            .env_remove("HORARY_EVAL_PROGRAM")
            .env_remove("HORARY_EVAL_ORIGIN_SHA256")
            .env_remove("HORARY_EVAL_FIXED_CANDIDATE_SHA256")
            .env_remove("HORARY_EVAL_EXPERIMENT_ROLE")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(options.state.join("native.stdout.log"))
                    .map_err(|e| e.to_string())?,
            ))
            .stderr(Stdio::from(
                fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(options.state.join("native.stderr.log"))
                    .map_err(|e| e.to_string())?,
            ));
        let mut child = OwnedChild(command.spawn().map_err(|e| e.to_string())?);
        store::atomic_json(
            &options.state.join("submitted.json"),
            &json!({"pid":child.0.id(),"selected_ids":remaining,
            "owner_pid":std::process::id(),"owner":"platform service, separate from the chat turn","remote_call_retries":"Only the frozen application retry policy applies"}),
            true,
        )?;
        let mut collected = BTreeSet::new();
        loop {
            require_storage(&options.state, ACTIVE_FREE_BYTES)?;
            let exit = child.0.try_wait().map_err(|e| e.to_string())?;
            if child_dir.join("manifest.json").is_file() {
                scope_matches(
                    &manifest,
                    &store::json(&child_dir.join("manifest.json"))?,
                    &remaining,
                )?;
                let mut changed = false;
                for fixture in fixtures.as_array().ok_or("Missing fixtures")? {
                    let id = fixture["id"].as_str().ok_or("Missing ID")?;
                    if remaining.iter().any(|v| v == id)
                        && !collected.contains(id)
                        && import_case(
                            &child_dir,
                            &options.state,
                            fixture,
                            "explicit_fresh_whole_case_attempt",
                        )?
                    {
                        collected.insert(id.to_owned());
                        changed = true;
                        println!(
                            "{}",
                            json!({"event":"case_collected","id":id,"total":reused.len()+collected.len()})
                        );
                    }
                }
                if changed {
                    report(&options.state, &manifest, &fixtures, "running")?;
                }
            }
            if let Some(exit) = exit {
                store::atomic_json(
                    &options.state.join("child-exit.json"),
                    &json!({"code":exit.code(),"completed_marker":child_dir.join("completed.json").is_file()}),
                    true,
                )?;
                if !child_dir.join("completed.json").is_file() || collected.len() != remaining.len()
                {
                    return Err("Fresh child stopped before collecting every planned outcome; keep partial attempts, no silent restart".into());
                }
                break;
            }
            thread::sleep(Duration::from_secs(2));
        }
        sources_match(&options.repository, &manifest)?;
    }
    if outcomes(&options.state, &fixtures)?.len()
        != manifest["selected_count"].as_u64().ok_or("Missing count")? as usize
    {
        return Err("Recovered catalogue is incomplete".into());
    }
    report(&options.state, &manifest, &fixtures, "completed")?;
    store::atomic_json(
        &options.state.join("completed.json"),
        &json!({"status":"completed",
        "meaning":"All selected cases have terminal outcomes, including failures; interpretations retain independent gates",
        "manifest_sha256":digest(manifest_bytes),"selected_count":fixtures.as_array().map(Vec::len),"recovery":"recovery.json"}),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopped_after_child_outcome(root: &Path) -> (PathBuf, Value, Value) {
        let parent = root.join("parent");
        let child = parent.join("fresh-attempt");
        fs::create_dir_all(child.join("cases/a")).unwrap();
        let fixture = json!({"id":"a","words":"My ring"});
        let manifest = json!({"selected_count":2,"selection_filter":"","model":{"id":"frozen"}});
        store::atomic_json(&parent.join("manifest.json"), &manifest, true).unwrap();
        store::atomic_json(
            &parent.join("recovery.json"),
            &json!({"manifest_sha256":digest(store::read(&parent.join("manifest.json")).unwrap()),"replacement_case_ids":["a"]}),
            true,
        )
        .unwrap();
        let mut child_manifest = manifest.clone();
        child_manifest["selected_count"] = json!(1);
        child_manifest["selection_filter"] = json!("a");
        store::atomic_json(&child.join("manifest.json"), &child_manifest, true).unwrap();
        for directory in [&parent, &child] {
            store::atomic_json(
                &directory.join("fixtures.json"),
                &json!([fixture,{"id":"b"}]),
                true,
            )
            .unwrap();
            store::atomic_json(
                &directory.join("reading-rubrics.json"),
                &json!({"a":{},"b":{}}),
                true,
            )
            .unwrap();
        }
        store::atomic_json(&child.join("cases/a/fixture.json"), &fixture, true).unwrap();
        store::atomic_json(
            &child.join("cases/a/outcome.json"),
            &json!({"id":"a","full_reading":true,"hurdles":{"extraction":{"status":"fail"}}}),
            true,
        )
        .unwrap();
        for file in [
            "initial.json",
            "first-turn.json",
            "final.json",
            "reading-rubric.json",
            "trace.html",
        ] {
            store::atomic(&child.join("cases/a").join(file), b"{}", true).unwrap();
        }
        (parent, manifest, fixture)
    }

    #[test]
    fn a_committed_child_outcome_survives_its_controllers_missing_import() {
        let root = tempfile::tempdir().unwrap();
        let (parent, manifest, fixture) = stopped_after_child_outcome(root.path());
        let state = root.path().join("next-recovery");
        fs::create_dir_all(state.join("cases")).unwrap();
        let all_ids = ids(&store::json(&parent.join("fixtures.json")).unwrap()).unwrap();
        let child = parent_child_attempt(&parent, &manifest, &all_ids).unwrap();
        assert!(!parent.join("cases/a/outcome.json").exists());
        let original = tree(&parent.join("fresh-attempt/cases/a")).unwrap();
        assert!(import_retained_case(&parent, &state, &fixture, child.as_ref()).unwrap());
        let mut replacements = Vec::new();
        for case in store::json(&parent.join("fixtures.json"))
            .unwrap()
            .as_array()
            .unwrap()
        {
            if !import_retained_case(&parent, &state, case, child.as_ref()).unwrap() {
                replacements.push(case["id"].as_str().unwrap().to_owned());
            }
        }
        assert_eq!(replacements, ["b"]); // Completed failure a must not be paid for again.
        assert_eq!(tree(&state.join("cases/a")).unwrap(), original);
        assert_eq!(
            tree(&parent.join("fresh-attempt/cases/a")).unwrap(),
            original
        );
        let origin = store::json(&state.join("case-origins/a.json")).unwrap();
        assert_eq!(origin["origin"], "completed_parent_child_attempt");
        assert_eq!(origin["selected_outcome_unchanged"], true);
    }

    #[test]
    fn child_outcome_reuse_refuses_changed_scope_fixtures_and_unbound_completion() {
        let root = tempfile::tempdir().unwrap();
        let (parent, manifest, _) = stopped_after_child_outcome(root.path());
        let all_ids = vec!["a".to_owned(), "b".to_owned()];
        let child = parent.join("fresh-attempt");
        let manifest_path = child.join("manifest.json");
        let frozen_manifest = store::read(&manifest_path).unwrap();
        let mut changed = store::json(&manifest_path).unwrap();
        changed["model"]["id"] = json!("different-provider-model");
        store::atomic_json(&manifest_path, &changed, false).unwrap();
        assert!(parent_child_attempt(&parent, &manifest, &all_ids)
            .unwrap_err()
            .contains("changed frozen"));
        store::atomic(&manifest_path, &frozen_manifest, false).unwrap();
        for file in ["fixtures.json", "reading-rubrics.json"] {
            let path = child.join(file);
            let frozen = store::read(&path).unwrap();
            store::atomic(&path, b"changed", false).unwrap();
            assert!(parent_child_attempt(&parent, &manifest, &all_ids)
                .unwrap_err()
                .contains(file));
            store::atomic(&path, &frozen, false).unwrap();
        }
        fs::create_dir_all(child.join("cases/b")).unwrap();
        store::atomic(&child.join("cases/b/outcome.json"), b"{}", true).unwrap();
        assert!(parent_child_attempt(&parent, &manifest, &all_ids)
            .unwrap_err()
            .contains("outside its declared scope"));
        fs::remove_file(child.join("cases/b/outcome.json")).unwrap();
        fs::remove_file(manifest_path).unwrap();
        assert!(parent_child_attempt(&parent, &manifest, &all_ids)
            .unwrap_err()
            .contains("without its frozen manifest"));
    }

    #[test]
    fn storage_guard_stops_before_the_reserve_is_consumed() {
        assert!(storage_floor(START_FREE_BYTES, START_FREE_BYTES).is_ok());
        let error = storage_floor(ACTIVE_FREE_BYTES - 1, ACTIVE_FREE_BYTES).unwrap_err();
        assert!(error.contains("no further provider calls permitted"));
        assert!(error.contains(&(ACTIVE_FREE_BYTES - 1).to_string()));
    }

    #[test]
    fn paid_review_origin_can_follow_only_a_frozen_recovery_chain() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let recovered = root.path().join("recovered");
        let twice = root.path().join("twice");
        let bytes = br#"{"source":"same immutable campaign"}"#;
        let sha = digest(bytes);
        for directory in [&original, &recovered, &twice] {
            fs::create_dir(directory).unwrap();
            store::atomic(&directory.join("manifest.json"), bytes, true).unwrap();
        }
        for (child, parent) in [(&recovered, &original), (&twice, &recovered)] {
            store::atomic_json(
                &child.join("recovery.json"),
                &json!({"parent":parent,"manifest_sha256":sha}),
                true,
            )
            .unwrap();
        }
        review_origin_matches(&twice, &original, &sha).unwrap();
        store::atomic(&recovered.join("manifest.json"), b"changed", false).unwrap();
        assert!(review_origin_matches(&twice, &original, &sha)
            .unwrap_err()
            .contains("changed its frozen manifest"));
        store::atomic(&recovered.join("manifest.json"), bytes, false).unwrap();
        store::atomic_json(
            &recovered.join("recovery.json"),
            &json!({"parent":twice,"manifest_sha256":sha}),
            false,
        )
        .unwrap();
        assert!(review_origin_matches(&twice, &original, &sha)
            .unwrap_err()
            .contains("cycle"));
    }

    #[test]
    fn recovery_pins_all_scope_except_its_declared_selection() {
        let parent = json!({"selected_count":2,"selection_filter":"","model":{"id":"gemma","temperature":0},"frozen_clock":{"local":"noon"},"sources":{"rules":"sha"}});
        let selected = vec!["a".to_owned()];
        let mut child = parent.clone();
        child["selected_count"] = json!(1);
        child["selection_filter"] = json!("a");
        scope_matches(&parent, &child, &selected).unwrap();
        for pointer in [
            "/model/temperature",
            "/frozen_clock/local",
            "/sources/rules",
        ] {
            let mut changed = child.clone();
            *changed.pointer_mut(pointer).unwrap() = json!("drift");
            assert!(scope_matches(&parent, &changed, &selected).is_err());
        }
    }
    #[test]
    fn duplicate_ids_and_unsafe_paths_never_acquire_a_worker() {
        assert!(ids(&json!([{"id":"a"},{"id":"a"}])).is_err());
        assert!(ids(&json!([{"id":"../outside"}])).is_err());
    }
    #[test]
    fn a_completed_case_is_reconciled_without_rewriting_failure_or_replaying_it() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("parent");
        let state = dir.path().join("recovered");
        fs::create_dir_all(state.join("cases")).unwrap();
        fs::create_dir_all(source.join("cases/a")).unwrap();
        let fixture = json!({"id":"a","words":"My ring"});
        store::atomic_json(
            &source.join("manifest.json"),
            &json!({"model":"frozen"}),
            true,
        )
        .unwrap();
        store::atomic_json(&source.join("reading-rubrics.json"), &json!({"a":{}}), true).unwrap();
        store::atomic_json(&source.join("cases/a/fixture.json"), &fixture, true).unwrap();
        let failed = json!({"id":"a","full_reading":true,"hurdles":{"extraction":{"status":"fail"}},"execution_status":"completed"});
        store::atomic_json(&source.join("cases/a/outcome.json"), &failed, true).unwrap();
        for file in [
            "initial.json",
            "first-turn.json",
            "final.json",
            "reading-rubric.json",
            "trace.html",
        ] {
            store::atomic(&source.join("cases/a").join(file), b"{}", true).unwrap();
        }
        assert!(import_case(&source, &state, &fixture, "parent").unwrap());
        let before = tree(&state.join("cases/a")).unwrap();
        fs::remove_file(state.join("case-origins/a.json")).unwrap(); // Crash after the case commit, before its derived lineage.
        assert!(import_case(&source, &state, &fixture, "parent").unwrap());
        assert_eq!(tree(&state.join("cases/a")).unwrap(), before);
        assert_eq!(
            store::json(&state.join("cases/a/outcome.json")).unwrap(),
            failed
        );
        store::atomic(&source.join("cases/a/final.json"), b"changed", false).unwrap();
        assert!(import_case(&source, &state, &fixture, "parent").is_err());
        assert!(!import_case(&source, &state, &json!({"id":"never_started"}), "parent").unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn an_active_parent_worker_cannot_be_replaced() {
        let parent = tempfile::tempdir().unwrap();
        store::atomic_json(
            &parent.path().join("orphaned-worker.json"),
            &json!({"worker_pid":std::process::id()}),
            true,
        )
        .unwrap();
        assert!(require_absent_worker(parent.path()).is_err());
    }
}
