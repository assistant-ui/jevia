//! Core types and routing logic for Jevia.

mod config;
mod jev;
mod route;

pub use config::{
    CacheConfig, Config, ConfigError, HarnessConfig, HarnessInvocation, JevConfig, PrivacyConfig,
    RouterConfig, TierConfig, VerificationConfig, VerificationInvocation,
};
pub use jev::{JevClient, JevError, route_cache_key};
pub use route::{
    DecisionSource, ExecutionEvidence, FeedbackEvent, Outcome, OutcomeEvidence, OutcomeSource,
    RECORD_SCHEMA_VERSION, RouteDecision, RouteRecord, RunLifecycle, RunState,
    VerificationEvidence,
};
