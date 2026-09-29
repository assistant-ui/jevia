use std::{
    collections::BTreeMap,
    fmt,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Current version of a persisted run record.
pub const RECORD_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeSource {
    ProcessExit,
    Verification,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeEvidence {
    pub source: OutcomeSource,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub previous_outcome: Outcome,
    pub previous_source: Option<OutcomeSource>,
    pub outcome: Outcome,
    pub recorded_at_ms: u64,
    pub reason: Option<String>,
}

/// Execution progress is separate from whether the task succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Routed,
    Running,
    Verifying,
    Completed,
    LaunchFailed,
    Interrupted,
    Cancelled,
    TimedOut,
}

impl RunState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Verifying)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLifecycle {
    pub state: RunState,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
}

impl Default for RunLifecycle {
    fn default() -> Self {
        Self {
            state: RunState::Routed,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }
}

/// Result observed after a routed task runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
    #[default]
    Unknown,
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Unknown => "unknown",
        })
    }
}

impl FromStr for Outcome {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "success" => Ok(Self::Success),
            "failure" => Ok(Self::Failure),
            "unknown" => Ok(Self::Unknown),
            _ => Err("outcome must be success, failure, or unknown"),
        }
    }
}

/// Origin of a routing decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    #[default]
    Live,
    Cache,
}

impl fmt::Display for DecisionSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Live => "live",
            Self::Cache => "cache",
        })
    }
}

/// Inspectable result after model output and local safety policy are combined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDecision {
    pub run_id: String,
    /// Tier Jevia will actually use.
    pub tier: String,
    /// Tier selected by Jev before a possible confidence fallback.
    pub suggested_tier: String,
    pub confidence: f64,
    pub probabilities: BTreeMap<String, f64>,
    pub fallback_applied: bool,
    pub jev_model: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub source: DecisionSource,
}

impl RouteDecision {
    /// Shared validation for provider output, persisted history, and cached decisions.
    /// Errors describe fields only; provider values must never enter diagnostics.
    pub fn validate(&self) -> Result<(), &'static str> {
        if [
            &self.run_id,
            &self.tier,
            &self.suggested_tier,
            &self.jev_model,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            return Err("routing identity, tiers, and model must be nonempty");
        }
        if !self.confidence.is_finite() || !(0.0..=1.0).contains(&self.confidence) {
            return Err("routing confidence must be finite and between 0 and 1");
        }
        // Empty maps remain valid for older providers/records. Do not require
        // a sum of one: providers may return only a subset of tier scores.
        if self
            .probabilities
            .values()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return Err("routing probabilities must be finite and between 0 and 1");
        }
        Ok(())
    }

    /// Reuse the decision signal while giving a cache hit its own run identity.
    pub fn for_cache_hit(&self) -> Self {
        let mut decision = self.clone();
        decision.run_id = Uuid::new_v4().to_string();
        decision.created_at_ms = now_ms();
        decision.source = DecisionSource::Cache;
        decision
    }
}

/// Observable facts from a configured harness execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionEvidence {
    pub harness: String,
    pub model: String,
    pub duration_ms: u64,
    /// Absent when the operating system terminates the process without an exit code.
    pub exit_code: Option<i32>,
    /// Present when a configured verifier ran after the harness succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationEvidence>,
}

/// Observable facts from a configured post-run verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEvidence {
    pub command: String,
    pub launched: bool,
    pub duration_ms: u64,
    /// Absent when the process could not start or terminated without an exit code.
    pub exit_code: Option<i32>,
}

/// Persisted record used by the outcome feedback loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRecord {
    pub schema_version: u32,
    #[serde(flatten)]
    pub decision: RouteDecision,
    /// Omitted when `privacy.store_task_text` is disabled.
    pub task: Option<String>,
    pub outcome: Outcome,
    /// Present when Jevia launched and observed a configured harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionEvidence>,
    /// Absent in legacy schema-version-1 records; never infer execution from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<RunLifecycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_evidence: Option<OutcomeEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback: Vec<FeedbackEvent>,
}

impl RouteRecord {
    /// Finished executions provide operational context, not a quality label.
    /// Routed-only and active runs must not be mistaken for completed attempts.
    pub fn is_execution_observation(&self) -> bool {
        self.execution.is_some()
            && self
                .lifecycle
                .as_ref()
                .is_some_and(|life| !life.state.is_active() && life.state != RunState::Routed)
    }

    pub fn new(decision: RouteDecision, task: Option<String>) -> Self {
        Self {
            schema_version: RECORD_SCHEMA_VERSION,
            decision,
            task,
            outcome: Outcome::Unknown,
            execution: None,
            lifecycle: Some(RunLifecycle::default()),
            outcome_evidence: None,
            feedback: Vec::new(),
        }
    }

    /// Process completion alone is not evidence of task correctness. Old records
    /// are kept readable but must be explicitly confirmed before reuse.
    pub fn is_learning_evidence(&self) -> bool {
        self.outcome != Outcome::Unknown
            && !self
                .lifecycle
                .as_ref()
                .is_some_and(|life| life.state.is_active())
            && self.outcome_evidence.as_ref().is_some_and(|evidence| {
                matches!(
                    evidence.source,
                    OutcomeSource::Verification | OutcomeSource::Manual
                )
            })
    }
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_records_without_execution_evidence_remain_readable() {
        let input = r#"{
            "schema_version": 1,
            "run_id": "run-1",
            "tier": "balanced",
            "suggested_tier": "balanced",
            "confidence": 0.8,
            "probabilities": {},
            "fallback_applied": false,
            "jev_model": "jev-test",
            "created_at_ms": 1,
            "task": "test task",
            "outcome": "success"
        }"#;

        let record: RouteRecord = serde_json::from_str(input).expect("legacy record decodes");

        assert_eq!(record.outcome, Outcome::Success);
        assert_eq!(record.execution, None);
        assert_eq!(record.decision.source, DecisionSource::Live);
    }

    #[test]
    fn execution_evidence_without_verification_remains_readable() {
        let input = r#"{
            "schema_version": 1,
            "run_id": "run-1",
            "tier": "balanced",
            "suggested_tier": "balanced",
            "confidence": 0.8,
            "probabilities": {},
            "fallback_applied": false,
            "jev_model": "jev-test",
            "created_at_ms": 1,
            "task": "test task",
            "outcome": "success",
            "execution": {
                "harness": "agent",
                "model": "provider/model",
                "duration_ms": 42,
                "exit_code": 0
            }
        }"#;

        let record: RouteRecord = serde_json::from_str(input).expect("existing record decodes");

        assert_eq!(
            record
                .execution
                .expect("execution evidence exists")
                .verification,
            None
        );
    }

    #[test]
    fn cache_hits_receive_fresh_run_metadata() {
        let decision = RouteDecision {
            run_id: "original".to_owned(),
            tier: "balanced".to_owned(),
            suggested_tier: "balanced".to_owned(),
            confidence: 0.8,
            probabilities: BTreeMap::new(),
            fallback_applied: false,
            jev_model: "jev-test".to_owned(),
            created_at_ms: 1,
            source: DecisionSource::Live,
        };

        let cached = decision.for_cache_hit();

        assert_ne!(cached.run_id, decision.run_id);
        assert!(cached.created_at_ms > decision.created_at_ms);
        assert_eq!(cached.source, DecisionSource::Cache);
        assert_eq!(cached.tier, decision.tier);
    }
}
