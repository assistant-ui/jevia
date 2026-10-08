//! Core types and routing logic for Jevia.

mod config;
mod jev;
mod observations;
mod recording;
mod route;
pub use recording::ExecutionRecording;

pub use observations::{
    HarnessEvent, HarnessEventKind, HarnessObservations, MAX_HARNESS_EVENTS, MAX_OBSERVED_MODELS,
    ObservationMode, ObservationSource, ObservationStatus, ObservationTotals, valid_identifier,
};

pub use config::{
    CacheConfig, Config, ConfigError, HarnessConfig, HarnessInvocation, JevConfig, PrivacyConfig,
    RouterConfig, StorageConfig, TierConfig, VerificationConfig, VerificationInvocation,
};
pub use jev::{
    JevClient, JevError, RoutingCandidate, RoutingHistory, route_cache_key,
    route_cache_key_with_history,
};
pub use route::{
    DecisionSource, ExecutionEvidence, FeedbackEvent, MAX_SAFE_INTEGER, Outcome, OutcomeEvidence,
    OutcomeSource, RECORD_SCHEMA_VERSION, RouteDecision, RouteRecord, RunLifecycle, RunState,
    VerificationEvidence,
};
