use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

use anyhow::{Result, bail};
use chrono::{Datelike, NaiveDate};
use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};
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
    #[serde(default)]
    pub(crate) schedule: Option<String>,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    #[serde(default)]
    pub(crate) prompt_file: Option<PathBuf>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) sandbox: Option<String>,
    #[serde(default)]
    pub(crate) checkout: Option<String>,
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
                if !matches!(workflow.kind.as_str(), "pr_manager" | "codex_task") {
                    bail!(
                        "loop workflow '{}' can set codex_home only when kind is 'pr_manager' or 'codex_task'",
                        workflow.id
                    );
                }
            }
            validate_schedule_fields(workflow)?;
            validate_codex_task_fields(workflow)?;
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
        "codex_task" | "noop_status" | "github_pr_status" | "pr_manager" => Ok(()),
        _ => bail!(
            "Unsupported loop workflow kind '{kind}'. Supported kinds: codex_task, noop_status, github_pr_status, pr_manager."
        ),
    }
}

fn validate_schedule_fields(workflow: &LoopWorkflowConfig) -> Result<()> {
    let Some(schedule) = workflow.schedule.as_deref() else {
        if workflow.timezone.is_some() {
            bail!(
                "loop workflow '{}' can set timezone only when schedule is configured",
                workflow.id
            );
        }
        return Ok(());
    };
    if schedule.trim().is_empty() {
        bail!("loop workflow '{}' schedule must not be empty", workflow.id);
    }
    parse_five_field_cron(schedule)
        .map_err(|error| anyhow::anyhow!("loop workflow '{}' has {error}", workflow.id))?;
    let timezone = workflow.timezone.as_deref().unwrap_or("UTC");
    timezone.parse::<chrono_tz::Tz>().map_err(|_| {
        anyhow::anyhow!(
            "loop workflow '{}' has invalid IANA timezone '{}'",
            workflow.id,
            timezone
        )
    })?;
    Ok(())
}

pub(crate) fn parse_five_field_cron(expression: &str) -> Result<Cron> {
    let cron = CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expression)
        .map_err(|error| {
            anyhow::anyhow!("invalid five-field cron schedule '{expression}': {error}")
        })?;
    if !cron_has_calendar_occurrence(&cron)? {
        bail!("five-field cron schedule '{expression}' has no possible calendar occurrence");
    }
    Ok(cron)
}

fn cron_has_calendar_occurrence(cron: &Cron) -> Result<bool> {
    let mut date =
        NaiveDate::from_ymd_opt(2_000, 1, 1).expect("the Gregorian cycle start is a valid date");
    let end =
        NaiveDate::from_ymd_opt(2_400, 1, 1).expect("the Gregorian cycle end is a valid date");
    while date < end {
        if cron
            .pattern
            .month_match(date.month())
            .map_err(|error| anyhow::anyhow!("failed to validate cron month: {error}"))?
            && cron
                .pattern
                .day_match(date.year(), date.month(), date.day())
                .map_err(|error| anyhow::anyhow!("failed to validate cron day: {error}"))?
        {
            return Ok(true);
        }
        date = date
            .succ_opt()
            .expect("the bounded Gregorian cycle has a successor");
    }
    Ok(false)
}

fn validate_codex_task_fields(workflow: &LoopWorkflowConfig) -> Result<()> {
    if workflow.kind == "codex_task" {
        if workflow.schedule.is_none() {
            bail!(
                "loop workflow '{}' kind 'codex_task' requires schedule",
                workflow.id
            );
        }
        let Some(prompt_file) = workflow.prompt_file.as_deref() else {
            bail!(
                "loop workflow '{}' kind 'codex_task' requires prompt_file",
                workflow.id
            );
        };
        validate_repo_relative_path("prompt_file", &workflow.id, prompt_file)?;
        if workflow
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty() || model.chars().any(char::is_control))
        {
            bail!(
                "loop workflow '{}' model must be non-empty and contain no control characters",
                workflow.id
            );
        }
        if let Some(sandbox) = workflow.sandbox.as_deref()
            && !matches!(sandbox, "read-only" | "workspace-write")
        {
            bail!(
                "loop workflow '{}' sandbox must be 'read-only' or 'workspace-write'",
                workflow.id
            );
        }
        if let Some(checkout) = workflow.checkout.as_deref()
            && !matches!(checkout, "repo" | "worktree")
        {
            bail!(
                "loop workflow '{}' checkout must be 'repo' or 'worktree'",
                workflow.id
            );
        }
        return Ok(());
    }

    for (name, configured) in [
        ("prompt_file", workflow.prompt_file.is_some()),
        ("model", workflow.model.is_some()),
        ("sandbox", workflow.sandbox.is_some()),
        ("checkout", workflow.checkout.is_some()),
    ] {
        if configured {
            bail!(
                "loop workflow '{}' can set {name} only when kind = 'codex_task'",
                workflow.id
            );
        }
    }
    Ok(())
}

fn validate_repo_relative_path(label: &str, workflow_id: &str, path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "loop workflow '{workflow_id}' {label} must be a non-empty repository-relative path without '..'"
        );
    }
    Ok(())
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
