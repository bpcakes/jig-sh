use std::path::PathBuf;

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::context::{LoopConfig, LoopWorkflowConfig, RepoContext};

use super::schedule::ScheduleSpec;
pub(super) use super::workflow_state::{
    RepositoryRevisionState, UnexecutedReason, WorkflowExecution, WorkflowOutcome,
};

pub(super) const CODEX_TASK_KIND: &str = "codex_task";
pub(super) const DEFAULT_WORKFLOW_ID: &str = "noop-status";
pub(super) const GITHUB_PR_STATUS_KIND: &str = "github_pr_status";
pub(super) const NOOP_STATUS_KIND: &str = "noop_status";
pub(super) const PR_MANAGER_KIND: &str = "pr_manager";
const WORKFLOW_LEASE_PREFIX: &str = "workflow:";
const REPO_CHECKOUT_LEASE_KEY: &str = "checkout:repo";

pub(super) struct WorkflowTick {
    pub(super) observed: Value,
    pub(super) actions: Vec<Value>,
    pub(super) completion: WorkflowCompletion,
}

impl WorkflowTick {
    pub(super) fn from_actions(observed: Value, actions: Vec<Value>) -> Self {
        let completion = WorkflowCompletion::from_actions(&actions);
        Self {
            observed,
            actions,
            completion,
        }
    }

