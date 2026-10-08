//! One bounded optimization round, rather than a human coordinating each call.
#![forbid(unsafe_code)]
use clap::Parser;
use fs2::FileExt;
use horary_loop::{Action, Options};
use horary_prompt_program::{digest, Program};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, String>;

#[derive(Parser)]
#[command(
    about = "Grade training receipts, propose one teaching change, and run its paired native experiment"
)]
struct Cli {
    #[arg(long)]
    discovery: PathBuf,
    #[arg(long)]
    fixtures: PathBuf,
    #[arg(long)]
    native_executable: PathBuf,
    #[arg(long)]
    model: PathBuf,
    #[arg(long)]
    state: PathBuf,
    /// Watch a running discovery campaign until a review batch is available.
    #[arg(long)]
    watch: bool,
    #[arg(long, default_value = "codex")]
    codex: PathBuf,
    #[arg(long, default_value_t = 6)]
    batch_size: usize,
    #[arg(long, default_value_t = 1)]
    review_jobs: usize,
    #[arg(long, default_value_t = 9)]
    max_native_cases: usize,
    #[arg(long, default_value_t = 1200)]
    judge_seconds: u64,
    #[arg(long, default_value_t = 240)]
    case_seconds: u64,
    #[arg(long, default_value_t = 16)]
    max_calls: u64,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Plan {
    version: u32,
    discovery: PathBuf,
    fixtures: PathBuf,
    manifest_sha256: String,
    native_executable: PathBuf,
    model: PathBuf,
    codex: PathBuf,
    watch: bool,
    batch_size: usize,
    review_jobs: usize,
    max_native_cases: usize,
    judge_seconds: u64,
    case_seconds: u64,
    max_calls: u64,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("horary-optimize: {error}");
        std::process::exit(1);
    }
}
fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}
fn load(path: &Path) -> Result<Value> {
    serde_json::from_slice(&read(path)?).map_err(|e| e.to_string())
}
fn keep(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if path.exists() {
        return if read(path)? == bytes {
            Ok(())
        } else {
            Err(format!(
                "Immutable round artifact changed: {}",
                path.display()
            ))
        };
    }
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

fn candidate_from(proposals: &[Value]) -> Result<Option<Program>> {
    let candidates = proposals
        .iter()
        .filter(|v| !v["candidate"].is_null())
        .map(|v| Program::parse(&serde_json::to_vec(&v["candidate"]).map_err(|e| e.to_string())?))
        .collect::<Result<Vec<_>>>()?;
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.into_iter().next()),
        _ => Err("This round has multiple saved candidates; preserve it and explicitly choose a fresh round".into()),
    }
}

fn affected_cases(program: &Program, split: &Value, limit: usize) -> Result<usize> {
    let cases = split["cases"]
        .as_array()
        .ok_or("The saved split has no cases")?;
    let count = cases
        .iter()
        .filter(|case| {
            program.overrides.iter().any(|o| {
                o.method
                    .as_deref()
                    .is_none_or(|method| case["method"] == method)
            })
        })
        .count();
    if count == 0 || count > limit {
        return Err(format!("Candidate affects {count} cases, exceeding this round's native-case budget {limit}; no native trial was started"));
    }
    Ok(count)
}

/// An uncertain paid job needs local recovery or an explicit retry outside this
/// driver. Moving on would conceal the missing grade and spend on other work.
fn require_settled_ledger(state: &Path) -> Result<()> {
    let path = state.join("ledger.json");
    if !path.exists() {
        return Ok(());
    }
    let ledger = load(&path)?;
    let jobs = ledger["jobs"]
        .as_object()
        .ok_or("Review ledger has no jobs")?;
    for (id, job) in jobs {
        if job["status"] != "complete" {
            return Err(format!("Saved review job {id} is {}; preserve its paid answer and recover it before resuming", job["status"]));
        }
    }
    Ok(())
}

fn remaining_review_jobs(state: &Path, budget: usize) -> Result<usize> {
    let path = state.join("ledger.json");
    if !path.exists() {
        return Ok(budget);
    }
    let ledger = load(&path)?;
    let used = ledger["jobs"]
        .as_object()
        .ok_or("Review ledger has no jobs")?
        .values()
        .filter(|job| job["kind"] == "judge" && job["phase"] == "training")
        .count();
    Ok(budget.saturating_sub(used))
}

