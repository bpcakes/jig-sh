use std::path::PathBuf;

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::context::{LoopConfig, LoopWorkflowConfig, RepoContext};

use super::schedule::ScheduleSpec;

pub(super) const CODEX_TASK_KIND: &str = "codex_task";
pub(super) const DEFAULT_WORKFLOW_ID: &str = "noop-status";
pub(super) const GITHUB_PR_STATUS_KIND: &str = "github_pr_status";
pub(super) const NOOP_STATUS_KIND: &str = "noop_status";
pub(super) const PR_MANAGER_KIND: &str = "pr_manager";
const WORKFLOW_LEASE_PREFIX: &str = "workflow:";

pub(super) struct WorkflowTick {
    pub(super) observed: Value,
    pub(super) actions: Vec<Value>,
}

#[derive(Clone, Copy)]
pub(super) struct TuningOverrides {
    pub(super) lease_ttl_seconds: Option<u64>,
    pub(super) max_attempts: Option<u32>,
    pub(super) backoff_seconds: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedWorkflow {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) enabled: bool,
    pub(super) configured: bool,
    pub(super) lease_ttl_seconds: u64,
    pub(super) max_attempts: u32,
    pub(super) backoff_seconds: u64,
    pub(super) codex_home_configured: Option<PathBuf>,
    pub(super) schedule: Option<ScheduleSpec>,
    pub(super) codex_task: Option<CodexTaskSettings>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CodexTaskCheckout {
    Repo,
    Worktree,
}

impl CodexTaskCheckout {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Worktree => "worktree",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct CodexTaskSettings {
    pub(super) prompt_file: PathBuf,
    pub(super) model: Option<String>,
    pub(super) sandbox: String,
    pub(super) checkout: CodexTaskCheckout,
}

impl ResolvedWorkflow {
    pub(super) fn lease_key(&self) -> String {
        format!("{WORKFLOW_LEASE_PREFIX}{}", self.id)
    }

    pub(super) fn value(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind,
            "enabled": self.enabled,
            "configured": self.configured,
            "lease_ttl_seconds": self.lease_ttl_seconds,
            "max_attempts": self.max_attempts,
            "backoff_seconds": self.backoff_seconds,
            "codex_home_configured": self
                .codex_home_configured
                .as_ref()
                .map(|home| home.display().to_string()),
            "schedule": self.schedule.as_ref().map(|schedule| json!({
                "cron": schedule.expression(),
                "timezone": schedule.timezone_name(),
            })),
            "codex_task": self.codex_task.as_ref().map(|task| json!({
                "prompt_file": task.prompt_file.display().to_string(),
                "model": task.model,
                "sandbox": task.sandbox,
                "checkout": task.checkout.as_str(),
            })),
        })
    }
}

pub(super) fn resolve_workflow(
    ctx: &RepoContext,
    requested: Option<&str>,
    overrides: TuningOverrides,
) -> Result<ResolvedWorkflow> {
    let workflow_id = requested.unwrap_or(DEFAULT_WORKFLOW_ID);
    if let Some(config) = ctx
        .loop_workflows()
        .iter()
        .find(|workflow| workflow.id == workflow_id)
    {
        return workflow_from_config(ctx.loop_config(), config, overrides);
    }

    if matches!(workflow_id, DEFAULT_WORKFLOW_ID | NOOP_STATUS_KIND) {
        return default_workflow(ctx.loop_config(), overrides);
    }

    bail!("Loop workflow not found: {workflow_id}")
}

pub(super) fn list_workflows(ctx: &RepoContext) -> Result<Vec<ResolvedWorkflow>> {
    let mut workflows = ctx
        .loop_workflows()
        .iter()
        .map(|workflow| {
            workflow_from_config(
                ctx.loop_config(),
                workflow,
                TuningOverrides {
                    lease_ttl_seconds: None,
                    max_attempts: None,
                    backoff_seconds: None,
                },
            )
        })
        .collect::<Result<Vec<_>>>()?;

    if !workflows
        .iter()
        .any(|workflow| workflow.id == DEFAULT_WORKFLOW_ID)
    {
        workflows.push(default_workflow(
            ctx.loop_config(),
            TuningOverrides {
                lease_ttl_seconds: None,
                max_attempts: None,
                backoff_seconds: None,
            },
        )?);
    }
    Ok(workflows)
}

