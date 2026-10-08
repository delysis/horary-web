#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use horary_loop::{run, Action, Options};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Review immutable Horary trials and propose scoped teaching changes"
)]
struct Cli {
    #[command(subcommand)]
    action: Command,
    #[arg(long, global = true)]
    campaign: Option<PathBuf>,
    #[arg(long, global = true)]
    fixtures: Option<PathBuf>,
    #[arg(long, global = true)]
    state: Option<PathBuf>,
    #[arg(long, global = true, default_value_t = 6)]
    batch_size: usize,
    #[arg(long, global = true, default_value_t = 1200)]
    job_seconds: u64,
    #[arg(long, global = true, default_value = "codex")]
    codex: PathBuf,
    #[arg(long, global = true)]
    retry_failed: bool,
    #[arg(long, global = true)]
    candidate: Option<PathBuf>,
    #[arg(long, global = true)]
    validation: bool,
    #[arg(long, global = true)]
    once: bool,
    #[arg(long, global = true, default_value_t = 1)]
    max_jobs: usize,
}

#[derive(Subcommand)]
enum Command {
    /// Stream completed training cases into durable review jobs.
    Watch,
    /// Review currently completed cases; --once limits this to one batch.
    Judge,
    /// Propose teaching changes from successful training reviews only.
    Propose,
    /// Inspect the durable ledger without starting Codex.
    Status,
    /// Revalidate saved successful-exit answers locally without starting Codex.
    Revalidate,
}

fn main() {
    let cli = Cli::parse();
    let required = |value: Option<PathBuf>, name: &str| {
        value.unwrap_or_else(|| {
            eprintln!("horary-loop: --{name} is required");
            std::process::exit(2);
        })
    };
    let options = Options {
        action: match cli.action {
            Command::Watch => Action::Watch,
            Command::Judge => Action::Judge,
            Command::Propose => Action::Propose,
            Command::Status => Action::Status,
            Command::Revalidate => Action::Revalidate,
        },
        campaign: required(cli.campaign, "campaign"),
        fixtures: required(cli.fixtures, "fixtures"),
        state: required(cli.state, "state"),
        batch_size: cli.batch_size,
        job_seconds: cli.job_seconds,
        codex: cli.codex,
        retry_failed: cli.retry_failed,
        validation: cli.validation,
        candidate: cli.candidate,
        once: cli.once,
        max_jobs: cli.max_jobs,
    };
    if let Err(error) = run(options) {
        eprintln!("horary-loop: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_paths_can_follow_the_action() {
        let cli = Cli::try_parse_from([
            "horary-loop",
            "judge",
            "--campaign",
            "a",
            "--fixtures",
            "b",
            "--state",
            "c",
        ])
        .unwrap();
        assert_eq!(cli.campaign, Some(PathBuf::from("a")));
    }
}
