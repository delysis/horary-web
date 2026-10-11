//! A durable, sequential list of conventional DsRust compiler invocations.
//! This is deliberately not a search, retry or recovery engine. One Models
//! instance owns provider pacing/counters/caches for the entire queue.
use crate::{
    book::Book,
    catalogue,
    functions::{self, Fixture},
    google,
    journey::{self, Journey},
    optimize::{self, CompileOptions, Graded},
    runner::{self, Models},
};
use anyhow::{Context, Result, ensure};
use dsrust::{Evaluate, Module};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_json_atomic(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().context("No artifact parent")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .context("No artifact filename")?
        .to_string_lossy();
    let temporary = parent.join(format!(
        ".{name}.{}.{}.partial",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::SeqCst)
    ));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn now() -> Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Component,
    CompleteFirstMessageGate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub method: String,
    pub function: String,
    pub kind: Kind,
    pub dependencies: Vec<String>,
    pub metric_calls: usize,
    pub scope: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32,
    pub executable_sha256: String,
    pub book_sha256: String,
    pub source_sha256: BTreeMap<String, String>,
    pub banks_sha256: BTreeMap<String, String>,
    pub case_banks_canonical_sha256: BTreeMap<String, String>,
    pub component_datasets_sha256: BTreeMap<String, String>,
    pub configuration: Value,
    pub bootstrap_demos: usize,
    pub requested_metric_calls: usize,
    pub jobs: Vec<Job>,
    pub scope: String,
    pub reservations: Value,
}

fn sources() -> BTreeMap<String, String> {
    let mut sources = optimize::teaching_sources();
    sources.extend(
        [
            ("queue.rs", include_bytes!("queue.rs").as_slice()),
            ("functions.rs", include_bytes!("functions.rs").as_slice()),
            ("optimize.rs", include_bytes!("optimize.rs").as_slice()),
            ("signatures.rs", include_bytes!("signatures.rs").as_slice()),
            ("catalogue.rs", include_bytes!("catalogue.rs").as_slice()),
            ("journey.rs", include_bytes!("journey.rs").as_slice()),
            ("book.rs", include_bytes!("book.rs").as_slice()),
            ("google.rs", include_bytes!("google.rs").as_slice()),
            ("codex.rs", include_bytes!("codex.rs").as_slice()),
            ("demo.rs", include_bytes!("demo.rs").as_slice()),
            ("main.rs", include_bytes!("main.rs").as_slice()),
            ("lib.rs", include_bytes!("lib.rs").as_slice()),
            ("runner.rs", include_bytes!("runner.rs").as_slice()),
            ("teachers.rs", include_bytes!("teachers.rs").as_slice()),
            ("preflight.rs", include_bytes!("preflight.rs").as_slice()),
            (
                "native_export.rs",
                include_bytes!("native_export.rs").as_slice(),
            ),
            ("Cargo.toml", include_bytes!("../Cargo.toml").as_slice()),
            ("Cargo.lock", include_bytes!("../Cargo.lock").as_slice()),
            (
                "coverage.json",
                include_bytes!("../coverage.json").as_slice(),
            ),
            (
                "reading-catalogue.json",
                include_bytes!("../../../docs/llm-process/reading-catalogue.json").as_slice(),
            ),
            (
                "source-manifest.json",
                include_bytes!("../../../docs/llm-process/source-manifest.json").as_slice(),
            ),
        ]
        .into_iter()
        .map(|(name, bytes)| (name.into(), google::digest(bytes))),
    );
    sources
}

fn banks(directory: &Path) -> Result<BTreeMap<String, String>> {
    let mut banks = BTreeMap::new();
    for method in runner::methods("all")? {
        let name = format!("{method}.jsonl");
        banks.insert(
            name.clone(),
            google::digest(fs::read(directory.join(name))?),
        );
    }
    for function in functions::AUXILIARY {
        let name = format!("functions/{function}.jsonl");
        banks.insert(
            name.clone(),
            google::digest(fs::read(directory.join(name))?),
        );
    }
    Ok(banks)
}

