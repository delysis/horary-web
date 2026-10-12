//! Verified transport-cache carriage into a separately planned conventional queue.
//! No partial optimizer state, grades or development traces enter reflection.
use crate::{google, queue::Plan};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileSeal {
    pub path: PathBuf,
    pub sha256: String,
}
impl FileSeal {
    pub fn read(path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.canonicalize()?,
            sha256: google::digest(fs::read(path)?),
        })
    }
    fn verify(&self) -> Result<()> {
        ensure!(
            google::digest(fs::read(&self.path)?) == self.sha256,
            "Sealed continuation evidence changed: {}",
            self.path.display()
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub maximum: usize,
    pub inherited: usize,
    pub remaining: usize,
    pub success_caches: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub before: String,
    pub after: String,
}
/// The one completed student request explicitly authorized by a closed-owner
/// receipt. This is a transport slot, never a model-answer repair.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudentRecovery {
    pub version: u32,
    pub model: String,
    pub original_key: String,
    pub original_request_sha256: String,
    pub closure_receipt_sha256: String,
    pub recovery_key: String,
    pub witnesses: BTreeMap<String, String>,
    pub maximum_new_attempts: usize,
    pub first_physical_ordinal: usize,
    pub last_physical_ordinal: usize,
    pub original_physical_cap: usize,
}

