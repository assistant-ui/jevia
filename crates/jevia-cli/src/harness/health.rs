//! Read stored metadata only. Never replay journals, run probes, or infer task success.
use crate::{observations, paths::ProjectPaths, storage::Storage};
use anyhow::Result;
use jevia_core::{
    Config, ObservationMode, ObservationSource, ObservationStatus, RouteRecord, RunState,
};
use serde::Serialize;
use std::process::ExitCode;

const WINDOW: usize = 100;

#[derive(Debug, Serialize)]
struct Latest {
    run_id: String,
    state: Option<RunState>,
    source: Option<ObservationSource>,
    status: Option<ObservationStatus>,
    event_count: u64,
    discarded_inputs: u64,
    last_sampled_event_at_ms: Option<u64>,
    sampled: bool,
    advice: &'static str,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    harness: String,
    ok: bool,
    code: &'static str,
    mode: Option<ObservationMode>,
    configured_source: Option<ObservationSource>,
    configuration: &'static str,
    scanned_runs: usize,
    window_limit: usize,
    latest: Option<Latest>,
    limitations: &'static str,
}

fn latest(records: &[RouteRecord], name: &str) -> Option<Latest> {
    records.iter().rev().find_map(|record| {
        let execution = record.execution.as_ref().filter(|e| e.harness == name)?;
        let observations = execution.observations.as_ref();
        let status = observations.map(|o| o.status);
        let advice = match status {
            None => "Legacy execution has no capture report. A new run can report native recording health.",
            Some(ObservationStatus::Disabled) => "Native capture was explicitly disabled; process recording still applies.",
            Some(ObservationStatus::Unsupported) => "No native adapter matched this launch. Use a supported direct executable or an explicitly compatible wrapper.",
            Some(ObservationStatus::Unavailable) => "Capture setup was unavailable. Check the tested version, platform, conflicting settings, and writable project directory; the saved status does not identify one exact cause.",
            Some(ObservationStatus::NoEvents) => "No native events were saved. Check harness hook/plugin policy and, for Codex, review /hooks. This does not prove trust was denied or recording is broken; an active run may not have checkpointed yet.",
            Some(ObservationStatus::Partial) => "Capture is incomplete. Inputs were discarded, a hook write failed, or a journal read failed. Discarded inputs can be a lower bound when writes failed; retain journals and inspect storage access. Do not assume missing events never happened.",
            Some(ObservationStatus::Recorded) => "Native events were received. This confirms some coverage, not complete coverage or task correctness.",
        };
        Some(Latest {
            run_id: record.decision.run_id.clone(),
            state: record.lifecycle.as_ref().map(|l| l.state),
            source: observations.and_then(|o| o.source),
            status,
            event_count: observations.map_or(0, |o| o.event_count()),
            discarded_inputs: observations.map_or(0, |o| o.counts().discarded_inputs),
            last_sampled_event_at_ms: observations.and_then(|o| o.events.iter().map(|e| e.recorded_at_ms).max()),
            sampled: observations.is_some_and(|o| o.event_count() > o.events.len() as u64),
            advice,
        })
    })
}

async fn inspect(paths: &ProjectPaths, name: &str) -> Report {
    let mut report = Report {
        schema_version: 1,
        harness: name.into(),
        ok: false,
        code: "configuration_unreadable",
        mode: None,
        configured_source: None,
        configuration: "not_checked",
        scanned_runs: 0,
        window_limit: WINDOW,
        latest: None,
        limitations: "Reads configuration and up to 100 recent records from configured storage (PostgreSQL may require a database connection). Does not run a harness/version probe, contact a model provider, replay journals, alter history, or run verification. Current config can differ from the latest launch. Latest means append order, not latest activity; timestamps are from a bounded sample, not a liveness guarantee. Extra launch arguments and global harness policy are not inspected.",
    };
    let Ok(raw) = std::fs::read_to_string(&paths.config) else {
        return report;
    };
    let Ok(config) = Config::from_toml(&raw) else {
        report.code = "invalid_configuration";
        return report;
    };
    let Some(harness) = config.harnesses.get(name) else {
        report.code = "harness_not_configured";
        return report;
    };
    report.mode = Some(harness.observations);
    report.configured_source =
        observations::configured_source(harness.observations, &harness.command);
    report.configuration = if harness.observations == ObservationMode::Off {
        "disabled"
    } else if let Some(source) = report.configured_source {
        // Render all tier templates so flags passed as a model mapping are also checked.
        let conflict = config.tiers.keys().any(|tier| {
            harness
                .invocation(name, tier, "health-check", "health-check", &[])
                .is_ok_and(|i| observations::capture_conflicts(source, &i.args))
        });
        if conflict {
            "preserving_custom_settings_or_unsupported_platform"
        } else {
            "adapter_candidate_not_probed"
        }
    } else {
        "process_only"
    };
    let history = async {
        let storage = Storage::open(&config, paths, false).await?;
        storage.recent(WINDOW, false).await
    }
    .await;
    let Ok(records) = history else {
        report.code = "storage_unavailable";
        return report;
    };
    report.scanned_runs = records.len();
    report.latest = latest(&records, name);
    report.code = if report.latest.is_some() {
        "stored_capture_report"
    } else {
        "no_matching_execution_in_window"
    };
    report.ok = true;
    report
}

pub async fn run(paths: &ProjectPaths, name: &str, json: bool) -> Result<ExitCode> {
    let report = inspect(paths, name).await;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Recording health: {} [{}]", report.harness, report.code);
        println!("Current configuration: {}", report.configuration);
        if let Some(latest) = &report.latest {
            println!(
                "Latest capture: {:?}; {} events, {} discarded inputs",
                latest.status, latest.event_count, latest.discarded_inputs
            );
            println!("{}", latest.advice);
        }
        println!("{}", report.limitations);
    }
    Ok(if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevia_core::{DecisionSource, ExecutionEvidence, HarnessObservations, RouteDecision};

    #[test]
    fn health_distinguishes_every_capture_state_without_raw_data() {
        for status in [
            None,
            Some(ObservationStatus::Disabled),
            Some(ObservationStatus::Unsupported),
            Some(ObservationStatus::Unavailable),
            Some(ObservationStatus::NoEvents),
            Some(ObservationStatus::Recorded),
            Some(ObservationStatus::Partial),
        ] {
            let mut record = RouteRecord::new(
                RouteDecision {
                    run_id: "run-1".into(),
                    tier: "fast".into(),
                    suggested_tier: "fast".into(),
                    confidence: 1.0,
                    probabilities: Default::default(),
                    fallback_applied: false,
                    jev_model: "private-model".into(),
                    created_at_ms: 1,
                    source: DecisionSource::Live,
                },
                Some("PRIVATE task".into()),
            );
            record.execution = Some(ExecutionEvidence {
                harness: "agent".into(),
                model: "PRIVATE model".into(),
                duration_ms: 0,
                exit_code: None,
                verification: None,
                observations: status.map(|status| HarnessObservations {
                    source: Some(ObservationSource::ClaudeHooks),
                    status,
                    events: vec![],
                    totals: None,
                }),
            });
            let report = latest(&[record.clone()], "agent").unwrap();
            assert_eq!(report.status, status);
            assert!(!report.advice.is_empty());
            assert!(!serde_json::to_string(&report).unwrap().contains("PRIVATE"));
            assert!(latest(&[record], "other").is_none());
        }
    }
}
