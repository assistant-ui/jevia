//! Core types and routing logic for Jevia.

mod config;
mod jev;
mod observations;
mod route;

pub use observations::{
    HarnessEvent, HarnessEventKind, HarnessObservations, MAX_HARNESS_EVENTS, MAX_OBSERVED_MODELS,
    ObservationMode, ObservationSource, ObservationStatus, ObservationTotals, valid_identifier,
};

pub use config::{
    CacheConfig, Config, ConfigError, HarnessConfig, HarnessInvocation, JevConfig, PrivacyConfig,
    RouterConfig, StorageConfig, TierConfig, VerificationConfig, VerificationInvocation,
};
pub use jev::{JevClient, JevError, route_cache_key};
pub use route::{
    DecisionSource, ExecutionEvidence, FeedbackEvent, Outcome, OutcomeEvidence, OutcomeSource,
    RECORD_SCHEMA_VERSION, RouteDecision, RouteRecord, RunLifecycle, RunState,
    VerificationEvidence,
};