    pub(super) fn with_completion(
        observed: Value,
        actions: Vec<Value>,
        completion: WorkflowCompletion,
    ) -> Self {
        Self {
            observed,
            actions,
            completion,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct WorkflowCompletion {
    pub(super) outcome: WorkflowOutcome,
    pub(super) execution: WorkflowExecution,
    pub(super) repository_revision: RepositoryRevisionState,
    pub(super) worker_receipt_id: Option<String>,
    pub(super) worktree: Option<String>,
    pub(super) error: Option<String>,
}

impl WorkflowCompletion {
    pub(super) fn from_actions(actions: &[Value]) -> Self {
        let outcome = if actions.iter().any(action_requires_attention) {
            WorkflowOutcome::NeedsAttention
        } else if actions
            .iter()
            .any(|action| action_status(action) == Some("failed"))
        {
            WorkflowOutcome::Failed
        } else {
            WorkflowOutcome::Succeeded
        };
        let evidence = completion_evidence_action(actions, outcome);
        Self {
            outcome,
            execution: WorkflowExecution::Executed,
            repository_revision: RepositoryRevisionState::NotApplicable,
            worker_receipt_id: evidence
                .and_then(|action| action["worker_receipt_id"].as_str())
                .map(str::to_string),
            worktree: evidence.and_then(retained_worktree).map(str::to_string),
            error: completion_error(actions, outcome),
        }
    }
}

pub(super) fn loop_status_is_success(status: &str) -> bool {
    !matches!(status, "failed" | "needs_attention")
}

fn action_status(action: &Value) -> Option<&str> {
    action.get("status").and_then(Value::as_str)
}

fn action_is_exhausted_attempt(action: &Value) -> bool {
    action["kind"].as_str() == Some("pr_manager_worker")
        && action["attention_kind"].as_str() == Some("exhausted_attempt")
        && action["attempt"]["exhausted"].as_bool() == Some(true)
}

fn action_requires_attention(action: &Value) -> bool {
    if action_is_exhausted_attempt(action) {
        return false;
    }
    [action_status(action), action["completed_status"].as_str()]
        .into_iter()
        .flatten()
        .any(|status| matches!(status, "needs_attention" | "cancelled_after_commit"))
}

fn action_matches_outcome(action: &Value, outcome: WorkflowOutcome) -> bool {
    match outcome {
        WorkflowOutcome::Failed => action_status(action) == Some("failed"),
        WorkflowOutcome::NeedsAttention => action_requires_attention(action),
        WorkflowOutcome::Succeeded => {
            action_status(action) != Some("failed") && !action_requires_attention(action)
        }
    }
}

fn completion_evidence_action(actions: &[Value], outcome: WorkflowOutcome) -> Option<&Value> {
    actions
        .iter()
        .filter(|action| action_matches_outcome(action, outcome))
        .find(|action| action_has_completion_evidence(action))
        .or_else(|| {
            actions
                .iter()
                .find(|action| action_has_completion_evidence(action))
        })
}

fn completion_error(actions: &[Value], outcome: WorkflowOutcome) -> Option<String> {
    let mut errors = Vec::new();
    for action in actions
        .iter()
        .filter(|action| action_matches_outcome(action, outcome))
    {
        push_action_error(&mut errors, action);
    }
    if outcome == WorkflowOutcome::NeedsAttention {
        for action in actions.iter().filter(|action| {
            action_status(action) == Some("failed") && !action_requires_attention(action)
        }) {
            push_action_error(&mut errors, action);
        }
    }
    (!errors.is_empty()).then(|| errors.join("; additionally: "))
}

fn push_action_error(errors: &mut Vec<String>, action: &Value) {
    if let Some(error) = action["error"].as_str()
        && !errors.iter().any(|existing| existing == error)
    {
        errors.push(error.to_string());
    }
}

fn action_has_completion_evidence(action: &Value) -> bool {
    action["worker_receipt_id"].is_string() || retained_worktree(action).is_some()
}

fn retained_worktree(action: &Value) -> Option<&str> {
    let codex_worktree = (action["checkout"]["mode"].as_str() == Some("worktree")
        && action["checkout"]["retained"].as_bool() == Some(true))
    .then(|| action["checkout"]["path"].as_str())
    .flatten();
    codex_worktree.or_else(|| {
        let pr_attention = action["kind"].as_str() == Some("pr_manager_worker")
            && action_requires_attention(action);
        pr_attention.then(|| action["worktree"].as_str()).flatten()
    })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkflowRunPolicy {
    UntilIdle,
    SingleTick,
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
        match self.codex_task.as_ref().map(|task| task.checkout) {
            Some(CodexTaskCheckout::Repo) => REPO_CHECKOUT_LEASE_KEY.into(),
            _ => format!("{WORKFLOW_LEASE_PREFIX}{}", self.id),
        }
    }

    pub(super) fn run_policy(&self) -> WorkflowRunPolicy {
        if self.kind == CODEX_TASK_KIND {
            WorkflowRunPolicy::SingleTick
        } else {
            WorkflowRunPolicy::UntilIdle
        }
    }

    pub(super) fn blocks_on_retained_worktree(&self) -> bool {
        self.kind == PR_MANAGER_KIND
            || self
                .codex_task
                .as_ref()
                .is_some_and(|task| task.checkout == CodexTaskCheckout::Worktree)
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
    let codex_task = config_codex_task(workflow)?;
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

fn config_codex_task(workflow: &LoopWorkflowConfig) -> Result<Option<CodexTaskSettings>> {
    if workflow.kind != CODEX_TASK_KIND {
        return Ok(None);
    }
    let prompt_file = workflow.prompt_file.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "Loop workflow '{}' is missing required codex_task prompt_file",
            workflow.id
        )
    })?;
    let checkout = match workflow.checkout.as_deref().unwrap_or("worktree") {
        "repo" => CodexTaskCheckout::Repo,
        "worktree" => CodexTaskCheckout::Worktree,
        checkout => bail!(
            "Loop workflow '{}' has unsupported codex_task checkout '{checkout}'",
            workflow.id
        ),
    };
    Ok(Some(CodexTaskSettings {
        prompt_file,
        model: workflow.model.clone(),
        sandbox: workflow
            .sandbox
            .clone()
            .unwrap_or_else(|| "read-only".into()),
        checkout,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_completion_prefers_failed_action_with_receipt() {
        let completion = WorkflowCompletion::from_actions(&[
            json!({
                "status": "skipped",
                "worker_receipt_id": null,
            }),
            json!({
                "status": "failed",
                "worker_receipt_id": "receipt_failed",
                "error": "worker failed",
            }),
        ]);

        assert_eq!(
            completion,
            WorkflowCompletion {
                outcome: WorkflowOutcome::Failed,
                execution: WorkflowExecution::Executed,
                repository_revision: RepositoryRevisionState::NotApplicable,
                worker_receipt_id: Some("receipt_failed".into()),
                worktree: None,
                error: Some("worker failed".into()),
            }
        );
    }

    #[test]
    fn workflow_completion_classifies_post_commit_cancellation_as_attention() {
        let completion = WorkflowCompletion::from_actions(&[json!({
            "status": "cancelled_after_commit",
            "worker_receipt_id": "receipt_worker",
            "error": "follow-up review thread updates are incomplete",
        })]);

        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt_worker")
        );
        assert_eq!(
            completion.error.as_deref(),
            Some("follow-up review thread updates are incomplete")
        );
    }

    #[test]
    fn workflow_completion_preserves_exhausted_attempt_status_without_escalating_occurrence() {
        let completion = WorkflowCompletion::from_actions(&[json!({
            "kind": "pr_manager_worker",
            "status": "needs_attention",
            "attention_kind": "exhausted_attempt",
            "attempt": { "exhausted": true },
        })]);

        assert_eq!(completion.outcome, WorkflowOutcome::Succeeded);
    }

    #[test]
    fn workflow_completion_requires_complete_exhausted_attempt_metadata() {
        for action in [
            json!({
                "kind": "pr_manager_worker",
                "status": "needs_attention",
                "attention_kind": "exhausted_attempt",
            }),
            json!({
                "kind": "other",
                "status": "needs_attention",
                "attention_kind": "exhausted_attempt",
                "attempt": { "exhausted": true },
            }),
        ] {
            assert_eq!(
                WorkflowCompletion::from_actions(&[action]).outcome,
                WorkflowOutcome::NeedsAttention
            );
        }
    }

    #[test]
    fn workflow_completion_preserves_attention_through_lease_cleanup_failure() {
        let completion = WorkflowCompletion::from_actions(&[json!({
            "status": "failed",
            "completed_status": "cancelled_after_commit",
            "worker_receipt_id": "receipt_worker",
            "error": "lease cleanup failed after post-push cancellation",
        })]);

        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt_worker")
        );
        assert_eq!(
            completion.error.as_deref(),
            Some("lease cleanup failed after post-push cancellation")
        );
    }

    #[test]
    fn workflow_completion_keeps_post_commit_cancellation_evidence() {
        let completion = WorkflowCompletion::from_actions(&[
            json!({
                "status": "needs_attention",
            }),
            json!({
                "status": "cancelled_after_commit",
                "worker_receipt_id": "receipt_worker",
                "error": "follow-up review thread updates are incomplete",
            }),
        ]);

        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt_worker")
        );
        assert_eq!(
            completion.error.as_deref(),
            Some("follow-up review thread updates are incomplete")
        );
    }

    #[test]
    fn workflow_completion_keeps_failure_diagnostic_when_attention_dominates() {
        let completion = WorkflowCompletion::from_actions(&[
            json!({
                "status": "failed",
                "error": "first PR failed",
            }),
            json!({
                "status": "cancelled_after_commit",
                "error": "second PR push may have completed",
            }),
        ]);

        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.error.as_deref(),
            Some("second PR push may have completed; additionally: first PR failed")
        );
    }

    #[test]
    fn workflow_completion_keeps_receipt_from_later_successful_action() {
        let completion = WorkflowCompletion::from_actions(&[
            json!({
                "status": "needs_attention",
                "error": "attempt budget is exhausted",
            }),
            json!({
                "status": "succeeded",
                "worker_receipt_id": "receipt_worker",
            }),
        ]);

        assert_eq!(completion.outcome, WorkflowOutcome::NeedsAttention);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt_worker")
        );
        assert_eq!(
            completion.error.as_deref(),
            Some("attempt budget is exhausted")
        );
    }