pub fn plan(
    examples: &Path,
    book_path: &Path,
    max_metrics: usize,
    configuration: Value,
) -> Result<Plan> {
    ensure!(max_metrics > 0, "GEPA needs a positive metric budget");
    catalogue::validate_all(examples)?;
    for function in functions::AUXILIARY {
        functions::cases(examples, function)?;
    }
    let book_sha256 = google::digest(fs::read(book_path)?);
    ensure!(
        book_sha256 == crate::book::OCR_SHA256,
        "Primary book bytes differ"
    );
    let preflight = preflight(examples, &Book::open(book_path)?)?;
    let component_datasets_sha256 = preflight["components"]
        .as_array()
        .context("No component preflight")?
        .iter()
        .map(|row| {
            Ok((
                row["job"].as_str().context("No component name")?.to_owned(),
                row["dataset_sha256"]
                    .as_str()
                    .context("No component digest")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut case_banks_canonical_sha256 = BTreeMap::new();
    for method in runner::methods("all")? {
        case_banks_canonical_sha256.insert(
            method.to_owned(),
            google::digest(serde_json::to_vec(&catalogue::cases(examples, method)?)?),
        );
    }
    let methods = runner::methods("all")?;
    let classifier_development = methods.len() * 4;
    // The GEPA budget is an iteration-boundary bound. Its initial validation
    // alone costs 168 on the mixed 840 bank; 24 would perform no optimization.
    let jobs = jobs(max_metrics)?;
    let component_fixed_evaluations =
        classifier_development * 3 + (methods.len() * 2 + functions::AUXILIARY.len()) * 12;
    let journey_fixed_evaluations = methods.len() * 12;
    let gepa_budget = jobs.iter().map(|job| job.metric_calls).sum::<usize>();
    Ok(Plan {version:1,executable_sha256:google::digest(fs::read(std::env::current_exe()?)?),book_sha256,source_sha256:sources(),banks_sha256:banks(examples)?,case_banks_canonical_sha256,component_datasets_sha256,configuration,
        bootstrap_demos:0,requested_metric_calls:max_metrics,jobs,
        scope:"Conventional pinned DsRust GEPA instruction optimization. The independent judge is not optimized. Candidate components and complete journeys never automatically install in the native app.".into(),
        reservations:json!({"component_fixed_student_evaluations":component_fixed_evaluations,"journey_fixed_evaluations":journey_fixed_evaluations,"journey_student_functions_per_complete_case":3,"gepa_soft_iteration_metric_budget":gepa_budget,
            "nominal_student_function_evaluations":component_fixed_evaluations+journey_fixed_evaluations*3+gepa_budget,
            "metric_budget_is_not_a_physical_submission_cap":true,"gepa_budget_can_overshoot_at_iteration_boundary":true,
            "models_and_physical_counters":"One shared student and teacher instance for the whole queue. Every provider retry counts against the same caller-configured caps and token window.",
            "forecast":"A call-count forecast, not elapsed time. Actual token counts, API pacing waits, teacher latency, semantic failures and cache hits determine elapsed time."})})
}

/// Check the actual Examples and port signatures before any provider call.
/// Canonical reading consultations are explicitly supplied component inputs;
/// this preflight does not claim that an agent acquired them from a question.
pub fn preflight(examples: &Path, book: &Book) -> Result<Value> {
    let model = Arc::new(dsrust::DummyLM::new([]));
    let mut rows = Vec::new();
    for job in jobs(1)?
        .into_iter()
        .filter(|job| job.kind == Kind::Component)
    {
        let fixtures = component_fixtures(examples, &job.method, &job.function, book)?;
        let examples = fixtures
            .iter()
            .map(|fixture| fixture.example.clone())
            .collect::<Vec<_>>();
        Graded::new(
            &job.method,
            &job.function,
            &examples,
            model.clone(),
            model.clone(),
        )?;
        rows.push(json!({"job":job.id,"function":job.function,"method":job.method,"examples":fixtures.len(),"dataset_sha256":optimize::fixtures_digest(&fixtures)?,"inputs_unique":true,"oracle_calls":0}));
    }
    Ok(
        json!({"components":rows,"actual_port_signatures":true,"declared_inputs_only":true,"provider_calls":0,"model_quality_qualified":false,"application_promoted":false}),
    )
}

fn component_fixtures(
    examples: &Path,
    method: &str,
    function: &str,
    book: &Book,
) -> Result<Vec<Fixture>> {
    if functions::AUXILIARY.contains(&function) {
        return functions::fixtures(examples, function, book);
    }
    let cases = if function == "classification" {
        runner::load_all(examples)?
    } else {
        catalogue::cases(examples, method)?
    };
    cases
        .iter()
        .map(|case| {
            Ok(Fixture {
                split: case.split.clone(),
                example: catalogue::example(case, function, book)?,
            })
        })
        .collect()
}

pub fn jobs(max_metrics: usize) -> Result<Vec<Job>> {
    ensure!(max_metrics > 0, "GEPA needs a positive metric budget");
    let methods = runner::methods("all")?;
    let classification_metrics = max_metrics.max(methods.len() * 4 * 2 + 6);
    let mut jobs=vec![Job { id:"classification".into(),method:"all".into(),function:"classification".into(),kind:Kind::Component,dependencies:vec![],metric_calls:classification_metrics,
        scope:"Mixed-family classification from original question; correct family is a private grading label".into() }];
    for function in functions::AUXILIARY {
        jobs.push(Job { id:(*function).into(),method:"all".into(),function:(*function).into(),kind:Kind::Component,dependencies:vec![],metric_calls:max_metrics,
            scope:"Auxiliary declared-input component; its native state decision is independently graded".into() });
    }
    for method in &methods {
        for function in ["extraction", "reading"] {
            jobs.push(Job { id:format!("{method}-{function}"),method:(*method).into(),function:function.into(),kind:Kind::Component,dependencies:vec![],metric_calls:max_metrics,
                scope:if function=="reading" {"Reading component with independently supplied accepted consultation and calculated native chart; no classification/extraction journey claimed"}else{"Extraction component with original words, device and this declared method contract; method classification is not counted"}.into() });
        }
        jobs.push(Job { id:format!("{method}-journey"),method:(*method).into(),function:"complete_first_message".into(),kind:Kind::CompleteFirstMessageGate,
            dependencies:vec!["classification".into(),format!("{method}-extraction"),format!("{method}-reading")],metric_calls:0,
            scope:"Fresh original-message journey selects its own method/branch, extracts its own inputs and calculates its own chart; source-matched compiled components only; this is not native application acceptance".into() });
    }
    Ok(jobs)
}

fn verify(saved: &Plan, examples: &Path, book_path: &Path) -> Result<()> {
    ensure!(
        saved.version == 1 && saved.bootstrap_demos == 0,
        "Unsupported queue policy"
    );
    ensure!(
        saved.source_sha256 == sources()
            && saved.executable_sha256 == google::digest(fs::read(std::env::current_exe()?)?),
        "Queue executable or integration source differs"
    );
    ensure!(
        saved.book_sha256 == crate::book::OCR_SHA256
            && saved.book_sha256 == google::digest(fs::read(book_path)?),
        "Primary source drifted before paid work"
    );
    ensure!(
        saved.banks_sha256 == banks(examples)? && saved.jobs == jobs(saved.requested_metric_calls)?,
        "Queue bank bytes or job membership/policy drifted before paid work"
    );
    Ok(())
}

pub fn verify_composition_plan(directory: &Path) -> Result<Plan> {
    let bytes = fs::read(directory.join("queue-plan.json"))?;
    let plan: Plan = serde_json::from_slice(&bytes)?;
    let started: Value = serde_json::from_slice(&fs::read(directory.join("queue-started.json"))?)?;
    ensure!(
        started["plan_sha256"] == google::digest(&bytes),
        "Immutable queue plan changed after the durable start receipt"
    );
    ensure!(
        plan.version == 1
            && plan.bootstrap_demos == 0
            && plan.source_sha256 == sources()
            && plan.executable_sha256 == google::digest(fs::read(std::env::current_exe()?)?)
            && plan.book_sha256 == crate::book::OCR_SHA256
            && plan.jobs == jobs(plan.requested_metric_calls)?,
        "Composition queue does not match this exact executable/source/book/policy"
    );
    Ok(plan)
}

pub fn write_plan(directory: &Path, plan: &Plan) -> Result<()> {
    fs::create_dir_all(directory)?;
    let path = directory.join("queue-plan.json");
    if path.exists() {
        ensure!(
            serde_json::from_slice::<Plan>(&fs::read(path)?)? == *plan,
            "Existing queue plan differs; preserve it and choose a new output"
        );
    } else {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(&serde_json::to_vec_pretty(plan)?)?;
        file.sync_all()?;
        fs::File::open(directory)?.sync_all()?;
    }
    Ok(())
}

fn transition(directory: &Path, job: &Job, state: &str, result: Value) -> Result<()> {
    let record = json!({"job":job.id,"state":state,"at_unix_ms":now()?,"result":result});
    let mut history = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("queue-transitions.jsonl"))?;
    writeln!(history, "{}", serde_json::to_string(&record)?)?;
    history.sync_data()?;
    write_json_atomic(
        &directory.join("jobs").join(&job.id).join("status.json"),
        &record,
    )
}

pub fn status(directory: &Path) -> Result<Value> {
    let plan: Plan = serde_json::from_slice(&fs::read(directory.join("queue-plan.json"))?)?;
    let mut states = Vec::new();
    for job in &plan.jobs {
        let path = directory.join("jobs").join(&job.id).join("status.json");
        let value = if path.exists() {
            serde_json::from_slice(&fs::read(path)?)?
        } else {
            json!({"job":job.id,"state":"not_started"})
        };
        states.push(value);
    }
    Ok(
        json!({"plan_sha256":google::digest(fs::read(directory.join("queue-plan.json"))?),"jobs":states,
        "candidate_components_only":true,"application_promoted":false}),
    )
}

async fn evaluate_journey(
    program: &Journey,
    cases: &[catalogue::Case],
    split: &str,
) -> dsrust::Evaluation {
    let failed = AtomicBool::new(false);
    Evaluate::new(cases.iter().filter(|case|case.split==split).map(journey::example).collect(),|input| async {
        ensure!(!failed.load(Ordering::SeqCst),"Journey stopped after an unobserved execution/review failure; no new call submitted");
        let result=program.forward(input).await;
        if result.is_err(){failed.store(true,Ordering::SeqCst);}
        result
    },Graded::metric).run().await
}

async fn gate(
    cases: &[catalogue::Case],
    method: &str,
    book: Arc<Book>,
    models: &Models,
    queue: &Path,
    directory: &Path,
) -> Result<Value> {
    let candidate = Journey::new(
        method,
        cases,
        book.clone(),
        models.student.clone(),
        models.teacher.clone(),
    )?
    .with_components(queue)?
    .with_stream(directory.join("candidate-observations.jsonl"))?;
    let mut composition = BTreeMap::new();
    for job in [
        "classification".to_owned(),
        format!("{method}-extraction"),
        format!("{method}-reading"),
    ] {
        let path = queue.join("jobs").join(job).join("compiled.json");
        ensure!(path.exists(), "Required compiled component is absent");
        composition.insert(path.display().to_string(), google::digest(fs::read(&path)?));
    }
    write_json_atomic(
        &directory.join("composition.json"),
        &json!({"required_components":composition,"expected_method_is_student_input":false,"expected_facts_are_student_input":false,"authored_chart_seed_is_chart_authority":false,"application_promoted":false}),
    )?;
    let baseline = Journey::new(
        method,
        cases,
        book.clone(),
        models.student.clone(),
        models.teacher.clone(),
    )?
    .with_stream(directory.join("baseline-observations.jsonl"))?;
    let before = evaluate_journey(&baseline, cases, "development").await;
    write_json_atomic(
        &directory.join("before.json"),
        &optimize::evaluation_receipt(&before),
    )?;
    ensure!(
        before.failure_count() == 0,
        "Unobserved baseline journey withholds composition comparison"
    );
    let after = evaluate_journey(&candidate, cases, "development").await;
    write_json_atomic(
        &directory.join("after.json"),
        &optimize::evaluation_receipt(&after),
    )?;
    ensure!(
        after.failure_count() == 0,
        "Unobserved candidate journey withholds held-out gate"
    );
    let held = evaluate_journey(&candidate, cases, "test").await;
    write_json_atomic(
        &directory.join("held.json"),
        &optimize::evaluation_receipt(&held),
    )?;
    let observed = held.failure_count() == 0;
    let passed = observed
        && after.results.iter().chain(&held.results).all(|row| {
            row.prediction
                .as_ref()
                .is_ok_and(|prediction| prediction.get("__passed") == Some(&json!(true)))
        });
    let result = json!({"method":method,"scope":"Original-message DSPy journey, not native application acceptance","before_development":before.score,"after_development":after.score,"held_out":if observed{Some(held.score)}else{None},"execution_observed":observed,"complete_first_message_pass":passed,"application_promoted":false,"on_device_qualified":false,"packaged_application_qualified":false});
    write_json_atomic(&directory.join("result.json"), &result)?;
    ensure!(observed, "Unobserved held-out journey remains unqualified");
    Ok(result)
}

/// Start once. The durable guard refuses every second execution, including a
/// crashed or known-failed predecessor. Recovery requires a separately reviewed
/// plan/output; this queue never buys an implicit replay of uncertain work.
pub async fn execute(
    plan: &Plan,
    examples: &Path,
    book_path: &Path,
    book: Arc<Book>,
    models: &Models,
    directory: &Path,
) -> Result<Value> {
    verify(plan, examples, book_path)?;
    write_plan(directory, plan)?;
    let mut guard = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("queue-started.json"))
        .context(
            "This queue already started; inspect its retained state rather than resubmitting",
        )?;
    guard.write_all(&serde_json::to_vec_pretty(&json!({"started_unix_ms":now()?,"pid":std::process::id(),"plan_sha256":google::digest(fs::read(directory.join("queue-plan.json"))?)}))?)?;
    guard.sync_all()?;
    fs::File::open(directory)?.sync_all()?;
    for job in &plan.jobs {
        transition(directory, job, "queued", Value::Null)?;
    }
    let mut completed = BTreeMap::new();
    for job in &plan.jobs {
        ensure!(
            job.dependencies.iter().all(|id| completed.contains_key(id)),
            "Required predecessor was not observed"
        );
        transition(directory, job, "running", Value::Null)?;
        let output = directory.join("jobs").join(&job.id);
        let result = async {
            write_plan(directory, plan)?;
            verify(plan, examples, book_path)?;
            match job.kind {
                Kind::Component => {
                    let fixtures = component_fixtures(examples, &job.method, &job.function, &book)?;
                    ensure!(
                        plan.component_datasets_sha256.get(&job.id)
                            == Some(&optimize::fixtures_digest(&fixtures)?),
                        "Actual component inputs/labels/split drifted before paid work"
                    );
                    optimize::compile_fixtures(
                        &fixtures,
                        &job.method,
                        &job.function,
                        models,
                        &output,
                        CompileOptions {
                            max_metrics: job.metric_calls,
                            bootstrap_demos: plan.bootstrap_demos,
                        },
                    )
                    .await
                }
                Kind::CompleteFirstMessageGate => {
                    gate(
                        &catalogue::cases(examples, &job.method)?,
                        &job.method,
                        book.clone(),
                        models,
                        directory,
                        &output,
                    )
                    .await
                }
            }
        }
        .await;
        match result {
            Ok(result) => {
                let passed = result[if job.kind == Kind::Component {
                    "component_pass"
                } else {
                    "complete_first_message_pass"
                }] == true;
                transition(
                    directory,
                    job,
                    if passed { "complete" } else { "unqualified" },
                    result.clone(),
                )?;
                completed.insert(job.id.clone(), result);
            }
            Err(error) => {
                transition(
                    directory,
                    job,
                    "failed",
                    json!({"error":format!("{error:#}"),"observed_semantic_score":null,"automatic_retry":false,"later_jobs":"Remain queued and unexecuted"}),
                )?;
                let receipt = json!({"state":"stopped","failed_job":job.id,"completed":completed,"remaining_jobs_executed":false,"application_promoted":false});
                write_json_atomic(&directory.join("queue-result.json"), &receipt)?;
                return Err(error.context(format!(
                    "Queue stopped at {}; paid work and partial traces are retained",
                    job.id
                )));
            }
        }
        write_json_atomic(&directory.join("queue-status.json"), &status(directory)?)?;
    }
    let receipt = json!({"state":"closed","completed":completed,"application_promoted":false,"qualification":"Read component and complete-message gate results separately; native application and on-device acceptance remain separate"});
    write_json_atomic(&directory.join("queue-result.json"), &receipt)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_queue_has_every_function_and_separate_journey_dependencies() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        // All 840 banks plus the60 auxiliaries are validated independently.
        for function in functions::AUXILIARY {
            assert_eq!(functions::cases(&examples, function).unwrap().len(), 20);
        }
        let methods = runner::methods("all").unwrap();
        assert_eq!(methods.len(), 42);
        let jobs = jobs(24).unwrap();
        assert_eq!(jobs.len(), 130);
        assert_eq!(
            jobs.iter()
                .map(|job| &job.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            130
        );
        assert_eq!(jobs[0].id, "classification");
        assert_eq!(jobs[0].metric_calls, 342);
        for method in methods {
            let extraction = jobs
                .iter()
                .find(|job| job.id == format!("{method}-extraction"))
                .unwrap();
            let reading = jobs
                .iter()
                .find(|job| job.id == format!("{method}-reading"))
                .unwrap();
            let journey = jobs
                .iter()
                .find(|job| job.id == format!("{method}-journey"))
                .unwrap();
            assert_eq!(extraction.kind, Kind::Component);
            assert!(reading.scope.contains("independently supplied"));
            assert_eq!(
                journey.dependencies,
                vec![
                    "classification".to_owned(),
                    extraction.id.clone(),
                    reading.id.clone()
                ]
            );
            assert_eq!(journey.kind, Kind::CompleteFirstMessageGate);
            assert!(journey.scope.contains("selects its own method"));
        }
    }
    #[test]
    fn atomic_status_replacement_retains_transition_history() {
        let temporary = tempfile::tempdir().unwrap();
        let job = Job {
            id: "test".into(),
            method: "all".into(),
            function: "place".into(),
            kind: Kind::Component,
            dependencies: vec![],
            metric_calls: 1,
            scope: "Synthetic mechanics".into(),
        };
        transition(temporary.path(), &job, "queued", Value::Null).unwrap();
        transition(temporary.path(), &job, "running", Value::Null).unwrap();
        transition(temporary.path(), &job, "failed", json!({"score":null})).unwrap();
        let history = fs::read_to_string(temporary.path().join("queue-transitions.jsonl")).unwrap();
        assert_eq!(history.lines().count(), 3);
        let status: Value = serde_json::from_slice(
            &fs::read(temporary.path().join("jobs/test/status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status["state"], "failed");
    }

    #[test]
    fn composition_requires_the_started_plan_and_this_exact_execution_source() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = Plan {
            version: 1,
            executable_sha256: google::digest(fs::read(std::env::current_exe().unwrap()).unwrap()),
            book_sha256: crate::book::OCR_SHA256.into(),
            source_sha256: sources(),
            banks_sha256: BTreeMap::new(),
            case_banks_canonical_sha256: BTreeMap::new(),
            component_datasets_sha256: BTreeMap::from([(
                "classification".into(),
                "fixture-digest".into(),
            )]),
            configuration: json!({"synthetic_mechanics_only": true}),
            bootstrap_demos: 0,
            requested_metric_calls: 1,
            jobs: jobs(1).unwrap(),
            scope: "Synthetic provenance boundary; no case or model-quality claim".into(),
            reservations: Value::Null,
        };
        write_plan(directory.path(), &plan).unwrap();
        assert!(verify_composition_plan(directory.path()).is_err());
        let seal = google::digest(fs::read(directory.path().join("queue-plan.json")).unwrap());
        write_json_atomic(
            &directory.path().join("queue-started.json"),
            &json!({"plan_sha256": seal}),
        )
        .unwrap();
        assert_eq!(verify_composition_plan(directory.path()).unwrap(), plan);

        plan.component_datasets_sha256
            .insert("classification".into(), "different-fixture-digest".into());
        assert!(write_plan(directory.path(), &plan).is_err());
        write_json_atomic(&directory.path().join("queue-plan.json"), &json!(&plan)).unwrap();
        let error = verify_composition_plan(directory.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("changed after the durable start")
        );

        plan.source_sha256.insert(
            "native-reading-method.rs".into(),
            "different-native-evidence".into(),
        );
        write_json_atomic(&directory.path().join("queue-plan.json"), &json!(&plan)).unwrap();
        let seal = google::digest(fs::read(directory.path().join("queue-plan.json")).unwrap());
        write_json_atomic(
            &directory.path().join("queue-started.json"),
            &json!({"plan_sha256": seal}),
        )
        .unwrap();
        let error = verify_composition_plan(directory.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exact executable/source/book/policy")
        );
    }
}
