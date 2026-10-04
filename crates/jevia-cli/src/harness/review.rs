//! Trust remains a user decision made in the native hook browser.
use crate::{observations, paths::ProjectPaths};
use anyhow::{Context, Result, bail};
use jevia_core::{
    Config, HarnessInvocation, ObservationMode, ObservationSource, ObservationStatus,
};
use std::{io::IsTerminal, process::ExitCode};

fn invocation(config: &Config, name: &str) -> Result<HarnessInvocation> {
    let harness = config
        .harnesses
        .get(name)
        .context("harness not configured")?;
    if harness.observations == ObservationMode::Off {
        bail!("native capture is disabled; review does not enable it");
    }
    if observations::configured_source(harness.observations, &harness.command)
        != Some(ObservationSource::CodexHooks)
        || harness.args != ["exec", "--model", "{model}", "{task}"]
    {
        bail!(
            "guided review requires the standard Codex exec preset; custom arguments are not silently discarded"
        );
    }
    Ok(HarnessInvocation {
        program: harness.command.clone(),
        args: vec!["--sandbox".into(), "read-only".into()],
        model: String::new(),
        verification: None,
    })
}

pub async fn run(paths: &ProjectPaths, name: &str, launch: bool) -> Result<ExitCode> {
    let config = crate::load_config(paths)?;
    let invocation = invocation(&config, name)?;
    println!(
        "Codex hook review: opens a local, read-only interactive session with Jevia's recording hooks."
    );
    println!(
        "Open /hooks, review the Jevia capture-event commands, trust only the hooks you accept, then quit."
    );
    println!(
        "No task prompt, routing request, verification, or run-history entry is created by Jevia. Codex may persist your explicit trust decisions."
    );
    println!(
        "Changing the Jevia executable path or hook definition can require another review. Existing Codex policy still applies."
    );
    if !launch {
        println!(
            "Preview only. Add --launch to open the review session; nothing was changed or executed."
        );
        return Ok(ExitCode::SUCCESS);
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("hook review needs an interactive terminal; run this command there with --launch");
    }
    let capture = observations::Capture::prepare(
        config.harnesses[name].observations,
        &invocation,
        &paths.directory,
        &uuid::Uuid::new_v4().to_string(),
    )
    .await;
    if capture.snapshot().status != ObservationStatus::NoEvents {
        bail!(
            "could not prepare native review hooks; check the supported Codex version and project directory"
        );
    }
    let result = tokio::process::Command::new(&invocation.program)
        .args(&capture.args)
        .envs(capture.environment.iter().cloned())
        .current_dir(&paths.root)
        .env("PWD", &paths.root)
        .status()
        .await;
    // Review observations are not task evidence. Clean only this private journal.
    capture.persisted(&capture.snapshot());
    let status = result.context("could not open Codex hook review")?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_requires_enabled_standard_preset_and_never_bypasses_trust() {
        let mut config = Config::default();
        let raw = r#"
[harnesses.codex]
command = "codex"
args = ["exec", "--model", "{model}", "{task}"]
[harnesses.codex.models]
fast = "model"
balanced = "model"
strong = "model"
"#;
        config.harnesses = Config::from_toml(&(config.to_toml().unwrap() + raw))
            .unwrap()
            .harnesses;
        let review = invocation(&config, "codex").unwrap();
        assert_eq!(review.args, ["--sandbox", "read-only"]);
        config.harnesses.get_mut("codex").unwrap().observations = ObservationMode::Off;
        assert!(invocation(&config, "codex").is_err());
        config.harnesses.get_mut("codex").unwrap().observations = ObservationMode::Auto;
        config
            .harnesses
            .get_mut("codex")
            .unwrap()
            .args
            .push("--disable=hooks".into());
        assert!(invocation(&config, "codex").is_err());
    }
}
