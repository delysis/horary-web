#![forbid(unsafe_code)]
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use dsrust::{Module, lm::DynChatModel};
use horary_dspy::{book, catalogue, codex, google, optimize, runner, signatures};
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, ValueEnum)]
enum Teacher {
    Gemini,
    Codex,
}
#[derive(Parser)]
struct Args {
    #[arg(long, value_enum, default_value = "codex")]
    teacher: Teacher,
    #[arg(long, default_value = "/opt/homebrew/bin/codex")]
    codex: PathBuf,
    #[arg(long,default_value=book::DEFAULT_OCR)]
    book: PathBuf,
    #[arg(long,default_value=google::CREDENTIAL_FILE)]
    credential_file: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
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
    match args.command {
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
                !models["teacher"].is_null() && !models["student"].is_null(),
                "Requested teacher/student pair is not available on this credential; no substitution performed"
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
            let teacher_id = teacher_identity(args.teacher);
            runner::manifest(
                &output,
                &all,
                &args.book,
                serde_json::json!({"command":"run","methods":methods,"teacher":teacher_id,"training_limit":training_limit,"cached_student_only":cached_student_only,"cached_teacher_only":cached_teacher_only}),
            )?;
            let book = Arc::new(book::Book::open(&args.book)?);
            let http = google::client(&args.credential_file)?;
            let student = Arc::new(google::Google::new(
                google::STUDENT,
                http.clone(),
                output.join("student"),
                if cached_student_only {
                    0
                } else {
                    240 * methods.len()
                },
                student_tokens_per_minute,
            )?);
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
            )?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_id.into(),
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
                serde_json::json!({"command":"optimize","methods":methods,"teacher":teacher_identity(args.teacher),"max_metric_calls_per_family":max_metric_calls}),
            )?;
            let book = Arc::new(book::Book::open(&args.book)?);
            let http = google::client(&args.credential_file)?;
            // All families share the same physical-call bounds and rolling token window.
            let student = Arc::new(google::Google::new(
                google::STUDENT,
                http.clone(),
                output.join("student"),
                500 * methods.len(),
                student_tokens_per_minute,
            )?);
            let teacher = teacher_model(
                args.teacher,
                args.codex,
                http,
                output.join("teacher"),
                300 * methods.len(),
            )?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(args.teacher).into(),
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
        Command::OptimizeClassification {
            examples,
            output,
            max_metric_calls,
            student_tokens_per_minute,
        } => {
            let cases = runner::load_all(&examples)?;
            runner::manifest(
                &output,
                &cases,
                &args.book,
                serde_json::json!({"command":"optimize-classification","teacher":teacher_identity(args.teacher),"max_metric_calls":max_metric_calls}),
            )?;
            let book = book::Book::open(&args.book)?;
            let http = google::client(&args.credential_file)?;
            let student = Arc::new(google::Google::new(
                google::STUDENT,
                http.clone(),
                output.join("student"),
                5000,
                student_tokens_per_minute,
            )?);
            let teacher =
                teacher_model(args.teacher, args.codex, http, output.join("teacher"), 3000)?;
            let models = runner::Models {
                student,
                teacher,
                teacher_id: teacher_identity(args.teacher).into(),
            };
            let result = optimize::compile(
                &cases,
                "all",
                "classification",
                &book,
                &models,
                &output,
                max_metric_calls,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}
fn teacher_identity(mode: Teacher) -> &'static str {
    match mode {
        Teacher::Gemini => google::TEACHER,
        Teacher::Codex => "codex-saved-login-temporary-teacher",
    }
}
fn teacher_model(
    mode: Teacher,
    executable: PathBuf,
    http: reqwest::Client,
    output: PathBuf,
    max_calls: usize,
) -> Result<Arc<dyn DynChatModel>> {
    match mode {
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
