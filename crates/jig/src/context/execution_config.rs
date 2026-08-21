use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::RepoContext;

pub(crate) const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 30 * 60;
pub(crate) const MAX_COMMAND_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommandTimeout(Duration);

impl CommandTimeout {
    pub(crate) const fn from_seconds(seconds: u64) -> Option<Self> {
        if seconds == 0 || seconds > MAX_COMMAND_TIMEOUT_SECONDS {
            return None;
        }
        Some(Self(Duration::from_secs(seconds)))
    }

    pub(crate) const fn duration(self) -> Duration {
        self.0
    }

    pub(crate) const fn as_secs(self) -> u64 {
        self.0.as_secs()
    }

    pub(crate) fn deadline_from(self, started: std::time::Instant) -> std::time::Instant {
        started
            .checked_add(self.0)
            .expect("a validated command timeout fits in Instant")
    }
}

impl Serialize for CommandTimeout {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.as_secs())
    }
}

impl<'de> Deserialize<'de> for CommandTimeout {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds = u64::deserialize(deserializer)?;
        Self::from_seconds(seconds).ok_or_else(|| {
            de::Error::custom(format!(
                "[execution].command_timeout_seconds must be between 1 and {MAX_COMMAND_TIMEOUT_SECONDS}"
            ))
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionConfig {
    #[serde(
        default = "default_command_timeout",
        rename = "command_timeout_seconds"
    )]
    command_timeout: CommandTimeout,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            command_timeout: default_command_timeout(),
        }
    }
}

impl ExecutionConfig {
    pub(crate) const fn command_timeout(&self) -> CommandTimeout {
        self.command_timeout
    }
}

impl RepoContext {
    pub(crate) const fn command_timeout(&self) -> CommandTimeout {
        self.config.execution.command_timeout()
    }
}

const fn default_command_timeout() -> CommandTimeout {
    CommandTimeout(Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECONDS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_bounded_and_nonzero() {
        let config = ExecutionConfig::default();
        assert_eq!(config.command_timeout.as_secs(), 1_800);
    }

    #[test]
    fn timeout_deserialization_rejects_values_outside_the_supported_range() {
        for seconds in [0, MAX_COMMAND_TIMEOUT_SECONDS + 1] {
            let error =
                toml::from_str::<ExecutionConfig>(&format!("command_timeout_seconds = {seconds}"))
                    .unwrap_err()
                    .to_string();
            assert!(error.contains("must be between 1 and 86400"), "{error}");
        }
    }

    #[test]
    fn timeout_serializes_as_seconds() {
        let config = ExecutionConfig {
            command_timeout: CommandTimeout::from_seconds(321).unwrap(),
        };
        assert_eq!(
            toml::to_string(&config).unwrap().trim(),
            "command_timeout_seconds = 321"
        );
    }
}
