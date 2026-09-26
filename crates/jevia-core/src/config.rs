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
    #[serde(default, skip_serializing_if = "StorageConfig::is_jsonl")]
    pub storage: StorageConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    pub tiers: BTreeMap<String, TierConfig>,
    #[serde(default)]
    pub harnesses: BTreeMap<String, HarnessConfig>,
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
        self.storage.validate()?;
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
        if self.cache.ttl_seconds == 0 {
            return Err(ConfigError::ZeroCacheTtl);
        }
        if self.cache.ttl_seconds > 604_800 {
            return Err(ConfigError::CacheTtlTooLarge(self.cache.ttl_seconds));
        }
        if self.cache.max_entries == 0 {
            return Err(ConfigError::ZeroCacheEntries);
        }
        if self.cache.max_entries > 10_000 {
            return Err(ConfigError::CacheEntriesTooLarge(self.cache.max_entries));
        }
        for (name, harness) in &self.harnesses {
            harness.validate(name, self.tiers.keys())?;
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
            storage: StorageConfig::default(),
            cache: CacheConfig::default(),
            tiers,
            harnesses: BTreeMap::new(),
        }
    }
}

/// Opt-in persistence. Credentials are only resolved by the CLI at runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum StorageConfig {
    #[default]
    Jsonl,
    Sqlite {
        url: String,
    },
    Postgres {
        url_env: String,
        project: String,
        /// Development only: permit plaintext connections to a loopback host.
        #[serde(default)]
        allow_insecure_localhost: bool,
    },
}

impl StorageConfig {
    pub fn is_jsonl(&self) -> bool {
        matches!(self, Self::Jsonl)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        match self {
            Self::Jsonl => Ok(()),
            Self::Sqlite { url } => {
                let path = url.strip_prefix("sqlite://").filter(|s| !s.is_empty());
                if path.is_none_or(|s| s.contains(['?', '#', '\0']) || s.contains(":memory:")) {
                    return Err(ConfigError::InvalidStorage(
                        "SQLite requires a persistent sqlite:// file URL without query parameters",
                    ));
                }
                Ok(())
            }
            Self::Postgres {
                url_env, project, ..
            } => {
                if url_env.is_empty()
                    || !url_env.bytes().enumerate().all(|(i, c)| {
                        c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                    })
                {
                    return Err(ConfigError::InvalidStorage(
                        "url_env must name an environment variable, not contain a connection string",
                    ));
                }
                if project.is_empty()
                    || project.len() > 128
                    || !project
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                {
                    return Err(ConfigError::InvalidStorage(
                        "project must contain 1–128 letters, digits, dots, hyphens, or underscores",
                    ));
                }
                Ok(())
            }
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

/// Persistence controls for every history backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyConfig {
    /// Persist raw task text in the configured run history.
    pub store_task_text: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            store_task_text: true,
        }
    }
}

/// Local routing-decision cache controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    /// Reuse equivalent, unexpired Jev decisions.
    pub enabled: bool,
    /// Maximum age of a cached decision.
    pub ttl_seconds: u64,
    /// Maximum number of decisions retained locally.
    pub max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_seconds: 900,
            max_entries: 256,
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

/// Process template and tier-to-model mapping for a coding-agent harness.
/// Arguments are executed directly, never through a shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    pub command: String,
    pub args: Vec<String>,
    pub models: BTreeMap<String, String>,
    #[serde(default)]
    pub verification: Option<VerificationConfig>,
}

/// Optional process that verifies a successful harness run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationConfig {
    pub command: String,
    pub args: Vec<String>,
}

