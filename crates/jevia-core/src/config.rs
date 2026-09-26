use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current version of the project configuration format.
pub const CONFIG_VERSION: u32 = 1;

/// Project-level Jevia configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub router: RouterConfig,
    pub jev: JevConfig,
    pub privacy: PrivacyConfig,
    pub tiers: BTreeMap<String, TierConfig>,
}

impl Config {
    /// Parse and validate a TOML configuration document.
    pub fn from_toml(input: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(input)?;
        config.validate()?;
        Ok(config)
    }

    /// Render this configuration as a stable, human-readable TOML document.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        self.validate()?;
        Ok(toml::to_string_pretty(self)?)
    }

    /// Validate cross-field policy invariants.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(self.version));
        }
        if !(0.0..=1.0).contains(&self.router.confidence_floor) {
            return Err(ConfigError::InvalidConfidenceFloor(
                self.router.confidence_floor,
            ));
        }
        if self.router.history_limit > 100 {
            return Err(ConfigError::HistoryLimitTooLarge(self.router.history_limit));
        }
        if self.tiers.len() < 2 {
            return Err(ConfigError::TooFewTiers);
        }
        if !self.tiers.contains_key(&self.router.fallback_tier) {
            return Err(ConfigError::UnknownFallbackTier(
                self.router.fallback_tier.clone(),
            ));
        }
        if self
            .tiers
            .iter()
            .any(|(name, tier)| name.trim().is_empty() || tier.description.trim().is_empty())
        {
            return Err(ConfigError::EmptyTier);
        }
        if self.jev.base_url.trim().is_empty() {
            return Err(ConfigError::EmptyBaseUrl);
        }
        if self.jev.model.trim().is_empty() {
            return Err(ConfigError::EmptyModel);
        }
        if self.jev.timeout_ms == 0 {
            return Err(ConfigError::ZeroTimeout);
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let tiers = [
            (
                "fast".to_owned(),
                TierConfig {
                    description: "Routine, mechanical, or narrowly scoped work with low ambiguity"
                        .to_owned(),
                },
            ),
            (
                "balanced".to_owned(),
                TierConfig {
                    description:
                        "Ordinary engineering work requiring moderate reasoning or repository context"
                            .to_owned(),
                },
            ),
            (
                "strong".to_owned(),
                TierConfig {
                    description: "Ambiguous, high-risk, architectural, or deeply multi-step work"
                        .to_owned(),
                },
            ),
        ]
        .into_iter()
        .collect();

        Self {
            version: CONFIG_VERSION,
            router: RouterConfig::default(),
            jev: JevConfig::default(),
            privacy: PrivacyConfig::default(),
            tiers,
        }
    }
}

/// Deterministic policy applied after Jev responds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouterConfig {
    /// Use the fallback tier below this confidence.
    pub confidence_floor: f64,
    /// Safe tier selected when Jev is uncertain.
    pub fallback_tier: String,
    /// Maximum number of recent completed outcomes supplied to Jev.
    pub history_limit: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            confidence_floor: 0.65,
            fallback_tier: "strong".to_owned(),
            history_limit: 20,
        }
    }
}

/// Connection settings for the Jev decision API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevConfig {
    /// API root without the `/v1/systemone` path.
    pub base_url: String,
    /// Jev model alias or pinned version.
    pub model: String,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.typesafe.ai".to_owned(),
            model: "jev-latest".to_owned(),
            timeout_ms: 10_000,
        }
    }
}

/// Local persistence controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyConfig {
    /// Persist raw task text in the local run history.
    pub store_task_text: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            store_task_text: true,
        }
    }
}

/// A stable capability lane. Model names are deliberately kept out of this
/// layer so harness adapters can map tiers independently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierConfig {
    pub description: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not parse configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("could not serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("configuration version {0} is not supported")]
    UnsupportedVersion(u32),
    #[error("confidence_floor must be between 0 and 1, got {0}")]
    InvalidConfidenceFloor(f64),
    #[error("history_limit cannot exceed 100, got {0}")]
    HistoryLimitTooLarge(usize),
    #[error("at least two tiers are required")]
    TooFewTiers,
    #[error("fallback tier `{0}` is not defined")]
    UnknownFallbackTier(String),
    #[error("tier names and descriptions cannot be empty")]
    EmptyTier,
    #[error("Jev base_url cannot be empty")]
    EmptyBaseUrl,
    #[error("Jev model cannot be empty")]
    EmptyModel,
    #[error("Jev timeout_ms must be greater than zero")]
    ZeroTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let expected = Config::default();
        let rendered = expected.to_toml().expect("default config serializes");
        let actual = Config::from_toml(&rendered).expect("rendered config parses");

        assert_eq!(actual, expected);
    }

    #[test]
    fn rejects_an_unknown_fallback_tier() {
        let mut config = Config::default();
        config.router.fallback_tier = "missing".to_owned();

        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownFallbackTier(tier)) if tier == "missing"
        ));
    }

    #[test]
    fn rejects_an_invalid_confidence_floor() {
        let mut config = Config::default();
        config.router.confidence_floor = 1.1;

        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidConfidenceFloor(value)) if value == 1.1
        ));
    }
}
