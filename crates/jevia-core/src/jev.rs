use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::{Config, JevConfig, Outcome, RouteDecision, RouteRecord};

/// Minimal Jev HTTP client. The API key is intentionally excluded from Debug,
/// errors, and serialized values.
pub struct JevClient {
    http: reqwest::Client,
    api_key: String,
    endpoint: String,
}

impl JevClient {
    pub fn new(api_key: impl Into<String>, config: &JevConfig) -> Result<Self, JevError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(JevError::MissingApiKey);
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .user_agent(concat!("jevia/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let endpoint = format!(
            "{}/v1/systemone",
            config.base_url.trim().trim_end_matches('/')
        );

        Ok(Self {
            http,
            api_key,
            endpoint,
        })
    }

    /// Ask Jev for a capability tier and apply Jevia's confidence fallback.
    pub async fn route(
        &self,
        task: &str,
        config: &Config,
        history: &[RouteRecord],
    ) -> Result<RouteDecision, JevError> {
        config.validate()?;
        if task.trim().is_empty() {
            return Err(JevError::EmptyTask);
        }

        let request = build_request(task, config, history);
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .header("Idempotency-Key", Uuid::new_v4().to_string())
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(JevError::ApiStatus(status.as_u16()));
        }

        let body = response.bytes().await?;
        decode_decision(&body, config)
    }
}

#[derive(Debug, Serialize)]
struct SystemOneRequest<'a> {
    model: &'a str,
    state: Value,
    questions: BTreeMap<&'static str, ChoiceQuestion>,
}

#[derive(Debug, Serialize)]
struct ChoiceQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: &'static str,
    criteria: BTreeMap<String, String>,
}

fn build_request<'a>(
    task: &str,
    config: &'a Config,
    history: &[RouteRecord],
) -> SystemOneRequest<'a> {
    let mut completed: Vec<_> = history
        .iter()
        .rev()
        .filter(|record| record.outcome != Outcome::Unknown)
        .take(config.router.history_limit)
        .map(|record| {
            json!({
                "task": record.task,
                "tier": record.decision.tier,
                "confidence": record.decision.confidence,
                "outcome": record.outcome,
                "execution": record.execution,
            })
        })
        .collect();
    completed.reverse();

    let state = json!({
        "current_task": task,
        "recent_completed_outcomes": completed,
        "policy": {
            "goal": "Select the least expensive capability tier likely to complete the task successfully.",
            "use_outcomes": "Treat relevant successes and failures as evidence, not absolute rules. Prefer the safer tier when evidence conflicts."
        }
    });

    let criteria = config
        .tiers
        .iter()
        .map(|(name, tier)| (name.clone(), tier.description.clone()))
        .collect();
    let questions = [(
        "tier",
        ChoiceQuestion {
            kind: "choice",
            instructions: "Which capability tier should handle the current task?",
            criteria,
        },
    )]
    .into_iter()
    .collect();

    SystemOneRequest {
        model: &config.jev.model,
        state,
        questions,
    }
}