impl HarnessConfig {
    fn validate<'a>(
        &self,
        name: &str,
        tier_names: impl Iterator<Item = &'a String>,
    ) -> Result<(), ConfigError> {
        if name.trim().is_empty() {
            return Err(ConfigError::EmptyHarnessName);
        }
        if self.command.trim().is_empty() {
            return Err(ConfigError::EmptyHarnessCommand(name.to_owned()));
        }
        if !self.args.iter().any(|argument| argument.contains("{task}")) {
            return Err(ConfigError::MissingHarnessPlaceholder {
                harness: name.to_owned(),
                placeholder: "{task}",
            });
        }
        if !self
            .args
            .iter()
            .any(|argument| argument.contains("{model}"))
        {
            return Err(ConfigError::MissingHarnessPlaceholder {
                harness: name.to_owned(),
                placeholder: "{model}",
            });
        }
        for argument in &self.args {
            validate_template(argument, name)?;
        }
        for tier in tier_names {
            match self.models.get(tier) {
                Some(model) if !model.trim().is_empty() => {}
                _ => {
                    return Err(ConfigError::MissingHarnessModel {
                        harness: name.to_owned(),
                        tier: tier.clone(),
                    });
                }
            }
        }
        if let Some(verification) = &self.verification {
            if verification.command.trim().is_empty() {
                return Err(ConfigError::EmptyVerificationCommand(name.to_owned()));
            }
            for argument in &verification.args {
                validate_template(argument, name)?;
            }
        }
        Ok(())
    }

    /// Render a direct process invocation for a selected tier.
    pub fn invocation(
        &self,
        harness_name: &str,
        tier: &str,
        task: &str,
        run_id: &str,
        extra_args: &[String],
    ) -> Result<HarnessInvocation, ConfigError> {
        let model = self
            .models
            .get(tier)
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| ConfigError::MissingHarnessModel {
                harness: harness_name.to_owned(),
                tier: tier.to_owned(),
            })?;
        let mut args: Vec<_> = self
            .args
            .iter()
            .map(|argument| render_argument(argument, harness_name, task, model, tier, run_id))
            .collect::<Result<_, _>>()?;
        args.extend_from_slice(extra_args);
        let verification = match &self.verification {
            Some(verification) => Some(VerificationInvocation {
                program: verification.command.clone(),
                args: verification
                    .args
                    .iter()
                    .map(|argument| {
                        render_argument(argument, harness_name, task, model, tier, run_id)
                    })
                    .collect::<Result<_, _>>()?,
            }),
            None => None,
        };

        Ok(HarnessInvocation {
            program: self.command.clone(),
            args,
            model: model.clone(),
            verification,
        })
    }
}

fn validate_template(argument: &str, harness_name: &str) -> Result<(), ConfigError> {
    let remainder = argument
        .replace("{task}", "")
        .replace("{model}", "")
        .replace("{tier}", "")
        .replace("{run_id}", "");
    if remainder.contains('{') || remainder.contains('}') {
        return Err(ConfigError::UnknownHarnessPlaceholder {
            harness: harness_name.to_owned(),
            argument: argument.to_owned(),
        });
    }
    Ok(())
}

fn render_argument(
    template: &str,
    harness_name: &str,
    task: &str,
    model: &str,
    tier: &str,
    run_id: &str,
) -> Result<String, ConfigError> {
    let mut rendered = String::with_capacity(template.len());
    let mut cursor = 0;

    while let Some(relative_start) = template[cursor..].find('{') {
        let start = cursor + relative_start;
        if template[cursor..start].contains('}') {
            return Err(ConfigError::UnknownHarnessPlaceholder {
                harness: harness_name.to_owned(),
                argument: template.to_owned(),
            });
        }
        rendered.push_str(&template[cursor..start]);
        let relative_end =
            template[start..]
                .find('}')
                .ok_or_else(|| ConfigError::UnknownHarnessPlaceholder {
                    harness: harness_name.to_owned(),
                    argument: template.to_owned(),
                })?;
        let end = start + relative_end + 1;
        let value = match &template[start..end] {
            "{task}" => task,
            "{model}" => model,
            "{tier}" => tier,
            "{run_id}" => run_id,
            _ => {
                return Err(ConfigError::UnknownHarnessPlaceholder {
                    harness: harness_name.to_owned(),
                    argument: template.to_owned(),
                });
            }
        };
        rendered.push_str(value);
        cursor = end;
    }

    if template[cursor..].contains('}') {
        return Err(ConfigError::UnknownHarnessPlaceholder {
            harness: harness_name.to_owned(),
            argument: template.to_owned(),
        });
    }
    rendered.push_str(&template[cursor..]);
    Ok(rendered)
}

