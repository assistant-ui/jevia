use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Current version of a persisted run record.
pub const RECORD_SCHEMA_VERSION: u32 = 1;

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

/// Persisted local record used by the outcome feedback loop.
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
}

impl RouteRecord {
    pub fn new(decision: RouteDecision, task: Option<String>) -> Self {
        Self {
            schema_version: RECORD_SCHEMA_VERSION,
            decision,
            task,
            outcome: Outcome::Unknown,
            execution: None,
        }
    }
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
}
