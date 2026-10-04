//! A bounded routing projection, never a rewrite of stored history.
use std::io::{self, Write};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    ExecutionEvidence, Outcome, OutcomeSource, RouteRecord, RunState, VerificationEvidence,
};

const MAX_WINDOW_BYTES: usize = 64 * 1024;
const MAX_TASK_BYTES: usize = 2048;

pub(super) struct HistoryContext {
    pub outcomes: Vec<Value>,
    pub observations: Vec<Value>,
    pub budget: Value,
}

pub(super) fn project(history: &[RouteRecord], limit: usize) -> HistoryContext {
    let (outcomes, outcome_usage) = window(
        history
            .iter()
            .rev()
            .filter(|r| r.is_learning_evidence())
            .take(limit),
        |record| Completed {
            task: TaskExcerpt::new(record.task.as_deref()),
            tier: &record.decision.tier,
            confidence: record.decision.confidence,
            outcome: record.outcome,
            outcome_source: record.outcome_evidence.as_ref().map(|e| e.source),
            execution: record.execution.as_ref().map(Execution::new),
        },
    );
    let (observations, observation_usage) = window(
        history
            .iter()
            .rev()
            .filter(|r| r.is_execution_observation() && !r.is_learning_evidence())
            .take(limit),
        |record| {
            let execution = record.execution.as_ref().expect("observed execution");
            Observation {
                task: TaskExcerpt::new(record.task.as_deref()),
                tier: &record.decision.tier,
                state: record.lifecycle.as_ref().map(|life| life.state),
                harness: &execution.harness,
                requested_model: &execution.model,
                duration_ms: execution.duration_ms,
                process_exit_code: execution.exit_code,
                harness_observations: execution.observations.as_ref().map(|o| o.routing_summary()),
            }
        },
    );
    HistoryContext {
        outcomes,
        observations,
        budget: json!({
            "max_bytes_per_kind": MAX_WINDOW_BYTES,
            "max_task_bytes": MAX_TASK_BYTES,
            "known_outcomes": outcome_usage,
            "passive_observations": observation_usage,
        }),
    }
}

#[derive(Default, Serialize)]
struct Usage {
    candidates: usize,
    included: usize,
    omitted: usize,
    truncated_tasks: usize,
    serialized_bytes: usize,
}

fn window<'a, T: Serialize>(
    candidates: impl Iterator<Item = &'a RouteRecord>,
    project: impl Fn(&'a RouteRecord) -> T,
) -> (Vec<Value>, Usage) {
    let mut records = Vec::new();
    let mut usage = Usage {
        serialized_bytes: 2,
        ..Usage::default()
    }; // []
    for record in candidates {
        usage.candidates += 1;
        let separator = usize::from(!records.is_empty());
        let remaining = MAX_WINDOW_BYTES.saturating_sub(usage.serialized_bytes + separator);
        let projection = project(record);
        // Count encoded bytes (including JSON escapes) before allocating a Value.
        // Borrowed metadata cannot cause a huge temporary clone just to reject it.
        let mut counter = ByteBudget {
            bytes: 0,
            limit: remaining,
        };
        if serde_json::to_writer(&mut counter, &projection).is_err() {
            usage.omitted += 1;
            continue;
        }
        records.push(serde_json::to_value(projection).expect("serializable history projection"));
        usage.included += 1;
        usage.serialized_bytes += counter.bytes + separator;
        usage.truncated_tasks += usize::from(
            record
                .task
                .as_ref()
                .is_some_and(|s| s.len() > MAX_TASK_BYTES),
        );
    }
    // Admit newest first, retain the established oldest-to-newest request order.
    // An oversized record does not prevent a smaller older candidate from fitting.
    records.reverse();
    (records, usage)
}

struct ByteBudget {
    bytes: usize,
    limit: usize,
}

impl Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.bytes {
            return Err(io::Error::other("routing history byte budget exceeded"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Serialize)]
struct TaskExcerpt<'a> {
    task: Option<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    task_truncated: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl<'a> TaskExcerpt<'a> {
    fn new(task: Option<&'a str>) -> Self {
        Self {
            task: task.map(|text| {
                let mut end = text.len().min(MAX_TASK_BYTES);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                &text[..end]
            }),
            task_truncated: task.is_some_and(|text| text.len() > MAX_TASK_BYTES),
        }
    }
}

#[derive(Serialize)]
struct Completed<'a> {
    #[serde(flatten)]
    task: TaskExcerpt<'a>,
    tier: &'a str,
    confidence: f64,
    outcome: Outcome,
    outcome_source: Option<OutcomeSource>,
    execution: Option<Execution<'a>>,
}