/// A shell-free process invocation rendered from a harness template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessInvocation {
    pub program: String,
    pub args: Vec<String>,
    /// Concrete model selected for this harness execution.
    pub model: String,
    pub verification: Option<VerificationInvocation>,
}

/// A shell-free verification process rendered from a harness template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationInvocation {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid storage configuration: {0}")]
    InvalidStorage(&'static str),
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
    #[error("cache ttl_seconds must be greater than zero")]
    ZeroCacheTtl,
    #[error("cache ttl_seconds cannot exceed 604800, got {0}")]
    CacheTtlTooLarge(u64),
    #[error("cache max_entries must be greater than zero")]
    ZeroCacheEntries,
    #[error("cache max_entries cannot exceed 10000, got {0}")]
    CacheEntriesTooLarge(usize),
    #[error("harness names cannot be empty")]
    EmptyHarnessName,
    #[error("harness `{0}` command cannot be empty")]
    EmptyHarnessCommand(String),
    #[error("harness `{0}` verification command cannot be empty")]
    EmptyVerificationCommand(String),
    #[error("harness `{harness}` args must contain {placeholder}")]
    MissingHarnessPlaceholder {
        harness: String,
        placeholder: &'static str,
    },
    #[error("harness `{harness}` argument contains an unknown placeholder: {argument}")]
    UnknownHarnessPlaceholder { harness: String, argument: String },
    #[error("harness `{harness}` does not map tier `{tier}` to a model")]
    MissingHarnessModel { harness: String, tier: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_is_opt_in_and_round_trips_without_credentials() {
        let old = Config::default().to_toml().unwrap();
        assert!(!old.contains("[storage]"));
        assert!(Config::from_toml(&old).unwrap().storage.is_jsonl());
        for section in [
            "\n[storage]\nbackend = 'sqlite'\nurl = 'sqlite://.jevia/jevia.db'\n",
            "\n[storage]\nbackend = 'postgres'\nurl_env = 'JEVIA_DATABASE_URL'\nproject = 'my-project'\n",
        ] {
            let parsed = Config::from_toml(&format!("{old}{section}")).unwrap();
            assert_eq!(
                Config::from_toml(&parsed.to_toml().unwrap()).unwrap(),
                parsed
            );
        }
        for section in [
            "\n[storage]\nbackend = 'mysql'\n",
            "\n[storage]\nbackend = 'sqlite'\nurl = 'sqlite::memory:'\n",
            "\n[storage]\nbackend = 'sqlite'\nurl = 'sqlite://file?mode=memory'\n",
            "\n[storage]\nbackend = 'postgres'\nurl = 'postgres://user:password@localhost/db'\nproject = 'test'\n",
            "\n[storage]\nbackend = 'postgres'\nurl_env = 'postgres://user:password@localhost/db'\nproject = 'test'\n",
            "\n[storage]\nbackend = 'postgres'\nurl_env = 'JEVIA_DATABASE_URL'\nproject = ''\n",
        ] {
            assert!(Config::from_toml(&format!("{old}{section}")).is_err());
        }
    }

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

    #[test]
    fn rejects_invalid_cache_limits() {
        let mut config = Config::default();
        config.cache.ttl_seconds = 0;
        assert!(matches!(config.validate(), Err(ConfigError::ZeroCacheTtl)));

        config.cache.ttl_seconds = 900;
        config.cache.max_entries = 0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ZeroCacheEntries)
        ));
    }

    #[test]
    fn harness_invocation_substitutes_without_a_shell() {
        let harness = HarnessConfig {
            command: "agent".to_owned(),
            args: vec![
                "run".to_owned(),
                "--model={model}".to_owned(),
                "{task}".to_owned(),
                "--trace={run_id}".to_owned(),
            ],
            models: [("balanced".to_owned(), "provider/model".to_owned())]
                .into_iter()
                .collect(),
            verification: Some(VerificationConfig {
                command: "cargo".to_owned(),
                args: vec![
                    "test".to_owned(),
                    "--package={tier}".to_owned(),
                    "--trace={run_id}".to_owned(),
                ],
            }),
        };
        let extra = vec!["--verbose".to_owned()];

        let invocation = harness
            .invocation(
                "agent",
                "balanced",
                "fix {model}; echo unsafe",
                "run-1",
                &extra,
            )
            .expect("invocation renders");

        assert_eq!(invocation.program, "agent");
        assert_eq!(invocation.model, "provider/model");
        assert_eq!(
            invocation.args,
            [
                "run",
                "--model=provider/model",
                "fix {model}; echo unsafe",
                "--trace=run-1",
                "--verbose",
            ]
        );
        assert_eq!(
            invocation.verification,
            Some(VerificationInvocation {
                program: "cargo".to_owned(),
                args: vec![
                    "test".to_owned(),
                    "--package=balanced".to_owned(),
                    "--trace=run-1".to_owned(),
                ],
            })
        );
    }

    #[test]
    fn config_rejects_incomplete_harness_model_mappings() {
        let mut config = Config::default();
        config.harnesses.insert(
            "agent".to_owned(),
            HarnessConfig {
                command: "agent".to_owned(),
                args: vec![
                    "--model".to_owned(),
                    "{model}".to_owned(),
                    "{task}".to_owned(),
                ],
                models: [("fast".to_owned(), "small".to_owned())]
                    .into_iter()
                    .collect(),
                verification: None,
            },
        );

        assert!(matches!(
            config.validate(),
            Err(ConfigError::MissingHarnessModel { harness, tier })
                if harness == "agent" && tier == "balanced"
        ));
    }

    #[test]
    fn invocation_rejects_unknown_placeholders_without_panicking() {
        let harness = HarnessConfig {
            command: "agent".to_owned(),
            args: vec!["{unknown}".to_owned()],
            models: [("balanced".to_owned(), "provider/model".to_owned())]
                .into_iter()
                .collect(),
            verification: None,
        };

        assert!(matches!(
            harness.invocation("agent", "balanced", "task", "run-1", &[]),
            Err(ConfigError::UnknownHarnessPlaceholder { harness, argument })
                if harness == "agent" && argument == "{unknown}"
        ));
    }

    #[test]
    fn config_rejects_an_empty_verification_command() {
        let mut config = Config::default();
        config.harnesses.insert(
            "agent".to_owned(),
            HarnessConfig {
                command: "agent".to_owned(),
                args: vec!["{model}".to_owned(), "{task}".to_owned()],
                models: config
                    .tiers
                    .keys()
                    .map(|tier| (tier.clone(), format!("provider/{tier}")))
                    .collect(),
                verification: Some(VerificationConfig {
                    command: " ".to_owned(),
                    args: vec![],
                }),
            },
        );

        assert!(matches!(
            config.validate(),
            Err(ConfigError::EmptyVerificationCommand(harness)) if harness == "agent"
        ));
    }

    #[test]
    fn config_rejects_unknown_verification_placeholders() {
        let mut config = Config::default();
        config.harnesses.insert(
            "agent".to_owned(),
            HarnessConfig {
                command: "agent".to_owned(),
                args: vec!["{model}".to_owned(), "{task}".to_owned()],
                models: config
                    .tiers
                    .keys()
                    .map(|tier| (tier.clone(), format!("provider/{tier}")))
                    .collect(),
                verification: Some(VerificationConfig {
                    command: "cargo".to_owned(),
                    args: vec!["test".to_owned(), "{unknown}".to_owned()],
                }),
            },
        );

        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownHarnessPlaceholder { harness, argument })
                if harness == "agent" && argument == "{unknown}"
        ));
    }
}
