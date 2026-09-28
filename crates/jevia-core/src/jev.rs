use std::{
    collections::BTreeMap,
    fmt::Write as _,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{Config, DecisionSource, JevConfig, RouteDecision, RouteRecord};

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

/// Hash every input that can change a routing decision without persisting task
/// text in the cache index.
pub fn route_cache_key(
    task: &str,
    harness_name: Option<&str>,
    config: &Config,
    history: &[RouteRecord],
) -> Result<String, JevError> {
    config.validate()?;
    if task.trim().is_empty() {
        return Err(JevError::EmptyTask);
    }

    let request = build_request(task, config, history);
    let harness = harness_name.map(|name| {
        json!({
            "name": name,
            "config": config.harnesses.get(name),
        })
    });
    let material = json!({
        "cache_schema_version": 1,
        "api_base_url": config.jev.base_url.trim().trim_end_matches('/'),
        "router": &config.router,
        "harness": harness,
        "request": request,
    });
    let encoded = serde_json::to_vec(&material).map_err(|_| JevError::CacheKey)?;
    let digest = Sha256::digest(encoded);
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut key, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(key)
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
        .filter(|record| record.is_learning_evidence())
        .take(config.router.history_limit)
        .map(|record| {
            json!({
                "task": record.task,
                "tier": record.decision.tier,
                "confidence": record.decision.confidence,
                "outcome": record.outcome,
                "outcome_source": record.outcome_evidence.as_ref().map(|evidence| evidence.source),
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
        return Err(JevError::UnexpectedAnswerType);
    }
    if !answer.confidence.is_finite() || !(0.0..=1.0).contains(&answer.confidence) {
        return Err(JevError::InvalidConfidence);
    }
    if !config.tiers.contains_key(&answer.choice) {
        return Err(JevError::UnknownTier);
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
        source: DecisionSource::Live,
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
    Request(&'static str),
    #[error("Jev returned HTTP status {0}")]
    ApiStatus(u16),
    #[error("Jev returned invalid JSON at line {line}, column {column} (values redacted)")]
    InvalidJson { line: usize, column: usize },
    #[error("could not encode a routing cache key")]
    CacheKey,
    #[error("Jev response did not contain the `tier` answer")]
    MissingTierAnswer,
    #[error("Jev returned an unexpected tier answer type; expected `choice` (value redacted)")]
    UnexpectedAnswerType,
    #[error("Jev returned invalid confidence; expected a number between 0 and 1 (value redacted)")]
    InvalidConfidence,
    #[error("Jev selected an undefined tier (value redacted)")]
    UnknownTier,
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
}

// Transport errors may carry credentials in URLs and provider parser errors can
// quote response values. Discard the raw sources, not just their Display text:
// Debug and error-chain formatting must be safe too.
impl From<reqwest::Error> for JevError {
    fn from(error: reqwest::Error) -> Self {
        Self::Request(if error.is_timeout() {
            "timeout"
        } else if error.is_connect() {
            "connection failed"
        } else if error.is_builder() {
            "invalid request configuration"
        } else if error.is_body() {
            "response body failed"
        } else if error.is_decode() {
            "response decoding failed"
        } else {
            "transport failure"
        })
    }
}

impl From<serde_json::Error> for JevError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson {
            line: error.line(),
            column: error.column(),
        }
    }
}

impl From<StatusCode> for JevError {
    fn from(status: StatusCode) -> Self {
        Self::ApiStatus(status.as_u16())
    }
}

#[cfg(test)]
mod tests {
    use crate::{ExecutionEvidence, Outcome, OutcomeSource, VerificationEvidence};

    use super::*;

    #[test]
    fn provider_errors_discard_private_values_in_all_formats() {
        use std::error::Error as _;
        const PRIVATE: &str = "PRIVATE_PROVIDER_SENTINEL";
        let valid = json!({
            "model": "test", "answers": {"tier": {
                "type": "choice", "choice": "fast", "confidence": 0.9
            }}
        });
        for field in ["type", "choice", "confidence", "probabilities"] {
            let mut invalid = valid.clone();
            invalid["answers"]["tier"][field] = PRIVATE.into();
            let error = decode_decision(&serde_json::to_vec(&invalid).unwrap(), &Config::default())
                .unwrap_err();
            assert!(!format!("{error} {error:?}").contains(PRIVATE));
            assert!(error.source().is_none());
        }
        let error: JevError = reqwest::Client::new()
            .get(format!("invalid-url-{PRIVATE}"))
            .build()
            .unwrap_err()
            .into();
        assert!(!format!("{error} {error:?}").contains(PRIVATE));
        assert!(error.source().is_none());
    }

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
            verification: Some(VerificationEvidence {
                command: "cargo".to_owned(),
                launched: true,
                duration_ms: 21,
                exit_code: Some(1),
            }),
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
        assert_eq!(outcomes[0]["execution"]["verification"]["command"], "cargo");
        assert_eq!(outcomes[0]["execution"]["verification"]["exit_code"], 1);
        assert_eq!(outcomes[0]["execution"]["verification"]["launched"], true);
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
    fn routing_excludes_unverified_results_and_local_feedback_notes() {
        let mut unverified = record("process only", "fast", Outcome::Success);
        unverified.outcome_evidence.as_mut().unwrap().source = OutcomeSource::ProcessExit;
        let mut legacy = record("legacy", "fast", Outcome::Success);
        legacy.outcome_evidence = None;
        let mut verified = record("verified", "fast", Outcome::Success);
        verified.outcome_evidence.as_mut().unwrap().source = OutcomeSource::Verification;
        let mut manual = record("manual", "fast", Outcome::Failure);
        manual.feedback.push(crate::FeedbackEvent {
            previous_outcome: Outcome::Success,
            previous_source: Some(OutcomeSource::ProcessExit),
            outcome: Outcome::Failure,
            recorded_at_ms: 2,
            reason: Some("private-note".into()),
        });
        let config = Config::default();
        let baseline = route_cache_key("task", None, &config, &[]).unwrap();
        assert_eq!(
            baseline,
            route_cache_key("task", None, &config, &[unverified.clone(), legacy.clone()]).unwrap()
        );
        let request = build_request("task", &config, &[unverified, legacy, verified, manual]);
        let outcomes = request.state["recent_completed_outcomes"]
            .as_array()
            .unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0]["outcome_source"], "verification");
        assert_eq!(outcomes[1]["outcome_source"], "manual");
        assert!(
            !serde_json::to_string(&request)
                .unwrap()
                .contains("private-note")
        );
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
        assert_eq!(decision.source, DecisionSource::Live);
    }

    #[test]
    fn cache_key_changes_only_when_routing_inputs_change() {
        let config = Config::default();
        let pending = record("pending", "balanced", Outcome::Unknown);
        let completed = record("completed", "balanced", Outcome::Success);

        let baseline = route_cache_key("fix parser", Some("agent"), &config, &[])
            .expect("cache key is created");
        let repeated = route_cache_key("fix parser", Some("agent"), &config, &[pending])
            .expect("cache key is created");
        let with_evidence = route_cache_key("fix parser", Some("agent"), &config, &[completed])
            .expect("cache key is created");
        let other_task = route_cache_key("fix lexer", Some("agent"), &config, &[])
            .expect("cache key is created");
        let other_harness = route_cache_key("fix parser", Some("other"), &config, &[])
            .expect("cache key is created");

        assert_eq!(baseline, repeated);
        assert_ne!(baseline, with_evidence);
        assert_ne!(baseline, other_task);
        assert_ne!(baseline, other_harness);
        assert_eq!(baseline.len(), 64);
        assert!(!baseline.contains("fix parser"));
    }

    #[test]
    fn cache_key_changes_with_local_routing_policy() {
        let config = Config::default();
        let baseline =
            route_cache_key("fix parser", None, &config, &[]).expect("cache key is created");
        let mut changed = config.clone();
        changed.router.confidence_floor = 0.9;

        let changed =
            route_cache_key("fix parser", None, &changed, &[]).expect("cache key is created");

        assert_ne!(baseline, changed);
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
                source: DecisionSource::Live,
            },
            task: Some(task.to_owned()),
            outcome,
            execution: None,
            lifecycle: None,
            outcome_evidence: Some(crate::OutcomeEvidence {
                source: crate::OutcomeSource::Manual,
                recorded_at_ms: 1,
            }),
            feedback: Vec::new(),
        }
    }
}