#[derive(Serialize)]
struct Execution<'a> {
    harness: &'a str,
    model: &'a str,
    duration_ms: u64,
    exit_code: Option<i32>,
    verification: Option<&'a VerificationEvidence>,
    harness_observations: Option<Value>,
}

impl<'a> Execution<'a> {
    fn new(execution: &'a ExecutionEvidence) -> Self {
        Self {
            harness: &execution.harness,
            model: &execution.model,
            duration_ms: execution.duration_ms,
            exit_code: execution.exit_code,
            verification: execution.verification.as_ref(),
            harness_observations: execution.observations.as_ref().map(|o| o.routing_summary()),
        }
    }
}

#[derive(Serialize)]
struct Observation<'a> {
    #[serde(flatten)]
    task: TaskExcerpt<'a>,
    tier: &'a str,
    state: Option<RunState>,
    harness: &'a str,
    requested_model: &'a str,
    duration_ms: u64,
    process_exit_code: Option<i32>,
    harness_observations: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::super::{build_request, route_cache_key, tests::record};
    use super::*;
    use crate::{Config, RunLifecycle};

    fn observed(task: &str) -> RouteRecord {
        let mut row = record(task, "fast", Outcome::Unknown);
        row.lifecycle = Some(RunLifecycle {
            state: RunState::Completed,
            started_at_ms: Some(1),
            finished_at_ms: Some(2),
        });
        row.execution = Some(ExecutionEvidence {
            harness: "fixture".into(),
            model: "requested".into(),
            duration_ms: 1,
            exit_code: Some(0),
            verification: None,
            observations: None,
        });
        row
    }

    fn assert_usage(rows: &[Value], usage: &Value, candidates: usize) {
        let size = serde_json::to_vec(rows).unwrap().len();
        assert!(size <= MAX_WINDOW_BYTES);
        assert_eq!(usage["serialized_bytes"], size);
        assert_eq!(usage["candidates"], candidates);
        assert_eq!(usage["included"], rows.len());
        assert_eq!(usage["omitted"], candidates - rows.len());
    }

    #[test]
    fn historical_task_excerpts_are_utf8_safe_explicit_and_nonmutating() {
        let tasks = [
            "".into(),
            "x".repeat(MAX_TASK_BYTES),
            "x".repeat(MAX_TASK_BYTES + 1),
            format!("{}🦀tail", "x".repeat(MAX_TASK_BYTES - 1)),
            "🦀".repeat(MAX_TASK_BYTES),
        ];
        let mut records: Vec<_> = tasks
            .iter()
            .map(|s| record(s, "fast", Outcome::Success))
            .collect();
        let mut private = record("private", "fast", Outcome::Success);
        private.task = None;
        records.push(private);
        let original = records.clone();
        let result = project(&records, 20);
        assert_usage(
            &result.outcomes,
            &result.budget["known_outcomes"],
            records.len(),
        );
        assert_eq!(result.budget["known_outcomes"]["truncated_tasks"], 3);
        for (input, projected) in records.iter().zip(&result.outcomes) {
            if let Some(task) = &input.task {
                let excerpt = projected["task"].as_str().unwrap();
                assert!(excerpt.len() <= MAX_TASK_BYTES);
                assert!(task.starts_with(excerpt));
                if task.len() > MAX_TASK_BYTES {
                    assert_eq!(projected["task_truncated"], true);
                } else {
                    assert_eq!(excerpt, task);
                    assert!(projected.get("task_truncated").is_none());
                }
            } else {
                assert!(projected["task"].is_null());
                assert!(projected.get("task_truncated").is_none());
            }
        }
        assert_eq!(
            result.outcomes[3]["task"].as_str().unwrap().len(),
            MAX_TASK_BYTES - 1
        );
        assert_eq!(records, original);
        let current = "unabridged current task".repeat(10_000);
        let config = Config::default();
        let request = build_request(&current, &config, &records, None).unwrap();
        assert_eq!(request.state["current_task"], current);
    }

    #[test]
    fn encoded_budgets_are_independent_and_prioritize_newest_candidates() {
        let mut records = Vec::new();
        // Each control character expands to six JSON bytes. Count encoded size,
        // not String length; both windows should fill well before 100 records.
        for i in 0..100 {
            let task = format!("{i:03}{}", "\0".repeat(MAX_TASK_BYTES));
            records.push(record(&task, "fast", Outcome::Success));
            records.push(observed(&task));
        }
        let result = project(&records, 100);
        for (rows, key) in [
            (&result.outcomes, "known_outcomes"),
            (&result.observations, "passive_observations"),
        ] {
            assert_usage(rows, &result.budget[key], 100);
            assert!(!rows.is_empty() && rows.len() < 100);
            assert_eq!(result.budget[key]["truncated_tasks"], rows.len());
            let ids: Vec<usize> = rows
                .iter()
                .map(|r| r["task"].as_str().unwrap()[..3].parse().unwrap())
                .collect();
            assert_eq!(ids, (100 - rows.len()..100).collect::<Vec<_>>());
        }
        assert!(
            result
                .observations
                .iter()
                .all(|r| r.get("outcome").is_none())
        );
        for limit in [0, 1] {
            let result = project(&records, limit);
            assert_usage(&result.outcomes, &result.budget["known_outcomes"], limit);
            assert_usage(
                &result.observations,
                &result.budget["passive_observations"],
                limit,
            );
            assert_eq!(result.outcomes.len(), limit);
            assert_eq!(result.observations.len(), limit);
        }
    }

    #[test]
    fn exact_array_boundary_and_oversized_metadata_do_not_starve_smaller_candidates() {
        let mut edge = record("edge", "fast", Outcome::Success);
        let base = serde_json::to_vec(&project(&[edge.clone()], 1).outcomes)
            .unwrap()
            .len();
        edge.decision.tier = "t".repeat(MAX_WINDOW_BYTES - base + 4);
        let full = project(&[edge.clone()], 1);
        assert_eq!(
            full.budget["known_outcomes"]["serialized_bytes"],
            MAX_WINDOW_BYTES
        );
        assert_usage(&full.outcomes, &full.budget["known_outcomes"], 1);
        edge.decision.tier.push('t');
        let small = record("small", "fast", Outcome::Failure);
        let result = project(&[small.clone(), edge.clone()], 2);
        assert_usage(&result.outcomes, &result.budget["known_outcomes"], 2);
        assert_eq!(result.outcomes.len(), 1);
        assert_eq!(result.outcomes[0]["task"], "small");
        // Never scan beyond the count window to backfill omitted candidates.
        assert!(project(&[small, edge], 1).outcomes.is_empty());

        for field in ["harness", "model", "verifier"] {
            let mut huge = observed("huge metadata");
            let execution = huge.execution.as_mut().unwrap();
            let text = "x".repeat(1024 * 1024);
            match field {
                "harness" => execution.harness = text,
                "model" => execution.model = text,
                _ => {
                    execution.verification = Some(VerificationEvidence {
                        command: text,
                        launched: true,
                        duration_ms: 1,
                        exit_code: Some(0),
                    })
                }
            }
            huge.outcome = Outcome::Success;
            let result = project(
                &[record("small", "fast", Outcome::Success), huge.clone()],
                2,
            );
            assert_eq!(result.outcomes.len(), 1, "{field}");
            assert_usage(&result.outcomes, &result.budget["known_outcomes"], 2);
            if field != "verifier" {
                huge.outcome = Outcome::Unknown;
                let result = project(&[observed("small"), huge], 2);
                assert_eq!(result.observations.len(), 1, "{field}");
                assert_usage(
                    &result.observations,
                    &result.budget["passive_observations"],
                    2,
                );
            }
        }
    }

    #[test]
    fn cache_fingerprint_tracks_the_projected_context_not_discarded_suffixes() {
        let config = Config::default();
        let mut row = record(
            &format!("{}old suffix", "x".repeat(MAX_TASK_BYTES)),
            "fast",
            Outcome::Success,
        );
        let key = route_cache_key("task", None, &config, &[row.clone()]).unwrap();
        row.task.as_mut().unwrap().push_str("new suffix");
        assert_eq!(
            key,
            route_cache_key("task", None, &config, &[row.clone()]).unwrap()
        );
        row.task.as_mut().unwrap().replace_range(..1, "z");
        assert_ne!(
            key,
            route_cache_key("task", None, &config, &[row.clone()]).unwrap()
        );
        row.task.as_mut().unwrap().replace_range(..1, "x");
        row.outcome = Outcome::Failure;
        assert_ne!(
            key,
            route_cache_key("task", None, &config, &[row.clone()]).unwrap()
        );
        row.outcome = Outcome::Success;
        row.task.as_mut().unwrap().truncate(MAX_TASK_BYTES);
        // The excerpt is now the entire task: the truncation marker changes.
        assert_ne!(
            key,
            route_cache_key("task", None, &config, &[row.clone()]).unwrap()
        );
        let request = build_request("task", &config, &[row], None).unwrap();
        assert!(
            request.state["policy"]["use_history_budget"]
                .as_str()
                .unwrap()
                .contains("not evidence of failure")
        );
    }
}
