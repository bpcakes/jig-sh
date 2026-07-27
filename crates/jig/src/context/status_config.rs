use std::collections::HashSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{RepoConfig, RepoContext};

pub(crate) const DEFAULT_STATUS_PROVIDER_TIMEOUT_SECONDS: u64 = 30;
const MAX_STATUS_PROVIDER_TIMEOUT_SECONDS: u64 = 60 * 60;
const MAX_STATUS_PROVIDERS: usize = 32;
const MAX_STATUS_PROVIDER_ID_BYTES: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusConfig {
    #[serde(default)]
    pub(crate) providers: Vec<StatusProviderConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusProviderConfig {
    pub(crate) id: String,
    pub(crate) argv: Vec<String>,
    #[serde(default = "default_status_provider_timeout_seconds")]
    pub(crate) timeout_seconds: u64,
}

impl StatusConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.providers.len() > MAX_STATUS_PROVIDERS {
            bail!(
                "[status] configures {} providers; at most {MAX_STATUS_PROVIDERS} are supported",
                self.providers.len()
            );
        }

        let mut ids = HashSet::new();
        for provider in &self.providers {
            validate_provider_id(&provider.id)?;
            if !ids.insert(provider.id.as_str()) {
                bail!(
                    "Duplicate status provider id '{}' in [[status.providers]]",
                    provider.id
                );
            }
            if provider.argv.is_empty() {
                bail!(
                    "Status provider '{}' must configure a nonempty argv array",
                    provider.id
                );
            }
            if provider.argv[0].trim().is_empty() {
                bail!(
                    "Status provider '{}' argv[0] must name an executable",
                    provider.id
                );
            }
            if let Some(index) = provider
                .argv
                .iter()
                .position(|arg| arg.chars().any(char::is_control))
            {
                bail!(
                    "Status provider '{}' argv[{index}] must not contain control characters",
                    provider.id
                );
            }
            if provider.timeout_seconds == 0
                || provider.timeout_seconds > MAX_STATUS_PROVIDER_TIMEOUT_SECONDS
            {
                bail!(
                    "Status provider '{}' timeout_seconds must be between 1 and {MAX_STATUS_PROVIDER_TIMEOUT_SECONDS}",
                    provider.id
                );
            }
        }
        Ok(())
    }
}

impl RepoContext {
    pub(crate) fn status_providers(&self) -> &[StatusProviderConfig] {
        &self.config.status.providers
    }
}

pub(super) fn validate_runtime_config(config: &RepoConfig) -> Result<()> {
    config.work.validate()?;
    config.loop_config.validate()?;
    config.status.validate()
}

fn validate_provider_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.trim() != id
        || id.len() > MAX_STATUS_PROVIDER_ID_BYTES
        || id.chars().any(char::is_control)
    {
        bail!(
            "Status provider id must be 1 to {MAX_STATUS_PROVIDER_ID_BYTES} bytes, have no surrounding whitespace, and contain no control characters"
        );
    }
    Ok(())
}

const fn default_status_provider_timeout_seconds() -> u64 {
    DEFAULT_STATUS_PROVIDER_TIMEOUT_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_PREFIX: &str = r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
"#;

    #[test]
    fn loads_provider_with_default_timeout() {
        let config: RepoConfig = toml::from_str(&format!(
            r#"{CONFIG_PREFIX}
[[status.providers]]
id = "factorish.example"
argv = ["ruby", "scripts/status.rb", "--jig-v1"]
"#
        ))
        .unwrap();

        validate_runtime_config(&config).unwrap();
        assert_eq!(config.status.providers.len(), 1);
        assert_eq!(config.status.providers[0].id, "factorish.example");
        assert_eq!(
            config.status.providers[0].timeout_seconds,
            DEFAULT_STATUS_PROVIDER_TIMEOUT_SECONDS
        );
    }

    #[test]
    fn rejects_duplicate_ids_empty_argv_and_invalid_timeout() {
        for (body, expected) in [
            (
                r#"[[status.providers]]
id = "factorish.duplicate"
argv = ["first"]

[[status.providers]]
id = "factorish.duplicate"
argv = ["second"]
"#,
                "Duplicate status provider id",
            ),
            (
                r#"[[status.providers]]
id = "factorish.empty"
argv = []
"#,
                "nonempty argv",
            ),
            (
                r#"[[status.providers]]
id = "factorish.timeout"
argv = ["provider"]
timeout_seconds = 0
"#,
                "timeout_seconds must be between",
            ),
            (
                r#"[[status.providers]]
id = "factorish.control"
argv = ["provider", "line\nbreak"]
"#,
                "must not contain control characters",
            ),
        ] {
            let config: RepoConfig = toml::from_str(&format!("{CONFIG_PREFIX}\n{body}")).unwrap();
            let error = validate_runtime_config(&config).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }
}
