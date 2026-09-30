use crate::{
    ExecutionEvidence, HarnessEvent, HarnessObservations, MAX_HARNESS_EVENTS, ObservationSource,
    ObservationStatus, valid_identifier,
};
use serde::{Deserialize, Serialize};

/// A finished, application-owned execution. Reporting is optional and does not
/// assert task correctness. One immutable snapshot per routed run permits retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecording {
    pub harness: String,
    /// Requested model; each event carries its own independently reported model.
    pub model: String,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub events: Vec<HarnessEvent>,
}

impl ExecutionRecording {
    pub fn into_evidence(self) -> Result<ExecutionEvidence, &'static str> {
        if !valid_identifier(&self.harness)
            || !valid_identifier(&self.model)
            || self.duration_ms > 9_007_199_254_740_991
            || self.events.len() > MAX_HARNESS_EVENTS
        {
            return Err("invalid execution recording");
        }
        let mut observations = HarnessObservations {
            source: Some(ObservationSource::Application),
            status: ObservationStatus::NoEvents,
            events: vec![],
            totals: None,
        };
        for event in self.events {
            if event.recorded_at_ms > 9_007_199_254_740_991 {
                return Err("invalid execution event timestamp");
            }
            event.validate()?;
            observations.observe(event);
        }
        Ok(ExecutionEvidence {
            harness: self.harness,
            model: self.model,
            duration_ms: self.duration_ms,
            exit_code: self.exit_code,
            verification: None,
            observations: Some(observations),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn application_input_is_bounded_and_cannot_smuggle_content_or_outcomes() {
        let input = json!({"harness":"app","model":"requested","duration_ms":1,"events":[{"kind":"turn_completed","recorded_at_ms":1}]});
        for (field, value) in [
            ("outcome", json!("success")),
            ("verification", json!({})),
            ("prompt", json!("PRIVATE")),
        ] {
            let mut bad = input.clone();
            bad[field] = value;
            assert!(serde_json::from_value::<ExecutionRecording>(bad).is_err());
        }
        let mut bad = input.clone();
        bad["events"][0]["output"] = json!("PRIVATE");
        assert!(serde_json::from_value::<ExecutionRecording>(bad).is_err());
        for count in [0, 256, 257] {
            let mut value = input.clone();
            value["events"] = json!(vec![input["events"][0].clone(); count]);
            let input: ExecutionRecording = serde_json::from_value(value).unwrap();
            assert_eq!(input.into_evidence().is_ok(), count <= 256);
        }
        for (field, number) in [
            ("duration_ms", 9_007_199_254_740_992_u64),
            ("duration_ms", u64::MAX),
        ] {
            let mut bad = input.clone();
            bad[field] = json!(number);
            assert!(
                serde_json::from_value::<ExecutionRecording>(bad)
                    .unwrap()
                    .into_evidence()
                    .is_err()
            );
        }
        let mut bad = input;
        bad["events"][0]["recorded_at_ms"] = json!(u64::MAX);
        assert!(
            serde_json::from_value::<ExecutionRecording>(bad)
                .unwrap()
                .into_evidence()
                .is_err()
        );
    }
}
