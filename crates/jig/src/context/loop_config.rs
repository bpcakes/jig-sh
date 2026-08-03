use std::{collections::HashSet, path::PathBuf};

use anyhow::{Result, bail};
use serde::Deserialize;

const DEFAULT_LEASE_TTL_SECONDS: u64 = 15 * 60;
const DEFAULT_MAX_ATTEMPTS: u32 = 3;
const DEFAULT_BACKOFF_SECONDS: u64 = 5 * 60;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoopConfig {
    #[serde(default = "default_lease_ttl_seconds")]
    pub(crate) lease_ttl_seconds: u64,
    #[serde(default = "default_max_attempts")]
    pub(crate) max_attempts: u32,
    #[serde(default = "default_backoff_seconds")]
    pub(crate) backoff_seconds: u64,
    #[serde(default)]
    workflows: Vec<LoopWorkflowConfig>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            lease_ttl_seconds: default_lease_ttl_seconds(),
            max_attempts: default_max_attempts(),
            backoff_seconds: default_backoff_seconds(),
            workflows: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoopWorkflowConfig {
    pub(crate) id: String,
    pub(crate) kind: String,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) lease_ttl_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) max_attempts: Option<u32>,
    #[serde(default)]
    pub(crate) backoff_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) codex_home: Option<PathBuf>,
}

impl LoopConfig {
    pub(crate) fn workflows(&self) -> &[LoopWorkflowConfig] {
        &self.workflows
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.lease_ttl_seconds == 0 {
            bail!("[loop].lease_ttl_seconds must be greater than zero");
        }
        if self.max_attempts == 0 {
            bail!("[loop].max_attempts must be greater than zero");
        }
        if self.backoff_seconds == 0 {
            bail!("[loop].backoff_seconds must be greater than zero");
        }

        let mut ids = HashSet::new();
        for workflow in &self.workflows {
            if !ids.insert(workflow.id.as_str()) {
                bail!("Duplicate loop workflow id '{}'", workflow.id);
            }
            validate_loop_token("loop workflow id", &workflow.id)?;
            validate_workflow_kind(&workflow.kind)?;
            if let Some(codex_home) = &workflow.codex_home {
                if codex_home.as_os_str().is_empty() {
                    bail!(
                        "loop workflow '{}' codex_home must not be empty",
                        workflow.id
                    );
                }
                if workflow.kind != "pr_manager" {
                    bail!(
                        "loop workflow '{}' can set codex_home only when kind = 'pr_manager'",
                        workflow.id
                    );
                }
            }
            if workflow.lease_ttl_seconds == Some(0) {
                bail!(
                    "loop workflow '{}' lease_ttl_seconds must be greater than zero",
                    workflow.id
                );
            }
            if workflow.max_attempts == Some(0) {
                bail!(
                    "loop workflow '{}' max_attempts must be greater than zero",
                    workflow.id
                );
            }
            if workflow.backoff_seconds == Some(0) {
                bail!(
                    "loop workflow '{}' backoff_seconds must be greater than zero",
                    workflow.id
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_workflow_kind(kind: &str) -> Result<()> {
    match kind {
        "noop_status" | "github_pr_status" | "pr_manager" => Ok(()),
        _ => bail!(
            "Unsupported loop workflow kind '{kind}'. Supported kinds: noop_status, github_pr_status, pr_manager."
        ),
    }
}

fn validate_loop_token(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
    {
        bail!(
            "Unsupported {label} value '{value}'. Use only ASCII letters, numbers, '.', '_', '-', or '/'."
        );
    }
    Ok(())
}

const fn default_lease_ttl_seconds() -> u64 {
    DEFAULT_LEASE_TTL_SECONDS
}

const fn default_max_attempts() -> u32 {
    DEFAULT_MAX_ATTEMPTS
}

const fn default_backoff_seconds() -> u64 {
    DEFAULT_BACKOFF_SECONDS
}

const fn default_true() -> bool {
    true
}
