//! Descriptive statistics over one append-ordered history snapshot. Never infer
//! task correctness from process exit, or count superseded feedback as new runs.

use std::{collections::BTreeMap, fmt::Write};

use anyhow::Result;
use jevia_core::{DecisionSource, Outcome, OutcomeSource, RouteRecord};
use serde::Serialize;

use crate::storage::Storage;

#[derive(Default, Serialize)]
struct Outcomes {
    successes: usize,
    failures: usize,
}

impl Outcomes {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Success => self.successes += 1,
            Outcome::Failure => self.failures += 1,
            Outcome::Unknown => unreachable!("unknown outcomes are counted separately"),
        }
    }

    fn total(&self) -> usize {
        self.successes + self.failures
    }
}

#[derive(Default, Serialize)]
struct Counts {
    records: usize,
    cache_hits: usize,
    learning_evidence: usize,
    verified: Outcomes,
    manual: Outcomes,
    process_exit: Outcomes,
    unattributed: Outcomes,
    active: usize,
    unknown: usize,
}

impl Counts {
    fn add(&mut self, record: &RouteRecord) {
        self.records += 1;
        self.cache_hits += usize::from(record.decision.source == DecisionSource::Cache);
        self.learning_evidence += usize::from(record.is_learning_evidence());
        // These outcome buckets are mutually exclusive. Active runs are never
        // eligible, even if imported history contains a premature known outcome.
        if record
            .lifecycle
            .as_ref()
            .is_some_and(|l| l.state.is_active())
        {
            self.active += 1;
        } else if record.outcome == Outcome::Unknown {
            self.unknown += 1;
        } else {
            match record.outcome_evidence.as_ref().map(|e| e.source) {
                Some(OutcomeSource::Verification) => &mut self.verified,
                Some(OutcomeSource::Manual) => &mut self.manual,
                Some(OutcomeSource::ProcessExit) => &mut self.process_exit,
                None => &mut self.unattributed,
            }
            .add(record.outcome);
        }
    }

    fn summarize(self) -> Metrics {
        Metrics {
            verified_success_rate: ratio(self.verified.successes, self.verified.total()),
            cache_hit_rate: ratio(self.cache_hits, self.records),
            counts: self,
        }
    }
}

#[derive(Serialize)]
struct Metrics {
    #[serde(flatten)]
    counts: Counts,
    /// Fractions in [0, 1]; no observations is null, not a 0% success rate.
    verified_success_rate: Option<f64>,
    cache_hit_rate: Option<f64>,
}

#[derive(Serialize)]
struct Window {
    limit: usize,
    order: &'static str,
    has_older_records: bool,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    storage: &'static str,
    window: Window,
    totals: Metrics,
    tiers: BTreeMap<String, Metrics>,
}

pub async fn collect(storage: &Storage, limit: usize) -> Result<Report> {
    // One extra row detects truncation without a separate count query racing a
    // concurrent append. SQL reads only this project's bounded snapshot.
    let records = storage.recent(limit + 1, false).await?;
    Ok(Report::new(storage.name(), limit, &records))
}

impl Report {
    fn new(storage: &'static str, limit: usize, records: &[RouteRecord]) -> Self {
        let mut totals = Counts::default();
        let mut tiers = BTreeMap::<String, Counts>::new();
        for record in &records[records.len().saturating_sub(limit)..] {
            totals.add(record);
            tiers
                .entry(record.decision.tier.clone())
                .or_default()
                .add(record);
        }
        Self {
            schema_version: 1,
            storage,
            window: Window {
                limit,
                order: "append",
                has_older_records: records.len() > limit,
            },
            totals: totals.summarize(),
            tiers: tiers
                .into_iter()
                .map(|(tier, counts)| (tier, counts.summarize()))
                .collect(),
        }
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        let counts = &self.totals.counts;
        writeln!(output, "Routing stats ({})", self.storage).unwrap();
        writeln!(
            output,
            "Window: {} records, latest {} in append order; {}",
            counts.records,
            self.window.limit,
            if self.window.has_older_records {
                "older records excluded"
            } else {
                "all retained records included"
            }
        )
        .unwrap();
        if counts.records == 0 {
            writeln!(output, "No runs recorded. Rates: n/a (no observations).").unwrap();
            return output;
        }
        writeln!(
            output,
            "Cache hits: {}/{} ({}) | Learning evidence: {}",
            counts.cache_hits,
            counts.records,
            percent(self.totals.cache_hit_rate),
            counts.learning_evidence
        )
        .unwrap();
        writeln!(
            output,
            "Verified success: {}/{} ({})",
            counts.verified.successes,
            counts.verified.total(),
            percent(self.totals.verified_success_rate)
        )
        .unwrap();
        writeln!(
            output,
            "\n{:<16} {:>6} {:>7} {:>15} {:>9} {:>13} {:>9}",
            "Tier", "Runs", "Cached", "Verified ok/fail", "Success", "Manual ok/fail", "Evidence"
        )
        .unwrap();
        for (tier, metrics) in &self.tiers {
            let c = &metrics.counts;
            // Tier names can come from imported history; don't emit terminal controls.
            writeln!(
                output,
                "{:<16} {:>6} {:>7} {:>15} {:>9} {:>13} {:>9}",
                tier.escape_debug().to_string(),
                c.records,
                c.cache_hits,
                format!("{}/{}", c.verified.successes, c.verified.failures),
                percent(metrics.verified_success_rate),
                format!("{}/{}", c.manual.successes, c.manual.failures),
                c.learning_evidence
            )
            .unwrap();
        }
        writeln!(output, "\nOther outcomes (excluded from verified rate): process-exit-only {} ok/{} fail; unattributed {} ok/{} fail; active {}; unknown {}.",
            counts.process_exit.successes, counts.process_exit.failures, counts.unattributed.successes, counts.unattributed.failures, counts.active, counts.unknown).unwrap();
        writeln!(
            output,
            "Manual feedback is separate. Latest outcome per run; n/a means no verified outcomes."
        )
        .unwrap();
        writeln!(
            output,
            "Observed results only, not a model ranking or proof of routing improvement."
        )
        .unwrap();
        output
    }
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator != 0).then(|| numerator as f64 / denominator as f64)
}

