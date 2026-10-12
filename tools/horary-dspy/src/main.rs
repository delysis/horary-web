#![forbid(unsafe_code)]
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use dsrust::{Module, lm::DynChatModel};
use horary_dspy::{
    book, catalogue, classification_export, codex, continuation, google, local, native_export,
    optimize, preflight, queue, runner, signatures, teachers,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Teacher {
    Pool,
    Gemini,
    Codex,
}
#[derive(Parser)]
struct Args {
    #[arg(long, value_enum, default_value = "pool")]
    teacher: Teacher,
    #[arg(long, default_value = "/opt/homebrew/bin/codex")]
    codex: PathBuf,
    #[arg(long,default_value=book::DEFAULT_OCR)]
    book: PathBuf,
    #[arg(long,default_value=google::CREDENTIAL_FILE)]
    credential_file: PathBuf,
    /// Select the resident loopback server for this run's student only.
    #[arg(long)]
    student_local_url: Option<String>,
    #[arg(long, default_value = local::MODEL)]
    student_model: String,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Bind all conventional DSPy jobs to executable, book, corpus and provider policy.
    QueuePlan {
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 24)]
        max_metric_calls: usize,
        #[arg(long, default_value_t = 14_000)]
        student_tokens_per_minute: usize,
        #[arg(long, default_value_t = 224_000)]
        student_probe_ceiling: usize,
        #[arg(long, default_value_t = 18_000)]
        student_call_cap: usize,
        #[arg(long, default_value_t = 10_000)]
        teacher_call_cap: usize,
        #[arg(long)]
        continuation_manifest: Option<PathBuf>,
    },
    /// Execute a sealed queue once; provider clients and caps span every job.
    QueueExecute {
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        continuation_manifest: Option<PathBuf>,
    },
    /// Inspect source-bound closed work and prepare carrier-only continuation metadata.
    QueueContinuationInspect {
        #[arg(long)]
        parent: PathBuf,
        #[arg(long)]
        closure_receipt: PathBuf,
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        review_note: String,
    },
    QueueStatus {
        #[arg(long)]
        output: PathBuf,
    },
    /// Format every baseline exchange, then count a source-bound sample or all.
    Preflight {
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        all_counts: bool,
    },
    ExportRequirements {
        #[arg(long)]
        method: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Prepare an explicit native-format teaching seed; does not install or qualify it.
    ExportNative {
        #[arg(long)]
        compiled: PathBuf,
        #[arg(long)]
        baseline_manifest: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Inspect the explicit classifier producer and native target contract.
    ExportClassificationRequirements {
        #[arg(long)]
        output: PathBuf,
    },
    /// Export authored classifier teaching without installing or qualifying it.
    ExportNativeClassification {
        #[arg(long)]
        compiled: PathBuf,
        #[arg(long)]
        producer_source: PathBuf,
        #[arg(long)]
        baseline_manifest: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    ImportCodex {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    Models {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    TeacherCheck {
        #[arg(long)]
        method: String,
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Check {
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
    },
    Inspect {
        #[arg(long)]
        method: String,
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
    },
    Signatures {
        #[arg(long)]
        output: PathBuf,
    },
    Run {
        #[arg(long)]
        method: String,
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 14_000)]
        student_tokens_per_minute: usize,
        /// An initial transport check uses training cases only; not a full score.
        #[arg(long)]
        training_limit: Option<usize>,
        /// Replay completed student calls only; useful when retrying a known failed review.
        #[arg(long)]
        cached_student_only: bool,
        #[arg(long)]
        cached_teacher_only: bool,
    },
    OptimizeClassification {
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 2048)]
        max_metric_calls: usize,
        #[arg(long, default_value_t = 14_000)]
        student_tokens_per_minute: usize,
        /// Exact existing training IDs for a bounded round; original splits remain fixed.
        #[arg(long)]
        training_case: Vec<String>,
        #[arg(long)]
        development_case: Vec<String>,
        #[arg(long)]
        test_case: Vec<String>,
        #[arg(long, default_value_t = 2)]
        bootstrap_demos: usize,
    },
    /// Optimize one NewJob Predict with the complete salient family source.
    OptimizeReading {
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 16)]
        max_metric_calls: usize,
        #[arg(long, default_value_t = 72)]
        student_call_cap: usize,
        #[arg(long, default_value_t = 96)]
        teacher_call_cap: usize,
    },
    /// Compare an already proposed instruction on one complete development pool.
    CompareClassification {
        #[arg(long)]
        method: String,
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        candidate_instructions: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Optimize {
        #[arg(long)]
        method: String,
        #[arg(long, default_value = "examples")]
        examples: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 24)]
        max_metric_calls: usize,
        #[arg(long, default_value_t = 14_000)]
        student_tokens_per_minute: usize,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(url) = &args.student_local_url {
        local::validate_url(url)?;
        ensure!(
            matches!(
                &args.command,
                Command::Run { .. }
                    | Command::Optimize { .. }
                    | Command::OptimizeClassification { .. }
                    | Command::OptimizeReading { .. }
                    | Command::CompareClassification { .. }
            ),
            "Local selection applies to run/optimize; sealed hosted queues remain unchanged"
        );
    }
    match args.command {
        Command::QueuePlan {
            examples,
            models,
            output,
            max_metric_calls,
            student_tokens_per_minute,
            student_probe_ceiling,
            student_call_cap,
            teacher_call_cap,
            continuation_manifest,
        } => {
            ensure!(
                student_call_cap > 0 && teacher_call_cap > 0,
                "Physical-call bounds must be positive"
            );
            ensure!(
                student_probe_ceiling >= student_tokens_per_minute,
                "Probe ceiling must cover the initial hint"
            );
            let bytes = fs::read(models)?;
            let listing: serde_json::Value = serde_json::from_slice(&bytes)?;
            google::model_context_limit(&listing, google::STUDENT)?;
            let configuration = QueueConfiguration {
                teacher: args.teacher,
                model_listing_sha256: google::digest(&bytes),
                teacher_sequence: teachers::available(&listing)?
                    .iter()
                    .map(|info| info.name.clone())
                    .collect(),
                codex: args.codex.clone(),
                codex_sha256: google::digest(fs::read(&args.codex)?),
                student_tokens_per_minute,
                student_probe_ceiling,
                student_call_cap,
                teacher_call_cap,
                teacher_tokens_per_minute: 120_000,
                continuation: continuation_manifest
                    .as_deref()
                    .map(continuation::FileSeal::read)
                    .transpose()?,
            };
            let plan = queue::plan(
                &examples,
                &args.book,
                max_metric_calls,
                serde_json::to_value(&configuration)?,
            )?;
            if let Some(seal) = &configuration.continuation {
                ensure!(
                    google::digest(fs::read(&seal.path)?) == seal.sha256,
                    "Continuation manifest changed"
                );
                continuation::verify(&continuation::load(&seal.path)?, &plan)?;
            }
            // A fresh output prevents changing a live queue's available routes.
            fs::create_dir(&output)?;
            fs::write(output.join("model-listing.json"), bytes)?;
            queue::write_plan(&output, &plan)?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        Command::QueueExecute {
            examples,
            plan,
            continuation_manifest,
        } => {
            let output = plan
                .parent()
                .context("Queue plan must have a parent directory")?;
            ensure!(
                plan.file_name()
                    .is_some_and(|name| name == "queue-plan.json"),
                "Use the bound queue-plan.json"
            );
            let saved: queue::Plan = serde_json::from_slice(&fs::read(&plan)?)?;
            let configuration: QueueConfiguration =
                serde_json::from_value(saved.configuration.clone())?;
            ensure!(
                continuation_manifest
                    .as_deref()
                    .map(continuation::FileSeal::read)
                    .transpose()?
                    == configuration.continuation,
                "Explicit continuation manifest differs from the sealed queue configuration"
            );
            let carried = configuration
                .continuation
                .as_ref()
                .map(|seal| {
                    ensure!(
                        google::digest(fs::read(&seal.path)?) == seal.sha256,
                        "Continuation manifest changed"
                    );
                    let manifest = continuation::load(&seal.path)?;
                    continuation::import(&manifest, &saved, output)?;
                    Ok::<_, anyhow::Error>(manifest)
                })
                .transpose()?;
            let model_bytes = fs::read(output.join("model-listing.json"))?;
            ensure!(
                google::digest(&model_bytes) == configuration.model_listing_sha256,
                "Model listing drifted"
            );
            ensure!(
                google::digest(fs::read(&configuration.codex)?) == configuration.codex_sha256,
                "Codex executable drifted"
            );
            let listing: serde_json::Value = serde_json::from_slice(&model_bytes)?;
            ensure!(
                teachers::available(&listing)?
                    .iter()
                    .map(|info| info.name.clone())
                    .collect::<Vec<_>>()
                    == configuration.teacher_sequence,
                "Actual available teacher order differs from the sealed configuration"
            );
            let http = google::client(&args.credential_file)?;
            let student_budget = google::CallBudget::seeded(
                configuration.student_call_cap,
                carried.as_ref().map_or(0, |m| m.student.inherited),
            )?;
            let teacher_budget = google::CallBudget::seeded(
                configuration.teacher_call_cap,
                carried.as_ref().map_or(0, |m| m.teacher.inherited),
            )?;
            let student_limiter = google::RateLimiter::with_probe_ceiling(
                configuration.student_tokens_per_minute,
                configuration.student_probe_ceiling,
            )?;
            let student_info: google::ModelInfo =
                serde_json::from_value(listing["student"].clone()).context("No listed student")?;
            let mut student = google::Google::from_listing(
                student_info,
                http.clone(),
                output.join("student"),
                student_budget,
                student_limiter,
            )?;
            if let Some(slot) = carried
                .as_ref()
                .and_then(|manifest| manifest.student_recovery.clone())
            {
                student = student.with_student_recovery(slot)?;
            }
            let student = Arc::new(student);
            let teacher = if let Some(carried) = &carried {
                ensure!(
                    matches!(configuration.teacher, Teacher::Pool),
                    "Continuation requires the original complete text-teacher pool"
                );
                let fallback = codex::Codex::new(
                    configuration.codex.clone(),
                    output.join("teacher/codex"),
                    configuration.teacher_call_cap,
                )?;
                Arc::new(
                    teachers::TeacherPool::new(
                        http,
                        &listing,
                        output.join("teacher"),
                        teacher_budget,
                        configuration.teacher_tokens_per_minute,
                        Some(fallback),
                    )?
                    .with_start_route(carried.teacher_start_route)?,
                ) as Arc<dyn DynChatModel>
            } else {
                pooled_teacher(
                    configuration.teacher,
                    configuration.codex,
                    http,
                    output.join("teacher"),
                    configuration.teacher_call_cap,
                    teacher_budget,
                    &listing,
                    configuration.teacher_tokens_per_minute,
                )?
            };
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(configuration.teacher).into(),
                student_id: google::STUDENT.into(),
            };
            let result = queue::execute(
                &saved,
                &examples,
                &args.book,
                Arc::new(book::Book::open(&args.book)?),
                &models,
                output,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::QueueStatus { output } => println!(
            "{}",
            serde_json::to_string_pretty(&queue::status(&output)?)?
        ),
        Command::QueueContinuationInspect {
            parent,
            closure_receipt,
            examples,
            review_note,
        } => {
            let previous: queue::Plan =
                serde_json::from_slice(&fs::read(parent.join("run/queue-plan.json"))?)?;
            let current = queue::plan(
                &examples,
                &args.book,
                previous.requested_metric_calls,
                previous.configuration.clone(),
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&continuation::prepare(
                    &parent,
                    &closure_receipt,
                    &current,
                    &review_note
                )?)?
            );
        }
        Command::Preflight {
            examples,
            models,
            output,
            all_counts,
        } => {
            let listing: serde_json::Value = serde_json::from_slice(&fs::read(models)?)?;
            let limit = google::model_context_limit(&listing, google::STUDENT)?;
            let result = preflight::run(
                &examples,
                &book::Book::open(&args.book)?,
                &google::client(&args.credential_file)?,
                &output,
                limit,
                all_counts,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::ExportRequirements { method, output } => fs::write(
            output,
            serde_json::to_vec_pretty(&native_export::requirements(&method)?)?,
        )?,
        Command::ExportNative {
            compiled,
            baseline_manifest,
            plan,
            output,
        } => {
            let compiled_bytes = fs::read(compiled)?;
            let manifest_bytes = fs::read(baseline_manifest)?;
            let plan_bytes = fs::read(plan)?;
            let plan: native_export::ExportPlan = serde_json::from_slice(&plan_bytes)?;
            let export = native_export::export(&compiled_bytes, &manifest_bytes, &plan)?;
            // Validation precedes creating evidence. Never overwrite an earlier
            // proposal or relabel a changed-format seed as the DSPy winner.
            fs::create_dir(&output)?;
            fs::write(output.join("compiled-source.json"), compiled_bytes)?;
            fs::write(output.join("baseline-manifest-source.json"), manifest_bytes)?;
            fs::write(output.join("export-plan.json"), plan_bytes)?;
            fs::write(
                output.join("program.json"),
                serde_json::to_vec_pretty(&export.program)?,
            )?;
            fs::write(
                output.join("export-receipt.json"),
                serde_json::to_vec_pretty(&export.receipt)?,
            )?;
            println!("Native teaching seed exported; application validation is still required");
        }
        Command::ExportClassificationRequirements { output } => fs::write(
            output,
            serde_json::to_vec_pretty(&classification_export::requirements()?)?,
        )?,
        Command::ExportNativeClassification {
            compiled,
            producer_source,
            baseline_manifest,
            plan,
            output,
        } => {
            let compiled_bytes = fs::read(compiled)?;
            let producer_bytes = fs::read(producer_source)?;
            let manifest_bytes = fs::read(baseline_manifest)?;
            let plan_bytes = fs::read(plan)?;
            let plan: classification_export::Plan = serde_json::from_slice(&plan_bytes)?;
            let export = classification_export::export(
                &compiled_bytes,
                &producer_bytes,
                &manifest_bytes,
                &plan,
            )?;
            // Source and adapter validation precedes creating an uninstalled
            // proposal. The exact producer artifacts remain separately bound.
            fs::create_dir(&output)?;
            fs::write(output.join("compiled-source.json"), compiled_bytes)?;
            fs::write(output.join("producer-source-receipt.json"), producer_bytes)?;
            fs::write(output.join("baseline-manifest-source.json"), manifest_bytes)?;
            fs::write(output.join("export-plan.json"), plan_bytes)?;
            fs::write(
                output.join("program.json"),
                serde_json::to_vec_pretty(&export.program)?,
            )?;
            fs::write(
                output.join("export-receipt.json"),
                serde_json::to_vec_pretty(&export.receipt)?,
            )?;
            println!(
                "Native classification seed exported; application validation is still required"
            );
        }
        Command::ImportCodex {
            source,
            destination,
        } => codex::import_completed(&source, &destination)?,
        Command::TeacherCheck {
            method,
            examples,
            output,
        } => {
            let case = catalogue::cases(&examples, &method)?
                .into_iter()
                .find(|c| c.split == "train")
                .ok_or_else(|| anyhow::anyhow!("No training case"))?;
            fs::create_dir_all(&output)?;
            let teacher = Arc::new(google::Google::new(
                google::TEACHER,
                google::client(&args.credential_file)?,
                output.join("teacher"),
                1,
                120_000,
            )?);
            let p=dsrust::Predict::from_signature(signatures::auxiliary("classification")?).with_lm(teacher).with_adapter(dsrust::adapter::ChatAdapter::without_json_fallback()).with_config(dsrust::lm::LmConfig{max_tokens:Some(4096),temperature:Some(0.),..Default::default()}).forward(dsrust::input!{question:case.question,conversation:"[]",catalogue:catalogue::method_index()?}).await?;
            fs::write(
                output.join("teacher-classification.json"),
                serde_json::to_vec_pretty(&p.example.rendered())?,
            )?;
            println!(
                "Teacher classification completed; this is transport/stage evidence, not a qualified reading"
            );
        }
        Command::Models { output } => {
            let http = google::client(&args.credential_file)?;
            let models = google::models(&http).await?;
            println!("{}", serde_json::to_string_pretty(&models)?);
            if let Some(path) = output {
                fs::write(path, serde_json::to_vec_pretty(&models)?)?;
            }
            ensure!(
                !models["student"].is_null(),
                "Requested student is not available on this credential; no substitution performed"
            );
        }
        Command::Check { examples } => println!(
            "{}",
            serde_json::to_string_pretty(&catalogue::validate_all(&examples)?)?
        ),
        Command::Inspect { method, examples } => {
            let book = book::Book::open(&args.book)?;
            for case in catalogue::cases(&examples, &method)? {
                let inputs = catalogue::example(&case, "reading", &book)?.inputs()?;
                let bytes = serde_json::to_vec(&inputs.rendered())?.len();
                println!(
                    "{} {} reading_component_bytes={} native_chart_bytes={} source_pages={:?}",
                    case.id,
                    case.split,
                    bytes,
                    serde_json::to_vec(&catalogue::chart_evidence(&case)?)?.len(),
                    case.expected.source_pages
                );
            }
        }
        Command::Signatures { output } => {
            let mut rows = Vec::new();
            for method in signatures::METHODS {
                for (function, sig) in [
                    ("reading", signatures::reading(method)?),
                    ("extraction", signatures::extraction(method)?),
                ] {
                    rows.push(serde_json::json!({"method":method,"function":function,"instructions":sig.instructions,"inputs":sig.inputs.iter().map(|f|serde_json::json!({"name":f.name,"description":f.desc,"type":f.annotation()})).collect::<Vec<_>>(),"outputs":sig.outputs.iter().map(|f|serde_json::json!({"name":f.name,"description":f.desc,"type":f.annotation()})).collect::<Vec<_>>() }));
                }
            }
            for function in ["classification", "place", "moment", "update", "judge"] {
                let sig = signatures::auxiliary(function)?;
                rows.push(serde_json::json!({"function":function,"instructions":sig.instructions,"inputs":sig.inputs.iter().map(|f|&f.name).collect::<Vec<_>>(),"outputs":sig.outputs.iter().map(|f|&f.name).collect::<Vec<_>>() }));
            }
            let sig = horary_dspy::reading::selector_signature();
            rows.push(serde_json::json!({"function":"reading_branch","module_version":horary_dspy::reading::VERSION,"named_predictor":"reading_branch.<family>","instructions":sig.instructions,"inputs":sig.inputs.iter().map(|f|serde_json::json!({"name":f.name,"description":f.desc,"type":f.annotation()})).collect::<Vec<_>>(),"outputs":sig.outputs.iter().map(|f|serde_json::json!({"name":f.name,"description":f.desc,"type":f.annotation()})).collect::<Vec<_>>() }));
            fs::write(output, serde_json::to_vec_pretty(&rows)?)?;
            println!("{} signatures exported", rows.len());
        }
        Command::Run {
            method,
            examples,
            output,
            student_tokens_per_minute,
            training_limit,
            cached_student_only,
            cached_teacher_only,
        } => {
            let methods = runner::methods(&method)?;
            let banks = methods
                .iter()
                .map(|m| {
                    let mut c = catalogue::cases(&examples, m)?;
                    if let Some(n) = training_limit {
                        ensure!(n > 0, "Training limit must be positive");
                        c.retain(|c| c.split == "train");
                        c.truncate(n);
                    }
                    Ok(c)
                })
                .collect::<Result<Vec<_>>>()?;
            let all = banks.iter().flatten().cloned().collect::<Vec<_>>();
            ensure!(
                !(cached_student_only && args.student_local_url.is_some()),
                "Local runs execute the actual student; cached hosted replay remains separate"
            );
            let teacher_id = teacher_identity(args.teacher);
            runner::manifest(
                &output,
                &all,
                &args.book,
                serde_json::json!({"command":"run","student":student_identity(args.student_local_url.as_deref(), &args.student_model),"student_local_url":args.student_local_url,"methods":methods,"teacher":teacher_id,"training_limit":training_limit,"cached_student_only":cached_student_only,"cached_teacher_only":cached_teacher_only}),
            )?;
            let book = Arc::new(book::Book::open(&args.book)?);
            let http = google::client(&args.credential_file)?;
            let student = selected_student(
                args.student_local_url.as_deref(),
                &args.student_model,
                http.clone(),
                output.join("student"),
                if cached_student_only {
                    0
                } else {
                    240 * methods.len()
                },
                student_tokens_per_minute,
            )?;
            let teacher = teacher_model(
                args.teacher,
                args.codex,
                http,
                output.join("teacher"),
                if cached_teacher_only {
                    0
                } else {
                    80 * methods.len()
                },
            )
            .await?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_id.into(),
                student_id: student_identity(
                    args.student_local_url.as_deref(),
                    &args.student_model,
                ),
            };
            for (m, cases) in methods.iter().zip(&banks) {
                let directory = if methods.len() == 1 {
                    output.clone()
                } else {
                    output.join(m)
                };
                let result = runner::run(
                    cases,
                    m,
                    book.clone(),
                    &models,
                    &directory,
                    training_limit.is_some(),
                )
                .await?;
                println!("{}", serde_json::to_string(&result)?);
            }
        }
        Command::Optimize {
            method,
            examples,
            output,
            max_metric_calls,
            student_tokens_per_minute,
        } => {
            let methods = runner::methods(&method)?;
            let banks = methods
                .iter()
                .map(|m| catalogue::cases(&examples, m))
                .collect::<Result<Vec<_>>>()?;
            let all = banks.iter().flatten().cloned().collect::<Vec<_>>();
            runner::manifest(
                &output,
                &all,
                &args.book,
                serde_json::json!({"command":"optimize","student":student_identity(args.student_local_url.as_deref(), &args.student_model),"student_local_url":args.student_local_url,"methods":methods,"teacher":teacher_identity(args.teacher),"max_metric_calls_per_family":max_metric_calls}),
            )?;
            let book = Arc::new(book::Book::open(&args.book)?);
            let http = google::client(&args.credential_file)?;
            // All families share the same physical-call bounds and rolling token window.
            let student = selected_student(
                args.student_local_url.as_deref(),
                &args.student_model,
                http.clone(),
                output.join("student"),
                500 * methods.len(),
                student_tokens_per_minute,
            )?;
            let teacher = teacher_model(
                args.teacher,
                args.codex,
                http,
                output.join("teacher"),
                300 * methods.len(),
            )
            .await?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(args.teacher).into(),
                student_id: student_identity(
                    args.student_local_url.as_deref(),
                    &args.student_model,
                ),
            };
            for (m, cases) in methods.iter().zip(&banks) {
                let directory = if methods.len() == 1 {
                    output.clone()
                } else {
                    output.join(m)
                };
                let result = runner::optimize(
                    cases,
                    m,
                    book.clone(),
                    &models,
                    &directory,
                    max_metric_calls,
                )
                .await?;
                println!("{}", serde_json::to_string(&result)?);
            }
        }
        Command::OptimizeReading {
            examples,
            output,
            max_metric_calls,
            student_call_cap,
            teacher_call_cap,
        } => {
            ensure!(
                args.student_local_url.is_some(),
                "This bounded reader round requires the resident local student"
            );
            ensure!(
                max_metric_calls > 0 && student_call_cap > 0 && teacher_call_cap > 0,
                "Reader budgets must be positive"
            );
            ensure!(
                matches!(args.teacher, Teacher::Pool),
                "Use the available Gemini sequence before Codex fallback"
            );
            ensure!(
                !output.exists(),
                "Preserve an existing attempt; use a fresh reader output directory"
            );
            let cases = catalogue::cases(&examples, "new_job")?;
            let baseline = horary_dspy::reading::new_job_reader_baseline()?;
            runner::manifest(
                &output,
                &cases,
                &args.book,
                serde_json::json!({"command":"optimize-reading","method":"new_job","program_kind":"direct_family_reading","student":student_identity(args.student_local_url.as_deref(), &args.student_model),"student_local_url":args.student_local_url,"teacher":teacher_identity(args.teacher),"max_metric_calls_soft":max_metric_calls,"reflection_minibatch":3,"seed":23,"bootstrap_demos":0,"optimizable_predictors":["reading.new_job"],"branch_selector":"none; declared family uses complete salient branch union, never a gold branch","student_call_cap":student_call_cap,"teacher_call_cap":teacher_call_cap,"component_inputs":"Independently supplied accepted consultation, actual device clock/place and native calculated chart; no acquisition journey claimed","baseline_instruction_sha256":google::digest(&baseline),"evaluation_concurrency":1,"application_installation":false}),
            )?;
            fs::write(output.join("baseline-instructions.txt"), &baseline)?;
            let book = Arc::new(book::Book::open(&args.book)?);
            let fixtures = cases
                .iter()
                .map(|case| {
                    Ok(horary_dspy::functions::Fixture {
                        split: case.split.clone(),
                        example: {
                            let mut example = catalogue::example(case, "reading", &book)?;
                            let source = horary_dspy::reading::new_job_family_source(&book)?;
                            example.set("source_text", serde_json::json!(&source));
                            example.set("__review_source", serde_json::json!(source));
                            example
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let http = google::client(&args.credential_file)?;
            let student = selected_student(
                args.student_local_url.as_deref(),
                &args.student_model,
                http.clone(),
                output.join("student"),
                student_call_cap,
                14_000,
            )?;
            let listing = google::models(&http).await?;
            fs::write(
                output.join("teacher-model-listing.json"),
                serde_json::to_vec_pretty(&listing)?,
            )?;
            fs::write(
                output.join("teacher-sequence.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"listing_sha256":google::digest(serde_json::to_vec(&listing)?),"models":teachers::available(&listing)?,"fallback":"codex-saved-login-temporary-teacher","shared_physical_call_cap":teacher_call_cap}),
                )?,
            )?;
            let teacher = pooled_teacher(
                args.teacher,
                args.codex,
                http,
                output.join("teacher"),
                teacher_call_cap,
                google::CallBudget::new(teacher_call_cap),
                &listing,
                120_000,
            )?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(args.teacher).into(),
                student_id: student_identity(
                    args.student_local_url.as_deref(),
                    &args.student_model,
                ),
            };
            let result = optimize::compile_reader_fixtures(
                &fixtures,
                "new_job",
                book,
                &models,
                &output,
                optimize::CompileOptions {
                    max_metrics: max_metric_calls,
                    bootstrap_demos: 0,
                },
                &baseline,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::OptimizeClassification {
            examples,
            output,
            max_metric_calls,
            student_tokens_per_minute,
            training_case,
            development_case,
            test_case,
            bootstrap_demos,
        } => {
            let cases = runner::select_cases(
                &runner::load_all(&examples)?,
                &training_case,
                &development_case,
                &test_case,
            )?;
            runner::manifest(
                &output,
                &cases,
                &args.book,
                serde_json::json!({"command":"optimize-classification","student":student_identity(args.student_local_url.as_deref(), &args.student_model),"student_local_url":args.student_local_url,"teacher":teacher_identity(args.teacher),"max_metric_calls":max_metric_calls,"bootstrap_demos":bootstrap_demos,"training_case":training_case,"development_case":development_case,"test_case":test_case}),
            )?;
            let book = book::Book::open(&args.book)?;
            let http = google::client(&args.credential_file)?;
            let student = selected_student(
                args.student_local_url.as_deref(),
                &args.student_model,
                http.clone(),
                output.join("student"),
                5000,
                student_tokens_per_minute,
            )?;
            let teacher =
                teacher_model(args.teacher, args.codex, http, output.join("teacher"), 3000).await?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(args.teacher).into(),
                student_id: student_identity(
                    args.student_local_url.as_deref(),
                    &args.student_model,
                ),
            };
            let fixtures = cases
                .iter()
                .map(|case| {
                    Ok(horary_dspy::functions::Fixture {
                        split: case.split.clone(),
                        example: catalogue::example(case, "classification", &book)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let result = optimize::compile_fixtures(
                &fixtures,
                "all",
                "classification",
                Arc::new(book),
                &models,
                &output,
                optimize::CompileOptions {
                    max_metrics: max_metric_calls,
                    bootstrap_demos,
                },
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::CompareClassification {
            method,
            examples,
            candidate_instructions,
            output,
        } => {
            ensure!(
                runner::methods(&method)?.len() == 1,
                "Select one complete family development pool"
            );
            ensure!(
                args.student_local_url.is_some(),
                "This bounded comparison uses the resident local student"
            );
            let cases = catalogue::cases(&examples, &method)?
                .into_iter()
                .filter(|c| c.split == "development")
                .collect::<Vec<_>>();
            let instruction = fs::read_to_string(&candidate_instructions)?;
            ensure!(
                !instruction.trim().is_empty(),
                "Candidate instruction is empty"
            );
            fs::create_dir(&output)
                .context("Use a fresh comparison directory; prior attempts remain intact")?;
            runner::manifest(
                &output,
                &cases,
                &args.book,
                serde_json::json!({
                    "command":"compare-classification", "method":method,
                    "student":args.student_model, "student_local_url":args.student_local_url,
                    "teacher":teacher_identity(args.teacher), "candidate_instructions":candidate_instructions,
                    "candidate_instructions_sha256":google::digest(&instruction),
                    "student_call_bound":2 * cases.len(), "teacher_physical_call_bound":64,
                    "reflection_calls":0, "selection":"Fixed baseline and proposed candidate; no search or deployment"
                }),
            )?;
            fs::write(output.join("candidate-instructions.txt"), &instruction)?;
            let book = book::Book::open(&args.book)?;
            let fixtures = cases
                .iter()
                .map(|case| {
                    Ok(horary_dspy::functions::Fixture {
                        split: case.split.clone(),
                        example: catalogue::example(case, "classification", &book)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let http = google::client(&args.credential_file)?;
            let student = selected_student(
                args.student_local_url.as_deref(),
                &args.student_model,
                http.clone(),
                output.join("student"),
                2 * cases.len(),
                14_000,
            )?;
            let teacher =
                teacher_model(args.teacher, args.codex, http, output.join("teacher"), 64).await?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(args.teacher).into(),
                student_id: args.student_model,
            };
            let result =
                optimize::compare_classification(&fixtures, &instruction, &models, &output).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}
fn student_identity(local_url: Option<&str>, model: &str) -> String {
    if local_url.is_some() {
        model.into()
    } else {
        google::STUDENT.into()
    }
}
fn selected_student(
    local_url: Option<&str>,
    model: &str,
    http: reqwest::Client,
    directory: PathBuf,
    hosted_max_calls: usize,
    hosted_tokens_per_minute: usize,
) -> Result<Arc<dyn DynChatModel>> {
    if let Some(url) = local_url {
        Ok(Arc::new(
            local::LocalStudent::new(url, model, &directory)?.with_max_calls(hosted_max_calls),
        ))
    } else {
        Ok(Arc::new(google::Google::new(
            google::STUDENT,
            http,
            directory,
            hosted_max_calls,
            hosted_tokens_per_minute,
        )?))
    }
}
fn teacher_identity(mode: Teacher) -> &'static str {
    match mode {
        Teacher::Pool => "available-gemini-text-sequence-then-codex",
        Teacher::Gemini => google::TEACHER,
        Teacher::Codex => "codex-saved-login-temporary-teacher",
    }
}
async fn teacher_model(
    mode: Teacher,
    executable: PathBuf,
    http: reqwest::Client,
    output: PathBuf,
    max_calls: usize,
) -> Result<Arc<dyn DynChatModel>> {
    match mode {
        Teacher::Pool => {
            let listing = google::models(&http).await?;
            pooled_teacher(
                mode,
                executable,
                http,
                output,
                max_calls,
                google::CallBudget::new(max_calls),
                &listing,
                120_000,
            )
        }
        Teacher::Gemini => Ok(Arc::new(google::Google::new(
            google::TEACHER,
            http,
            output,
            max_calls,
            120_000,
        )?)),
        Teacher::Codex => Ok(Arc::new(codex::Codex::new(executable, output, max_calls)?)),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueConfiguration {
    teacher: Teacher,
    model_listing_sha256: String,
    teacher_sequence: Vec<String>,
    codex: PathBuf,
    codex_sha256: String,
    student_tokens_per_minute: usize,
    student_probe_ceiling: usize,
    student_call_cap: usize,
    teacher_call_cap: usize,
    teacher_tokens_per_minute: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    continuation: Option<continuation::FileSeal>,
}
#[allow(clippy::too_many_arguments)]
fn pooled_teacher(
    mode: Teacher,
    executable: PathBuf,
    http: reqwest::Client,
    output: PathBuf,
    max_calls: usize,
    budget: Arc<google::CallBudget>,
    listing: &serde_json::Value,
    rate: usize,
) -> Result<Arc<dyn DynChatModel>> {
    match mode {
        Teacher::Pool => {
            let fallback = codex::Codex::new(executable, output.join("codex"), max_calls)?;
            Ok(Arc::new(teachers::TeacherPool::new(
                http,
                listing,
                output,
                budget,
                rate,
                Some(fallback),
            )?))
        }
        Teacher::Codex => Ok(Arc::new(
            codex::Codex::new(executable, output, max_calls)?.with_budget(budget),
        )),
        Teacher::Gemini => {
            let info: google::ModelInfo = serde_json::from_value(listing["teacher"].clone())
                .context("Preferred Gemini teacher is not listed")?;
            Ok(Arc::new(google::Google::from_listing(
                info,
                http,
                output,
                budget,
                google::RateLimiter::new(rate)?,
            )?))
        }
    }
}