fn workflow_from_config(
    loop_config: &LoopConfig,
    workflow: &LoopWorkflowConfig,
    overrides: TuningOverrides,
) -> Result<ResolvedWorkflow> {
    let lease_ttl_seconds = overrides
        .lease_ttl_seconds
        .or(workflow.lease_ttl_seconds)
        .unwrap_or(loop_config.lease_ttl_seconds);
    let max_attempts = overrides
        .max_attempts
        .or(workflow.max_attempts)
        .unwrap_or(loop_config.max_attempts);
    let backoff_seconds = overrides
        .backoff_seconds
        .or(workflow.backoff_seconds)
        .unwrap_or(loop_config.backoff_seconds);
    validate_tuning(lease_ttl_seconds, max_attempts, backoff_seconds)?;

    let schedule = config_schedule(workflow)?;
    let codex_task = config_codex_task(workflow);
    Ok(ResolvedWorkflow {
        id: workflow.id.clone(),
        kind: workflow.kind.clone(),
        enabled: workflow.enabled,
        configured: true,
        lease_ttl_seconds,
        max_attempts,
        backoff_seconds,
        codex_home_configured: workflow.codex_home.clone(),
        schedule,
        codex_task,
    })
}

fn default_workflow(
    loop_config: &LoopConfig,
    overrides: TuningOverrides,
) -> Result<ResolvedWorkflow> {
    let lease_ttl_seconds = overrides
        .lease_ttl_seconds
        .unwrap_or(loop_config.lease_ttl_seconds);
    let max_attempts = overrides.max_attempts.unwrap_or(loop_config.max_attempts);
    let backoff_seconds = overrides
        .backoff_seconds
        .unwrap_or(loop_config.backoff_seconds);

    validate_tuning(lease_ttl_seconds, max_attempts, backoff_seconds)?;

    Ok(ResolvedWorkflow {
        id: DEFAULT_WORKFLOW_ID.into(),
        kind: NOOP_STATUS_KIND.into(),
        enabled: true,
        configured: false,
        lease_ttl_seconds,
        max_attempts,
        backoff_seconds,
        codex_home_configured: None,
        schedule: None,
        codex_task: None,
    })
}

fn config_schedule(workflow: &LoopWorkflowConfig) -> Result<Option<ScheduleSpec>> {
    workflow
        .schedule
        .as_deref()
        .map(|schedule| ScheduleSpec::parse(schedule, workflow.timezone.as_deref()))
        .transpose()
}

fn config_codex_task(workflow: &LoopWorkflowConfig) -> Option<CodexTaskSettings> {
    (workflow.kind == CODEX_TASK_KIND).then(|| CodexTaskSettings {
        prompt_file: workflow
            .prompt_file
            .clone()
            .expect("validated codex_task config has a prompt_file"),
        model: workflow.model.clone(),
        sandbox: workflow
            .sandbox
            .clone()
            .unwrap_or_else(|| "read-only".into()),
        checkout: match workflow.checkout.as_deref().unwrap_or("worktree") {
            "repo" => CodexTaskCheckout::Repo,
            "worktree" => CodexTaskCheckout::Worktree,
            _ => unreachable!("validated codex_task config has a supported checkout"),
        },
    })
}

fn validate_tuning(lease_ttl_seconds: u64, max_attempts: u32, backoff_seconds: u64) -> Result<()> {
    if lease_ttl_seconds == 0 {
        bail!("lease_ttl_seconds must be greater than zero");
    }
    if max_attempts == 0 {
        bail!("max_attempts must be greater than zero");
    }
    if backoff_seconds == 0 {
        bail!("backoff_seconds must be greater than zero");
    }
    Ok(())
}
