//! Explicit harness onboarding. These commands never run configured programs.
mod check;
mod setup;

use crate::paths::ProjectPaths;
use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

#[derive(Debug, Subcommand)]
pub enum Action {
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
}

pub fn run(paths: &ProjectPaths, action: Action) -> Result<ExitCode> {
    match action {
        Action::Setup(options) => setup::run(paths, options)?,
        Action::Check { name, json } => return check::run(paths, &name, json),
    }
    Ok(ExitCode::SUCCESS)
}
