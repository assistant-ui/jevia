//! Explicit harness onboarding. These commands never run configured programs.
mod setup;

use crate::paths::ProjectPaths;
use anyhow::Result;
use clap::Subcommand;
use std::process::ExitCode;

#[derive(Debug, Subcommand)]
pub enum Action {
    /// Preview a harness configuration; --apply backs up and saves it.
    Setup(setup::Options),
}

pub fn run(paths: &ProjectPaths, action: Action) -> Result<ExitCode> {
    match action {
        Action::Setup(options) => setup::run(paths, options)?,
    }
    Ok(ExitCode::SUCCESS)
}
