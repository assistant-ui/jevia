//! Safe CLI diagnostics. Do not retain parser errors as sources: alternate
//! anyhow formatting would otherwise print their private payloads again.
use anyhow::{Error, anyhow};
use jevia_core::ConfigError;

pub fn configuration(error: ConfigError, input: &str) -> Error {
    if let ConfigError::Parse(error) = &error {
        let position = error.span().and_then(|span| input.get(..span.start));
        return match position {
            Some(prefix) => anyhow!(
                "invalid configuration at line {}, column {} (contents redacted); check TOML syntax and field types",
                prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
                prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
            ),
            None => anyhow!(
                "invalid configuration (contents redacted); check TOML syntax and field types"
            ),
        };
    }
    let guidance = match error {
        ConfigError::InvalidStorage(detail) => detail,
        ConfigError::UnsupportedVersion(_) => "unsupported configuration version",
        ConfigError::InvalidConfidenceFloor(_) => "confidence_floor must be between 0 and 1",
        ConfigError::HistoryLimitTooLarge(_) => "history_limit cannot exceed 100",
        ConfigError::TooFewTiers => "at least two tiers are required",
        ConfigError::UnknownFallbackTier(_) => "fallback_tier must name a configured tier",
        ConfigError::EmptyTier => "tier names and descriptions cannot be empty",
        ConfigError::EmptyBaseUrl => "Jev base_url cannot be empty",
        ConfigError::EmptyModel => "Jev model cannot be empty",
        ConfigError::ZeroTimeout => "Jev timeout_ms must be greater than zero",
        ConfigError::ZeroCacheTtl => "cache ttl_seconds must be greater than zero",
        ConfigError::CacheTtlTooLarge(_) => "cache ttl_seconds cannot exceed 604800",
        ConfigError::ZeroCacheEntries => "cache max_entries must be greater than zero",
        ConfigError::CacheEntriesTooLarge(_) => "cache max_entries cannot exceed 10000",
        ConfigError::EmptyHarnessName => "harness names cannot be empty",
        ConfigError::EmptyHarnessCommand(_) => "harness commands cannot be empty",
        ConfigError::EmptyVerificationCommand(_) => "verification commands cannot be empty",
        ConfigError::MissingHarnessPlaceholder { .. } => {
            "harness args require {task} and {model} placeholders"
        }
        ConfigError::UnknownHarnessPlaceholder { .. } => {
            "allowed placeholders are {task}, {model}, {tier}, and {run_id}"
        }
        ConfigError::MissingHarnessModel { .. } => {
            "each harness must map every configured tier to a model"
        }
        ConfigError::Parse(_) | ConfigError::Serialize(_) => "configuration could not be processed",
    };
    anyhow!("invalid configuration: {guidance} (contents redacted)")
}

pub fn json_line(kind: &str, line: usize, error: &serde_json::Error) -> Error {
    anyhow!(
        "invalid {kind} on line {line}, column {} (contents redacted)",
        error.column()
    )
}