    #[test]
    fn workflow_completion_keeps_workflow_evidence_but_not_unrelated_diagnostics() {
        let completion = WorkflowCompletion::from_actions(&[
            json!({
                "status": "failed",
                "error": "first PR failed",
            }),
            json!({
                "status": "succeeded",
                "worker_receipt_id": "receipt_second_pr",
                "checkout": {
                    "mode": "worktree",
                    "retained": true,
                    "path": "/tmp/second-pr",
                },
            }),
        ]);

        assert_eq!(completion.outcome, WorkflowOutcome::Failed);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt_second_pr")
        );
        assert_eq!(completion.worktree.as_deref(), Some("/tmp/second-pr"));
        assert_eq!(completion.error.as_deref(), Some("first PR failed"));
    }

    #[test]
    fn workflow_completion_takes_receipt_and_worktree_from_one_action() {
        let completion = WorkflowCompletion::from_actions(&[
            json!({
                "status": "failed",
                "error": "first PR failed",
            }),
            json!({
                "status": "succeeded",
                "worker_receipt_id": "receipt_second_pr",
            }),
            json!({
                "status": "succeeded",
                "checkout": {
                    "mode": "worktree",
                    "retained": true,
                    "path": "/tmp/third-pr",
                },
            }),
        ]);

        assert_eq!(completion.outcome, WorkflowOutcome::Failed);
        assert_eq!(
            completion.worker_receipt_id.as_deref(),
            Some("receipt_second_pr")
        );
        assert_eq!(completion.worktree, None);
        assert_eq!(completion.error.as_deref(), Some("first PR failed"));
    }

    #[test]
    fn needs_attention_is_not_a_successful_loop_status() {
        assert!(!loop_status_is_success("failed"));
        assert!(!loop_status_is_success("needs_attention"));
        assert!(loop_status_is_success("acted"));
        assert!(loop_status_is_success("idle"));
    }

    #[test]
    fn workflow_completion_does_not_report_repository_as_worktree() {
        let completion = WorkflowCompletion::from_actions(&[json!({
            "status": "succeeded",
            "checkout": {
                "mode": "repo",
                "path": "/repo",
                "retained": true,
            },
        })]);

        assert_eq!(completion.worktree, None);
    }

    #[test]
    fn repository_checkouts_share_one_execution_lease() {
        let first = workflow_with_checkout("first", CodexTaskCheckout::Repo);
        let second = workflow_with_checkout("second", CodexTaskCheckout::Repo);
        let isolated = workflow_with_checkout("isolated", CodexTaskCheckout::Worktree);

        assert_eq!(first.lease_key(), REPO_CHECKOUT_LEASE_KEY);
        assert_eq!(second.lease_key(), first.lease_key());
        assert_eq!(isolated.lease_key(), "workflow:isolated");
    }

    #[test]
    fn scheduled_codex_tasks_are_single_tick_workflows() {
        assert_eq!(
            workflow_with_checkout("scheduled", CodexTaskCheckout::Worktree).run_policy(),
            WorkflowRunPolicy::SingleTick
        );
    }

    #[test]
    fn isolated_tasks_and_pr_managers_block_on_retained_worktrees() {
        let isolated = workflow_with_checkout("isolated", CodexTaskCheckout::Worktree);
        let shared = workflow_with_checkout("shared", CodexTaskCheckout::Repo);
        let mut pr_manager = workflow_with_checkout("pr-manager", CodexTaskCheckout::Repo);
        pr_manager.kind = PR_MANAGER_KIND.into();
        pr_manager.codex_task = None;

        assert!(isolated.blocks_on_retained_worktree());
        assert!(pr_manager.blocks_on_retained_worktree());
        assert!(!shared.blocks_on_retained_worktree());
    }

    #[test]
    fn codex_task_conversion_reports_invalid_internal_config_without_panicking() {
        let mut workflow = raw_codex_task_config();
        workflow.prompt_file = None;
        assert!(
            config_codex_task(&workflow)
                .unwrap_err()
                .to_string()
                .contains("missing required codex_task prompt_file")
        );

        let mut workflow = raw_codex_task_config();
        workflow.checkout = Some("unsupported".into());
        assert!(
            config_codex_task(&workflow)
                .unwrap_err()
                .to_string()
                .contains("unsupported codex_task checkout")
        );
    }

    fn workflow_with_checkout(id: &str, checkout: CodexTaskCheckout) -> ResolvedWorkflow {
        ResolvedWorkflow {
            id: id.into(),
            kind: CODEX_TASK_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 3,
            backoff_seconds: 60,
            codex_home_configured: None,
            schedule: None,
            codex_task: Some(CodexTaskSettings {
                prompt_file: "task.md".into(),
                model: None,
                sandbox: "read-only".into(),
                checkout,
            }),
        }
    }

    fn raw_codex_task_config() -> LoopWorkflowConfig {
        LoopWorkflowConfig {
            id: "scheduled".into(),
            kind: CODEX_TASK_KIND.into(),
            enabled: true,
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
            codex_home: None,
            schedule: Some("* * * * *".into()),
            timezone: None,
            prompt_file: Some("task.md".into()),
            model: None,
            sandbox: None,
            checkout: None,
        }
    }
}
