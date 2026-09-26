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

/// Persisted local record used by the outcome feedback loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRecord {
    pub schema_version: u32,
    #[serde(flatten)]
    pub decision: RouteDecision,
    /// Omitted when `privacy.store_task_text` is disabled.
    pub task: Option<String>,
    pub outcome: Outcome,
}

impl RouteRecord {
    pub fn new(decision: RouteDecision, task: Option<String>) -> Self {
        Self {
            schema_version: RECORD_SCHEMA_VERSION,
            decision,
            task,
            outcome: Outcome::Unknown,
        }
    }
}