#[derive(Debug, Deserialize)]
struct SystemOneResponse {
    model: String,
    answers: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct ChoiceAnswer {
    #[serde(rename = "type")]
    kind: String,
    choice: String,
    confidence: f64,
    #[serde(default)]
    probabilities: BTreeMap<String, f64>,
}

fn decode_decision(body: &[u8], config: &Config) -> Result<RouteDecision, JevError> {
    let response: SystemOneResponse = serde_json::from_slice(body)?;
    let answer = response
        .answers
        .get("tier")
        .ok_or(JevError::MissingTierAnswer)?;
    let answer: ChoiceAnswer = serde_json::from_value(answer.clone())?;

    if answer.kind != "choice" {
        return Err(JevError::UnexpectedAnswerType(answer.kind));
    }
    if !answer.confidence.is_finite() || !(0.0..=1.0).contains(&answer.confidence) {
        return Err(JevError::InvalidConfidence(answer.confidence));
    }
    if !config.tiers.contains_key(&answer.choice) {
        return Err(JevError::UnknownTier(answer.choice));
    }

    let fallback_applied = answer.confidence < config.router.confidence_floor;
    let tier = if fallback_applied {
        config.router.fallback_tier.clone()
    } else {
        answer.choice.clone()
    };

    Ok(RouteDecision {
        run_id: Uuid::new_v4().to_string(),
        tier,
        suggested_tier: answer.choice,
        confidence: answer.confidence,
        probabilities: answer.probabilities,
        fallback_applied,
        jev_model: response.model,
        created_at_ms: now_ms(),
    })
}

fn now_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[derive(Debug, Error)]
pub enum JevError {
    #[error("TYPESAFE_API_KEY is missing or empty")]
    MissingApiKey,
    #[error("task cannot be empty")]
    EmptyTask,
    #[error("Jev request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Jev returned HTTP status {0}")]
    ApiStatus(u16),
    #[error("Jev returned invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Jev response did not contain the `tier` answer")]
    MissingTierAnswer,
    #[error("Jev returned `{0}` for the tier answer instead of `choice`")]
    UnexpectedAnswerType(String),
    #[error("Jev returned invalid confidence {0}")]
    InvalidConfidence(f64),
    #[error("Jev selected undefined tier `{0}`")]
    UnknownTier(String),
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
}

impl From<StatusCode> for JevError {
    fn from(status: StatusCode) -> Self {
        Self::ApiStatus(status.as_u16())
    }
}

#[cfg(test)]
mod tests {
    use crate::ExecutionEvidence;

    use super::*;

    #[test]
    fn request_includes_only_recent_completed_outcomes() {
        let mut config = Config::default();
        config.router.history_limit = 1;
        let mut completed = record("last", "strong", Outcome::Failure);
        completed.execution = Some(ExecutionEvidence {
            harness: "agent".to_owned(),
            model: "provider/frontier".to_owned(),
            duration_ms: 42,
            exit_code: Some(1),
        });
        let history = vec![
            record("first", "fast", Outcome::Success),
            record("pending", "balanced", Outcome::Unknown),
            completed,
        ];

        let request = build_request("current", &config, &history);
        let outcomes = request.state["recent_completed_outcomes"]
            .as_array()
            .expect("outcomes are an array");

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0]["task"], "last");
        assert_eq!(outcomes[0]["outcome"], "failure");
        assert_eq!(outcomes[0]["execution"]["harness"], "agent");
        assert_eq!(outcomes[0]["execution"]["model"], "provider/frontier");
        assert_eq!(outcomes[0]["execution"]["duration_ms"], 42);
        assert_eq!(outcomes[0]["execution"]["exit_code"], 1);
    }

    #[test]
    fn low_confidence_uses_the_safe_fallback() {
        let config = Config::default();
        let body = br#"{
            "model": "jev-1.13.0",
            "answers": {
                "tier": {
                    "type": "choice",
                    "choice": "fast",
                    "confidence": 0.42,
                    "probabilities": {"fast": 0.42, "balanced": 0.35, "strong": 0.23}
                }
            }
        }"#;

        let decision = decode_decision(body, &config).expect("response decodes");

        assert_eq!(decision.suggested_tier, "fast");
        assert_eq!(decision.tier, "strong");
        assert!(decision.fallback_applied);
    }

    #[test]
    fn confident_answer_keeps_the_selected_tier() {
        let config = Config::default();
        let body = br#"{
            "model": "jev-1.13.0",
            "answers": {
                "tier": {
                    "type": "choice",
                    "choice": "balanced",
                    "confidence": 0.87,
                    "probabilities": {"fast": 0.05, "balanced": 0.87, "strong": 0.08}
                }
            }
        }"#;

        let decision = decode_decision(body, &config).expect("response decodes");

        assert_eq!(decision.tier, "balanced");
        assert!(!decision.fallback_applied);
    }

    fn record(task: &str, tier: &str, outcome: Outcome) -> RouteRecord {
        RouteRecord {
            schema_version: 1,
            decision: RouteDecision {
                run_id: Uuid::new_v4().to_string(),
                tier: tier.to_owned(),
                suggested_tier: tier.to_owned(),
                confidence: 0.9,
                probabilities: BTreeMap::new(),
                fallback_applied: false,
                jev_model: "jev-test".to_owned(),
                created_at_ms: 1,
            },
            task: Some(task.to_owned()),
            outcome,
            execution: None,
        }
    }
}
