//! Static checks only: never spawn a program, inspect credentials, or open storage.
mod executable;

use anyhow::Result;
use jevia_core::{Config, ConfigError};
use serde::Serialize;
use std::{env, fs, process::ExitCode};

use crate::paths::ProjectPaths;
use executable::Lookup;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Pass,
    Warning,
    Fail,
}

#[derive(Serialize)]
struct Check {
    id: &'static str,
    status: Status,
    code: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    harness: String,
    scope: &'static str,
    ok: bool,
    checks: Vec<Check>,
    limitations: &'static str,
}

impl Report {
    fn new(name: &str) -> Self {
        Self {
            schema_version: 1,
            harness: name.into(),
            scope: "static",
            ok: true,
            checks: Vec::new(),
            limitations: "No programs were launched and no API/database requests were made. File checks do not prove launchability, valid agent flags, provider authentication, model access, or task correctness. Files and PATH can change after this check.",
        }
    }

    fn add(&mut self, id: &'static str, status: Status, code: &'static str, message: &'static str) {
        if status == Status::Fail {
            self.ok = false;
        }
        self.checks.push(Check {
            id,
            status,
            code,
            message,
        });
    }

    fn executable(&mut self, id: &'static str, result: Lookup) {
        match result {
            Lookup::Candidate { batch: false } => self.add(id, Status::Pass, "executable_candidate", "Found a regular file candidate (with execute bits on Unix). No launch attempted."),
            Lookup::Candidate { batch: true } => self.add(id, Status::Warning, "windows_batch_wrapper", "Found a Windows batch wrapper; runtime uses cmd.exe. Review argument handling, or prefer a native executable. No launch attempted."),
            Lookup::InvalidCommand => self.add(id, Status::Fail, "invalid_command", "Configure a nonempty executable name/path without NUL bytes; pass arguments separately."),
            Lookup::NotFound => self.add(id, Status::Fail, "executable_not_found", "No file candidate found in the checked locations. Install the program, fix PATH, or configure an absolute executable path."),
            Lookup::NotExecutable => self.add(id, Status::Fail, "not_executable", "The candidate is not a regular file or has no Unix execute bits; check its type and permissions."),
            Lookup::PathUnavailable => self.add(id, Status::Fail, "path_unavailable", "PATH is unavailable or empty; configure an absolute executable path for a deterministic check."),
            Lookup::RelativeWindowsPath => self.add(id, Status::Fail, "ambiguous_windows_path", "Use an absolute command path or absolute PATH entries on Windows; relative executable resolution can depend on the caller's directory."),
        }
    }

    fn render(&self) -> String {
        let mut out = format!(
            "Harness {:?}: {} (static checks only)\n",
            self.harness,
            if self.ok { "ok" } else { "failed" }
        );
        for check in &self.checks {
            let status = match check.status {
                Status::Pass => "ok",
                Status::Warning => "warning",
                Status::Fail => "failed",
            };
            out.push_str(&format!(
                "{}: {} [{}] — {}\n",
                check.id, status, check.code, check.message
            ));
        }
        out.push_str(self.limitations);
        out.push('\n');
        out
    }
}

fn inspect(paths: &ProjectPaths, name: &str) -> Report {
    let mut report = Report::new(name);
    let raw = match fs::read_to_string(&paths.config) {
        Ok(raw) => raw,
        Err(_) => {
            report.add("configuration", Status::Fail, "configuration_unreadable", "Could not read project configuration; contents and filesystem error details are redacted.");
            return report;
        }
    };
    let config = match Config::from_toml(&raw) {
        Ok(config) => config,
        Err(error) => {
            let (code, message) = match error {
                ConfigError::MissingHarnessModel { .. } => (
                    "missing_model_mapping",
                    "A harness is missing a model mapping for a configured tier; review its models table.",
                ),
                ConfigError::MissingHarnessPlaceholder { .. }
                | ConfigError::UnknownHarnessPlaceholder { .. } => (
                    "invalid_template",
                    "A harness has missing or unsupported placeholders. Harness args require {model} and {task}; allowed placeholders are {model}, {task}, {tier}, and {run_id}.",
                ),
                ConfigError::EmptyHarnessCommand(_) | ConfigError::EmptyVerificationCommand(_) => (
                    "empty_command",
                    "A harness or verifier executable command is empty.",
                ),
                _ => (
                    "invalid_configuration",
                    "Project configuration is malformed, unsupported, or violates policy validation; contents are redacted.",
                ),
            };
            report.add("configuration", Status::Fail, code, message);
            return report;
        }
    };
    report.add(
        "configuration",
        Status::Pass,
        "configuration_valid",
        "Project schema, policy, model mappings, and template placeholders are valid.",
    );
    let Some(harness) = config.harnesses.get(name) else {
        report.add("harness", Status::Fail, "harness_not_configured", "Adapter is not configured. Use jevia harness setup <name> or edit the harnesses table.");
        return report;
    };
    let valid_arguments = config.tiers.keys().all(|tier| {
        harness
            .invocation(name, tier, "preflight task", "preflight-run", &[])
            .is_ok_and(|invocation| {
                invocation.args.iter().all(|arg| !arg.contains('\0'))
                    && invocation
                        .verification
                        .as_ref()
                        .is_none_or(|verify| verify.args.iter().all(|arg| !arg.contains('\0')))
            })
    });
    if valid_arguments {
        report.add("arguments", Status::Pass, "templates_render", "Argument templates render for every configured tier without NUL bytes; provider flags/model access are not tested.");
    } else {
        report.add("arguments", Status::Fail, "invalid_arguments", "A rendered harness/verifier argument is invalid or contains a NUL byte (contents redacted).");
    }
    let path = env::var_os("PATH");
    report.executable(
        "executable",
        executable::lookup(
            &harness.command,
            &paths.root,
            path.as_deref(),
            cfg!(windows),
        ),
    );
    if let Some(verify) = &harness.verification {
        report.executable(
            "verification",
            executable::lookup(&verify.command, &paths.root, path.as_deref(), cfg!(windows)),
        );
    } else if harness.auto_verify {
        match crate::verification::detect(&paths.root) {
            crate::verification::Detection::Found(plan) => {
                report.add("verification_policy", Status::Pass, "automatic_verification", "Root project tests will run automatically after the agent exits; results are recorded without manual feedback.");
                report.executable(
                    "verification",
                    executable::lookup(&plan.program, &paths.root, path.as_deref(), cfg!(windows)),
                );
            }
            crate::verification::Detection::Unavailable(message) => report.add(
                "verification",
                Status::Warning,
                "verification_not_configured",
                message,
            ),
        }
    } else {
        report.add(
            "verification",
            Status::Pass,
            "verification_disabled",
            "No additional verification requested. Execution is still recorded; process exit is not proof of task success.",
        );
    }
    if cfg!(windows) {
        report.add("lookup_scope", Status::Warning, "windows_lookup_scope", "This check searches explicit paths/PATH only, not extra Windows application/system directories. Prefer absolute executable paths; PATHEXT and shell aliases are not expanded.");
    }
    report
}

pub fn run(paths: &ProjectPaths, name: &str, json: bool) -> Result<ExitCode> {
    let report = inspect(paths, name);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render());
    }
    Ok(if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests;
