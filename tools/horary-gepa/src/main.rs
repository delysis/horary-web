#![forbid(unsafe_code)]
use clap::{Parser, Subcommand};
use horary_gepa::{
    load, prepare, read, run, BookContext, ControlMode, Function, Objective, Plan, ReadingStage,
    ENGINE_REV,
};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Parser)]
#[command(
    about = "Pinned Rust GEPA search over actual Horary neural functions; never auto-promotes"
)]
struct Cli {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Offline sensitivity and safety probes, no credentials or models.
    Calibrate,
    /// One fresh, bounded hosted production catalogue; never retries an existing owner.
    Campaign {
        #[arg(long)]
        evidence: PathBuf,
        #[arg(long)]
        native_executable: PathBuf,
        #[arg(long, value_delimiter = ',')]
        cases: Vec<String>,
        #[arg(long, default_value_t = 3600)]
        case_seconds: u64,
        #[arg(long, default_value_t = 28)]
        max_calls: u64,
        /// Exact prompt program for a fresh full journey, never an accepted checkpoint.
        #[arg(long, requires = "program_sha256")]
        program: Option<PathBuf>,
        #[arg(long, requires = "program")]
        program_sha256: Option<String>,
    },
    /// Seal a closed native corpus after its owner and child have exited.
    SealCorpus {
        #[arg(long)]
        campaign: PathBuf,
        #[arg(long)]
        closure: PathBuf,
    },
    /// Credential-free pinning of an already closed full-journey pair.
    PrepareCompleteJourney(Box<CompletePreparation>),
    /// Import one exact successful paid answer after a stopped host validation.
    PrepareCompleteJourneyContinuation {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        state: PathBuf,
        /// Pinned stopped-owner state, owner/controller PIDs and exit1 receipt.
        #[arg(long)]
        owner_receipt: PathBuf,
    },
    /// Independent bounded review of existing native outputs; never runs a student.
    VerifyCompleteJourney {
        #[arg(long)]
        state: PathBuf,
    },
    Prepare(Box<Preparation>),
    PrepareRecovery {
        #[arg(long)]
        predecessor: PathBuf,
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        audit: PathBuf,
    },
    /// Separate one-reflection experiment, reusing the closed pilot's exact seed controls.
    PrepareFeedbackTrial {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        state: PathBuf,
    },
    /// Reuse one settled student control after a stopped review preflight.
    PrepareComponentContinuation {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        state: PathBuf,
        /// Revalidate a completed paid judge whose original host parser failed.
        #[arg(long)]
        paid_review_source: Option<PathBuf>,
    },
    /// Separate bounded component experiment, preserving an uninvoked source journey.
    PrepareComponentCurriculum {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        state: PathBuf,
        /// Source-hashed external settlement of the original unscored operation.
        #[arg(long)]
        settlement: PathBuf,
    },
    Run {
        #[arg(long)]
        state: PathBuf,
    },
    Status {
        #[arg(long)]
        state: PathBuf,
    },
}
#[derive(clap::Args)]
struct Preparation {
    #[arg(long, value_enum, default_value_t=Objective::WholeFunction)]
    objective: Objective,
    #[arg(long)]
    selected_stage_observation: bool,
    #[arg(long)]
    campaign: PathBuf,
    #[arg(long)]
    split: PathBuf,
    #[arg(long)]
    native_executable: PathBuf,
    #[arg(long)]
    codex: PathBuf,
    #[arg(long)]
    state: PathBuf,
    #[arg(long, value_delimiter = ',')]
    training: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    development: Vec<String>,
    #[arg(long, default_value_t = 24)]
    max_metric_calls: usize,
    #[arg(long, default_value_t = 3)]
    max_teacher_calls: u64,
    #[arg(long, default_value_t = 108)]
    max_physical_generation_attempts: u64,
    #[arg(long, default_value_t = 3)]
    logical_calls_per_function: u64,
    #[arg(long, default_value_t = 300)]
    function_seconds: u64,
    #[arg(long, default_value_t = 600)]
    teacher_seconds: u64,
    #[arg(long, default_value_t = 7)]
    seed: u64,
    #[arg(long)]
    wait_owner_pid: Option<u32>,
    #[arg(long, value_enum, default_value_t=ControlMode::ArchivedCapture)]
    control_mode: ControlMode,
    #[arg(long, value_enum, default_value_t=Function::Classification)]
    function: Function,
    #[arg(long)]
    target_method: Option<String>,
    #[arg(long, value_enum)]
    target_stage: Option<ReadingStage>,
    #[arg(long, requires = "fixed_program_sha256")]
    fixed_program: Option<PathBuf>,
    #[arg(long, requires = "fixed_program")]
    fixed_program_sha256: Option<String>,
    #[arg(long)]
    exact_response_memoization: bool,
    #[arg(long, default_value_t = 0)]
    max_review_calls: u64,
    #[arg(long, default_value_t = 600)]
    review_seconds: u64,
    #[arg(long)]
    book_source: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    book_ocr_pages: Vec<usize>,
}
#[derive(clap::Args)]
struct CompletePreparation {
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    baseline: PathBuf,
    #[arg(long)]
    baseline_closure: PathBuf,
    #[arg(long)]
    candidate: PathBuf,
    #[arg(long)]
    candidate_closure: PathBuf,
    #[arg(long)]
    program: PathBuf,
    #[arg(long)]
    program_sha256: String,
    #[arg(long)]
    split: PathBuf,
    #[arg(long)]
    passages: PathBuf,
    #[arg(long)]
    book_source: PathBuf,
    #[arg(long, value_delimiter = ',')]
    book_ocr_pages: Vec<usize>,
    /// Exact page-map provenance when the native quotation index is partial.
    #[arg(long)]
    book_page_witnesses: Option<PathBuf>,
    #[arg(long)]
    codex: PathBuf,
    #[arg(long)]
    max_review_calls: u64,
    #[arg(long, default_value_t = 600)]
    review_seconds: u64,
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = execute(Cli::parse()).await {
        eprintln!("horary-gepa: {error}");
        std::process::exit(1)
    }
}
async fn execute(cli: Cli) -> horary_gepa::Result<()> {
    match cli.action {
        Action::PrepareComponentContinuation {
            source,
            state,
            paid_review_source,
        } => {
            let controller = std::env::current_exe().map_err(|e| e.to_string())?;
            let plan = horary_gepa::controls::prepare_component_continuation(
                &source,
                &state,
                &controller,
                paid_review_source.as_deref(),
            )?;
            println!(
                "Prepared unchanged {} component with retained observations and zero new paid calls",
                plan.signature().stage
            );
        }
        Action::Calibrate => {
            println!("{}", horary_gepa::metric::calibration()?);
        }
        Action::Campaign {
            evidence,
            native_executable,
            cases,
            case_seconds,
            max_calls,
            program,
            program_sha256,
        } => {
            let program = match (program, program_sha256) {
                (Some(file), Some(sha256)) => {
                    Some(horary_gepa::campaign::ProgramSource { file, sha256 })
                }
                (None, None) => None,
                _ => return Err("--program and --program-sha256 must be supplied together".into()),
            };
            println!(
                "{}",
                horary_gepa::campaign::run_with_program(
                    &evidence,
                    &native_executable,
                    &cases,
                    case_seconds,
                    max_calls,
                    program.as_ref()
                )?
            );
        }
        Action::SealCorpus { campaign, closure } => {
            println!("{}", horary_gepa::corpus::seal(&campaign, &closure)?);
        }
        Action::PrepareCompleteJourney(options) => {
            let book = horary_gepa::complete_journey::CompleteReadingBook {
                sha256: horary_prompt_program::digest(read(&options.book_source)?),
                file: options.book_source,
                ocr_pages: options.book_ocr_pages,
                page_witnesses: options
                    .book_page_witnesses
                    .map(|file| {
                        read(&file).map(|bytes| horary_gepa::complete_journey::Pin {
                            sha256: horary_prompt_program::digest(bytes),
                            file,
                        })
                    })
                    .transpose()?,
            };
            let plan = horary_gepa::complete_journey::prepare(
                horary_gepa::complete_journey::Preparation {
                    baseline: options.baseline,
                    baseline_closure: options.baseline_closure,
                    candidate: options.candidate,
                    candidate_closure: options.candidate_closure,
                    program: options.program,
                    program_sha256: options.program_sha256,
                    split: options.split,
                    passages: options.passages,
                    book,
                    codex: options.codex,
                    controller: std::env::current_exe().map_err(|e| e.to_string())?,
                    max_review_calls: options.max_review_calls,
                    review_seconds: options.review_seconds,
                },
                &options.state,
            )?;
            println!(
                "{}",
                serde_json::json!({"prepared":options.state,"cases":plan.cases,
                "max_review_calls":plan.max_review_calls,"student_calls":0,"reflection_calls":0,"paid_calls":0})
            );
        }
        Action::VerifyCompleteJourney { state } => {
            let plan: horary_gepa::complete_journey::PairPlan =
                serde_json::from_value(load(&state.join("plan.json"))?)
                    .map_err(|e| e.to_string())?;
            println!(
                "{}",
                horary_gepa::complete_journey::verify_pair(plan, &state)?
            );
        }
        Action::PrepareCompleteJourneyContinuation {
            source,
            state,
            owner_receipt,
        } => {
            let plan = horary_gepa::complete_journey::prepare_continuation(
                &source,
                &state,
                &owner_receipt,
                &std::env::current_exe().map_err(|e| e.to_string())?,
            )?;
            println!(
                "{}",
                serde_json::json!({"prepared":state,"original_paid_answer_reused":true,
                "inherited_review_calls":1,"remaining_new_review_calls":plan.max_review_calls-1,
                "new_paid_calls":0,"student_calls":0,"reflection_calls":0})
            );
        }
        Action::Prepare(options) => {
            let Preparation {
                objective,
                selected_stage_observation,
                campaign,
                split,
                native_executable,
                codex,
                state,
                training,
                development,
                max_metric_calls,
                max_teacher_calls,
                max_physical_generation_attempts,
                logical_calls_per_function,
                function_seconds,
                teacher_seconds,
                seed,
                wait_owner_pid,
                control_mode,
                function,
                target_method,
                target_stage,
                fixed_program,
                fixed_program_sha256,
                exact_response_memoization,
                max_review_calls,
                review_seconds,
                book_source,
                book_ocr_pages,
            } = *options;
            let fixed_program = match (fixed_program, fixed_program_sha256) {
                (Some(file), Some(sha256)) => Some(horary_gepa::fixed::Source {
                    file: std::fs::canonicalize(file).map_err(|e| e.to_string())?,
                    sha256,
                }),
                (None, None) => None,
                _ => {
                    return Err(
                        "--fixed-program and --fixed-program-sha256 must be supplied together"
                            .into(),
                    )
                }
            };
            let review_book = book_source
                .map(|p| {
                    let file = std::fs::canonicalize(p).map_err(|error| error.to_string())?;
                    Ok::<BookContext, String>(BookContext {
                        sha256: horary_prompt_program::digest(read(&file)?),
                        file,
                        ocr_pages: book_ocr_pages,
                    })
                })
                .transpose()?;
            let plan=Plan {version:match function {Function::Classification=>1,Function::InputJourney=>2,Function::ReadingJourney=>3},engine_rev:ENGINE_REV.into(),controller_executable:std::env::current_exe().map_err(|error|error.to_string())?,controller_executable_sha256:String::new(),campaign:std::fs::canonicalize(campaign).map_err(|error|error.to_string())?,
                manifest_sha256:String::new(),split_file:std::fs::canonicalize(split).map_err(|error|error.to_string())?,split_sha256:String::new(),
                native_executable:std::fs::canonicalize(native_executable).map_err(|error|error.to_string())?,native_executable_sha256:String::new(),
                codex:std::fs::canonicalize(codex).map_err(|error|error.to_string())?,codex_executable_sha256:String::new(),codex_snapshot:None,recovery:None,component_curriculum:None,fixed_program,exact_response_memoization,guide:String::new(),guide_sha256:String::new(),seed:BTreeMap::new(),training:vec![],development:vec![],reserved_ids:vec![],
                max_metric_calls,max_teacher_calls,max_physical_generation_attempts,logical_calls_per_function,function_seconds,teacher_seconds,rng_seed:seed,wait_owner_pid,control_mode,
                function,objective,initial_extractor_observation:false,selected_stage_observation,target_method,target_stage,max_review_calls,review_seconds:if function==Function::Classification {0} else {review_seconds},review_book,
                qualification:if function == Function::Classification {
                    "Training-only classification search; separate development selects candidates. No reading, reserved, on-device or deployment qualification. Fresh whole-journey/source/reserved gates required before promotion."
                } else if function == Function::InputJourney {
                    "Training-only initial-extractor correctness and repair-reliability search. Fixed conversation and supplying stages are measured separately, not locally optimized. No reading generated or whole-journey, interpretation, reserved, on-device or deployment qualification. Fresh production reading/source/reserved gates required before promotion."
                } else {
                    "Training-only selected reading-stage search through fresh production question, elicitation, accepted inputs and chart. Independent source-cited component merit and whole-reading gates are separate. No reserved, on-device, packaged acceptance or automatic deployment qualification. Unsupported methods remain at expert-review boundaries."
                }.into()};
            let plan = prepare(plan, &state, &training, &development)?;
            println!(
                "{}",
                serde_json::json!({"prepared":state,"engine_rev":ENGINE_REV,"training":plan.training.iter().map(|example|&example.id).collect::<Vec<_>>(),
                "development":plan.development.iter().map(|example|&example.id).collect::<Vec<_>>(),"reserved_case_count":plan.reserved_ids.len(),"components":plan.seed.keys().collect::<Vec<_>>()})
            );
        }
        Action::PrepareRecovery {
            predecessor,
            state,
            audit,
        } => {
            let controller = std::env::current_exe().map_err(|e| e.to_string())?;
            let plan = horary_gepa::recovery::prepare(&predecessor, &state, &audit, &controller)?;
            println!(
                "{}",
                serde_json::json!({"prepared":state,"predecessor":predecessor,"new_model_calls":0,
                "remaining_hard_budgets":{"physical":plan.max_physical_generation_attempts,"review":plan.max_review_calls,"teacher":plan.max_teacher_calls}})
            );
        }
        Action::PrepareFeedbackTrial { source, state } => {
            let controller = std::env::current_exe().map_err(|e| e.to_string())?;
            let plan = horary_gepa::controls::prepare(&source, &state, &controller)?;
            println!(
                "{}",
                serde_json::json!({"prepared":state,"source":source,
                "new_paid_calls":0,"reused_controls":4,"actual_writer_stdin_preflight":true,
                "new_bounded_experiment":{"teacher":plan.max_teacher_calls,"review":plan.max_review_calls,
                    "physical_attempt_reservations":plan.max_physical_generation_attempts}})
            );
        }
        Action::PrepareComponentCurriculum {
            source,
            state,
            settlement,
        } => {
            let controller = std::env::current_exe().map_err(|e| e.to_string())?;
            let plan = horary_gepa::curriculum::prepare(&source, &state, &settlement, &controller)?;
            println!(
                "{}",
                serde_json::json!({"prepared":state,"source":source,"new_paid_calls":0,
                "training":plan.training.iter().map(|e|&e.id).collect::<Vec<_>>(),
                "development":plan.development.iter().map(|e|&e.id).collect::<Vec<_>>(),
                "soft_metric_cap":plan.max_metric_calls,"maximum_metric_observations":6,
                "remaining_hard_budgets":{"physical":plan.max_physical_generation_attempts,
                "reviews":plan.max_review_calls,"teacher":plan.max_teacher_calls},
                "qualification":plan.qualification})
            );
        }
        Action::Run { state } => {
            let plan: Plan = serde_json::from_value(load(&state.join("plan.json"))?)
                .map_err(|error| error.to_string())?;
            println!("{}", run(plan, state).await?);
        }
        Action::Status { state } => {
            println!("{}", horary_gepa::status::inspect(&state)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_reading_page_witness_option_preserves_legacy_arguments() {
        let args = [
            "horary-gepa",
            "prepare-complete-journey",
            "--state",
            "/synthetic/review",
            "--baseline",
            "/synthetic/baseline",
            "--baseline-closure",
            "/synthetic/baseline/closure.json",
            "--candidate",
            "/synthetic/candidate",
            "--candidate-closure",
            "/synthetic/candidate/closure.json",
            "--program",
            "/synthetic/program.json",
            "--program-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--split",
            "/synthetic/split.json",
            "--passages",
            "/synthetic/passages.json",
            "--book-source",
            "/synthetic/book.md",
            "--book-ocr-pages",
            "1,2",
            "--codex",
            "/synthetic/codex",
            "--max-review-calls",
            "6",
        ];
        let Action::PrepareCompleteJourney(legacy) = Cli::try_parse_from(args).unwrap().action
        else {
            panic!("Wrong action")
        };
        assert!(legacy.book_page_witnesses.is_none());
        let mut explicit = args.to_vec();
        explicit.extend(["--book-page-witnesses", "/synthetic/page-map.json"]);
        let Action::PrepareCompleteJourney(mapped) = Cli::try_parse_from(explicit).unwrap().action
        else {
            panic!("Wrong action")
        };
        assert_eq!(
            mapped.book_page_witnesses,
            Some(PathBuf::from("/synthetic/page-map.json"))
        );
        assert_eq!(mapped.book_ocr_pages, vec![1, 2]);
        assert_eq!(mapped.max_review_calls, legacy.max_review_calls);
    }

    #[test]
    fn complete_paid_continuation_requires_explicit_source_state_and_closure() {
        let args = [
            "horary-gepa",
            "prepare-complete-journey-continuation",
            "--source",
            "/synthetic/stopped",
            "--state",
            "/synthetic/new",
            "--owner-receipt",
            "/synthetic/direct-tool-closure.json",
        ];
        let Action::PrepareCompleteJourneyContinuation {
            source,
            state,
            owner_receipt,
        } = Cli::try_parse_from(args).unwrap().action
        else {
            panic!("Wrong action")
        };
        assert_eq!(source, PathBuf::from("/synthetic/stopped"));
        assert_eq!(state, PathBuf::from("/synthetic/new"));
        assert_eq!(
            owner_receipt,
            PathBuf::from("/synthetic/direct-tool-closure.json")
        );
        assert!(Cli::try_parse_from(&args[..6]).is_err());
    }

    #[test]
    fn preparation_fixed_program_pin_is_optional_but_requires_both_fields() {
        let args = [
            "horary-gepa",
            "prepare",
            "--campaign",
            "/synthetic/campaign",
            "--split",
            "/synthetic/split.json",
            "--native-executable",
            "/synthetic/native",
            "--codex",
            "/synthetic/codex",
            "--state",
            "/synthetic/state",
        ];
        let baseline = Cli::try_parse_from(args).unwrap();
        let Action::Prepare(p) = baseline.action else {
            panic!("Wrong action")
        };
        assert!(p.fixed_program.is_none());
        assert!(p.fixed_program_sha256.is_none());
        let mut only_path = args.to_vec();
        only_path.extend(["--fixed-program", "/synthetic/prior.json"]);
        assert!(Cli::try_parse_from(&only_path).is_err());
        let mut only_hash = args.to_vec();
        only_hash.extend([
            "--fixed-program-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]);
        assert!(Cli::try_parse_from(&only_hash).is_err());
        only_path.extend([
            "--fixed-program-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]);
        let Action::Prepare(parsed) = Cli::try_parse_from(&only_path).unwrap().action else {
            panic!("Wrong action")
        };
        assert!(parsed.fixed_program.is_some());
        assert!(parsed.fixed_program_sha256.is_some());
    }

    #[test]
    fn campaign_program_and_hash_are_an_explicit_pair_and_baseline_stays_optional() {
        let baseline = [
            "horary-gepa",
            "campaign",
            "--evidence",
            "/synthetic/run",
            "--native-executable",
            "/synthetic/native",
            "--cases",
            "synthetic-explicit",
        ];
        assert!(Cli::try_parse_from(baseline).is_ok());
        let mut args = baseline.to_vec();
        args.extend(["--program", "/synthetic/program.json"]);
        assert!(Cli::try_parse_from(&args).is_err());
        args.extend([
            "--program-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]);
        let parsed = Cli::try_parse_from(&args).unwrap();
        assert!(matches!(
            parsed.action,
            Action::Campaign {
                program: Some(_),
                program_sha256: Some(_),
                ..
            }
        ));
        let mut hash_only = baseline.to_vec();
        hash_only.extend([
            "--program-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]);
        assert!(Cli::try_parse_from(&hash_only).is_err());
    }
}