fn percent(value: Option<f64>) -> String {
    value
        .map(|n| format!("{:.1}%", n * 100.0))
        .unwrap_or_else(|| "n/a".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevia_core::{OutcomeEvidence, RouteDecision, RunState};

    fn record(outcome: Outcome, source: Option<OutcomeSource>) -> RouteRecord {
        let mut record = RouteRecord::new(
            RouteDecision {
                run_id: "private-run-id".into(),
                tier: "fast".into(),
                suggested_tier: "strong".into(),
                confidence: 0.9,
                probabilities: BTreeMap::new(),
                fallback_applied: true,
                jev_model: "private-model".into(),
                created_at_ms: 1,
                source: DecisionSource::Live,
            },
            Some("private-task".into()),
        );
        record.outcome = outcome;
        record.outcome_evidence = source.map(|source| OutcomeEvidence {
            source,
            recorded_at_ms: 1,
        });
        record
    }

    #[test]
    fn outcome_buckets_partition_records_and_exclude_active_or_unknown_evidence() {
        let mut records = vec![];
        for source in [
            Some(OutcomeSource::Verification),
            Some(OutcomeSource::Manual),
            Some(OutcomeSource::ProcessExit),
            None,
        ] {
            for outcome in [Outcome::Success, Outcome::Failure, Outcome::Unknown] {
                for state in [RunState::Completed, RunState::Running, RunState::Verifying] {
                    let mut record = record(outcome, source);
                    record.lifecycle.as_mut().unwrap().state = state;
                    records.push(record);
                }
            }
        }
        let report = Report::new("jsonl", 1000, &records);
        let c = &report.totals.counts;
        assert_eq!(c.records, 36);
        assert_eq!(c.active, 24);
        assert_eq!(c.unknown, 4);
        assert_eq!(c.verified.successes, 1);
        assert_eq!(c.verified.failures, 1);
        assert_eq!(c.manual.total(), 2);
        assert_eq!(c.process_exit.total(), 2);
        assert_eq!(c.unattributed.total(), 2);
        assert_eq!(c.learning_evidence, 4);
        assert_eq!(
            c.records,
            c.active
                + c.unknown
                + c.verified.total()
                + c.manual.total()
                + c.process_exit.total()
                + c.unattributed.total()
        );
        assert_eq!(report.totals.verified_success_rate, Some(0.5));
        // Group by actual selected tier, not Jev's suggestion or current config.
        assert_eq!(report.tiers.len(), 1);
        assert_eq!(report.tiers["fast"].counts.records, 36);
    }

    #[test]
    fn no_observations_are_null_not_zero_and_only_current_outcomes_count() {
        let empty = Report::new("sqlite", 10, &[]);
        assert_eq!(empty.totals.cache_hit_rate, None);
        assert_eq!(empty.totals.verified_success_rate, None);
        assert!(empty.render().contains("No runs recorded"));
        assert!(serde_json::to_value(&empty).unwrap()["totals"]["verified_success_rate"].is_null());
        let mut manual = record(Outcome::Failure, Some(OutcomeSource::Manual));
        manual.feedback = vec![
            jevia_core::FeedbackEvent {
                previous_outcome: Outcome::Success,
                previous_source: Some(OutcomeSource::Verification),
                outcome: Outcome::Failure,
                recorded_at_ms: 2,
                reason: Some("private-reason".into()),
            };
            5
        ];
        let report = Report::new("jsonl", 10, &[manual]);
        assert_eq!(report.totals.counts.records, 1);
        assert_eq!(report.totals.counts.manual.failures, 1);
        assert_eq!(report.totals.counts.learning_evidence, 1);
        assert_eq!(report.totals.verified_success_rate, None);
        assert_eq!(report.totals.cache_hit_rate, Some(0.0));
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("private-"));
        assert!(!report.render().contains("private-"));
    }

    #[test]
    fn bounded_window_uses_append_order_and_distinguishes_zero_from_no_data() {
        let mut old = record(Outcome::Success, Some(OutcomeSource::Verification));
        old.decision.created_at_ms = 9999;
        old.decision.tier = "old-tier".into();
        let mut latest = record(Outcome::Failure, Some(OutcomeSource::Verification));
        latest.decision.source = DecisionSource::Cache;
        let report = Report::new("postgres", 1, &[old, latest]);
        assert!(report.window.has_older_records);
        assert_eq!(report.totals.counts.records, 1);
        assert_eq!(report.totals.verified_success_rate, Some(0.0));
        assert_eq!(report.totals.cache_hit_rate, Some(1.0));
        assert!(!report.tiers.contains_key("old-tier"));
        assert!(report.render().contains("0.0%"));
    }

    #[test]
    fn imported_tier_names_cannot_emit_terminal_controls() {
        let mut record = record(Outcome::Unknown, None);
        record.decision.tier = "tier\n\u{1b}[2J".into();
        let report = Report::new("jsonl", 1, &[record]);
        let output = report.render();
        assert!(output.contains("tier\\n\\u{1b}[2J"));
        assert!(!output.contains('\u{1b}'));
    }
}
