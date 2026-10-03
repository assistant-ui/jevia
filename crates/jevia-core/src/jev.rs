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

// Routing responses are small classification results, not generated content.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

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
        self.route_with_context(task, None, config, history).await
    }

    /// Route for one configured harness. Only its name and candidate tier/model
    /// mapping are sent; launch arguments, verifiers and other settings stay local.
    pub async fn route_for_harness(
        &self,
        task: &str,
        harness_name: &str,
        config: &Config,
        history: &[RouteRecord],
    ) -> Result<RouteDecision, JevError> {
        self.route_with_context(task, Some(harness_name), config, history)
            .await
    }

    async fn route_with_context(
        &self,
        task: &str,
        harness_name: Option<&str>,
        config: &Config,
        history: &[RouteRecord],
    ) -> Result<RouteDecision, JevError> {
        config.validate()?;
        if task.trim().is_empty() {
            return Err(JevError::EmptyTask);
        }

        let request = build_request(task, config, history, harness_name)?;
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

        let body = read_response(response).await?;
        decode_decision(&body, config)
    }
}

async fn read_response(mut response: reqwest::Response) -> Result<Vec<u8>, JevError> {
    let too_large = || JevError::Request("response exceeds 1 MiB limit");
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        // Also bound chunked/unknown-length responses. Do not reserve from an
        // untrusted header or retain a chunk that crosses the cumulative limit.
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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

    let request = build_request(task, config, history, harness_name)?;
    let harness = harness_name.map(|name| {
        json!({
            "name": name,
            "config": config.harnesses.get(name),
        })
    });
    let material = json!({
        "cache_schema_version": 2,
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
    harness_name: Option<&str>,
) -> Result<SystemOneRequest<'a>, JevError> {
    let harness = harness_name
        .map(|name| {
            let harness = config.harnesses.get(name).ok_or(JevError::UnknownHarness)?;
            let tier_models: BTreeMap<_, _> = config
                .tiers
                .keys()
                .map(|tier| (tier, &harness.models[tier]))
                .collect();
            Ok::<_, JevError>(json!({"name": name, "tier_models": tier_models}))
        })
        .transpose()?;
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
                "execution": record.execution.as_ref().map(|execution| json!({
                    "harness": execution.harness, "model": execution.model,
                    "duration_ms": execution.duration_ms, "exit_code": execution.exit_code,
                    "verification": execution.verification,
                    "harness_observations": execution.observations.as_ref().map(|o| o.routing_summary()),
                })),
            })
        })
        .collect();
    completed.reverse();

    let mut observations: Vec<_> = history
        .iter()
        .rev()
        .filter(|record| record.is_execution_observation() && !record.is_learning_evidence())
        .take(config.router.history_limit)
        .map(|record| {
            let execution = record.execution.as_ref().expect("observed execution");
            json!({
                "task": record.task,
                "tier": record.decision.tier,
                "state": record.lifecycle.as_ref().map(|life| life.state),
                "harness": execution.harness,
                "requested_model": execution.model,
                "duration_ms": execution.duration_ms,
                "process_exit_code": execution.exit_code,
                "harness_observations": execution.observations.as_ref().map(|o| o.routing_summary()),
            })
        })
        .collect();
    observations.reverse();

    let mut state = json!({
        "current_task": task,
        "recent_completed_outcomes": completed,
        "recent_execution_observations": observations,
        "policy": {
            "goal": "Select the least expensive capability tier likely to complete the task successfully.",
            "use_outcomes": "Treat relevant successes and failures as evidence, not absolute rules. Prefer the safer tier when evidence conflicts.",
            "use_observations": "Execution observations are operational context, not task-success labels. A process exit, duration, or agent-reported completion does not prove correctness. The requested model is not proof of which models actually executed. A model_observed event reports request selection, not a provider completion. tool_completed is neutral and does not establish tool success. Do not infer quality or model capability from missing feedback."
        }
    });
    if let Some(harness) = harness {
        state["current_harness"] = harness;
        state["policy"]["use_current_harness"] = json!(
            "The tier/model mapping describes configured candidates for this harness, not proof of execution or success. Use the tier descriptions and relevant recorded evidence. Model names alone do not establish quality, cost, or capability; do not transfer outcomes between different models just because their tier labels match."
        );
    }

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

    Ok(SystemOneRequest {
        model: &config.jev.model,
        state,
        questions,
    })
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

    let decision = RouteDecision {
        run_id: Uuid::new_v4().to_string(),
        tier,
        suggested_tier: answer.choice,
        confidence: answer.confidence,
        probabilities: answer.probabilities,
        fallback_applied,
        jev_model: response.model,
        created_at_ms: now_ms(),
        source: DecisionSource::Live,
    };
    decision.validate().map_err(JevError::InvalidDecision)?;
    Ok(decision)
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
    #[error("Jev returned an invalid routing decision: {0} (values redacted)")]
    InvalidDecision(&'static str),
    #[error("TYPESAFE_API_KEY is missing or empty")]
    MissingApiKey,
    #[error("task cannot be empty")]
    EmptyTask,
    #[error("routing harness is not configured (name redacted)")]
    UnknownHarness,
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

    fn harness(model: &str) -> crate::HarnessConfig {
        crate::HarnessConfig {
            command: "PRIVATE-command".into(),
            args: vec!["PRIVATE-argument".into(), "{task}".into(), "{model}".into()],
            models: Config::default()
                .tiers
                .keys()
                .map(|tier| (tier.clone(), model.into()))
                .collect(),
            auto_verify: false,
            observations: Default::default(),
            verification: Some(crate::VerificationConfig {
                command: "PRIVATE-verifier".into(),
                args: vec!["PRIVATE-verifier-argument".into()],
            }),
        }
    }

    #[test]
    fn current_harness_context_is_allowlisted_and_changes_with_model_mapping() {
        let mut config = Config::default();
        config
            .harnesses
            .insert("alpha".into(), harness("provider/alpha"));
        config.harnesses.insert(
            "PRIVATE-other-harness".into(),
            harness("PRIVATE-other-model"),
        );
        config
            .harnesses
            .get_mut("alpha")
            .unwrap()
            .models
            .insert("unused".into(), "PRIVATE-unused-model".into());
        config.validate().unwrap();
        let request = build_request("task", &config, &[], Some("alpha")).unwrap();
        assert_eq!(
            request.state["current_harness"],
            json!({
                "name": "alpha", "tier_models": {
                    "fast": "provider/alpha", "balanced": "provider/alpha", "strong": "provider/alpha"
                }
            })
        );
        assert!(!serde_json::to_string(&request).unwrap().contains("PRIVATE"));
        let key = route_cache_key("task", Some("alpha"), &config, &[]).unwrap();
        let plain = build_request("task", &config, &[], None).unwrap();
        assert!(plain.state.get("current_harness").is_none());
        let plain = serde_json::to_value(plain).unwrap();
        config
            .harnesses
            .get_mut("alpha")
            .unwrap()
            .models
            .insert("fast".into(), "provider/new".into());
        let changed = build_request("task", &config, &[], Some("alpha")).unwrap();
        assert_eq!(
            changed.state["current_harness"]["tier_models"]["fast"],
            "provider/new"
        );
        assert_ne!(
            key,
            route_cache_key("task", Some("alpha"), &config, &[]).unwrap()
        );
        assert_eq!(
            plain,
            serde_json::to_value(build_request("task", &config, &[], None).unwrap()).unwrap()
        );
    }

    #[test]
    fn unknown_routing_harness_is_rejected_without_echoing_its_name() {
        let config = Config::default();
        let error = build_request("task", &config, &[], Some("PRIVATE-harness")).unwrap_err();
        assert!(matches!(error, JevError::UnknownHarness));
        assert!(!format!("{error} {error:?}").contains("PRIVATE"));
        assert!(route_cache_key("task", Some("PRIVATE-harness"), &config, &[]).is_err());
    }

    #[test]
    fn rejects_invalid_probabilities_and_blank_models_without_echoing_values() {
        let valid = json!({"model": "test", "answers": {"tier": {
            "type": "choice", "choice": "fast", "confidence": 0.9,
            "probabilities": {"private-tier": 0.9}
        }}});
        for probability in [-1.0, 1.01, 2.0] {
            let mut body = valid.clone();
            body["answers"]["tier"]["probabilities"]["private-tier"] = probability.into();
            let error = decode_decision(&serde_json::to_vec(&body).unwrap(), &Config::default())
                .unwrap_err();
            assert!(matches!(error, JevError::InvalidDecision(_)));
            assert!(!format!("{error:?} {error}").contains("private-tier"));
        }
        for model in ["", " \n\t"] {
            let mut body = valid.clone();
            body["model"] = model.into();
            assert!(
                decode_decision(&serde_json::to_vec(&body).unwrap(), &Config::default()).is_err()
            );
        }
        for probabilities in [json!({}), json!({"fast": 0.0, "strong": 1.0})] {
            let mut body = valid.clone();
            body["answers"]["tier"]["probabilities"] = probabilities;
            let mut decision =
                decode_decision(&serde_json::to_vec(&body).unwrap(), &Config::default()).unwrap();
            for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                decision.probabilities.insert("private-tier".into(), number);
                assert!(decision.validate().is_err());
            }
        }
    }

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
            observations: None,
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

        let request = build_request("current", &config, &history, None).unwrap();
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
    fn passive_history_changes_routing_context_without_inventing_success() {
        let config = Config::default();
        let mut observed = record("passive", "fast", Outcome::Unknown);
        observed.lifecycle = Some(crate::RunLifecycle {
            state: crate::RunState::Completed,
            started_at_ms: Some(1),
            finished_at_ms: Some(2),
        });
        observed.execution = Some(ExecutionEvidence {
            observations: None,
            harness: "agent".into(),
            model: "requested-model".into(),
            duration_ms: 42,
            exit_code: Some(0),
            verification: None,
        });
        let baseline = route_cache_key("task", None, &config, &[]).unwrap();
        let with_history = route_cache_key("task", None, &config, &[observed.clone()]).unwrap();
        assert_ne!(baseline, with_history);
        let request = build_request("task", &config, &[observed.clone()], None).unwrap();
        assert_eq!(request.state["recent_completed_outcomes"], json!([]));
        let facts = &request.state["recent_execution_observations"][0];
        assert_eq!(facts["requested_model"], "requested-model");
        assert_eq!(facts["process_exit_code"], 0);
        assert!(facts.get("outcome").is_none());
        observed.lifecycle.as_mut().unwrap().state = crate::RunState::Running;
        assert_eq!(
            baseline,
            route_cache_key("task", None, &config, &[observed.clone()]).unwrap()
        );
        observed.lifecycle.as_mut().unwrap().state = crate::RunState::Cancelled;
        observed.task = None;
        let mut disabled = config;
        disabled.router.history_limit = 0;
        let request = build_request("task", &disabled, &[observed], None).unwrap();
        assert_eq!(request.state["recent_execution_observations"], json!([]));
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
        let request = build_request(
            "task",
            &config,
            &[unverified, legacy, verified, manual],
            None,
        )
        .unwrap();
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
        let mut config = Config::default();
        config
            .harnesses
            .insert("agent".into(), harness("agent-model"));
        config
            .harnesses
            .insert("other".into(), harness("other-model"));
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
