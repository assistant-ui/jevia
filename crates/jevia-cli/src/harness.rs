//! Explicit harness onboarding. Only the opt-in review launch runs a harness.
mod check;
mod health;
mod review;
mod setup;

use crate::paths::ProjectPaths;
use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

#[derive(Debug, Subcommand)]
pub enum Action {
    /// List built-in shell-free command templates for supported harnesses.
    Presets,
    /// Preview a harness configuration; --apply backs up and saves it.
    Setup(setup::Options),
    /// Check local configuration and executable candidates without running anything.
    Check {
        /// Adapter name from [harnesses].
        name: String,
        /// Print a versioned, redacted report (also on check failures).
        #[arg(long)]
        json: bool,
    },
    /// Inspect recording configuration and recent stored capture; never launch an agent.
    Health {
        name: String,
        #[arg(long)]
        json: bool,
        /// Fail unless the latest saved execution has recorded, non-partial events.
        /// Does not launch the harness or prove task correctness.
        #[arg(long)]
        require_events: bool,
    },
    /// Preview a read-only Codex session for reviewing Jevia hooks with /hooks.
    Review {
        name: String,
        /// Open the interactive review session; no prompt, routing, or verification.
        #[arg(long)]
        launch: bool,
    },
}

pub async fn run(paths: &ProjectPaths, action: Action) -> Result<ExitCode> {
    match action {
        Action::Presets => setup::print_presets(),
        Action::Setup(options) => setup::run(paths, options)?,
        Action::Check { name, json } => return check::run(paths, &name, json),
        Action::Health {
            name,
            json,
            require_events,
        } => return health::run(paths, &name, json, require_events).await,
        Action::Review { name, launch } => return review::run(paths, &name, launch).await,
    }
    Ok(ExitCode::SUCCESS)
}
