mod paths;
mod store;

use std::{env, fs, io::Write, process::ExitCode, str::FromStr, time::Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use jevia_core::{Config, ExecutionEvidence, HarnessInvocation, JevClient, Outcome, RouteRecord};
use tokio::process::Command as ChildCommand;

use crate::paths::ProjectPaths;

#[derive(Debug, Parser)]
#[command(
    name = "jevia",
    version,
    about = "Outcome-aware model routing for coding agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create project-local Jevia configuration.
    Init {
        /// Replace an existing config.toml with the default configuration.
        #[arg(long)]
        force: bool,
    },
    /// Ask Jev which capability tier should handle a task.
    Route {
        /// Task or request to classify.
        task: String,
        /// Print the complete record as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Route a task and launch a configured coding-agent harness.
    Run {
        /// Harness name from the `[harnesses]` configuration.
        harness: String,
        /// Task passed to the harness argument template.
        task: String,
        /// Additional arguments appended after the configured template.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Show recent local routing records.
    Runs {
        /// Maximum number of records to print.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Print records as a JSON array.
        #[arg(long)]
        json: bool,
    },
    /// Attach an observed outcome to a previous routing decision.
    Feedback {
        /// Run identifier printed by jevia route.
        run_id: String,
        /// Observed task result.
        outcome: OutcomeArgument,
        /// Print the updated record as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Validate the local installation without making an API request.
    Doctor,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutcomeArgument {
    Success,
    Failure,
    Unknown,
}

impl From<OutcomeArgument> for Outcome {
    fn from(value: OutcomeArgument) -> Self {
        Outcome::from_str(match value {
            OutcomeArgument::Success => "success",
            OutcomeArgument::Failure => "failure",
            OutcomeArgument::Unknown => "unknown",
        })
        .expect("clap values are valid outcomes")
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { force } => {
            init(force)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Route { task, json } => {
            route(&task, json).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            harness,
            task,
            args,
        } => run_harness(&harness, &task, &args).await,
        Command::Runs { limit, json } => {
            runs(limit, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Feedback {
            run_id,
            outcome,
            json,
        } => {
            feedback(&run_id, outcome.into(), json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            doctor()?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn init(force: bool) -> Result<()> {
    let paths = ProjectPaths::current()?;
    if paths.config.exists() && !force {
        bail!(
            "{} already exists; use --force to replace it",
            paths.config.display()
        );
    }

    fs::create_dir_all(&paths.directory)
        .with_context(|| format!("could not create {}", paths.directory.display()))?;
    let config = Config::default().to_toml()?;
    fs::write(&paths.config, config)
        .with_context(|| format!("could not write {}", paths.config.display()))?;

    let ignore = paths.directory.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "runs.jsonl\nruns.lock\n*.tmp\n")
            .with_context(|| format!("could not write {}", ignore.display()))?;
    }

    println!("Initialized Jevia in {}", paths.root.display());
    println!("Config: {}", paths.config.display());
    Ok(())
}

async fn route(task: &str, print_json: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let record = routed_record(task, &config, &paths).await?;
    store::append(&paths.runs, &record)?;

    if print_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        print_record(&record);
    }
    Ok(())
}

async fn run_harness(harness_name: &str, task: &str, extra_args: &[String]) -> Result<ExitCode> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let harness = config.harnesses.get(harness_name).with_context(|| {
        let available = config
            .harnesses
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "harness `{harness_name}` is not configured; available harnesses: {}",
            if available.is_empty() {
                "none"
            } else {
                &available
            }
        )
    })?;
    let record = routed_record(task, &config, &paths).await?;
    let invocation = harness.invocation(
        harness_name,
        &record.decision.tier,
        task,
        &record.decision.run_id,
        extra_args,
    )?;
    store::append(&paths.runs, &record)?;

    execute_harness(&paths, harness_name, &invocation, &record).await
}

async fn execute_harness(
    paths: &ProjectPaths,
    harness_name: &str,
    invocation: &HarnessInvocation,
    record: &RouteRecord,
) -> Result<ExitCode> {
    eprintln!(
        "jevia: run={} tier={} suggested={} confidence={:.2} fallback={}",
        record.decision.run_id,
        record.decision.tier,
        record.decision.suggested_tier,
        record.decision.confidence,
        record.decision.fallback_applied
    );
    eprintln!("jevia: launching harness `{harness_name}`");

    let started = Instant::now();
    let status = ChildCommand::new(&invocation.program)
        .args(&invocation.args)
        .current_dir(&paths.root)
        .status()
        .await
        .with_context(|| format!("could not launch harness `{harness_name}`"))?;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let outcome = if status.success() {
        Outcome::Success
    } else {
        Outcome::Failure
    };
    let execution = ExecutionEvidence {
        harness: harness_name.to_owned(),
        model: invocation.model.clone(),
        duration_ms,
        exit_code: status.code(),
    };
    store::record_execution(&paths.runs, &record.decision.run_id, outcome, execution)?;
    eprintln!("jevia: outcome={outcome} duration_ms={duration_ms}");

    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

async fn routed_record(task: &str, config: &Config, paths: &ProjectPaths) -> Result<RouteRecord> {
    let history = store::load(&paths.runs)?;
    let api_key = env::var("TYPESAFE_API_KEY")
        .context("TYPESAFE_API_KEY is not set; Jevia never stores this key in config")?;
    let client = JevClient::new(api_key, &config.jev)?;
    let decision = client.route(task, config, &history).await?;
    let stored_task = config.privacy.store_task_text.then(|| task.to_owned());
    Ok(RouteRecord::new(decision, stored_task))
}

fn runs(limit: usize, print_json: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let records = store::load(&paths.runs)?;
    let start = records.len().saturating_sub(limit);
    let records = &records[start..];

    if print_json {
        println!("{}", serde_json::to_string_pretty(records)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("No local runs recorded.");
        return Ok(());
    }
    for record in records.iter().rev() {
        if let Some(execution) = &record.execution {
            println!(
                "{}  tier={}  model={}  confidence={:.2}  outcome={}  duration={}ms",
                record.decision.run_id,
                record.decision.tier,
                execution.model,
                record.decision.confidence,
                record.outcome,
                execution.duration_ms
            );
        } else {
            println!(
                "{}  tier={}  confidence={:.2}  outcome={}",
                record.decision.run_id,
                record.decision.tier,
                record.decision.confidence,
                record.outcome
            );
        }
    }
    Ok(())
}

fn feedback(run_id: &str, outcome: Outcome, print_json: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let record = store::update_outcome(&paths.runs, run_id, outcome)?;
    if print_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        println!("Updated {run_id}: outcome={outcome}");
    }
    Ok(())
}

fn doctor() -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let history = store::load(&paths.runs)?;
    println!("config: ok ({})", paths.config.display());
    println!("tiers: ok ({})", config.tiers.len());
    println!("harnesses: ok ({})", config.harnesses.len());
    println!(
        "local store: ok ({} records at {})",
        history.len(),
        paths.runs.display()
    );

    match env::var("TYPESAFE_API_KEY") {
        Ok(value) if !value.trim().is_empty() => println!("TYPESAFE_API_KEY: set"),
        _ => {
            println!("TYPESAFE_API_KEY: missing");
            bail!("doctor found a missing required credential")
        }
    }
    Ok(())
}

fn load_config(paths: &ProjectPaths) -> Result<Config> {
    let input = fs::read_to_string(&paths.config)
        .with_context(|| format!("could not read {}", paths.config.display()))?;
    Config::from_toml(&input).map_err(Into::into)
}

fn print_record(record: &RouteRecord) {
    println!("run:        {}", record.decision.run_id);
    println!("tier:       {}", record.decision.tier);
    println!("suggested:  {}", record.decision.suggested_tier);
    println!("confidence: {:.2}", record.decision.confidence);
    println!("fallback:   {}", record.decision.fallback_applied);
    println!("jev model:  {}", record.decision.jev_model);
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use jevia_core::{HarnessInvocation, RouteDecision};
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn successful_harness_process_records_success() {
        let directory = tempdir().expect("temporary directory");
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        fs::create_dir_all(&paths.directory).expect("Jevia directory is created");
        let record = sample_record();
        store::append(&paths.runs, &record).expect("record is appended");
        let invocation = HarnessInvocation {
            program: "rustc".to_owned(),
            args: vec!["--version".to_owned()],
            model: "provider/test".to_owned(),
        };

        execute_harness(&paths, "test", &invocation, &record)
            .await
            .expect("harness succeeds");

        let records = store::load(&paths.runs).expect("records load");
        assert_eq!(records[0].outcome, Outcome::Success);
        let execution = records[0]
            .execution
            .as_ref()
            .expect("execution evidence is recorded");
        assert_eq!(execution.harness, "test");
        assert_eq!(execution.model, "provider/test");
        assert_eq!(execution.exit_code, Some(0));
    }

    #[tokio::test]
    async fn failed_harness_process_records_failure() {
        let directory = tempdir().expect("temporary directory");
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        fs::create_dir_all(&paths.directory).expect("Jevia directory is created");
        let record = sample_record();
        store::append(&paths.runs, &record).expect("record is appended");
        let invocation = HarnessInvocation {
            program: "rustc".to_owned(),
            args: vec!["--definitely-not-a-real-rustc-option".to_owned()],
            model: "provider/test".to_owned(),
        };

        execute_harness(&paths, "test", &invocation, &record)
            .await
            .expect("process failure is a recorded outcome, not a Jevia error");

        let records = store::load(&paths.runs).expect("records load");
        assert_eq!(records[0].outcome, Outcome::Failure);
        let execution = records[0]
            .execution
            .as_ref()
            .expect("execution evidence is recorded");
        assert_eq!(execution.harness, "test");
        assert_eq!(execution.model, "provider/test");
        assert_ne!(execution.exit_code, Some(0));
    }

    fn sample_record() -> RouteRecord {
        RouteRecord {
            schema_version: 1,
            decision: RouteDecision {
                run_id: "run-1".to_owned(),
                tier: "balanced".to_owned(),
                suggested_tier: "balanced".to_owned(),
                confidence: 0.8,
                probabilities: BTreeMap::new(),
                fallback_applied: false,
                jev_model: "jev-test".to_owned(),
                created_at_ms: 1,
            },
            task: Some("test task".to_owned()),
            outcome: Outcome::Unknown,
            execution: None,
        }
    }
}
