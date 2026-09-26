mod paths;
mod store;

use std::{env, fs, io::Write, process::ExitCode, str::FromStr};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use jevia_core::{Config, JevClient, Outcome, RouteRecord};

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
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { force } => init(force),
        Command::Route { task, json } => route(&task, json).await,
        Command::Runs { limit, json } => runs(limit, json),
        Command::Feedback {
            run_id,
            outcome,
            json,
        } => feedback(&run_id, outcome.into(), json),
        Command::Doctor => doctor(),
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
        fs::write(&ignore, "runs.jsonl\n*.tmp\n")
            .with_context(|| format!("could not write {}", ignore.display()))?;
    }

    println!("Initialized Jevia in {}", paths.root.display());
    println!("Config: {}", paths.config.display());
    Ok(())
}

async fn route(task: &str, print_json: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let history = store::load(&paths.runs)?;
    let api_key = env::var("TYPESAFE_API_KEY")
        .context("TYPESAFE_API_KEY is not set; Jevia never stores this key in config")?;
    let client = JevClient::new(api_key, &config.jev)?;
    let decision = client.route(task, &config, &history).await?;
    let stored_task = config.privacy.store_task_text.then(|| task.to_owned());
    let record = RouteRecord::new(decision, stored_task);
    store::append(&paths.runs, &record)?;

    if print_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        print_record(&record);
    }
    Ok(())
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
        println!(
            "{}  tier={}  confidence={:.2}  outcome={}",
            record.decision.run_id,
            record.decision.tier,
            record.decision.confidence,
            record.outcome
        );
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
    println!("config: ok ({})", paths.config.display());
    println!("tiers: ok ({})", config.tiers.len());
    println!("local store: {}", paths.runs.display());

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
