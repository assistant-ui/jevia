mod cache;
mod lease;
mod paths;
mod processes;
mod store;

use std::{env, fs, io::Write, process::ExitCode, str::FromStr, time::Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use jevia_core::{
    Config, ExecutionEvidence, HarnessInvocation, JevClient, Outcome, RouteRecord, RunState,
    VerificationEvidence, route_cache_key,
};

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
        /// Bypass the local routing-decision cache.
        #[arg(long)]
        no_cache: bool,
    },
    /// Route a task and launch a configured coding-agent harness.
    Run {
        /// Harness name from the `[harnesses]` configuration.
        harness: String,
        /// Own the process tree and disable stdin (use a headless harness).
        #[arg(long)]
        non_interactive: bool,
        /// Stop the harness after this many seconds; does not limit routing.
        #[arg(long, requires = "non_interactive", value_parser = clap::value_parser!(u64).range(1..=86400))]
        timeout_seconds: Option<u64>,
        /// Independent deadline for post-run verification.
        #[arg(long, requires = "non_interactive", value_parser = clap::value_parser!(u64).range(1..=86400))]
        verification_timeout_seconds: Option<u64>,
        /// Task passed to the harness argument template.
        task: String,
        /// Bypass the local routing-decision cache.
        #[arg(long)]
        no_cache: bool,
        /// Additional arguments appended after the configured template.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Show recent local routing records.
    Runs {
        #[command(subcommand)]
        action: Option<RunsAction>,
        /// Maximum number of records to print.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Print records as a JSON array.
        #[arg(long, global = true)]
        json: bool,
    },
    /// Attach an observed outcome to a previous routing decision.
    Feedback {
        /// Run identifier printed by jevia route.
        run_id: String,
        /// Observed task result.
        outcome: OutcomeArgument,
        /// Explanation required when changing a known outcome. Stored locally.
        #[arg(long)]
        reason: Option<String>,
        /// Print the updated record as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Validate the local installation without making an API request.
    Doctor,
    /// Validate the local installation and make a live routing request.
    Check,
    /// Inspect or clear the local routing-decision cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Debug, Subcommand)]
enum RunsAction {
    /// Inspect one complete run record, including its execution lifecycle.
    Show { run_id: String },
    /// Mark an execution whose Jevia supervisor exited as interrupted. Never reruns it.
    Recover { run_id: String },
}

#[derive(Default, Clone, Copy)]
struct RunOptions {
    non_interactive: bool,
    timeout_seconds: Option<u64>,
    verification_timeout_seconds: Option<u64>,
}

#[derive(Debug, Subcommand)]
enum CacheAction {
    /// Show cache configuration and entry counts.
    Status,
    /// Remove all cached routing decisions.
    Clear,
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
        Command::Route {
            task,
            json,
            no_cache,
        } => {
            route(&task, json, no_cache).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            harness,
            non_interactive,
            timeout_seconds,
            verification_timeout_seconds,
            task,
            args,
            no_cache,
        } => {
            run_harness(
                &harness,
                &task,
                &args,
                no_cache,
                RunOptions {
                    non_interactive,
                    timeout_seconds,
                    verification_timeout_seconds,
                },
            )
            .await
        }
        Command::Runs {
            action,
            limit,
            json,
        } => {
            match action {
                None => runs(limit, json)?,
                Some(RunsAction::Show { run_id }) => show_run(&run_id)?,
                Some(RunsAction::Recover { run_id }) => recover_run(&run_id)?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Feedback {
            run_id,
            outcome,
            reason,
            json,
        } => {
            feedback(&run_id, outcome.into(), reason.as_deref(), json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            doctor()?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Check => {
            check().await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Cache { action } => {
            cache_command(action)?;
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

    ensure_local_ignore(&paths.directory.join(".gitignore"))?;

    println!("Initialized Jevia in {}", paths.root.display());
    println!("Config: {}", paths.config.display());
    Ok(())
}

async fn route(task: &str, print_json: bool, no_cache: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let record = routed_record(task, None, no_cache, &config, &paths).await?;
    store::append(&paths.runs, &record)?;

    if print_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        print_record(&record);
    }
    Ok(())
}

async fn run_harness(
    harness_name: &str,
    task: &str,
    extra_args: &[String],
    no_cache: bool,
    options: RunOptions,
) -> Result<ExitCode> {
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
    let record = routed_record(task, Some(harness_name), no_cache, &config, &paths).await?;
    let invocation = harness.invocation(
        harness_name,
        &record.decision.tier,
        task,
        &record.decision.run_id,
        extra_args,
    )?;
    store::append(&paths.runs, &record)?;

    execute_harness_with_options(&paths, harness_name, &invocation, &record, options).await
}

#[cfg(test)]
async fn execute_harness(
    paths: &ProjectPaths,
    harness_name: &str,
    invocation: &HarnessInvocation,
    record: &RouteRecord,
) -> Result<ExitCode> {
    execute_harness_with_options(
        paths,
        harness_name,
        invocation,
        record,
        RunOptions::default(),
    )
    .await
}

async fn execute_harness_with_options(
    paths: &ProjectPaths,
    harness_name: &str,
    invocation: &HarnessInvocation,
    record: &RouteRecord,
    options: RunOptions,
) -> Result<ExitCode> {
    let _lease = lease::try_acquire(&paths.directory.join("run-leases"), &record.decision.run_id)?
        .context("run already has an active Jevia supervisor")?;
    let mut runner = processes::Runner::new(options.non_interactive)?;
    eprintln!(
        "jevia: run={} tier={} suggested={} confidence={:.2} fallback={}",
        record.decision.run_id,
        record.decision.tier,
        record.decision.suggested_tier,
        record.decision.confidence,
        record.decision.fallback_applied
    );
    eprintln!("jevia: launching harness `{harness_name}`");

    let mut execution = ExecutionEvidence {
        harness: harness_name.to_owned(),
        model: invocation.model.clone(),
        duration_ms: 0,
        exit_code: None,
        verification: None,
    };
    store::record_state(
        &paths.runs,
        &record.decision.run_id,
        RunState::Running,
        Outcome::Unknown,
        Some(execution.clone()),
    )?;
    let started = Instant::now();
    let status = runner
        .run(
            &invocation.program,
            &invocation.args,
            &paths.root,
            options.timeout_seconds.map(std::time::Duration::from_secs),
        )
        .await;
    let status = match status {
        Ok(processes::ProcessResult::Exited(status)) => status,
        Ok(processes::ProcessResult::Stopped { state, .. }) => {
            execution.duration_ms =
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            return record_stopped(paths, record, execution, state);
        }
        Err(error) => {
            execution.duration_ms =
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            store::record_state(
                &paths.runs,
                &record.decision.run_id,
                RunState::LaunchFailed,
                Outcome::Unknown,
                Some(execution),
            )?;
            return Err(error)
                .with_context(|| format!("could not launch harness `{harness_name}`"));
        }
    };
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    execution.duration_ms = duration_ms;
    execution.exit_code = status.code();
    if !status.success() {
        store::record_execution(
            &paths.runs,
            &record.decision.run_id,
            Outcome::Failure,
            execution,
        )?;
        eprintln!("jevia: outcome=failure duration_ms={duration_ms}");
        return Ok(child_exit_code(&status));
    }

    let Some(verification) = &invocation.verification else {
        store::record_execution(
            &paths.runs,
            &record.decision.run_id,
            Outcome::Success,
            execution,
        )?;
        eprintln!("jevia: outcome=success duration_ms={duration_ms}");
        return Ok(child_exit_code(&status));
    };

    eprintln!("jevia: verifying harness `{harness_name}`");
    store::record_state(
        &paths.runs,
        &record.decision.run_id,
        RunState::Verifying,
        Outcome::Unknown,
        Some(execution.clone()),
    )?;
    let verification_started = Instant::now();
    let verification_status = runner
        .run(
            &verification.program,
            &verification.args,
            &paths.root,
            options
                .verification_timeout_seconds
                .map(std::time::Duration::from_secs),
        )
        .await;
    let verification_status = match verification_status {
        Ok(processes::ProcessResult::Exited(status)) => status,
        Ok(processes::ProcessResult::Stopped { state, launched }) => {
            execution.verification = Some(VerificationEvidence {
                command: verification.program.clone(),
                launched,
                duration_ms: u64::try_from(verification_started.elapsed().as_millis())
                    .unwrap_or(u64::MAX),
                exit_code: None,
            });
            return record_stopped(paths, record, execution, state);
        }
        Err(error) => {
            let verification_duration_ms =
                u64::try_from(verification_started.elapsed().as_millis()).unwrap_or(u64::MAX);
            execution.verification = Some(VerificationEvidence {
                command: verification.program.clone(),
                launched: false,
                duration_ms: verification_duration_ms,
                exit_code: None,
            });
            store::record_state(
                &paths.runs,
                &record.decision.run_id,
                RunState::LaunchFailed,
                Outcome::Unknown,
                Some(execution),
            )?;
            return Err(error).with_context(|| {
                format!("could not launch verification for harness `{harness_name}`")
            });
        }
    };
    let verification_duration_ms =
        u64::try_from(verification_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    execution.verification = Some(VerificationEvidence {
        command: verification.program.clone(),
        launched: true,
        duration_ms: verification_duration_ms,
        exit_code: verification_status.code(),
    });
    let outcome = if verification_status.success() {
        Outcome::Success
    } else {
        Outcome::Failure
    };
    store::record_execution(&paths.runs, &record.decision.run_id, outcome, execution)?;
    eprintln!(
        "jevia: outcome={outcome} duration_ms={duration_ms} verification_duration_ms={verification_duration_ms}"
    );

    Ok(child_exit_code(&verification_status))
}

fn record_stopped(
    paths: &ProjectPaths,
    record: &RouteRecord,
    execution: ExecutionEvidence,
    state: RunState,
) -> Result<ExitCode> {
    store::record_state(
        &paths.runs,
        &record.decision.run_id,
        state,
        Outcome::Unknown,
        Some(execution),
    )?;
    eprintln!("jevia: state={state:?} outcome=unknown; work was not rerun");
    Ok(ExitCode::from(match state {
        RunState::Cancelled => 130,
        RunState::TimedOut => 124,
        _ => 1,
    }))
}

fn child_exit_code(status: &std::process::ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from)
}

async fn routed_record(
    task: &str,
    harness_name: Option<&str>,
    no_cache: bool,
    config: &Config,
    paths: &ProjectPaths,
) -> Result<RouteRecord> {
    let history = store::load(&paths.runs)?;
    let cache_key = if config.cache.enabled && !no_cache {
        match route_cache_key(task, harness_name, config, &history) {
            Ok(key) => match cache::lookup(&paths.cache, &key) {
                Ok(Some(decision)) => {
                    eprintln!("jevia: routing cache hit");
                    let stored_task = config.privacy.store_task_text.then(|| task.to_owned());
                    return Ok(RouteRecord::new(decision.for_cache_hit(), stored_task));
                }
                Ok(None) => Some(key),
                Err(error) => {
                    eprintln!("jevia: routing cache unavailable: {error:#}");
                    None
                }
            },
            Err(error) => {
                eprintln!("jevia: routing cache key unavailable: {error:#}");
                None
            }
        }
    } else {
        None
    };

    let api_key = env::var("TYPESAFE_API_KEY")
        .context("TYPESAFE_API_KEY is not set; Jevia never stores this key in config")?;
    let client = JevClient::new(api_key, &config.jev)?;
    let decision = client.route(task, config, &history).await?;
    if let Some(key) = cache_key
        && let Err(error) = cache::insert(
            &paths.cache,
            key,
            decision.clone(),
            config.cache.ttl_seconds,
            config.cache.max_entries,
        )
    {
        eprintln!("jevia: could not update routing cache: {error:#}");
    }
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
        let state = record
            .lifecycle
            .as_ref()
            .map(|life| format!("{:?}", life.state))
            .unwrap_or_else(|| "legacy".to_owned());
        let evidence = record
            .outcome_evidence
            .as_ref()
            .map(|evidence| format!("{:?}", evidence.source))
            .unwrap_or_else(|| "legacy".to_owned());
        println!(
            "state={state} evidence={evidence} learning={}",
            record.is_learning_evidence()
        );
        if let Some(execution) = &record.execution {
            let verification = match &execution.verification {
                Some(verification) if !verification.launched => "unknown",
                Some(verification) if verification.exit_code == Some(0) => "pass",
                Some(_) => "fail",
                None => "none",
            };
            println!(
                "{}  tier={}  model={}  source={}  confidence={:.2}  outcome={}  verification={}  duration={}ms",
                record.decision.run_id,
                record.decision.tier,
                execution.model,
                record.decision.source,
                record.decision.confidence,
                record.outcome,
                verification,
                execution.duration_ms
            );
        } else {
            println!(
                "{}  tier={}  source={}  confidence={:.2}  outcome={}",
                record.decision.run_id,
                record.decision.tier,
                record.decision.source,
                record.decision.confidence,
                record.outcome
            );
        }
    }
    Ok(())
}

fn show_run(run_id: &str) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let record = store::load(&paths.runs)?
        .into_iter()
        .find(|r| r.decision.run_id == run_id)
        .context("run id was not found in local history")?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

fn recover_run(run_id: &str) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let _lease = lease::try_acquire(&paths.directory.join("run-leases"), run_id)?
        .context("run still has an active Jevia supervisor; recovery refused")?;
    let record = store::record_state(
        &paths.runs,
        run_id,
        RunState::Interrupted,
        Outcome::Unknown,
        None,
    )?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    eprintln!(
        "jevia: marked interrupted; no processes were rerun or stopped. Inspect the workspace and any surviving agent before retrying."
    );
    Ok(())
}

fn feedback(run_id: &str, outcome: Outcome, reason: Option<&str>, print_json: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let record = store::update_outcome(&paths.runs, run_id, outcome, reason)?;
    if print_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        println!("Updated {run_id}: outcome={outcome}");
    }
    Ok(())
}

fn doctor() -> Result<()> {
    let _config = local_diagnostics()?;
    required_api_key("doctor")?;
    Ok(())
}

async fn check() -> Result<()> {
    let config = local_diagnostics()?;
    let api_key = required_api_key("check")?;
    let client = JevClient::new(api_key, &config.jev)?;
    let decision = client
        .route(
            "Verify that Jevia can reach Jev and decode a routing decision.",
            &config,
            &[],
        )
        .await
        .context("live routing check failed")?;

    println!(
        "jev api: ok (model={}, tier={}, confidence={:.2})",
        decision.jev_model, decision.tier, decision.confidence
    );
    println!("jevia: ready");
    Ok(())
}

fn local_diagnostics() -> Result<Config> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let history = store::load(&paths.runs)?;
    let cache_stats = cache::stats(&paths.cache)
        .context("routing cache is invalid; run `jevia cache clear` to reset it")?;
    println!("config: ok ({})", paths.config.display());
    println!("tiers: ok ({})", config.tiers.len());
    println!("harnesses: ok ({})", config.harnesses.len());
    println!(
        "local store: ok ({} records at {})",
        history.len(),
        paths.runs.display()
    );
    println!(
        "routing cache: ok ({} total, {} active, {} expired at {})",
        cache_stats.total,
        cache_stats.active,
        cache_stats.expired,
        paths.cache.display()
    );

    Ok(config)
}

fn required_api_key(command: &str) -> Result<String> {
    match env::var("TYPESAFE_API_KEY") {
        Ok(value) if !value.trim().is_empty() => {
            println!("TYPESAFE_API_KEY: set");
            Ok(value)
        }
        _ => {
            println!("TYPESAFE_API_KEY: missing");
            bail!("{command} found a missing required credential")
        }
    }
}

fn cache_command(action: CacheAction) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    match action {
        CacheAction::Status => {
            let config = load_config(&paths)?;
            let stats = cache::stats(&paths.cache)?;
            println!("enabled:     {}", config.cache.enabled);
            println!("ttl:         {}s", config.cache.ttl_seconds);
            println!("max entries: {}", config.cache.max_entries);
            println!("total:       {}", stats.total);
            println!("active:      {}", stats.active);
            println!("expired:     {}", stats.expired);
            println!("path:        {}", paths.cache.display());
        }
        CacheAction::Clear => {
            if cache::clear(&paths.cache)? {
                println!("Cleared routing cache at {}", paths.cache.display());
            } else {
                println!("Routing cache is already empty.");
            }
        }
    }
    Ok(())
}

fn ensure_local_ignore(path: &std::path::Path) -> Result<()> {
    const RULES: [&str; 6] = [
        "run-leases/",
        "runs.jsonl",
        "runs.lock",
        "cache.jsonl",
        "cache.lock",
        "*.tmp",
    ];

    let mut contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    let mut changed = false;
    for rule in RULES {
        if !contents.lines().any(|line| line == rule) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(rule);
            contents.push('\n');
            changed = true;
        }
    }
    if changed {
        fs::write(path, contents).with_context(|| format!("could not write {}", path.display()))?;
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
    println!("source:     {}", record.decision.source);
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use jevia_core::{HarnessInvocation, RouteDecision, VerificationInvocation};
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn routing_cache_hit_needs_no_live_request() {
        let directory = tempdir().expect("temporary directory");
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        fs::create_dir_all(&paths.directory).expect("Jevia directory is created");
        let config = Config::default();
        let cached = sample_record().decision;
        let key = route_cache_key("test task", None, &config, &[]).expect("cache key is created");
        cache::insert(
            &paths.cache,
            key,
            cached.clone(),
            config.cache.ttl_seconds,
            config.cache.max_entries,
        )
        .expect("decision is cached");

        let record = routed_record("test task", None, false, &config, &paths)
            .await
            .expect("cached routing succeeds");

        assert_eq!(record.decision.source, jevia_core::DecisionSource::Cache);
        assert_eq!(record.decision.tier, cached.tier);
        assert_ne!(record.decision.run_id, cached.run_id);
    }

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
            verification: None,
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
        assert!(!records[0].is_learning_evidence());
        assert_eq!(
            records[0].outcome_evidence.as_ref().unwrap().source,
            jevia_core::OutcomeSource::ProcessExit
        );
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
            verification: Some(VerificationInvocation {
                program: "rustc".to_owned(),
                args: vec!["--version".to_owned()],
            }),
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
        assert_eq!(execution.verification, None);
    }

    #[tokio::test]
    async fn harness_launch_failure_retains_identity_and_terminal_state() {
        let directory = tempdir().unwrap();
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        let record = sample_record();
        store::append(&paths.runs, &record).unwrap();
        let invocation = HarnessInvocation {
            program: "jevia-nonexistent-lifecycle-harness".to_owned(),
            args: vec![],
            model: "provider/test".to_owned(),
            verification: None,
        };
        assert!(
            execute_harness(&paths, "test", &invocation, &record)
                .await
                .is_err()
        );
        let record = store::load(&paths.runs).unwrap().remove(0);
        let life = record.lifecycle.unwrap();
        assert_eq!(life.state, RunState::LaunchFailed);
        assert!(life.started_at_ms.is_some());
        assert!(life.finished_at_ms.is_some());
        assert_eq!(record.outcome, Outcome::Unknown);
        assert_eq!(record.execution.unwrap().model, "provider/test");
    }

    #[tokio::test]
    async fn successful_verification_records_success() {
        let directory = tempdir().expect("temporary directory");
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        fs::create_dir_all(&paths.directory).expect("Jevia directory is created");
        let record = sample_record();
        store::append(&paths.runs, &record).expect("record is appended");
        let invocation = verified_invocation(vec!["--version".to_owned()]);

        execute_harness(&paths, "test", &invocation, &record)
            .await
            .expect("harness and verification succeed");

        let records = store::load(&paths.runs).expect("records load");
        assert_eq!(records[0].outcome, Outcome::Success);
        let verification = records[0]
            .execution
            .as_ref()
            .and_then(|execution| execution.verification.as_ref())
            .expect("verification evidence is recorded");
        assert_eq!(verification.command, "rustc");
        assert!(verification.launched);
        assert_eq!(verification.exit_code, Some(0));
        assert!(records[0].is_learning_evidence());
        assert_eq!(
            records[0].outcome_evidence.as_ref().unwrap().source,
            jevia_core::OutcomeSource::Verification
        );
    }

    #[tokio::test]
    async fn failed_verification_overrides_a_successful_harness() {
        let directory = tempdir().expect("temporary directory");
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        fs::create_dir_all(&paths.directory).expect("Jevia directory is created");
        let record = sample_record();
        store::append(&paths.runs, &record).expect("record is appended");
        let invocation =
            verified_invocation(vec!["--definitely-not-a-real-rustc-option".to_owned()]);

        execute_harness(&paths, "test", &invocation, &record)
            .await
            .expect("verification failure is a recorded outcome");

        let records = store::load(&paths.runs).expect("records load");
        assert_eq!(records[0].outcome, Outcome::Failure);
        let execution = records[0]
            .execution
            .as_ref()
            .expect("execution evidence is recorded");
        assert_eq!(execution.exit_code, Some(0));
        assert_ne!(
            execution
                .verification
                .as_ref()
                .expect("verification evidence is recorded")
                .exit_code,
            Some(0)
        );
    }

    #[tokio::test]
    async fn verifier_launch_failure_keeps_the_outcome_unknown() {
        let directory = tempdir().expect("temporary directory");
        let paths = ProjectPaths::at(directory.path().to_path_buf());
        fs::create_dir_all(&paths.directory).expect("Jevia directory is created");
        let record = sample_record();
        store::append(&paths.runs, &record).expect("record is appended");
        let invocation = HarnessInvocation {
            program: "rustc".to_owned(),
            args: vec!["--version".to_owned()],
            model: "provider/test".to_owned(),
            verification: Some(VerificationInvocation {
                program: "jevia-command-that-does-not-exist".to_owned(),
                args: vec![],
            }),
        };

        let error = execute_harness(&paths, "test", &invocation, &record)
            .await
            .expect_err("missing verifier is reported");

        assert!(
            error
                .to_string()
                .contains("could not launch verification for harness `test`")
        );
        let records = store::load(&paths.runs).expect("records load");
        assert_eq!(records[0].outcome, Outcome::Unknown);
        let execution = records[0]
            .execution
            .as_ref()
            .expect("harness evidence is retained");
        assert_eq!(execution.exit_code, Some(0));
        let verification = execution
            .verification
            .as_ref()
            .expect("verification attempt is retained");
        assert_eq!(verification.command, "jevia-command-that-does-not-exist");
        assert!(!verification.launched);
        assert_eq!(verification.exit_code, None);
    }

    fn verified_invocation(verification_args: Vec<String>) -> HarnessInvocation {
        HarnessInvocation {
            program: "rustc".to_owned(),
            args: vec!["--version".to_owned()],
            model: "provider/test".to_owned(),
            verification: Some(VerificationInvocation {
                program: "rustc".to_owned(),
                args: verification_args,
            }),
        }
    }

    fn long_process() -> (String, Vec<String>) {
        #[cfg(unix)]
        {
            ("sleep".into(), vec!["20".into()])
        }
        #[cfg(windows)]
        {
            (
                "cmd".into(),
                vec!["/C".into(), "ping -n 20 127.0.0.1 >NUL".into()],
            )
        }
    }

    #[tokio::test]
    async fn harness_timeout_records_unknown_and_skips_verification() {
        let dir = tempdir().unwrap();
        let paths = ProjectPaths::at(dir.path().to_path_buf());
        let record = sample_record();
        store::append(&paths.runs, &record).unwrap();
        let (program, args) = long_process();
        let invocation = HarnessInvocation {
            program,
            args,
            model: "test".into(),
            verification: Some(VerificationInvocation {
                program: "must-not-run".into(),
                args: vec![],
            }),
        };
        let code = execute_harness_with_options(
            &paths,
            "test",
            &invocation,
            &record,
            RunOptions {
                non_interactive: true,
                timeout_seconds: Some(1),
                verification_timeout_seconds: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(code, ExitCode::from(124));
        let stored = store::load(&paths.runs).unwrap().remove(0);
        assert_eq!(stored.outcome, Outcome::Unknown);
        assert_eq!(stored.lifecycle.unwrap().state, RunState::TimedOut);
        assert!(stored.execution.unwrap().verification.is_none());
    }

    #[tokio::test]
    async fn verifier_timeout_preserves_successful_harness_evidence() {
        let dir = tempdir().unwrap();
        let paths = ProjectPaths::at(dir.path().to_path_buf());
        let record = sample_record();
        store::append(&paths.runs, &record).unwrap();
        let (program, args) = long_process();
        let invocation = HarnessInvocation {
            program: "rustc".into(),
            args: vec!["--version".into()],
            model: "test".into(),
            verification: Some(VerificationInvocation { program, args }),
        };
        let code = execute_harness_with_options(
            &paths,
            "test",
            &invocation,
            &record,
            RunOptions {
                non_interactive: true,
                timeout_seconds: None,
                verification_timeout_seconds: Some(1),
            },
        )
        .await
        .unwrap();
        assert_eq!(code, ExitCode::from(124));
        let stored = store::load(&paths.runs).unwrap().remove(0);
        assert_eq!(stored.outcome, Outcome::Unknown);
        assert_eq!(stored.lifecycle.unwrap().state, RunState::TimedOut);
        let execution = stored.execution.unwrap();
        assert_eq!(execution.exit_code, Some(0));
        assert!(execution.verification.unwrap().launched);
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
                source: jevia_core::DecisionSource::Live,
            },
            task: Some("test task".to_owned()),
            outcome: Outcome::Unknown,
            execution: None,
            lifecycle: None,
            outcome_evidence: None,
            feedback: Vec::new(),
        }
    }
}
