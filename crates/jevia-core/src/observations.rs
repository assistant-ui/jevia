//! Bounded, allowlisted harness facts. None of these events asserts task correctness.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_HARNESS_EVENTS: usize = 256;
pub const MAX_OBSERVED_MODELS: usize = 32;
type EventCounts = BTreeMap<HarnessEventKind, u64>;

/// Whole-session counters, independent of the bounded recent event sample.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationTotals {
    pub event_counts: EventCounts,
    pub models: BTreeMap<String, EventCounts>,
    pub unattributed_event_counts: EventCounts,
    pub omitted_model_event_counts: EventCounts,
    pub models_truncated: bool,
    pub discarded_inputs: u64,
}

impl ObservationTotals {
    pub fn event_count(&self) -> u64 {
        self.event_counts.values().sum()
    }

    pub fn observe(&mut self, event: &HarnessEvent) {
        *self.event_counts.entry(event.kind).or_default() += 1;
        let counts = if let Some(model) = &event.model {
            if self.models.contains_key(model) || self.models.len() < MAX_OBSERVED_MODELS {
                self.models.entry(model.clone()).or_default()
            } else {
                self.models_truncated = true;
                &mut self.omitted_model_event_counts
            }
        } else {
            &mut self.unattributed_event_counts
        };
        *counts.entry(event.kind).or_default() += 1;
        if let Some(previous) = &event.previous_model {
            if self.models.len() < MAX_OBSERVED_MODELS || self.models.contains_key(previous) {
                self.models.entry(previous.clone()).or_default();
            } else {
                self.models_truncated = true;
            }
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        const MAX_SAFE: u64 = 9_007_199_254_740_991;
        if self.models.len() > MAX_OBSERVED_MODELS
            || self.models.keys().any(|m| !valid_identifier(m))
            || self.discarded_inputs > MAX_SAFE
        {
            return Err("invalid observation totals");
        }
        let mut attributed = self.unattributed_event_counts.clone();
        for counts in self
            .models
            .values()
            .chain(std::iter::once(&self.omitted_model_event_counts))
        {
            for (kind, count) in counts {
                let sum = attributed.entry(*kind).or_default();
                *sum = sum
                    .checked_add(*count)
                    .ok_or("observation count overflow")?;
            }
        }
        let sum = self
            .event_counts
            .values()
            .try_fold(0_u64, |n, v| n.checked_add(*v))
            .ok_or("observation count overflow")?;
        if sum > MAX_SAFE
            || attributed != self.event_counts
            || (!self.omitted_model_event_counts.is_empty() && !self.models_truncated)
        {
            return Err("inconsistent observation totals");
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMode {
    #[default]
    Auto,
    ClaudeHooks,
    CodexHooks,
    OpencodePlugin,
    Off,
}

impl ObservationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ClaudeHooks => "claude_hooks",
            Self::CodexHooks => "codex_hooks",
            Self::OpencodePlugin => "opencode_plugin",
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
    CodexHooks,
    OpencodePlugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessEventKind {
    SessionStarted,
    SessionEnded,
    TurnStarted,
    TurnCompleted,
    TurnFailed,
    TurnInterrupted,
    /// A tool returned; its result does not establish success or failure.
    ToolCompleted,
    ToolSucceeded,
    ToolFailed,
    TaskReportedComplete,
    ModelChanged,
    /// The harness reported a model for a request, not proof of provider execution.
    ModelObserved,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totals: Option<ObservationTotals>,
}

#[derive(Deserialize)]
struct WireObservations {
    source: Option<ObservationSource>,
    status: ObservationStatus,
    events: Vec<HarnessEvent>,
    #[serde(default)]
    totals: Option<ObservationTotals>,
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
        if let Some(totals) = &wire.totals {
            totals.validate()?;
            if totals.event_count() < wire.events.len() as u64
                || (totals.event_count() > 0 && wire.events.is_empty())
                || (totals.discarded_inputs > 0 && wire.status != ObservationStatus::Partial)
            {
                return Err("inconsistent observation sample");
            }
            let mut sample = ObservationTotals::default();
            for event in &wire.events {
                sample.observe(event);
            }
            if sample.event_counts.iter().any(|(kind, count)| {
                *count > totals.event_counts.get(kind).copied().unwrap_or_default()
            }) {
                return Err("sample exceeds observation totals");
            }
        }
        Ok(Self {
            source: wire.source,
            status: wire.status,
            events: wire.events,
            totals: wire.totals,
        })
    }
}

impl HarnessObservations {
    pub fn event_count(&self) -> u64 {
        self.totals
            .as_ref()
            .map_or(self.events.len() as u64, ObservationTotals::event_count)
    }

    pub fn counts(&self) -> ObservationTotals {
        self.totals.clone().unwrap_or_else(|| {
            let mut totals = ObservationTotals::default();
            for event in &self.events {
                totals.observe(event);
            }
            totals
        })
    }

    pub fn observe(&mut self, event: HarnessEvent) {
        let mut totals = self.counts();
        totals.observe(&event);
        if self.events.len() == MAX_HARNESS_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
        self.totals = Some(totals);
        if self.status != ObservationStatus::Partial {
            self.status = ObservationStatus::Recorded;
        }
    }

    /// Routing gets bounded aggregates, not local session identifiers or an event transcript.
    pub fn routing_summary(&self) -> Value {
        let mut changes = Vec::new();
        for event in self.events.iter().rev() {
            if event.kind == HarnessEventKind::ModelChanged && changes.len() < 16 {
                changes.push(json!({"from": event.previous_model, "to": event.model}));
            }
        }
        changes.reverse();
        let totals = self.counts();
        let summary_truncated = totals.models_truncated
            || self.event_count() > self.events.len() as u64
            || self
                .events
                .iter()
                .filter(|e| e.kind == HarnessEventKind::ModelChanged)
                .count()
                > 16;
        json!({"source": self.source, "status": self.status, "event_counts": totals.event_counts,
            "observed_models": totals.models.keys().collect::<Vec<_>>(),
            "model_event_counts": totals.models, "unattributed_event_counts": totals.unattributed_event_counts,
            "omitted_model_event_counts": totals.omitted_model_event_counts, "discarded_inputs": totals.discarded_inputs,
            "total_events": self.event_count(), "sampled_events": self.events.len(),
            "model_switches": changes,
            "summary_truncated": summary_truncated})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_session_counts_survive_sampling_and_model_cardinality_limits() {
        let mut observations = HarnessObservations {
            source: Some(ObservationSource::ClaudeHooks),
            status: ObservationStatus::NoEvents,
            events: vec![],
            totals: None,
        };
        for i in 0..1000 {
            observations.observe(HarnessEvent {
                kind: HarnessEventKind::ToolSucceeded,
                recorded_at_ms: i,
                session_id: None,
                agent_id: None,
                model: (i % 3 != 0).then(|| format!("model-{}", i % 40)),
                previous_model: None,
                tool_name: None,
            });
        }
        assert_eq!(observations.event_count(), 1000);
        assert_eq!(observations.events.len(), 256);
        assert_eq!(observations.events[0].recorded_at_ms, 744);
        let totals = observations.counts();
        totals.validate().unwrap();
        assert_eq!(totals.models.len(), MAX_OBSERVED_MODELS);
        assert!(totals.models_truncated);
        assert_eq!(
            totals.unattributed_event_counts[&HarnessEventKind::ToolSucceeded],
            334
        );
        assert!(!totals.omitted_model_event_counts.is_empty());
        assert_eq!(
            serde_json::from_value::<HarnessObservations>(
                serde_json::to_value(&observations).unwrap()
            )
            .unwrap(),
            observations
        );
        let mut invalid = serde_json::to_value(&observations).unwrap();
        invalid["totals"]["event_counts"]["tool_succeeded"] = json!(999);
        assert!(serde_json::from_value::<HarnessObservations>(invalid).is_err());
        assert_eq!(observations.routing_summary()["total_events"], 1000);
        assert_eq!(observations.routing_summary()["sampled_events"], 256);
    }
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
