//! Bounded, allowlisted harness facts. None of these events asserts task correctness.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_HARNESS_EVENTS: usize = 256;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMode {
    #[default]
    Auto,
    ClaudeHooks,
    Off,
}

impl ObservationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ClaudeHooks => "claude_hooks",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    Unsupported,
    Disabled,
    Unavailable,
    NoEvents,
    Recorded,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    ClaudeHooks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessEventKind {
    SessionStarted,
    SessionEnded,
    TurnStarted,
    TurnCompleted,
    TurnFailed,
    ToolSucceeded,
    ToolFailed,
    TaskReportedComplete,
    ModelChanged,
    SubagentStarted,
    SubagentStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessEvent {
    pub kind: HarnessEventKind,
    pub recorded_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Only a model explicitly reported in this event, never the configured model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl HarnessEvent {
    pub fn validate(&self) -> Result<(), &'static str> {
        for value in [
            &self.session_id,
            &self.agent_id,
            &self.model,
            &self.previous_model,
            &self.tool_name,
        ]
        .into_iter()
        .flatten()
        {
            if !valid_identifier(value) {
                return Err("invalid harness event identifier (value redacted)");
            }
        }
        Ok(())
    }
}

/// Identifiers only: never free text, tool arguments, transcript paths, or output.
pub fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/@+-".contains(&b))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "WireObservations")]
pub struct HarnessObservations {
    pub source: Option<ObservationSource>,
    pub status: ObservationStatus,
    pub events: Vec<HarnessEvent>,
}

#[derive(Deserialize)]
struct WireObservations {
    source: Option<ObservationSource>,
    status: ObservationStatus,
    events: Vec<HarnessEvent>,
}

impl TryFrom<WireObservations> for HarnessObservations {
    type Error = &'static str;
    fn try_from(wire: WireObservations) -> Result<Self, Self::Error> {
        if wire.events.len() > MAX_HARNESS_EVENTS {
            return Err("too many harness events");
        }
        if (!wire.events.is_empty()
            && (wire.source.is_none()
                || !matches!(
                    wire.status,
                    ObservationStatus::Recorded | ObservationStatus::Partial
                )))
            || (wire.status == ObservationStatus::Recorded && wire.events.is_empty())
        {
            return Err("inconsistent harness observation status");
        }
        for event in &wire.events {
            event.validate()?;
        }
        Ok(Self {
            source: wire.source,
            status: wire.status,
            events: wire.events,
        })
    }
}

impl HarnessObservations {
    /// Routing gets bounded aggregates, not local session identifiers or an event transcript.
    pub fn routing_summary(&self) -> Value {
        let mut counts = BTreeMap::<HarnessEventKind, usize>::new();
        let mut models = BTreeSet::new();
        let mut changes = Vec::new();
        for event in &self.events {
            *counts.entry(event.kind).or_default() += 1;
            models.extend(event.model.iter());
            models.extend(event.previous_model.iter());
            if event.kind == HarnessEventKind::ModelChanged && changes.len() < 16 {
                changes.push(json!({"from": event.previous_model, "to": event.model}));
            }
        }
        let summary_truncated = models.len() > 32
            || self
                .events
                .iter()
                .filter(|e| e.kind == HarnessEventKind::ModelChanged)
                .count()
                > 16;
        json!({"source": self.source, "status": self.status, "event_counts": counts,
            "observed_models": models.into_iter().take(32).collect::<Vec<_>>(),
            "model_switches": changes,
            "summary_truncated": summary_truncated})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_deserialization_rejects_free_text_without_echoing_it() {
        let event =
            json!({"kind": "model_changed", "recorded_at_ms": 1, "model": "private content\n"});
        let wire = json!({"source": "claude_hooks", "status": "recorded", "events": [event]});
        let error = serde_json::from_value::<HarnessObservations>(wire).unwrap_err();
        assert!(!error.to_string().contains("private content"));
        let event = json!({"kind": "turn_completed", "recorded_at_ms": 1});
        assert!(serde_json::from_value::<HarnessObservations>(json!({"source": "claude_hooks", "status": "recorded", "events": vec![event; MAX_HARNESS_EVENTS + 1]})).is_err());
    }
}
