#![forbid(unsafe_code)]
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Recover a frozen hosted catalogue into a fresh collection without replaying completed cases"
)]
struct Cli {
    #[arg(long)]
    parent: PathBuf,
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    repository: PathBuf,
    #[arg(long)]
    native_executable: PathBuf,
    #[arg(long)]
    native_executable_sha256: String,
    #[arg(long, requires_all=["completed_review_state","fixtures","codex"])]
    optimizer_state: Option<PathBuf>,
    #[arg(long)]
    completed_review_state: Option<PathBuf>,
    #[arg(long)]
    fixtures: Option<PathBuf>,
    #[arg(long)]
    codex: Option<PathBuf>,
    #[arg(long, default_value_t = 10)]
    distinct_case_budget: usize,
}
fn main() {
    let cli = Cli::parse();
    let review = cli
        .optimizer_state
        .map(|state| horary_loop::catalogue::Review {
            state,
            completed_review_state: cli
                .completed_review_state
                .expect("CLI requires review state"),
            fixtures: cli.fixtures.expect("CLI requires fixtures"),
            codex: cli.codex.expect("CLI requires Codex executable"),
            distinct_case_budget: cli.distinct_case_budget,
        });
    if let Err(error) = horary_loop::catalogue::run(horary_loop::catalogue::Options {
        parent: cli.parent,
        state: cli.state,
        repository: cli.repository,
        executable: cli.native_executable,
        expected_executable_sha256: cli.native_executable_sha256,
        review,
    }) {
        eprintln!("horary-catalogue: {error}");
        std::process::exit(1);
    }
}
