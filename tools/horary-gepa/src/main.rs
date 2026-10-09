#![forbid(unsafe_code)]
use clap::{Parser, Subcommand};
use horary_gepa::{load, prepare, run, ControlMode, Plan, ENGINE_REV};
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
    Prepare(Box<Preparation>),
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
        Action::Prepare(options) => {
            let Preparation {
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
            } = *options;
            let plan=Plan {version:1,engine_rev:ENGINE_REV.into(),controller_executable:std::env::current_exe().map_err(|error|error.to_string())?,controller_executable_sha256:String::new(),campaign:std::fs::canonicalize(campaign).map_err(|error|error.to_string())?,
                manifest_sha256:String::new(),split_file:std::fs::canonicalize(split).map_err(|error|error.to_string())?,split_sha256:String::new(),
                native_executable:std::fs::canonicalize(native_executable).map_err(|error|error.to_string())?,native_executable_sha256:String::new(),
                codex:std::fs::canonicalize(codex).map_err(|error|error.to_string())?,codex_executable_sha256:String::new(),guide:String::new(),guide_sha256:String::new(),seed:BTreeMap::new(),training:vec![],development:vec![],reserved_ids:vec![],
                max_metric_calls,max_teacher_calls,max_physical_generation_attempts,logical_calls_per_function,function_seconds,teacher_seconds,rng_seed:seed,wait_owner_pid,control_mode,
                qualification:"Training-only search; separate development selects candidates. Original reserved cases are never sent to reflection. First-classification executor only; full journey/source gates required before promotion.".into()};
            let plan = prepare(plan, &state, &training, &development)?;
            println!(
                "{}",
                serde_json::json!({"prepared":state,"engine_rev":ENGINE_REV,"training":plan.training.iter().map(|example|&example.id).collect::<Vec<_>>(),
                "development":plan.development.iter().map(|example|&example.id).collect::<Vec<_>>(),"reserved_case_count":plan.reserved_ids.len(),"components":plan.seed.keys().collect::<Vec<_>>()})
            );
        }
        Action::Run { state } => {
            let plan: Plan = serde_json::from_value(load(&state.join("plan.json"))?)
                .map_err(|error| error.to_string())?;
            println!("{}", run(plan, state).await?);
        }
        Action::Status { state } => {
            println!(
                "{}",
                serde_json::json!({"state":state,"completed":state.join("search-result.json").exists(),"interruption_record":state.join("interruptions").exists(),
                "operations":if state.join("operations").exists(){std::fs::read_dir(state.join("operations")).map_err(|error|error.to_string())?.count()}else{0},"qualification":"Search/component status only"})
            );
        }
    }
    Ok(())
}
