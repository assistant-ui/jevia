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
    evidence_count: usize,
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
            evidence_count: history
                .iter()
                .filter(|r| r.is_learning_evidence())
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
            "jevia: explain eligible_evidence={} history_limit={}",
            self.evidence_count, self.history_limit
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