fn run(cli: Cli) -> Result<()> {
    if cli.review_jobs == 0 || cli.max_native_cases == 0 {
        return Err(
            "An automatic round requires finite, positive review and native-case budgets".into(),
        );
    }
    let plan = Plan {
        version: 1,
        discovery: fs::canonicalize(&cli.discovery).map_err(|e| e.to_string())?,
        fixtures: fs::canonicalize(&cli.fixtures).map_err(|e| e.to_string())?,
        manifest_sha256: digest(read(&cli.discovery.join("manifest.json"))?),
        native_executable: fs::canonicalize(&cli.native_executable).map_err(|e| e.to_string())?,
        model: fs::canonicalize(&cli.model).map_err(|e| e.to_string())?,
        codex: cli.codex.clone(),
        watch: cli.watch,
        batch_size: cli.batch_size,
        review_jobs: cli.review_jobs,
        max_native_cases: cli.max_native_cases,
        judge_seconds: cli.judge_seconds,
        case_seconds: cli.case_seconds,
        max_calls: cli.max_calls,
    };
    fs::create_dir_all(&cli.state).map_err(|e| e.to_string())?;
    let state = fs::canonicalize(&cli.state).map_err(|e| e.to_string())?;
    if state.starts_with(&plan.discovery) || state.starts_with(&plan.fixtures) {
        return Err("Round state must be outside discovery and fixture directories".into());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("round.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock_exclusive()
        .map_err(|e| format!("Another process owns this round: {e}"))?;
    keep(&state.join("plan.json"), &plan)?;
    let reviews = state.join("training-review");
    let options = |action, max_jobs| Options {
        action,
        campaign: plan.discovery.clone(),
        fixtures: plan.fixtures.clone(),
        state: reviews.clone(),
        batch_size: cli.batch_size,
        job_seconds: cli.judge_seconds,
        codex: cli.codex.clone(),
        retry_failed: false,
        validation: false,
        candidate: None,
        once: false,
        max_jobs,
    };
    let reviewed = state.join("review-phase.json");
    if !reviewed.exists() {
        require_settled_ledger(&reviews)?;
        let remaining = remaining_review_jobs(&reviews, cli.review_jobs)?;
        if remaining > 0 {
            horary_loop::run(options(
                if cli.watch {
                    Action::Watch
                } else {
                    Action::Judge
                },
                remaining,
            ))?;
        }
        let grades = horary_loop::completed_judgments(&reviews)?;
        if grades.is_empty() {
            return Err("No completed training reviews; no writer call was made".into());
        }
        keep(
            &reviewed,
            &json!({"judgments":grades,"budget_jobs":cli.review_jobs}),
        )?;
    }
    require_settled_ledger(&reviews)?;
    let mut proposals = horary_loop::completed_proposals(&reviews)?;
    if proposals.is_empty() {
        horary_loop::run(options(Action::Propose, 1))?;
        proposals = horary_loop::completed_proposals(&reviews)?;
    }
    let Some(program) = candidate_from(&proposals)? else {
        keep(
            &state.join("decision.json"),
            &json!({"eligible_for_review":false,
            "state":"writer_abstained_or_native_repair_needed","proposals":proposals,
            "native_trials_started":false}),
        )?;
        println!("Writer abstained or requested native repairs; no native trial was spent.");
        return Ok(());
    };
    let count = affected_cases(
        &program,
        &load(&reviews.join("split.json"))?,
        cli.max_native_cases,
    )?;
    let candidate = state.join("candidate.json");
    keep(&candidate, &program)?;
    let experiment = state.join("experiment");
    let experiment_binary = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name(if cfg!(windows) {
            "horary-experiment.exe"
        } else {
            "horary-experiment"
        });
    println!("Fixed candidate: {count} affected cases; paired training then reserved validation.");
    let status = Command::new(&experiment_binary)
        .arg("--candidate")
        .arg(&candidate)
        .arg("--discovery")
        .arg(&plan.discovery)
        .arg("--split")
        .arg(reviews.join("split.json"))
        .arg("--fixtures")
        .arg(&plan.fixtures)
        .arg("--native-executable")
        .arg(&plan.native_executable)
        .arg("--model")
        .arg(&plan.model)
        .arg("--state")
        .arg(&experiment)
        .arg("--codex")
        .arg(&cli.codex)
        .arg("--case-seconds")
        .arg(cli.case_seconds.to_string())
        .arg("--max-calls")
        .arg(cli.max_calls.to_string())
        .arg("--judge-seconds")
        .arg(cli.judge_seconds.to_string())
        .status()
        .map_err(|e| format!("Start paired experiment: {e}"))?;
    if !status.success() {
        return Err("Paired experiment stopped; its receipts remain resumable, no paid/native retry was made".into());
    }
    let decision = load(&experiment.join("decision.json"))?;
    keep(
        &state.join("decision.json"),
        &json!({"candidate_sha256":digest(read(&candidate)?),
        "experiment":decision,"eligible_for_review":decision["eligible_for_review"],
        "promotion":"Reviewable result only; application source and defaults were not edited"}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_abstention_does_not_become_an_experiment() {
        assert!(candidate_from(&[
            json!({"candidate":null,"repair_required":[{"repair_owner":"native_code"}]})
        ])
        .unwrap()
        .is_none());
    }
    #[test]
    fn failed_paid_jobs_cannot_be_skipped_on_resume() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("ledger.json"),
            br#"{"jobs":{"paid":{"status":"failed"}}}"#,
        )
        .unwrap();
        assert!(require_settled_ledger(temp.path())
            .unwrap_err()
            .contains("paid"));
        fs::write(
            temp.path().join("ledger.json"),
            br#"{"jobs":{"paid":{"status":"complete"}}}"#,
        )
        .unwrap();
        require_settled_ledger(temp.path()).unwrap();
    }
    #[test]
    fn a_lost_phase_marker_does_not_reset_the_paid_review_budget() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("ledger.json"),
            br#"{"jobs":{"paid":{"status":"complete","kind":"judge","phase":"training"}}}"#,
        )
        .unwrap();
        assert_eq!(remaining_review_jobs(temp.path(), 1).unwrap(), 0);
        assert_eq!(remaining_review_jobs(temp.path(), 2).unwrap(), 1);
    }
    #[test]
    fn a_global_prompt_cannot_silently_spend_the_entire_native_suite() {
        let p = json!({"version":1,"id":"global-test","baseline_manifest_sha256":digest("m"),"overrides":[{"stage":"conversation","recognition_phase":null,"method":null,"expected_guide_sha256":digest("g"),"replacement_text":"A sufficiently long focused teaching body which preserves input authority and native constraints.","edits":[]}],"rationale":"r","evidence":[{"case_id":"train","file":"f","json_pointer":"","sha256":digest("f")}],"training_case_ids":["train"],"holdout_case_ids":["reserved"]});
        let program = Program::parse(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(affected_cases(
            &program,
            &json!({"cases":[{"method":"lost_animal"},{"method":"new_job"}]}),
            1
        )
        .is_err());
    }
}
