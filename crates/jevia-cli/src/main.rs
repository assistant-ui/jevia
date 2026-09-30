mod cache;
mod config_edit;
mod diagnostics;
mod harness;
mod lease;
mod observations;
mod paths;
mod processes;
mod setup;
mod stats;
mod storage;
mod store;
mod verification;

use std::{env, fs, io::Write, process::ExitCode, str::FromStr, time::Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use jevia_core::{
    Config, ExecutionEvidence, HarnessInvocation, JevClient, Outcome, RouteRecord, RunState,
    VerificationEvidence, route_cache_key,
};

use crate::paths::ProjectPaths;
use crate::storage::Storage;

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
    #[command(hide = true)]
    CaptureEvent {
        #[arg(long)]
        journal: Option<std::path::PathBuf>,
        #[arg(long, default_value = "claude_hooks", value_parser = ["claude_hooks", "codex_hooks", "opencode_plugin"])]
        source: String,
    },
    /// Create project-local Jevia configuration.
    Init {
        /// Replace an existing config.toml with the default configuration.
        #[arg(long)]
        force: bool,
    },
    /// Configure or inspect coding-agent harness adapters without launching them.
    Harness {
        #[command(subcommand)]
        action: harness::Action,
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
    /// Show recent records from the configured history backend.
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
        /// Explanation required when changing a known outcome. Stored in history.
        #[arg(long)]
        reason: Option<String>,
        /// Print the updated record as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Summarize recent routing outcomes without calling Jev or changing history.
    Stats {
        /// Latest records in append order to include (not a time window).
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..=100_000))]
        limit: u32,
        /// Print versioned aggregate JSON without task text or run identifiers.
        #[arg(long)]
        json: bool,
    },
    /// Validate config and configured storage without making a Jev request.
    Doctor,
    /// Validate configured storage and make a live routing request.
    Check,
    /// Inspect or clear the local routing-decision cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Initialize, check, import, or export the configured history backend.
    Storage {
        #[command(subcommand)]
        action: StorageAction,
    },
}

