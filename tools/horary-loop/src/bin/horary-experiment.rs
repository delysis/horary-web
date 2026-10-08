//! Execute paired native trials and review them without a human in each step.
#![forbid(unsafe_code)]
use clap::Parser;
use fs2::FileExt;
use horary_loop::{Action, Options};
use horary_prompt_program::{comparison, digest, Program};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

type Result<T> = std::result::Result<T, String>;

#[derive(Parser)]
#[command(
    about = "Run a fixed prompt candidate against paired native control and reserved validation"
)]
struct Cli {
    #[arg(long)]
    candidate: PathBuf,
    #[arg(long)]
    discovery: PathBuf,
    #[arg(long)]
    split: PathBuf,
    #[arg(long)]
    fixtures: PathBuf,
    #[arg(long)]
    native_executable: PathBuf,
    #[arg(long)]
    model: PathBuf,
    #[arg(long)]
    state: PathBuf,
    #[arg(long, default_value = "codex")]
    codex: PathBuf,
    #[arg(long, default_value_t = 240)]
    case_seconds: u64,
    #[arg(long, default_value_t = 16)]
    max_calls: u64,
    #[arg(long, default_value_t = 1200)]
    judge_seconds: u64,
}

#[derive(Deserialize)]
struct Split {
    cases: Vec<SplitCase>,
}
#[derive(Deserialize)]
struct SplitCase {
    id: String,
    method: String,
    partition: String,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Plan {
    version: u32,
    candidate_sha256: String,
    native_executable_sha256: String,
    discovery_manifest_sha256: String,
    split_sha256: String,
    training_ids: Vec<String>,
    validation_ids: Vec<String>,
    case_seconds: u64,
    max_calls: u64,
    model: PathBuf,
    model_sha256: String,
    codex: PathBuf,
    fixtures: PathBuf,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("horary-experiment: {error}");
        std::process::exit(1)
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
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
fn file_digest(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 {
            break;
        }
        hasher.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn resolve_executable(path: &Path) -> Result<PathBuf> {
    if path.components().count() == 1 && !path.exists() {
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let candidate = directory.join(path);
                if candidate.is_file() {
                    return fs::canonicalize(candidate).map_err(|e| e.to_string());
                }
            }
        }
    }
    fs::canonicalize(path).map_err(|e| e.to_string())
}

fn select(program: &Program, split: &Split) -> Result<(Vec<String>, Vec<String>)> {
    let methods: BTreeSet<_> = program
        .overrides
        .iter()
        .filter_map(|o| o.method.as_deref())
        .collect();
    let global = program.overrides.iter().any(|o| o.method.is_none());
    let matching: Vec<_> = split
        .cases
        .iter()
        .filter(|c| global || methods.contains(c.method.as_str()))
        .collect();
    let training = matching
        .iter()
        .filter(|c| c.partition == "training")
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    let validation = matching
        .iter()
        .filter(|c| c.partition == "reserved_validation")
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    if training.is_empty() || validation.is_empty() {
        return Err("Candidate needs both affected training and reserved validation cases".into());
    }
    let unique: BTreeSet<_> = training.iter().chain(&validation).collect();
    if unique.len() != training.len() + validation.len() {
        return Err("Split has duplicate or overlapping case ids".into());
    }
    Ok((training, validation))
}

fn run(cli: Cli) -> Result<()> {
    // Pin the resolved executable in Plan while retaining the invocation path
    // recorded in established ledgers (for example Homebrew's stable symlink).
    let candidate = fs::canonicalize(&cli.candidate).map_err(|e| e.to_string())?;
    let bytes = read(&candidate)?;
    let program = Program::parse(&bytes)?;
    let split_bytes = read(&cli.split)?;
    let split: Split = serde_json::from_slice(&split_bytes).map_err(|e| e.to_string())?;
    let (training_ids, validation_ids) = select(&program, &split)?;
    let manifest_bytes = read(&cli.discovery.join("manifest.json"))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
    if program.baseline_manifest_sha256 != digest(&manifest_bytes) {
        return Err("Candidate belongs to a different discovery baseline".into());
    }
    // Finished immutable training traces can seed trials while the full suite
    // continues in another checkout. Reserved words are never read here.
    for id in &program.training_case_ids {
        if !cli
            .discovery
            .join("cases")
            .join(id)
            .join("outcome.json")
            .exists()
        {
            return Err(format!("Candidate training trace {id} has not completed"));
        }
    }
    fs::create_dir_all(&cli.state).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cli.state.join("experiment.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock_exclusive()
        .map_err(|e| format!("Another process owns this experiment: {e}"))?;
    let plan = Plan {
        version: 1,
        candidate_sha256: digest(&bytes),
        native_executable_sha256: file_digest(&cli.native_executable)?,
        discovery_manifest_sha256: digest(manifest_bytes),
        split_sha256: digest(split_bytes),
        training_ids,
        validation_ids,
        case_seconds: cli.case_seconds,
        max_calls: cli.max_calls,
        model: fs::canonicalize(&cli.model).map_err(|e| e.to_string())?,
        model_sha256: manifest["model"]["sha256"]
            .as_str()
            .ok_or("Discovery has no model hash")?
            .into(),
        codex: resolve_executable(&cli.codex)?,
        fixtures: fs::canonicalize(&cli.fixtures).map_err(|e| e.to_string())?,
    };
    let plan_path = cli.state.join("plan.json");
    if plan_path.exists() {
        let old: Plan = serde_json::from_slice(&read(&plan_path)?).map_err(|e| e.to_string())?;
        if old != plan {
            return Err(
                "Experiment identity changed; use a new state directory and preserve this trial"
                    .into(),
            );
        }
    } else {
        keep(&plan_path, &plan)?;
        keep(&cli.state.join("candidate.json"), &program)?;
    }
    let mut decisions = Vec::new();
    for (phase, ids, validation) in [
        ("training", &plan.training_ids, false),
        ("validation", &plan.validation_ids, true),
    ] {
        let control_dir = cli.state.join(format!("{phase}-control"));
        let trial_dir = cli.state.join(format!("{phase}-candidate"));
        native(&cli, &control_dir, ids, None, &plan)?;
        native(&cli, &trial_dir, ids, Some(&candidate), &plan)?;
        // Each reviewer owns a separate ledger and immutable packet. Neither
        // depends on the other, so wait for both without serializing their work.
        let control_state = cli.state.join(format!("{phase}-control-review"));
        let trial_state = cli.state.join(format!("{phase}-candidate-review"));
        let (control_reviews, trial_reviews) = std::thread::scope(|scope| {
            let control =
                scope.spawn(|| judge(&cli, &control_dir, &control_state, validation, &candidate));
            let trial =
                scope.spawn(|| judge(&cli, &trial_dir, &trial_state, validation, &candidate));
            let before = control
                .join()
                .map_err(|_| "Control reviewer panicked; preserve both ledgers".to_owned());
            let after = trial
                .join()
                .map_err(|_| "Candidate reviewer panicked; preserve both ledgers".to_owned());
            Ok::<_, String>((before??, after??))
        })?;
        let control = load(&control_dir.join("report.json"))?;
        let trial = load(&trial_dir.join("report.json"))?;
        let policy = if validation {
            comparison::Policy::ReservedValidation
        } else {
            comparison::Policy::Training
        };
        let decision = comparison::compare_with_policy(
            &control,
            &trial,
            &control_reviews,
            &trial_reviews,
            policy,
        )?;
        let path = cli.state.join(format!("{phase}-comparison.json"));
        if !path.exists() {
            keep(&path, &decision)?;
        }
        println!("{phase}: eligible={} blockers={} native improvements={} conversational improvements={}",decision.eligible,decision.blockers.len(),decision.semantic_improvements.len()+decision.journey_improvements.len(),decision.conversation_improvements.len());
        // Validation confirms no regressions; it need not independently improve
        // when training already improved. Keep all evidence and any blockers.
        decisions.push(serde_json::to_value(decision).map_err(|e| e.to_string())?);
        if !validation && decisions[0]["eligible"] != true {
            let rejected = json!({"candidate_sha256":plan.candidate_sha256,"training":decisions[0],
                "reserved_validation":{"state":"withheld","reason":"Training did not qualify; do not spend reserved cases or judge tokens on this candidate"},
                "eligible_for_review":false,"promotion":"No application source or defaults were mutated"});
            let path = cli.state.join("decision.json");
            if !path.exists() {
                keep(&path, &rejected)?;
            }
            println!(
                "{}",
                serde_json::to_string(&rejected).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
    }
    let eligible = decisions.iter().all(|d| d["eligible"] == true);
    let output = json!({"candidate_sha256":plan.candidate_sha256,"training":decisions[0],"reserved_validation":decisions[1],"eligible_for_review":eligible,
       "promotion":"No source or application default was mutated. A qualified candidate is a reviewable artifact; integration must retain its exact hash and source boundary."});
    let path = cli.state.join("decision.json");
    if !path.exists() {
        keep(&path, &output)?;
    }
    println!(
        "{}",
        serde_json::to_string(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn require_selected_cases(report: &Value, ids: &[String]) -> Result<()> {
    let cases = report["cases"].as_array().ok_or("Missing native cases")?;
    let selected = cases
        .iter()
        .map(|case| {
            case["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| "Native case has a missing or invalid id".to_owned())
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let planned: BTreeSet<_> = ids.iter().map(String::as_str).collect();
    if selected.len() != cases.len() || planned.len() != ids.len() || selected != planned {
        return Err("Native filter did not execute the exact selected case set".into());
    }
    Ok(())
}

fn native(
    cli: &Cli,
    directory: &Path,
    ids: &[String],
    candidate: Option<&Path>,
    plan: &Plan,
) -> Result<()> {
    let receipt = directory.with_extension("native-receipt.json");
    if receipt.exists() {
        let saved = load(&receipt)?;
        let report_bytes = read(&directory.join("report.json"))?;
        if saved["report_sha256"] != digest(&report_bytes)
            || saved["manifest_sha256"] != digest(read(&directory.join("manifest.json"))?)
        {
            return Err("Completed native receipts changed".into());
        }
        let report = serde_json::from_slice(&report_bytes).map_err(|e| e.to_string())?;
        return require_selected_cases(&report, ids);
    }
    if directory.exists() {
        return Err(format!(
            "Partial native trial {} is preserved; no silent retry",
            directory.display()
        ));
    }
    if file_digest(&cli.native_executable)? != plan.native_executable_sha256 {
        return Err("Native executable changed mid experiment".into());
    }
    let stdout = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.with_extension("stdout.log"))
        .map_err(|e| e.to_string())?;
    let stderr = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.with_extension("stderr.log"))
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(&cli.native_executable);
    command
        .args([
            "elicitation_eval::real_model_catalogue_campaign",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("HORARY_NATIVE_LLAMA_TEST_MODEL", &cli.model)
        .env("HORARY_EVAL_EVIDENCE", directory)
        .env("HORARY_EVAL_PHASE", "paired_prompt_trial")
        .env("HORARY_EVAL_ORIGIN_SHA256", &plan.discovery_manifest_sha256)
        .env("HORARY_EVAL_FIXED_CANDIDATE_SHA256", &plan.candidate_sha256)
        .env(
            "HORARY_EVAL_EXPERIMENT_ROLE",
            if candidate.is_some() {
                "candidate"
            } else {
                "control"
            },
        )
        .env("HORARY_EVAL_FILTER", ids.join(","))
        .env("HORARY_EVAL_BATCH", "1")
        .env("HORARY_EVAL_FULL", "0")
        .env("HORARY_EVAL_CASE_SECONDS", cli.case_seconds.to_string())
        .env("HORARY_EVAL_MAX_CALLS", cli.max_calls.to_string())
        .env_remove("HORARY_EVAL_PROGRAM")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some(path) = candidate {
        command.env("HORARY_EVAL_PROGRAM", path);
    }
    println!(
        "Native {}: {} fixed cases",
        directory.file_name().unwrap_or_default().to_string_lossy(),
        ids.len()
    );
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let start = directory.with_extension("submitted.json");
    keep(
        &start,
        &json!({"pid":child.id(),"native_executable_sha256":plan.native_executable_sha256,"ids":ids,"candidate":candidate,"candidate_sha256":candidate.map(|_|&plan.candidate_sha256)}),
    )?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if !directory.join("completed.json").exists() {
        return Err(format!(
            "Native campaign stopped before completion (exit {:?}); preserve {}",
            status.code(),
            directory.display()
        ));
    }
    let report = load(&directory.join("report.json"))?;
    if report["manifest"]["model"]["sha256"] != plan.model_sha256 {
        return Err("Native trial did not use the fixed discovery model".into());
    }
    require_selected_cases(&report, ids)?;
    keep(
        &receipt,
        &json!({"exit_code":status.code(),"manifest_sha256":digest(read(&directory.join("manifest.json"))?),"report_sha256":digest(read(&directory.join("report.json"))?)}),
    )?;
    Ok(())
}

fn judge(
    cli: &Cli,
    campaign: &Path,
    state: &Path,
    validation: bool,
    candidate: &Path,
) -> Result<Vec<Value>> {
    horary_loop::run(Options {
        action: Action::Judge,
        campaign: campaign.into(),
        fixtures: cli.fixtures.clone(),
        state: state.into(),
        batch_size: 4,
        job_seconds: cli.judge_seconds,
        codex: cli.codex.clone(),
        retry_failed: false,
        validation,
        candidate: Some(candidate.into()),
        once: false,
        max_jobs: 256,
    })?;
    horary_loop::completed_judgments(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unused_cli_and_plan(root: &Path) -> (Cli, Plan) {
        let unavailable = root.join("must-not-execute");
        (
            Cli {
                candidate: unavailable.clone(),
                discovery: unavailable.clone(),
                split: unavailable.clone(),
                fixtures: unavailable.clone(),
                native_executable: unavailable.clone(),
                model: unavailable.clone(),
                state: root.into(),
                codex: unavailable.clone(),
                case_seconds: 1,
                max_calls: 1,
                judge_seconds: 1,
            },
            Plan {
                version: 1,
                candidate_sha256: digest("candidate"),
                native_executable_sha256: digest("native"),
                discovery_manifest_sha256: digest("discovery"),
                split_sha256: digest("split"),
                training_ids: vec!["investment-explicit".into()],
                validation_ids: vec!["investment-implicit".into()],
                case_seconds: 1,
                max_calls: 1,
                model: unavailable.clone(),
                model_sha256: digest("model"),
                codex: unavailable.clone(),
                fixtures: unavailable,
            },
        )
    }

    fn saved_native_cache(directory: &Path, report: &Value) -> Vec<(PathBuf, Vec<u8>)> {
        fs::create_dir(directory).unwrap();
        keep(&directory.join("report.json"), report).unwrap();
        keep(&directory.join("manifest.json"), &json!({"model":"fixed"})).unwrap();
        let receipt = directory.with_extension("native-receipt.json");
        keep(
            &receipt,
            &json!({
                "exit_code":0,
                "report_sha256":digest(read(&directory.join("report.json")).unwrap()),
                "manifest_sha256":digest(read(&directory.join("manifest.json")).unwrap()),
            }),
        )
        .unwrap();
        [
            directory.join("report.json"),
            directory.join("manifest.json"),
            receipt,
        ]
        .into_iter()
        .map(|path| {
            let bytes = read(&path).unwrap();
            (path, bytes)
        })
        .collect()
    }

    #[test]
    fn agreeing_cached_trials_cannot_overselect_the_plan_or_mutate_receipts() {
        let root = tempfile::tempdir().unwrap();
        let (cli, plan) = unused_cli_and_plan(root.path());
        let report = json!({"cases":[
            {"id":"investment-explicit"},
            {"id":"invariant-investment-explicit-owned-clause"},
        ]});
        let mut originals = Vec::new();
        for (phase, candidate) in [
            ("training-control", None),
            ("training-candidate", Some(cli.candidate.as_path())),
        ] {
            let directory = root.path().join(phase);
            originals.extend(saved_native_cache(&directory, &report));
            assert!(
                native(&cli, &directory, &plan.training_ids, candidate, &plan)
                    .unwrap_err()
                    .contains("exact selected case set")
            );
            assert!(!directory.with_extension("submitted.json").exists());
            assert!(!directory.with_extension("stdout.log").exists());
            assert!(!directory.with_extension("stderr.log").exists());
        }
        for (path, bytes) in originals {
            assert_eq!(read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn exact_cached_trial_reuses_receipts_without_starting_a_process() {
        let root = tempfile::tempdir().unwrap();
        let (cli, plan) = unused_cli_and_plan(root.path());
        let directory = root.path().join("training-control");
        let originals =
            saved_native_cache(&directory, &json!({"cases":[{"id":"investment-explicit"}]}));
        native(&cli, &directory, &plan.training_ids, None, &plan).unwrap();
        assert!(!directory.with_extension("submitted.json").exists());
        for (path, bytes) in originals {
            assert_eq!(read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn exact_case_set_rejects_duplicates_and_missing_ids() {
        let ids = vec!["investment-explicit".into()];
        for report in [
            json!({"cases":[{"id":"investment-explicit"},{"id":"investment-explicit"}]}),
            json!({"cases":[{"id":"investment-explicit"},{}]}),
            json!({"cases":[{"id":"investment-explicit"},{"id":""}]}),
            json!({"cases":[{"id":"investment-implicit"}]}),
            json!({"cases":[]}),
        ] {
            assert!(require_selected_cases(&report, &ids).is_err());
        }
        assert!(
            require_selected_cases(&json!({"cases":[{"id":"investment-explicit"}]}), &ids,).is_ok()
        );
    }

    #[test]
    fn method_scope_selects_all_three_cases_without_exposing_other_methods() {
        let mut p: Value = json!({"version":1,"id":"test","baseline_manifest_sha256":digest("m"),"overrides":[{"stage":"conversation","recognition_phase":null,"method":"lost_animal","expected_guide_sha256":digest("g"),"replacement_text":"A sufficiently long focused teaching body which preserves input authority and the actual native constraints.","edits":[]}],"rationale":"r","evidence":[{"case_id":"pet-missing","file":"f","json_pointer":"","sha256":digest("f")}],"training_case_ids":["pet-missing"],"holdout_case_ids":["pet-explicit"]});
        let program = Program::parse(&serde_json::to_vec(&p).unwrap()).unwrap();
        let split = Split {
            cases: vec![
                SplitCase {
                    id: "pet-missing".into(),
                    method: "lost_animal".into(),
                    partition: "training".into(),
                },
                SplitCase {
                    id: "pet-explicit".into(),
                    method: "lost_animal".into(),
                    partition: "reserved_validation".into(),
                },
                SplitCase {
                    id: "job-explicit".into(),
                    method: "new_job".into(),
                    partition: "reserved_validation".into(),
                },
            ],
        };
        assert_eq!(
            select(&program, &split).unwrap(),
            (vec!["pet-missing".into()], vec!["pet-explicit".into()])
        );
        p["overrides"][0]["method"] = Value::Null;
        let global = Program::parse(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(select(&global, &split).unwrap().1.len(), 2);
    }
}
