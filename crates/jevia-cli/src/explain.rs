//! Opt-in observations, not model reasoning. Only static labels and numeric
//! facts are rendered: never task text, tier names, cache keys, or raw errors.

use std::{fmt, time::Instant};

use jevia_core::{Config, RouteDecision, RouteRecord};

use crate::cache::MissReason;

#[derive(Clone, Copy)]
pub enum CacheStatus {
    Hit,
    Miss(MissReason),
    Disabled,
    Bypassed,
    Unavailable,
    KeyUnavailable,
}

impl fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hit => "hit",
            Self::Miss(MissReason::NotFound) => "miss:not_found",
            Self::Miss(MissReason::Expired) => "miss:expired",
            Self::Disabled => "disabled",
            Self::Bypassed => "bypassed",
            Self::Unavailable => "unavailable",
            Self::KeyUnavailable => "key_unavailable",
        })
    }
}

#[derive(Default, Clone, Copy)]
pub enum Coordination {
    #[default]
    NotNeeded,
    Acquired,
    Waited,
    TimedOut,
    Unavailable,
}

impl fmt::Display for Coordination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotNeeded => "not_needed",
            Self::Acquired => "acquired",
            Self::Waited => "waited",
            Self::TimedOut => "timed_out",
            Self::Unavailable => "unavailable",
        })
    }
}

#[derive(Default)]
pub enum CacheWrite {
    #[default]
    Skipped,
    Stored,
    Failed,
}

impl fmt::Display for CacheWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Skipped => "skipped",
            Self::Stored => "stored",
            Self::Failed => "failed",
        })
    }
}

pub struct Trace {
    pub cache: CacheStatus,
    pub coordination: Coordination,
    pub write: CacheWrite,
}

pub struct Routed {
    pub record: RouteRecord,
    trace: Trace,
    known_outcomes: usize,
    passive_observations: usize,
    history_limit: usize,
    confidence_floor: f64,
    elapsed_ms: u128,
}

impl Routed {
    pub fn new(
        decision: RouteDecision,
        task: &str,
        config: &Config,
        history: &[RouteRecord],
        started: Instant,
        trace: Trace,
    ) -> Self {
        Self {
            record: RouteRecord::new(
                decision,
                config.privacy.store_task_text.then(|| task.to_owned()),
            ),
            trace,
            // Candidate counts precede the shared core byte-budget projection.
            // Label that stage explicitly without serializing history again just
            // to produce opt-in diagnostics on the routing hot path.
            known_outcomes: history
                .iter()
                .rev()
                .filter(|r| r.is_learning_evidence())
                .take(config.router.history_limit)
                .count(),
            passive_observations: history
                .iter()
                .rev()
                .filter(|r| r.is_execution_observation() && !r.is_learning_evidence())
                .take(config.router.history_limit)
                .count(),
            history_limit: config.router.history_limit,
            confidence_floor: config.router.confidence_floor,
            elapsed_ms: started.elapsed().as_millis(),
        }
    }

    pub fn print_explanation(&self) {
        eprintln!("{self}");
    }
}

impl fmt::Display for Routed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "jevia: explain cache={} coordination={} write={}",
            self.trace.cache, self.trace.coordination, self.trace.write
        )?;
        writeln!(
            f,
            "jevia: explain known_outcomes={} passive_observations={} history_limit_per_kind={} history_stage=candidates_before_byte_budget",
            self.known_outcomes, self.passive_observations, self.history_limit
        )?;
        write!(
            f,
            "jevia: explain source={} confidence={} floor={} fallback={} elapsed_ms={}",
            self.record.decision.source,
            self.record.decision.confidence,
            self.confidence_floor,
            if self.record.decision.fallback_applied {
                "below_confidence_floor"
            } else {
                "not_applied"
            },
            self.elapsed_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jevia_core::DecisionSource;

    #[test]
    fn formatting_never_renders_private_record_fields_or_configured_tier_names() {
        let private = "PRIVATE_VALUE\u{1b}[31m";
        let decision = RouteDecision {
            run_id: private.into(),
            tier: private.into(),
            suggested_tier: private.into(),
            confidence: 0.4,
            probabilities: [(private.into(), 0.4)].into(),
            fallback_applied: true,
            jev_model: private.into(),
            created_at_ms: 1,
            source: DecisionSource::Live,
        };
        let mut config = Config::default();
        config.privacy.store_task_text = true;
        config.router.fallback_tier = private.into();
        let routed = Routed::new(
            decision,
            private,
            &config,
            &[],
            Instant::now(),
            Trace {
                cache: CacheStatus::KeyUnavailable,
                coordination: Coordination::Unavailable,
                write: CacheWrite::Failed,
            },
        );
        let output = routed.to_string();
        assert_eq!(routed.record.task.as_deref(), Some(private));
        assert!(!output.contains("PRIVATE_VALUE"));
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("cache=key_unavailable coordination=unavailable write=failed"));
        assert!(output.contains("confidence=0.4"));
    }
}
