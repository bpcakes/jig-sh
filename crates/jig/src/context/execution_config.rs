use std::time::Duration;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{RepoConfig, RepoContext};

pub(crate) const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 30 * 60;
const MAX_COMMAND_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionConfig {
    #[serde(default = "default_command_timeout_seconds")]
    pub(crate) command_timeout_seconds: u64,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            command_timeout_seconds: DEFAULT_COMMAND_TIMEOUT_SECONDS,
        }
    }
}

impl ExecutionConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.command_timeout_seconds == 0
            || self.command_timeout_seconds > MAX_COMMAND_TIMEOUT_SECONDS
        {
            bail!(
                "[execution].command_timeout_seconds must be between 1 and {MAX_COMMAND_TIMEOUT_SECONDS}"
            );
        }
        Ok(())
    }
}

impl RepoContext {
    pub(crate) fn command_timeout(&self) -> Duration {
        Duration::from_secs(self.config.execution.command_timeout_seconds)
    }
}

pub(super) fn validate_runtime_config(config: &RepoConfig) -> Result<()> {
    config.execution.validate()
}

const fn default_command_timeout_seconds() -> u64 {
    DEFAULT_COMMAND_TIMEOUT_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_bounded_and_nonzero() {
        let config = ExecutionConfig::default();
        config.validate().unwrap();
        assert_eq!(config.command_timeout_seconds, 1_800);
    }
}