#[derive(Debug, Subcommand)]
enum StorageAction {
    /// Preview database setup; --apply validates and saves the new storage config.
    Setup(setup::Options),
    /// Explicitly create the database schema and project (does not import history).
    Init,
    /// Check storage access, or deeply validate history without a write probe.
    Check {
        /// Scan all records and SQL routing/order metadata without changing history.
        /// Does not test write permissions or perform repairs.
        #[arg(long)]
        deep: bool,
    },
    /// Preview importing JSONL into the configured database; source is never changed.
    ImportJsonl {
        #[arg(long, default_value = ".jevia/runs.jsonl")]
        from: std::path::PathBuf,
        #[arg(long)]
        apply: bool,
    },
    /// Export a consistent history snapshot; never overwrite an existing file.
    Export {
        #[arg(long)]
        output: std::path::PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum RunsAction {
    /// Inspect one complete run record, including its execution lifecycle.
    Show { run_id: String },
    /// Save one finished app-owned execution from bounded JSON on stdin; no outcome inference.
    RecordExecution { run_id: String },
    /// Finish an externally executed pending run with an explicit manual outcome.
    Complete {
        run_id: String,
        outcome: OutcomeArgument,
        /// Confirm all external work and verification have stopped.
        #[arg(long, required = true)]
        confirm_stopped: bool,
        /// Explanation required when changing a known outcome. Stored in history.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Mark an execution whose Jevia supervisor exited as interrupted. Never reruns it.
    Recover {
        run_id: String,
        /// For PostgreSQL: confirm the original supervisor and its processes stopped.
        #[arg(long)]
        confirm_stopped: bool,
    },
    /// Preview repair of an incomplete final JSON line or missing final newline.
    Repair {
        /// Back up the original bytes, then atomically apply the repair.
        #[arg(long)]
        apply: bool,
    },
    /// Preview archival of older terminal records; never archives active/pending runs.
    Archive {
        /// Number of most recently appended terminal records to retain.
        #[arg(long, default_value_t = 1000)]
        keep: usize,
        /// Back up history, write the archive, then atomically update active history.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Default, Clone, Copy)]
struct RunOptions {
    observation_mode: jevia_core::ObservationMode,
    non_interactive: bool,
    timeout_seconds: Option<u64>,
    verification_timeout_seconds: Option<u64>,
    automatic_verification: bool,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Frequent passive hooks need neither an async runtime nor project discovery.
    if let Command::CaptureEvent { journal, source } = &cli.command {
        let journal = journal
            .clone()
            .or_else(|| std::env::var_os(observations::JOURNAL_ENV).map(Into::into));
        if let Some(journal) = journal {
            observations::receive_from(&journal, source, std::io::stdin().lock());
        }
        return ExitCode::SUCCESS;
    }
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not start runtime")
        .and_then(|runtime| runtime.block_on(run(cli)));
    match result {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::CaptureEvent { .. } => unreachable!("handled before runtime startup"),
        Command::Init { force } => {
            init(force)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Harness { action } => harness::run(&ProjectPaths::discover()?, action).await,
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
                    ..Default::default()
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
                None => runs(limit, json).await?,
                Some(RunsAction::Show { run_id }) => show_run(&run_id).await?,
                Some(RunsAction::RecordExecution { run_id }) => {
                    use std::io::Read;
                    const LIMIT: u64 = 256 * 1024;
                    let mut bytes = Vec::new();
                    std::io::stdin()
                        .lock()
                        .take(LIMIT + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| anyhow::anyhow!("could not read execution recording"))?;
                    if bytes.len() as u64 > LIMIT {
                        bail!("execution recording exceeds 256 KiB");
                    }
                    let input = serde_json::from_slice::<jevia_core::ExecutionRecording>(&bytes)
                        .map_err(|_| {
                            anyhow::anyhow!("invalid execution recording (contents redacted)")
                        })?;
                    let paths = ProjectPaths::discover()?;
                    let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
                    let record = storage.record_application(&run_id, input).await?;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                }
                Some(RunsAction::Complete {
                    run_id,
                    outcome,
                    confirm_stopped,
                    reason,
                }) => {
                    let paths = ProjectPaths::discover()?;
                    let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
                    let record = storage
                        .complete_external(
                            &run_id,
                            outcome.into(),
                            reason.as_deref(),
                            confirm_stopped,
                        )
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                }
                Some(RunsAction::Recover {
                    run_id,
                    confirm_stopped,
                }) => recover_run(&run_id, confirm_stopped).await?,
                Some(RunsAction::Repair { apply }) => {
                    maintain_history(store::Maintenance::Repair, apply, json).await?
                }
                Some(RunsAction::Archive { keep, apply }) => {
                    maintain_history(store::Maintenance::Archive { keep }, apply, json).await?
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Feedback {
            run_id,
            outcome,
            reason,
            json,
        } => {
            feedback(&run_id, outcome.into(), reason.as_deref(), json).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            doctor().await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Stats { limit, json } => {
            let paths = ProjectPaths::discover()?;
            let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
            let report = stats::collect(&storage, limit as usize).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
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
        Command::Storage { action } => {
            storage_command(action).await?;
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
    let storage = Storage::open(&config, &paths, false).await?;
    let record = routed_record_in(task, None, no_cache, &config, &paths, &storage).await?;
    storage.append(&record).await?;

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
    mut options: RunOptions,
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
    let storage = Storage::open(&config, &paths, false).await?;
    let record = routed_record_in(
        task,
        Some(harness_name),
        no_cache,
        &config,
        &paths,
        &storage,
    )
    .await?;
    let mut invocation = harness.invocation(
        harness_name,
        &record.decision.tier,
        task,
        &record.decision.run_id,
        extra_args,
    )?;
    options.observation_mode = harness.observations;
    if invocation.verification.is_none() && harness.auto_verify {
        match verification::detect(&paths.root) {
            verification::Detection::Found(plan) => {
                eprintln!(
                    "jevia: automatic verification: {} {}",
                    plan.program,
                    plan.args.join(" ")
                );
                invocation.verification = Some(plan);
                options.automatic_verification = true;
            }
            verification::Detection::Unavailable(message) => {
                eprintln!("jevia: verification unavailable: {message}")
            }
        }
    }
    storage.append(&record).await?;

    execute_stored_harness(
        &paths,
        harness_name,
        &invocation,
        &record,
        options,
        &storage,
    )
    .await
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

#[cfg(test)]
async fn execute_harness_with_options(
    paths: &ProjectPaths,
    harness_name: &str,
    invocation: &HarnessInvocation,
    record: &RouteRecord,
    options: RunOptions,
) -> Result<ExitCode> {
    let storage = Storage::open(&Config::default(), paths, false).await?;
    execute_stored_harness(paths, harness_name, invocation, record, options, &storage).await
}

async fn execute_stored_harness(
    paths: &ProjectPaths,
    harness_name: &str,
    invocation: &HarnessInvocation,
    record: &RouteRecord,
    options: RunOptions,
    storage: &Storage,
) -> Result<ExitCode> {
    let _lease = storage.execution_guard(&record.decision.run_id).await?;
    let capture = observations::Capture::prepare(
        options.observation_mode,
        invocation,
        &paths.directory,
        &record.decision.run_id,
    )
    .await;
    let result = execute_observed_harness(
        paths,
        harness_name,
        invocation,
        record,
        options,
        storage,
        &capture,
    )
    .await;
    if result.is_ok() {
        // Read the committed snapshot: a concurrent/timed-out checkpoint may have
        // saved newer observations than the supervisor's in-memory copy.
        if let Ok(saved) = storage.get(&record.decision.run_id).await
            && let Some(observations) = saved.execution.and_then(|e| e.observations)
        {
            capture.persisted(&observations);
        }
    }
    result
}

async fn execute_observed_harness(
    paths: &ProjectPaths,
    harness_name: &str,
    invocation: &HarnessInvocation,
    record: &RouteRecord,
    options: RunOptions,
    storage: &Storage,
    capture: &observations::Capture,
) -> Result<ExitCode> {
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
        observations: Some(capture.snapshot()),
        harness: harness_name.to_owned(),
        model: invocation.model.clone(),
        duration_ms: 0,
        exit_code: None,
        verification: None,
    };
    storage
        .state(
            &record.decision.run_id,
            RunState::Running,
            Outcome::Unknown,
            Some(execution.clone()),
        )
        .await?;
    let started = Instant::now();
    let mut previous = execution.observations.clone().expect("capture initialized");
    runner.set_environment(&capture.environment);
    let status = {
        let run = runner.run(
            &invocation.program,
            &capture.args,
            &paths.root,
            options.timeout_seconds.map(std::time::Duration::from_secs),
        );
        tokio::pin!(run);
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(2));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut warned = false;
        loop {
            tokio::select! {
                result = &mut run => break result,
                _ = timer.tick() => {
                    if !matches!(tokio::time::timeout(std::time::Duration::from_secs(1), capture.checkpoint(storage, &record.decision.run_id, &mut previous)).await, Ok(Ok(()))) && !warned {
                        eprintln!("jevia: observation checkpoint delayed; journal retained (details redacted)");
                        warned = true;
                    }
                }
            }
        }
    };
    execution.observations = Some(capture.snapshot_after(&previous));
    // Optional verifiers must not inherit a harness adapter's private config.
    runner.set_environment(&[]);
    let status = match status {
        Ok(processes::ProcessResult::Exited(status)) => status,
        Ok(processes::ProcessResult::Stopped { state, .. }) => {
            execution.duration_ms =
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            return record_stopped(storage, record, execution, state).await;
        }
        Err(error) => {
            execution.duration_ms =
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            storage
                .state(
                    &record.decision.run_id,
                    RunState::LaunchFailed,
                    Outcome::Unknown,
                    Some(execution),
                )
                .await?;
            return Err(error)
                .with_context(|| format!("could not launch harness `{harness_name}`"));
        }
    };
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    execution.duration_ms = duration_ms;
    execution.exit_code = status.code();
    if !status.success() {
        storage
            .state(
                &record.decision.run_id,
                RunState::Completed,
                Outcome::Unknown,
                Some(execution),
            )
            .await?;
        eprintln!(
            "jevia: process=failed outcome=unknown observations=recorded duration_ms={duration_ms}"
        );
        return Ok(child_exit_code(&status));
    }

    let Some(verification) = &invocation.verification else {
        storage
            .state(
                &record.decision.run_id,
                RunState::Completed,
                Outcome::Unknown,
                Some(execution),
            )
            .await?;
        eprintln!(
            "jevia: process=success outcome=unknown verification=not_run observations=recorded duration_ms={duration_ms}"
        );
        return Ok(child_exit_code(&status));
    };

    eprintln!("jevia: verifying harness `{harness_name}`");
    storage
        .state(
            &record.decision.run_id,
            RunState::Verifying,
            Outcome::Unknown,
            Some(execution.clone()),
        )
        .await?;
    let verification_started = Instant::now();
    // Automatic tests must not open a watch session or inherit interactive stdin.
    // Own their process tree even when the coding agent itself was interactive.
    let verification_status = async {
        let mut automatic_runner = if options.automatic_verification {
            Some(processes::Runner::automatic_verification()?)
        } else {
            None
        };
        let verifier = automatic_runner.as_mut().unwrap_or(&mut runner);
        verifier
            .run(
                &verification.program,
                &verification.args,
                &paths.root,
                options
                    .verification_timeout_seconds
                    .or(options.automatic_verification.then_some(300))
                    .map(std::time::Duration::from_secs),
            )
            .await
    }
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
            return record_stopped(storage, record, execution, state).await;
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
            storage
                .state(
                    &record.decision.run_id,
                    RunState::LaunchFailed,
                    Outcome::Unknown,
                    Some(execution),
                )
                .await?;
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
    storage
        .state(
            &record.decision.run_id,
            RunState::Completed,
            outcome,
            Some(execution),
        )
        .await?;
    let evidence = if verification_status.code().is_some() {
        "verification"
    } else {
        "process_exit"
    };
    eprintln!(
        "jevia: outcome={outcome} evidence={evidence} recorded=true duration_ms={duration_ms} verification_duration_ms={verification_duration_ms}"
    );

    Ok(child_exit_code(&verification_status))
}

async fn record_stopped(
    storage: &Storage,
    record: &RouteRecord,
    execution: ExecutionEvidence,
    state: RunState,
) -> Result<ExitCode> {
    storage
        .state(
            &record.decision.run_id,
            state,
            Outcome::Unknown,
            Some(execution),
        )
        .await?;
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

#[cfg(test)]
async fn routed_record(
    task: &str,
    harness_name: Option<&str>,
    no_cache: bool,
    config: &Config,
    paths: &ProjectPaths,
) -> Result<RouteRecord> {
    let storage = Storage::open(config, paths, false).await?;
    routed_record_in(task, harness_name, no_cache, config, paths, &storage).await
}

async fn routed_record_in(
    task: &str,
    harness_name: Option<&str>,
    no_cache: bool,
    config: &Config,
    paths: &ProjectPaths,
    storage: &Storage,
) -> Result<RouteRecord> {
    // Replay must not turn a slow/offline observation store into an unbounded
    // startup delay. Cancellation leaves the journal for the next invocation.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        observations::replay(paths, storage, None),
    )
    .await;
    let wait_started = Instant::now();
    let wait_budget =
        std::time::Duration::from_millis(config.jev.timeout_ms.saturating_add(1_000).min(30_000));
    // The lease remains held through the HTTP request and cache insertion, never
    // while holding a global history/cache lock. Reload evidence after each wait.
    let (history, cache_key, _request_lease) = loop {
        let history = storage.routing_history(config.router.history_limit).await?;
        if !config.cache.enabled || no_cache {
            break (history, None, None);
        }
        let key = match route_cache_key(task, harness_name, config, &history) {
            Ok(key) => key,
            Err(error) => {
                eprintln!("jevia: routing cache key unavailable: {error:#}");
                break (history, None, None);
            }
        };
        match cache::lookup(&paths.cache, &key) {
            Ok(Some(decision)) => {
                eprintln!("jevia: routing cache hit");
                let stored_task = config.privacy.store_task_text.then(|| task.to_owned());
                return Ok(RouteRecord::new(decision.for_cache_hit(), stored_task));
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("jevia: routing cache unavailable: {error:#}");
                break (history, None, None);
            }
        }
        if wait_started.elapsed() >= wait_budget {
            eprintln!("jevia: cache coordination wait expired; routing live");
            break (history, Some(key), None);
        }
        // The first byte of the SHA-256 key selects one of 256 fixed stripes.
        // Sidecars stay stable without growing once per distinct task forever.
        match lease::try_acquire(&paths.directory.join("cache-leases"), &key[..2]) {
            Ok(Some(guard)) => {
                let latest = storage.routing_history(config.router.history_limit).await?;
                let latest_key = route_cache_key(task, harness_name, config, &latest)?;
                if latest_key != key {
                    continue;
                }
                // Another process may have populated the cache between our
                // first lookup and acquisition of the request lease.
                match cache::lookup(&paths.cache, &key) {
                    Ok(Some(decision)) => {
                        eprintln!("jevia: routing cache hit");
                        let stored_task = config.privacy.store_task_text.then(|| task.to_owned());
                        return Ok(RouteRecord::new(decision.for_cache_hit(), stored_task));
                    }
                    Ok(None) => break (latest, Some(key), Some(guard)),
                    Err(error) => {
                        eprintln!("jevia: routing cache unavailable: {error:#}");
                        break (latest, None, None);
                    }
                }
            }
            Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(25)).await,
            Err(error) => {
                eprintln!("jevia: cache coordination unavailable: {error:#}");
                break (history, Some(key), None);
            }
        }
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

async fn maintain_history(
    operation: store::Maintenance,
    apply: bool,
    print_json: bool,
) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    if !config.storage.is_jsonl() && matches!(operation, store::Maintenance::Repair) {
        bail!(
            "runs repair is JSONL-only; database integrity repair requires database-native tooling"
        );
    }
    if apply {
        ensure_local_ignore(&paths.directory.join(".gitignore"))?;
    }
    let report = match operation {
        store::Maintenance::Repair => store::maintain(&paths.runs, operation, apply)?,
        store::Maintenance::Archive { keep } => {
            Storage::open(&config, &paths, false)
                .await?
                .archive(&paths, keep, apply)
                .await?
        }
    };
    if print_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{} {}: retain {} records, archive {} records, remove {} incomplete trailing bytes, add final newline: {}",
            if report.applied { "Applied" } else { "Preview" },
            report.operation,
            report.retained_records,
            report.archived_records,
            report.truncated_tail_bytes,
            report.added_final_newline
        );
        if !report.would_change {
            println!("No changes needed.");
        } else if !apply {
            println!("Inspect this preview, then repeat with --apply to save a backup and apply.");
        }
        if let Some(path) = report.backup {
            println!("Original backup: {}", path.display());
        }
        if let Some(path) = report.archive {
            println!("Archive: {}", path.display());
        }
    }
    Ok(())
}

async fn runs(limit: usize, print_json: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
    let records = storage.recent(limit, false).await?;

    if print_json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("No runs recorded.");
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
            "state={state} evidence={evidence} quality_evidence={} execution_observed={}",
            record.is_learning_evidence(),
            record.is_execution_observation()
        );
        if let Some(execution) = &record.execution {
            let verification = match &execution.verification {
                Some(verification) if !verification.launched => "unknown",
                Some(verification) if verification.exit_code == Some(0) => "pass",
                Some(_) => "fail",
                None => "none",
            };
            println!(
                "{}  tier={}  requested_model={}  source={}  confidence={:.2}  outcome={}  verification={}  duration={}ms",
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

async fn show_run(run_id: &str) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
    let record = storage.get(run_id).await?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

async fn recover_run(run_id: &str, confirm_stopped: bool) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
    storage.recover(run_id, confirm_stopped).await?;
    observations::replay(&paths, &storage, Some(run_id)).await;
    let record = storage.get(run_id).await?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    eprintln!(
        "jevia: marked interrupted; no processes were rerun or stopped. Inspect the workspace and any surviving agent before retrying."
    );
    Ok(())
}

async fn feedback(
    run_id: &str,
    outcome: Outcome,
    reason: Option<&str>,
    print_json: bool,
) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    let storage = Storage::open(&load_config(&paths)?, &paths, false).await?;
    let record = storage.outcome(run_id, outcome, reason).await?;
    if print_json {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        println!("Updated {run_id}: outcome={outcome}");
    }
    Ok(())
}

async fn doctor() -> Result<()> {
    let _config = local_diagnostics().await?;
    required_api_key("doctor")?;
    Ok(())
}

async fn check() -> Result<()> {
    let config = local_diagnostics().await?;
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

async fn local_diagnostics() -> Result<Config> {
    let paths = ProjectPaths::discover()?;
    let config = load_config(&paths)?;
    let storage = Storage::open(&config, &paths, false).await?;
    let count = storage.check().await?;
    let cache_stats = cache::stats(&paths.cache)
        .context("routing cache is invalid; run `jevia cache clear` to reset it")?;
    println!("config: ok ({})", paths.config.display());
    println!("tiers: ok ({})", config.tiers.len());
    println!("harnesses: ok ({})", config.harnesses.len());
    println!("store: ok ({count} records, backend={})", storage.name());
    println!(
        "routing cache: ok ({} total, {} active, {} expired at {})",
        cache_stats.total,
        cache_stats.active,
        cache_stats.expired,
        paths.cache.display()
    );

    Ok(config)
}

async fn storage_command(action: StorageAction) -> Result<()> {
    let paths = ProjectPaths::discover()?;
    if let StorageAction::Setup(options) = action {
        return setup::run(&paths, options).await;
    }
    let config = load_config(&paths)?;
    let storage = Storage::open(&config, &paths, matches!(action, StorageAction::Init)).await?;
    match action {
        StorageAction::Setup(_) => unreachable!("setup is handled before opening current storage"),
        StorageAction::Init => {
            ensure_local_ignore(&paths.directory.join(".gitignore"))?;
            println!(
                "Storage initialized ({}). Existing JSONL history was not imported or modified.",
                storage.name()
            );
        }
        StorageAction::Check { deep } => {
            let count = if deep {
                storage.check_deep().await?
            } else {
                storage.check().await?
            };
            println!(
                "storage: ok (backend={}, records={}{})",
                storage.name(),
                count,
                if deep { ", check=deep" } else { "" }
            );
        }
        StorageAction::ImportJsonl { from, apply } => {
            let (imported, skipped) = storage.import_jsonl(paths.root.join(from), apply).await?;
            println!(
                "{}: {imported} records {}, {skipped} identical records skipped. Source unchanged.",
                if apply { "Applied" } else { "Preview" },
                if apply { "imported" } else { "to import" }
            );
            if !apply {
                println!("Stop source writers, then repeat with --apply to import atomically.");
            }
        }
        StorageAction::Export { output } => {
            let count = storage.export(&paths.root.join(output)).await?;
            println!("Exported {count} records. Protect the snapshot: it may contain task text.");
        }
    }
    Ok(())
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
    const RULES: [&str; 15] = [
        "config.lock",
        "config-backups/",
        "*.db",
        "*.db-wal",
        "*.db-shm",
        "*.run-locks/",
        "run-leases/",
        "cache-leases/",
        "history-backups/",
        "history-archives/",
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
    Config::from_toml(&input).map_err(|error| diagnostics::configuration(error, &input))
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
    async fn successful_harness_process_keeps_task_outcome_unknown() {
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
        assert_eq!(records[0].outcome, Outcome::Unknown);
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
    async fn failed_harness_process_keeps_task_outcome_unknown() {
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
        assert_eq!(records[0].outcome, Outcome::Unknown);
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
                ..Default::default()
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

    #[cfg(unix)]
    #[tokio::test]
    async fn verification_and_completion_wait_for_background_children() {
        let dir = tempdir().unwrap();
        let paths = ProjectPaths::at(dir.path().to_path_buf());
        let record = sample_record();
        store::append(&paths.runs, &record).unwrap();
        let invocation = HarnessInvocation {
            program: "sh".into(),
            args: vec!["-c".into(), "(sleep 0.2; touch ready) & exit 0".into()],
            model: "test".into(),
            verification: Some(VerificationInvocation {
                program: "sh".into(),
                args: vec![
                    "-c".into(),
                    "test -f ready || exit 1; (sleep 0.2; touch verified) & exit 0".into(),
                ],
            }),
        };
        let code = execute_harness_with_options(
            &paths,
            "test",
            &invocation,
            &record,
            RunOptions {
                non_interactive: true,
                timeout_seconds: Some(5),
                verification_timeout_seconds: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(dir.path().join("ready").exists());
        assert!(dir.path().join("verified").exists());
        let stored = store::load(&paths.runs).unwrap().remove(0);
        assert_eq!(stored.outcome, Outcome::Success);
        assert_eq!(
            stored.lifecycle.as_ref().unwrap().state,
            RunState::Completed
        );
        assert!(stored.is_learning_evidence());
    }

    #[tokio::test]
    async fn verifier_timeout_preserves_successful_harness_evidence() {
        for automatic_verification in [false, true] {
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
                    non_interactive: !automatic_verification,
                    timeout_seconds: None,
                    verification_timeout_seconds: Some(1),
                    automatic_verification,
                    observation_mode: Default::default(),
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