impl StudentRecovery {
    pub fn expected_key(&self) -> String {
        google::digest(format!(
            "horary-student-completed-transient-recovery-v1\0{}\0{}\0{}",
            self.original_key, self.original_request_sha256, self.closure_receipt_sha256
        ))
    }
    /// Check the local imported carrier only. No network operation or old-key
    /// submission is permitted; every fresh artifact uses `recovery_key`.
    pub fn verify_witnesses(&self, directory: &Path) -> Result<()> {
        ensure!(
            self.version == 1
                && self.model == google::STUDENT
                && self.original_key.len() == 64
                && self.original_key.bytes().all(|b| b.is_ascii_hexdigit())
                && self.original_request_sha256.len() == 64
                && self
                    .original_request_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                && self.closure_receipt_sha256.len() == 64
                && self
                    .closure_receipt_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                && self.recovery_key == self.expected_key()
                && (1..=3).contains(&self.maximum_new_attempts)
                && self.first_physical_ordinal > 0
                && self
                    .first_physical_ordinal
                    .checked_add(self.maximum_new_attempts - 1)
                    == Some(self.last_physical_ordinal)
                && self.last_physical_ordinal <= self.original_physical_cap,
            "Invalid bounded student recovery identity or cumulative span"
        );
        for (name, hash) in &self.witnesses {
            ensure!(
                google::digest(fs::read(directory.join(safe_relative(name)?))?) == *hash,
                "Student recovery witness changed: {name}"
            );
        }
        let expected_request = format!("{}.request.json", self.original_key);
        ensure!(
            self.witnesses.get(&expected_request) == Some(&self.original_request_sha256),
            "Student recovery lacks its exact original request seal"
        );
        for suffix in ["response.json", "raw.json", "receipt.json"] {
            ensure!(
                !directory
                    .join(format!("{}.{suffix}", self.original_key))
                    .exists(),
                "Student recovery cannot replace a completed or parsed model answer"
            );
        }
        validate_student_failure(directory, &self.original_key, &self.witnesses)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub parent_campaign: PathBuf,
    pub closure_receipt: FileSeal,
    pub parent_plan: FileSeal,
    pub parent_guard: FileSeal,
    pub parent_executable_sha256: String,
    pub new_executable_sha256: String,
    pub new_sources_sha256: BTreeMap<String, String>,
    pub source_changes: BTreeMap<String, Change>,
    pub reviewed_source_changes: String,
    pub changed_reading_datasets: BTreeMap<String, Change>,
    pub student: Budget,
    pub teacher: Budget,
    pub teacher_start_route: usize,
    pub teacher_start_model: String,
    pub known_unusable_teacher_review: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub student_recovery: Option<StudentRecovery>,
    pub imported_cache_files: BTreeMap<String, String>,
    pub policy: String,
}
fn value(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn safe_relative(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    ensure!(
        !name.is_empty() && path.components().all(|c| matches!(c, Component::Normal(_))),
        "Unsafe evidence path"
    );
    Ok(path)
}
fn validate_student_failure(
    directory: &Path,
    key: &str,
    witnesses: &BTreeMap<String, String>,
) -> Result<()> {
    let failure_name = format!("{key}.failure.json");
    ensure!(
        witnesses.contains_key(&failure_name),
        "Student recovery has no sealed completed failure"
    );
    let failure = value(&directory.join(failure_name))?;
    ensure!(
        failure["response_complete"] == true
            && failure["automatic_retry"] == false
            && failure["disposition"] == "quota_or_unavailable",
        "Only a settled transient HTTP failure can authorize this student slot"
    );
    let count = failure["physical_submissions"]
        .as_u64()
        .context("No settled student-attempt count")? as usize;
    ensure!((1..=3).contains(&count), "Invalid old student attempt span");
    let logical_bytes = fs::read(directory.join(format!("{key}.request.json")))?;
    let logical: Value = serde_json::from_slice(&logical_bytes)?;
    ensure!(
        logical["model"] == google::STUDENT,
        "Wrong failed student model"
    );
    let expected_attempts = (1..=count)
        .map(|attempt| format!("{key}.attempt-{attempt}.request.json"))
        .collect::<std::collections::BTreeSet<_>>();
    let actual_attempts = witnesses
        .keys()
        .filter(|name| name.contains(".attempt-") && name.ends_with(".request.json"))
        .cloned()
        .collect();
    ensure!(
        expected_attempts == actual_attempts,
        "Student failure attempt set differs"
    );
    let mut previous = None;
    for attempt in 1..=count {
        let stem = format!("{key}.attempt-{attempt}");
        for suffix in ["request.json", "result.json", "body"] {
            ensure!(
                witnesses.contains_key(&format!("{stem}.{suffix}")),
                "Student failure lacks a sealed request/result/body witness"
            );
        }
        let request = value(&directory.join(format!("{stem}.request.json")))?;
        let result = value(&directory.join(format!("{stem}.result.json")))?;
        let body = fs::read(directory.join(format!("{stem}.body")))?;
        let status = result["http_status"]
            .as_u64()
            .context("Missing completed HTTP status")?;
        let ordinal = request["physical_call"]
            .as_u64()
            .context("Missing physical ordinal")?;
        if status == 429 {
            let hints = google::quota_hints(
                &serde_json::from_slice(&body)?,
                result["provider_quota_advice"]["retry_after_header"].as_str(),
                google::STUDENT,
            );
            ensure!(
                hints["zero_quota"] != true && hints["daily_quota"] != true,
                "Zero or daily quota cannot authorize this student transient slot"
            );
        }
        ensure!(
            matches!(status, 429 | 500 | 502 | 503 | 504)
                && result["response_complete"] == true
                && result["body_sha256"] == google::digest(&body)
                && request["model"] == google::STUDENT
                && request["logical_key"] == key
                && result["logical_key"] == key
                && request["attempt"] == attempt
                && result["attempt"] == attempt
                && result["physical_call"] == ordinal
                && ordinal > 0
                && previous.is_none_or(|old| ordinal == old + 1)
                && request["request_sha256"] == google::digest(&logical_bytes)
                && request["body"] == logical["body"]
                && request["body_sha256"] == google::digest(serde_json::to_vec(&logical["body"])?),
            "Student failure is incomplete, mismatched, non-transient or includes a model answer"
        );
        if attempt == count {
            ensure!(
                result["automatic_retry"] == false && failure["http_status"] == status,
                "Student failure does not match its final settled attempt"
            );
        }
        previous = Some(ordinal);
    }
    Ok(())
}
fn carrier_files(directory: &Path, prefix: &str) -> Result<std::collections::BTreeSet<String>> {
    let mut files = std::collections::BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "Carrier evidence may not be symlinked");
        let name = format!(
            "{prefix}/{}",
            entry
                .file_name()
                .to_str()
                .context("Non-UTF8 carrier artifact")?
        );
        if kind.is_dir() {
            files.extend(carrier_files(&entry.path(), &name)?);
        } else {
            ensure!(kind.is_file(), "Unsupported carrier artifact");
            files.insert(name);
        }
    }
    Ok(files)
}
fn absent(pids: &Value) -> Result<()> {
    let ids = pids
        .as_array()
        .context("No exact closed-owner PID list")?
        .iter()
        .map(|v| v.as_u64().context("Invalid PID").map(|n| n.to_string()))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        ids.len() == 2,
        "Require the exact owner and native child PID"
    );
    let result = Command::new("ps")
        .args(["-p", &ids.join(","), "-o", "pid="])
        .output()?;
    ensure!(
        result.status.code() == Some(1) && result.stdout.iter().all(|c| c.is_ascii_whitespace()),
        "A predecessor PID exists; continuation is withheld"
    );
    Ok(())
}
fn verified_audit(seal: &FileSeal, campaign: &Path) -> Result<Value> {
    seal.verify()?;
    let audit = value(&seal.path)?;
    ensure!(
        audit["version"] == 1 && audit["campaign"].as_str().map(Path::new) == Some(campaign),
        "Closure receipt belongs to a different campaign"
    );
    ensure!(
        audit["owner_exit"]["code"] == 1
            && audit["execute_exit"]["code"] == 1
            && audit["completed_jobs"] == 0
            && audit["later_jobs_executed"] == false
            && audit["raw_answers_preserved"] == true,
        "Only this closed, zero-complete-job transport continuation is supported"
    );
    absent(&audit["exact_pid_absence"]["pids"])?;
    let seals = audit["source_seals"]
        .as_object()
        .context("No closure evidence seals")?;
    for group in ["student", "teacher"] {
        let prefix = format!("run/{group}");
        let actual = carrier_files(&campaign.join(&prefix), &prefix)?;
        let sealed = seals
            .keys()
            .filter(|name| name.starts_with(&format!("{prefix}/")))
            .cloned()
            .collect();
        ensure!(
            actual == sealed,
            "The actual {group} carrier has an unsealed or omitted artifact; paid work is withheld"
        );
    }
    for (name, hash) in seals {
        ensure!(
            google::digest(fs::read(campaign.join(safe_relative(name)?))?)
                == hash.as_str().context("Invalid evidence hash")?,
            "Predecessor evidence changed: {name}"
        );
    }
    ensure!(
        value(&campaign.join("owner-exit.json"))? == audit["owner_exit"]
            && value(&campaign.join("execute-exit.json"))? == audit["execute_exit"],
        "Closed exit identities differ"
    );
    let pids = audit["exact_pid_absence"]["pids"]
        .as_array()
        .context("No absent PIDs")?;
    let started = value(&campaign.join("owner-started.json"))?;
    let guard = value(&campaign.join("run/queue-started.json"))?;
    ensure!(
        pids[0] == started["pid"]
            && pids[1] == guard["pid"]
            && guard["plan_sha256"] == audit["immutable_plan_sha256"],
        "Absent PIDs or predecessor guard do not match the paid owner"
    );
    Ok(audit)
}
fn budget(audit: &Value, parent: &Plan, group: &str, campaign: &Path) -> Result<Budget> {
    let ledger = &audit["actual_paid_ledgers"][group];
    ensure!(
        ledger["uncertain_attempts"] == 0,
        "Uncertain paid work blocks continuation"
    );
    let count = ledger["physical_attempts"]
        .as_u64()
        .context("No paid-attempt count")? as usize;
    let attempts = ledger["attempts"]
        .as_array()
        .context("No exact paid-attempt ledger")?;
    let mut ordinals = std::collections::BTreeSet::new();
    let actual_files = carrier_files(
        &campaign.join(format!("run/{group}")),
        &format!("run/{group}"),
    )?;
    let actual_requests = actual_files
        .iter()
        .filter(|name| name.contains(".attempt-") && name.ends_with(".request.json"))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let listed_requests = attempts
        .iter()
        .map(|row| {
            row["path"]
                .as_str()
                .context("No attempt path")
                .map(str::to_owned)
        })
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(
        actual_requests == listed_requests,
        "An actual paid attempt is omitted from the closed ledger"
    );
    for logical in actual_files
        .iter()
        .filter(|name| name.ends_with(".request.json") && !name.contains(".attempt-"))
    {
        let prefix = format!(
            "{}.attempt-",
            logical
                .strip_suffix(".request.json")
                .context("No logical request stem")?
        );
        ensure!(
            actual_requests.iter().any(|name| name.starts_with(&prefix)),
            "An unresolved logical request has no witnessed physical attempt"
        );
    }
    let normalized = actual_files
        .iter()
        .filter(|name| name.ends_with(".response.json"))
        .count();
    ensure!(
        ledger["settled_success_caches"] == normalized,
        "Normalized successful cache count differs from actual files"
    );
    for row in attempts {
        let name = row["path"].as_str().context("No attempt path")?;
        ensure!(
            name.starts_with(&format!("run/{group}/")) && name.ends_with(".request.json"),
            "Attempt is outside its paid carrier"
        );
        let request_path = campaign.join(safe_relative(name)?);
        let request = value(&request_path)?;
        let logical_prefix = name
            .split(".attempt-")
            .next()
            .context("No logical request prefix")?;
        let logical = fs::read(campaign.join(format!("{logical_prefix}.request.json")))?;
        ensure!(
            request["request_sha256"] == google::digest(logical)
                && request["body_sha256"] == google::digest(serde_json::to_vec(&request["body"])?)
                && request["model"] == row["model"],
            "Physical submission differs from its exact logical request/body/model"
        );
        let stem = name
            .strip_suffix("request.json")
            .context("No attempt stem")?;
        let result = value(&campaign.join(format!("{stem}result.json")))?;
        let body = fs::read(campaign.join(format!("{stem}body")))?;
        ensure!(
            result["response_complete"] == true
                && result["http_status"].is_u64()
                && result["body_sha256"] == google::digest(&body)
                && result["logical_key"] == request["logical_key"]
                && result["physical_call"] == request["physical_call"]
                && result["attempt"] == request["attempt"],
            "An attempt is missing, unsettled or has mismatched physical evidence"
        );
        ensure!(
            request["logical_key"] == row["logical_key"]
                && request["physical_call"] == row["physical_call"]
                && result["http_status"] == row["http_status"],
            "Recorded paid ledger differs from actual attempt"
        );
        ordinals.insert(
            request["physical_call"]
                .as_u64()
                .context("No physical ordinal")? as usize,
        );
    }
    ensure!(
        attempts.len() == count
            && ordinals == (1..=count).collect()
            && ledger["maximum_ordinal"] == count,
        "Paid ordinals are incomplete or duplicated"
    );
    let maximum = parent.configuration[format!("{group}_call_cap")]
        .as_u64()
        .context("No original physical cap")? as usize;
    ensure!(count <= maximum, "Inherited paid work exceeds original cap");
    Ok(Budget {
        maximum,
        inherited: count,
        remaining: maximum - count,
        success_caches: ledger["settled_success_caches"]
            .as_u64()
            .context("No settled-cache count")? as usize,
    })
}
fn changes(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> BTreeMap<String, Change> {
    before
        .keys()
        .chain(after.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .map(|name| {
            (
                name.clone(),
                Change {
                    before: before.get(name).cloned().unwrap_or_default(),
                    after: after.get(name).cloned().unwrap_or_default(),
                },
            )
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StudentAuthorization {
    mode: String,
    logical_key: String,
    request_sha256: String,
    failure_sha256: String,
    maximum_new_attempts: usize,
    explicit_replacement: bool,
}

fn student_slot(
    campaign: &Path,
    audit: &Value,
    authorization: &Value,
    student: &Budget,
    closure_sha256: &str,
) -> Result<StudentRecovery> {
    let authorization: StudentAuthorization = serde_json::from_value(authorization.clone())?;
    ensure!(
        authorization.mode == "completed_student_transient"
            && authorization.explicit_replacement
            && (1..=3).contains(&authorization.maximum_new_attempts)
            && student.remaining >= authorization.maximum_new_attempts,
        "Student recovery exceeds its explicit span or original cumulative budget"
    );
    let prefix = format!("run/student/{}.", authorization.logical_key);
    let witnesses = audit["source_seals"]
        .as_object()
        .context("No student carrier seals")?
        .iter()
        .filter(|(name, _)| name.starts_with(&prefix))
        .map(|(name, hash)| {
            Ok((
                name.strip_prefix("run/student/")
                    .context("Wrong student carrier")?
                    .to_owned(),
                hash.as_str()
                    .context("Invalid student witness hash")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        witnesses.get(&format!("{}.failure.json", authorization.logical_key))
            == Some(&authorization.failure_sha256),
        "Explicit student failure seal differs from closed evidence"
    );
    let mut slot = StudentRecovery {
        version: 1,
        model: google::STUDENT.into(),
        original_key: authorization.logical_key,
        original_request_sha256: authorization.request_sha256,
        closure_receipt_sha256: closure_sha256.into(),
        recovery_key: String::new(),
        witnesses,
        maximum_new_attempts: authorization.maximum_new_attempts,
        first_physical_ordinal: student.inherited + 1,
        last_physical_ordinal: student.inherited + authorization.maximum_new_attempts,
        original_physical_cap: student.maximum,
    };
    slot.recovery_key = slot.expected_key();
    slot.verify_witnesses(&campaign.join("run/student"))?;
    let failure = value(&campaign.join(format!("run/student/{}.failure.json", slot.original_key)))?;
    let count = failure["physical_submissions"]
        .as_u64()
        .context("No old attempt count")?;
    let last = value(&campaign.join(format!(
        "run/student/{}.attempt-{count}.result.json",
        slot.original_key
    )))?;
    ensure!(
        last["physical_call"] == student.inherited,
        "The explicitly recoverable failure is not the final settled student operation"
    );
    Ok(slot)
}

fn version_matches(manifest: &Manifest) -> bool {
    match manifest.version {
        1 => {
            manifest.student_recovery.is_none() && !manifest.known_unusable_teacher_review.is_null()
        }
        2 => {
            manifest.student_recovery.is_some() && manifest.known_unusable_teacher_review.is_null()
        }
        _ => false,
    }
}

/// Read-only preparation. The caller explicitly records review of source changes.
/// Frozen pool words, labels, scores and traces never become optimizer checkpoint input.
pub fn prepare(
    campaign: &Path,
    closure: &Path,
    current: &Plan,
    review_note: &str,
) -> Result<Manifest> {
    ensure!(
        !review_note.trim().is_empty(),
        "Record the reviewed operational source changes"
    );
    let campaign = campaign.canonicalize()?;
    let closure_receipt = FileSeal::read(closure)?;
    let audit = verified_audit(&closure_receipt, &campaign)?;
    let parent_plan = FileSeal::read(&campaign.join("run/queue-plan.json"))?;
    ensure!(
        audit["immutable_plan_sha256"] == parent_plan.sha256,
        "Parent plan does not match closure receipt"
    );
    let parent: Plan = serde_json::from_slice(&fs::read(&parent_plan.path)?)?;
    if let Some(binding) = parent
        .configuration
        .get("continuation")
        .filter(|v| !v.is_null())
    {
        let seal: FileSeal = serde_json::from_value(binding.clone())?;
        seal.verify()?;
        let predecessor = load(&seal.path)?;
        verify(&predecessor, &parent).context("Predecessor continuation ancestry differs")?;
    }
    ensure!(
        parent.book_sha256 == current.book_sha256
            && parent.banks_sha256 == current.banks_sha256
            && parent.case_banks_canonical_sha256 == current.case_banks_canonical_sha256
            && parent.jobs == current.jobs
            && parent.bootstrap_demos == current.bootstrap_demos
            && parent.requested_metric_calls == current.requested_metric_calls,
        "Book, original cases/splits, job scope or optimizer bounds changed"
    );
    for key in [
        "teacher",
        "teacher_sequence",
        "model_listing_sha256",
        "student_call_cap",
        "teacher_call_cap",
        "codex",
        "codex_sha256",
    ] {
        ensure!(
            parent.configuration[key] == current.configuration[key],
            "Bound provider or physical-cap policy changed: {key}"
        );
    }
    let changed_reading_datasets = changes(
        &parent.component_datasets_sha256,
        &current.component_datasets_sha256,
    );
    ensure!(
        changed_reading_datasets
            .keys()
            .all(|name| name.ends_with("-reading")),
        "Classification, extraction or auxiliary component inputs drifted"
    );
    let source_changes = changes(&parent.source_sha256, &current.source_sha256);
    let reviewed_deltas: BTreeMap<String, Change> = audit
        .get("reviewed_protected_source_changes")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    ensure!(
        reviewed_deltas.iter().all(|(name, delta)| {
            matches!(
                name.as_str(),
                "book.rs"
                    | "reading.rs"
                    | "native_export.rs"
                    | "preflight.rs"
                    | "classification_export.rs"
            ) && source_changes.get(name) == Some(delta)
        }),
        "Reviewed protected source deltas differ from exact current before/after hashes"
    );
    ensure!(
        source_changes.keys().all(|name| matches!(
            name.as_str(),
            "continuation.rs"
                | "queue.rs"
                | "main.rs"
                | "google.rs"
                | "teachers.rs"
                | "codex.rs"
                | "lib.rs"
                | "catalogue.rs"
                | "journey.rs"
                | "optimize.rs"
                | "demo.rs"
                | "source-manifest.json"
        ) || reviewed_deltas.contains_key(name)),
        "This carrier continuation cannot authorize book, method signatures, schemas, native calculations or other semantic source changes"
    );
    let student = budget(&audit, &parent, "student", &campaign)?;
    let teacher = budget(&audit, &parent, "teacher", &campaign)?;
    let last = audit["actual_paid_ledgers"]["teacher"]["attempts"]
        .as_array()
        .context("No teacher attempts")?
        .iter()
        .find(|row| row["physical_call"] == teacher.inherited)
        .context("No last teacher operation")?;
    let model = last["model"]
        .as_str()
        .context("No teacher route")?
        .to_owned();
    let key = last["logical_key"]
        .as_str()
        .context("No teacher logical key")?;
    let base = campaign.join(format!("run/teacher/{model}/{key}"));
    let alternate = audit.get("recovery").filter(|value| !value.is_null());
    let (known_unusable_teacher_review, student_recovery) = if let Some(alternate) = alternate {
        ensure!(
            alternate["mode"] == "completed_student_transient"
                && alternate["explicit_replacement"] == true,
            "Unknown or unauthorized alternate recovery closure mode"
        );
        let response: dsrust::lm::api::LmResponse =
            serde_json::from_slice(&fs::read(base.with_extension("response.json"))?)?;
        let receipt = value(&base.with_extension("receipt.json"))?;
        ensure!(
            last["http_status"] == 200
                && response.model.as_deref() == Some(model.as_str())
                && !response.first_text().trim().is_empty()
                && receipt["model"] == model
                && receipt["shared_physical_calls"] == teacher.inherited
                && receipt["request_sha256"]
                    == google::digest(fs::read(base.with_extension("request.json"))?),
            "Student recovery teacher route must come from its last settled completed receipt"
        );
        (
            Value::Null,
            Some(student_slot(
                &campaign,
                &audit,
                alternate,
                &student,
                &closure_receipt.sha256,
            )?),
        )
    } else {
        let raw_path = base.with_extension("raw.json");
        let raw = value(&raw_path)?;
        let attempt_body = fs::read(
            campaign.join(
                last["path"]
                    .as_str()
                    .context("No last teacher request")?
                    .replace(".request.json", ".body"),
            ),
        )?;
        ensure!(
            raw == serde_json::from_slice::<Value>(&attempt_body)?,
            "The MAX_TOKENS witness differs from the completed physical response body"
        );
        ensure!(
            last["http_status"] == 200
                && raw["candidates"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty()
                        && rows.iter().all(|row| row["finishReason"] == "MAX_TOKENS"))
                && !base.with_extension("response.json").exists(),
            "Only the witnessed completed MAX_TOKENS review without a settled normalized answer may be replaced"
        );
        (
            json!({"model":model,"key":key,"physical_call":last["physical_call"],"raw_sha256":google::digest(fs::read(raw_path)?),"http_status":200,"finish_reason":"MAX_TOKENS","parsed_semantic_review":false,"known_completed_replacement_only":true}),
            None,
        )
    };
    let teacher_start_route = parent.configuration["teacher_sequence"]
        .as_array()
        .context("No frozen teacher order")?
        .iter()
        .position(|row| row == &json!(format!("models/{model}")))
        .context("Failed teacher route is absent from frozen listing")?;
    let imported_cache_files = audit["source_seals"]
        .as_object()
        .context("No cache seals")?
        .iter()
        .filter_map(|(name, hash)| {
            let relative = name.strip_prefix("run/")?;
            ((relative.starts_with("student/") || relative.starts_with("teacher/"))
                && !relative.ends_with(".routes.jsonl"))
            .then(|| {
                (
                    relative.to_owned(),
                    hash.as_str().unwrap_or_default().to_owned(),
                )
            })
        })
        .collect();
    Ok(Manifest {version:if student_recovery.is_some(){2}else{1},parent_campaign:campaign.clone(),closure_receipt,parent_plan,parent_guard:FileSeal::read(&campaign.join("run/queue-started.json"))?,parent_executable_sha256:parent.executable_sha256,new_executable_sha256:current.executable_sha256.clone(),new_sources_sha256:current.source_sha256.clone(),source_changes,reviewed_source_changes:review_note.to_owned(),changed_reading_datasets,student,teacher,teacher_start_route,teacher_start_model:model.clone(),known_unusable_teacher_review,student_recovery,imported_cache_files,policy:"Replay the conventional compiler with exact settled carrier caches; deduct all inherited physical attempts. Keep failed/partial runs and original guards. No partial GEPA state, forced grades, development reflection or automatic installation. Changed reading input bytes require a new baseline.".into()})
}
pub fn load(path: &Path) -> Result<Manifest> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn verify_configuration(current: &Plan) -> Result<()> {
    if let Some(binding) = current
        .configuration
        .get("continuation")
        .filter(|v| !v.is_null())
    {
        let seal: FileSeal = serde_json::from_value(binding.clone())?;
        seal.verify()?;
        let manifest = load(&seal.path)?;
        ensure!(
            version_matches(&manifest)
                && manifest.new_executable_sha256 == current.executable_sha256
                && manifest.new_sources_sha256 == current.source_sha256,
            "Sealed continuation source identity differs from queue"
        );
        ensure!(
            current.configuration["student_call_cap"] == manifest.student.maximum
                && current.configuration["teacher_call_cap"] == manifest.teacher.maximum,
            "A continuation cannot reset its original physical caps"
        );
    }
    Ok(())
}
pub fn verify(manifest: &Manifest, current: &Plan) -> Result<()> {
    ensure!(
        version_matches(manifest)
            && manifest.new_executable_sha256 == current.executable_sha256
            && manifest.new_sources_sha256 == current.source_sha256,
        "Continuation executable/source identity differs"
    );
    let checked = prepare(
        &manifest.parent_campaign,
        &manifest.closure_receipt.path,
        current,
        &manifest.reviewed_source_changes,
    )?;
    ensure!(
        &checked == manifest,
        "Continuation ancestry/cache/budget evidence changed"
    );
    Ok(())
}
/// Carrier-only local import; no inference, score replay, checkpoint loading or network.
pub fn import(manifest: &Manifest, current: &Plan, output: &Path) -> Result<()> {
    verify(manifest, current)?;
    ensure!(
        !output.join("queue-started.json").exists(),
        "A started queue cannot be reimported or relaunched"
    );
    for (name, hash) in &manifest.imported_cache_files {
        let relative = safe_relative(name)?;
        let bytes = fs::read(manifest.parent_campaign.join("run").join(relative))?;
        ensure!(
            google::digest(&bytes) == *hash,
            "Exact predecessor cache changed: {name}"
        );
        let target = output.join(relative);
        fs::create_dir_all(target.parent().context("No cache parent")?)?;
        if target.exists() {
            ensure!(
                google::digest(fs::read(&target)?) == *hash,
                "Existing imported cache differs"
            );
        } else {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&target)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
    }
    crate::queue::write_json_atomic(
        &output.join("continuation-import.json"),
        &json!({"manifest_sha256":google::digest(serde_json::to_vec(manifest)?),"files":manifest.imported_cache_files,"student_inherited_physical_attempts":manifest.student.inherited,"teacher_inherited_physical_attempts":manifest.teacher.inherited,"fresh_student_ordinal":manifest.student.inherited+1,"fresh_teacher_ordinal":manifest.teacher.inherited+1,"student_remaining":manifest.student.remaining,"teacher_remaining":manifest.teacher.remaining,"student_recovery":manifest.student_recovery,"optimizer_state_imported":false,"semantic_scores_imported":false,"source_and_input_changes_require_fresh_observed_evaluation":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(path: &Path, data: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(data).unwrap()).unwrap();
    }
    fn plan() -> Plan {
        Plan {
            version: 1,
            executable_sha256: "new-executable".into(),
            book_sha256: "unchanged-book".into(),
            source_sha256: BTreeMap::from([
                ("queue.rs".into(), "new".into()),
                ("signatures.rs".into(), "unchanged-signatures".into()),
            ]),
            banks_sha256: BTreeMap::from([(
                "family.jsonl".into(),
                "unchanged-cases-and-gold".into(),
            )]),
            case_banks_canonical_sha256: BTreeMap::from([(
                "family".into(),
                "unchanged-membership".into(),
            )]),
            component_datasets_sha256: BTreeMap::from([
                ("classification".into(), "same-inputs".into()),
                ("family-reading".into(), "new-rendering".into()),
            ]),
            configuration: json!({"teacher":"pool","teacher_sequence":["models/gemini-test"],"model_listing_sha256":"same-listing","student_call_cap":3,"teacher_call_cap":3,"codex":"same-codex","codex_sha256":"same-codex-hash"}),
            bootstrap_demos: 0,
            requested_metric_calls: 1,
            jobs: vec![],
            scope: "Synthetic transport mechanics only".into(),
            reservations: Value::Null,
        }
    }
    #[cfg(unix)]
    fn closed(directory: &Path) -> (PathBuf, Plan) {
        let current = plan();
        let mut parent = current.clone();
        parent.executable_sha256 = "old-executable".into();
        parent.source_sha256.insert("queue.rs".into(), "old".into());
        parent
            .component_datasets_sha256
            .insert("family-reading".into(), "old-rendering".into());
        write(&directory.join("run/queue-plan.json"), &json!(&parent));
        let plan_sha = google::digest(fs::read(directory.join("run/queue-plan.json")).unwrap());
        write(
            &directory.join("run/queue-started.json"),
            &json!({"pid":99999998,"plan_sha256":plan_sha}),
        );
        write(
            &directory.join("owner-started.json"),
            &json!({"pid":99999999}),
        );
        let exit = json!({"code":1,"automatic_retry":false});
        write(&directory.join("owner-exit.json"), &exit);
        write(&directory.join("execute-exit.json"), &exit);
        let mut ledgers = serde_json::Map::new();
        for (group, model) in [("student", google::STUDENT), ("teacher", "gemini-test")] {
            let prefix = if group == "student" {
                "run/student/key".to_owned()
            } else {
                format!("run/teacher/{model}/key")
            };
            write(
                &directory.join(format!("{prefix}.request.json")),
                &json!({"model":model}),
            );
            let submitted = json!({"synthetic":"physical body identity only"});
            write(
                &directory.join(format!("{prefix}.attempt-1.request.json")),
                &json!({"physical_call":1,"logical_key":"key","attempt":1,"model":model,"request_sha256":google::digest(fs::read(directory.join(format!("{prefix}.request.json"))).unwrap()),"body":submitted,"body_sha256":google::digest(serde_json::to_vec(&submitted).unwrap())}),
            );
            let raw = if group == "teacher" {
                json!({"candidates":[{"finishReason":"MAX_TOKENS"}]})
            } else {
                json!({"synthetic":"complete physical response"})
            };
            let body = serde_json::to_vec(&raw).unwrap();
            fs::write(directory.join(format!("{prefix}.attempt-1.body")), &body).unwrap();
            write(
                &directory.join(format!("{prefix}.attempt-1.result.json")),
                &json!({"response_complete":true,"http_status":200,"physical_call":1,"logical_key":"key","attempt":1,"body_sha256":google::digest(&body)}),
            );
            if group == "student" {
                write(
                    &directory.join(format!("{prefix}.response.json")),
                    &json!({"normalized":"synthetic mechanics only"}),
                );
            } else {
                write(&directory.join(format!("{prefix}.raw.json")), &raw);
            }
            ledgers.insert(group.into(),json!({"physical_attempts":1,"maximum_ordinal":1,"settled_success_caches":if group=="student"{1}else{0},"uncertain_attempts":0,"attempts":[{"path":format!("{prefix}.attempt-1.request.json"),"physical_call":1,"logical_key":"key","http_status":200,"model":model}]}));
        }
        let mut seals = BTreeMap::new();
        for name in [
            "run/queue-plan.json",
            "run/queue-started.json",
            "owner-started.json",
            "owner-exit.json",
            "execute-exit.json",
        ] {
            seals.insert(
                name.to_owned(),
                google::digest(fs::read(directory.join(name)).unwrap()),
            );
        }
        for group in ["student", "teacher"] {
            for name in carrier_files(
                &directory.join(format!("run/{group}")),
                &format!("run/{group}"),
            )
            .unwrap()
            {
                seals.insert(
                    name.clone(),
                    google::digest(fs::read(directory.join(name)).unwrap()),
                );
            }
        }
        let audit = directory.join("closure.json");
        write(
            &audit,
            &json!({"version":1,"campaign":directory.canonicalize().unwrap(),"owner_exit":exit,"execute_exit":exit,"completed_jobs":0,"later_jobs_executed":false,"raw_answers_preserved":true,"exact_pid_absence":{"pids":[99999999,99999998]},"source_seals":seals,"immutable_plan_sha256":plan_sha,"actual_paid_ledgers":ledgers}),
        );
        (audit, current)
    }
    #[cfg(unix)]
    fn reseal(directory: &Path, audit: &Path, data: &mut Value) {
        let names = data["source_seals"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut seals = BTreeMap::new();
        for name in names {
            if !name.starts_with("run/student/") && !name.starts_with("run/teacher/") {
                seals.insert(
                    name.clone(),
                    google::digest(fs::read(directory.join(&name)).unwrap()),
                );
            }
        }
        for group in ["student", "teacher"] {
            for name in carrier_files(
                &directory.join(format!("run/{group}")),
                &format!("run/{group}"),
            )
            .unwrap()
            {
                seals.insert(
                    name.clone(),
                    google::digest(fs::read(directory.join(name)).unwrap()),
                );
            }
        }
        data["source_seals"] = json!(seals);
        write(audit, data);
    }
    #[cfg(unix)]
    fn closed_student(directory: &Path) -> (PathBuf, Plan) {
        let (audit, mut current) = closed(directory);
        current.configuration["student_call_cap"] = json!(6);
        let mut parent: Plan =
            serde_json::from_value(value(&directory.join("run/queue-plan.json")).unwrap()).unwrap();
        parent.configuration["student_call_cap"] = json!(6);
        write(&directory.join("run/queue-plan.json"), &json!(parent));
        let plan_sha = google::digest(fs::read(directory.join("run/queue-plan.json")).unwrap());
        write(
            &directory.join("run/queue-started.json"),
            &json!({"pid":99999998,"plan_sha256":plan_sha}),
        );
        for entry in fs::read_dir(directory.join("run/student")).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let key = "a".repeat(64);
        let prefix = format!("run/student/{key}");
        let submitted = json!({"synthetic":"identical bounded recovery body"});
        write(
            &directory.join(format!("{prefix}.request.json")),
            &json!({"model":google::STUDENT,"request":{"synthetic":"same request"},"body":submitted}),
        );
        let request_sha =
            google::digest(fs::read(directory.join(format!("{prefix}.request.json"))).unwrap());
        let mut attempts = Vec::new();
        for (attempt, status) in [(1, 500), (2, 429), (3, 500)] {
            let name = format!("{prefix}.attempt-{attempt}.request.json");
            write(
                &directory.join(&name),
                &json!({"physical_call":attempt,"logical_key":key,"attempt":attempt,"model":google::STUDENT,"request_sha256":request_sha,"body":submitted,"body_sha256":google::digest(serde_json::to_vec(&submitted).unwrap())}),
            );
            let body = serde_json::to_vec(
                &json!({"error":{"code":status,"message":"Synthetic fully received transient"}}),
            )
            .unwrap();
            fs::write(
                directory.join(format!("{prefix}.attempt-{attempt}.body")),
                &body,
            )
            .unwrap();
            write(
                &directory.join(format!("{prefix}.attempt-{attempt}.result.json")),
                &json!({"response_complete":true,"http_status":status,"physical_call":attempt,"logical_key":key,"attempt":attempt,"automatic_retry":attempt<3,"body_sha256":google::digest(&body)}),
            );
            attempts.push(json!({"path":name,"physical_call":attempt,"logical_key":key,"http_status":status,"model":google::STUDENT}));
        }
        write(
            &directory.join(format!("{prefix}.failure.json")),
            &json!({"http_status":500,"response_complete":true,"automatic_retry":false,"physical_submissions":3,"disposition":"quota_or_unavailable"}),
        );
        let teacher_prefix = "run/teacher/gemini-test/key";
        let teacher_raw = json!({"candidates":[{"content":{"parts":[{"text":"Existing paid teacher answer"}]},"finishReason":"STOP"}]});
        let teacher_body = serde_json::to_vec(&teacher_raw).unwrap();
        fs::write(
            directory.join(format!("{teacher_prefix}.attempt-1.body")),
            &teacher_body,
        )
        .unwrap();
        write(
            &directory.join(format!("{teacher_prefix}.attempt-1.result.json")),
            &json!({"http_status":200,"response_complete":true,"physical_call":1,"logical_key":"key","attempt":1,"body_sha256":google::digest(&teacher_body)}),
        );
        write(
            &directory.join(format!("{teacher_prefix}.raw.json")),
            &teacher_raw,
        );
        let response = dsrust::lm::api::LmResponse {
            model: Some("gemini-test".into()),
            outputs: vec![dsrust::lm::api::LmOutput::text(
                "Existing paid teacher answer",
            )],
            ..Default::default()
        };
        write(
            &directory.join(format!("{teacher_prefix}.response.json")),
            &serde_json::to_value(response).unwrap(),
        );
        write(
            &directory.join(format!("{teacher_prefix}.receipt.json")),
            &json!({"model":"gemini-test","shared_physical_calls":1,"request_sha256":google::digest(fs::read(directory.join(format!("{teacher_prefix}.request.json"))).unwrap())}),
        );
        let mut data = value(&audit).unwrap();
        data["immutable_plan_sha256"] = json!(plan_sha);
        data["actual_paid_ledgers"]["student"] = json!({"physical_attempts":3,"maximum_ordinal":3,"uncertain_attempts":0,"settled_success_caches":0,"attempts":attempts});
        data["actual_paid_ledgers"]["teacher"]["settled_success_caches"] = json!(1);
        data["recovery"] = json!({"mode":"completed_student_transient","logical_key":key,"request_sha256":request_sha,"failure_sha256":google::digest(fs::read(directory.join(format!("{prefix}.failure.json"))).unwrap()),"maximum_new_attempts":3,"explicit_replacement":true});
        reseal(directory, &audit, &mut data);
        (audit, current)
    }
    #[test]
    #[cfg(unix)]
    fn completed_student_slot_is_explicit_source_bound_and_preserves_all_old_attempts() {
        let directory = tempfile::tempdir().unwrap();
        let (audit, current) = closed_student(directory.path());
        let manifest = prepare(
            directory.path(),
            &audit,
            &current,
            "Reviewed bounded student transport recovery",
        )
        .unwrap();
        assert_eq!(manifest.version, 2);
        assert!(manifest.known_unusable_teacher_review.is_null());
        let slot = manifest.student_recovery.as_ref().unwrap();
        assert_eq!(
            (slot.first_physical_ordinal, slot.last_physical_ordinal),
            (4, 6)
        );
        assert_eq!(manifest.student.inherited, 3);
        assert_eq!(manifest.teacher.inherited, 1);
        assert_eq!(manifest.teacher_start_model, "gemini-test");
        assert_ne!(slot.recovery_key, slot.original_key);
        let output = tempfile::tempdir().unwrap();
        import(&manifest, &current, output.path()).unwrap();
        slot.verify_witnesses(&output.path().join("student"))
            .unwrap();
        assert!(
            output
                .path()
                .join(format!("student/{}.failure.json", slot.original_key))
                .exists()
        );
        assert!(
            !output
                .path()
                .join(format!("student/{}.request.json", slot.recovery_key))
                .exists()
        );
        assert!(!output.path().join("jobs").exists());
        let mut without_authorization = value(&audit).unwrap();
        without_authorization
            .as_object_mut()
            .unwrap()
            .remove("recovery");
        write(&audit, &without_authorization);
        assert!(
            prepare(
                directory.path(),
                &audit,
                &current,
                "Prose cannot grant a recovery slot"
            )
            .is_err()
        );
    }
    #[test]
    #[cfg(unix)]
    fn student_slot_rejects_uncertainty_model_answers_nontransients_and_oversized_spans() {
        for status in [200, 400, 401, 404, 408, 501] {
            let directory = tempfile::tempdir().unwrap();
            let (audit, current) = closed_student(directory.path());
            let mut data = value(&audit).unwrap();
            let key = data["recovery"]["logical_key"].as_str().unwrap().to_owned();
            let result_path = directory
                .path()
                .join(format!("run/student/{key}.attempt-3.result.json"));
            let mut result = value(&result_path).unwrap();
            result["http_status"] = json!(status);
            write(&result_path, &result);
            data["actual_paid_ledgers"]["student"]["attempts"][2]["http_status"] = json!(status);
            reseal(directory.path(), &audit, &mut data);
            assert!(
                prepare(
                    directory.path(),
                    &audit,
                    &current,
                    "No semantic answer retry"
                )
                .is_err(),
                "status {status}"
            );
        }
        let directory = tempfile::tempdir().unwrap();
        let (audit, current) = closed_student(directory.path());
        let mut data = value(&audit).unwrap();
        data["recovery"]["maximum_new_attempts"] = json!(4);
        write(&audit, &data);
        assert!(prepare(directory.path(), &audit, &current, "No enlarged span").is_err());
        data["recovery"]["maximum_new_attempts"] = json!(3);
        let key = data["recovery"]["logical_key"].as_str().unwrap().to_owned();
        let result_path = directory
            .path()
            .join(format!("run/student/{key}.attempt-2.result.json"));
        let mut result = value(&result_path).unwrap();
        result["response_complete"] = json!(false);
        write(&result_path, &result);
        reseal(directory.path(), &audit, &mut data);
        assert!(prepare(directory.path(), &audit, &current, "No uncertain replay").is_err());
    }
    #[test]
    #[cfg(unix)]
    fn protected_reading_source_delta_needs_exact_recorded_hashes() {
        let directory = tempfile::tempdir().unwrap();
        let (audit, mut current) = closed_student(directory.path());
        current
            .source_sha256
            .insert("reading.rs".into(), "new-reviewed-reader".into());
        assert!(
            prepare(
                directory.path(),
                &audit,
                &current,
                "Arbitrary prose is insufficient"
            )
            .is_err()
        );
        let mut data = value(&audit).unwrap();
        data["reviewed_protected_source_changes"] =
            json!({"reading.rs":{"before":"","after":"new-reviewed-reader"}});
        write(&audit, &data);
        prepare(
            directory.path(),
            &audit,
            &current,
            "Exact reviewed reading delta; fresh baseline",
        )
        .unwrap();
        data["reviewed_protected_source_changes"]["reading.rs"]["after"] = json!("other-reader");
        write(&audit, &data);
        assert!(prepare(directory.path(), &audit, &current, "Wrong hash").is_err());
    }
    #[test]
    fn inherited_reservations_are_not_reset_and_keep_their_original_ordinals() {
        let budget = google::CallBudget::seeded(30, 29).unwrap();
        assert_eq!(budget.remaining(), 1);
        assert_eq!(budget.reserve(), Some(30));
        assert_eq!(budget.reserve(), None);
        assert!(google::CallBudget::seeded(28, 29).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn carrier_import_keeps_known_failed_raw_and_never_loads_scores_or_started_guards() {
        let directory = tempfile::tempdir().unwrap();
        let (audit, current) = closed(directory.path());
        let manifest = prepare(
            directory.path(),
            &audit,
            &current,
            "Reviewed transport-only source update and lossless reading render",
        )
        .unwrap();
        assert_eq!(manifest.student.remaining, 2);
        assert_eq!(manifest.teacher_start_route, 0);
        assert_eq!(manifest.changed_reading_datasets.len(), 1);
        let output = tempfile::tempdir().unwrap();
        import(&manifest, &current, output.path()).unwrap();
        assert!(output.path().join("student/key.response.json").exists());
        assert!(
            output
                .path()
                .join("teacher/gemini-test/key.raw.json")
                .exists()
        );
        assert!(!output.path().join("jobs").exists());
        assert!(!output.path().join("queue-started.json").exists());
        assert!(directory.path().join("run/queue-started.json").exists());
        write(
            &output.path().join("queue-started.json"),
            &json!({"started":true}),
        );
        assert!(import(&manifest, &current, output.path()).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn unsealed_or_unsettled_attempts_and_protected_source_drift_fail_before_import() {
        let directory = tempfile::tempdir().unwrap();
        let (audit, mut current) = closed(directory.path());
        current
            .source_sha256
            .insert("signatures.rs".into(), "changed-method-authority".into());
        assert!(
            prepare(
                directory.path(),
                &audit,
                &current,
                "Arbitrary review cannot permit changed method signatures"
            )
            .is_err()
        );
        current = plan();
        write(
            &directory.path().join("run/student/unsettled.request.json"),
            &json!({"started":true}),
        );
        let error =
            prepare(directory.path(), &audit, &current, "Transport changes only").unwrap_err();
        assert!(error.to_string().contains("unsealed or omitted artifact"));
    }
    #[test]
    #[cfg(unix)]
    fn copied_attempt_ledger_cannot_hide_unknown_work_or_claim_extra_cached_successes() {
        let directory = tempfile::tempdir().unwrap();
        let (audit, current) = closed(directory.path());
        let mut changed = value(&audit).unwrap();
        changed["actual_paid_ledgers"]["student"]["settled_success_caches"] = json!(2);
        write(&audit, &changed);
        let error =
            prepare(directory.path(), &audit, &current, "Transport changes only").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Normalized successful cache count")
        );
        changed["actual_paid_ledgers"]["student"]["settled_success_caches"] = json!(1);
        changed["actual_paid_ledgers"]["student"]["attempts"] = json!([]);
        write(&audit, &changed);
        let error =
            prepare(directory.path(), &audit, &current, "Transport changes only").unwrap_err();
        assert!(error.to_string().contains("omitted from the closed ledger"));
    }
}
