use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail};
use jig_contract::RunConclusion;
use serde_json::{Value, json};

use crate::command::WorkCheckRequest;
use crate::context::{RepoContext, WorkGate};
use crate::repository::{PlanRunRequest, RepositoryCatalog, plan_run, resolve_evidence_targets};
use crate::state::{ReceiptInput, current_worktree_fingerprint, now_ms, record_receipt};
use crate::tool_defs::tool;

use super::super::run_execution::{ExecuteCheckRunRequest, execute_check_run};
use super::super::tool_execution::{
    execute_manifest_tool_result_without_worktree_fingerprint, manifest_tool_result_failure,
};
use super::tools::{selected_tools, validate_check_tool};

pub(super) fn check(ctx: &RepoContext, opts: WorkCheckRequest) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    if opts.tools.is_empty() {
        check_configured(ctx, &opts.plan_id, CheckFailureMode::ReportError)
    } else {
        check_tools(ctx, &opts.plan_id, selected_tools(ctx, &opts.tools)?)
    }
}

pub(super) fn check_tools(ctx: &RepoContext, plan_id: &str, tools: Vec<String>) -> Result<Value> {
    run_check_tools(ctx, plan_id, tools, CheckFailureMode::ReportError)?.require_success()
}

pub(super) fn check_configured_collect_failures(ctx: &RepoContext, plan_id: &str) -> Result<Value> {
    check_configured(ctx, plan_id, CheckFailureMode::Collect)
}

#[derive(Clone, Copy)]
enum CheckFailureMode {
    ReportError,
    Collect,
}

impl CheckFailureMode {
    const fn continues_after_failure(self) -> bool {
        matches!(self, Self::Collect)
    }
}

fn check_configured(
    ctx: &RepoContext,
    plan_id: &str,
    failure_mode: CheckFailureMode,
) -> Result<Value> {
    let tools = ctx.work_check_tools();
    let catalog = RepositoryCatalog::from_context(ctx)?;
    let targets = configured_evidence_targets(ctx, &catalog)?;
    if tools.is_empty() && targets.is_empty() {
        bail!(
            "No work check gates configured. Add check or evidence work.gates to .jig.toml, or pass --tool."
        );
    }

    let check_batch = if tools.is_empty() {
        CheckBatch {
            result: json!({
                "ok": true,
                "plan_id": plan_id,
                "checks": [],
                "receipt_id": null,
            }),
            failures: Vec::new(),
        }
    } else {
        run_check_tools(ctx, plan_id, tools, failure_mode)?
    };

    if targets.is_empty() {
        return match failure_mode {
            CheckFailureMode::ReportError => check_batch.require_success(),
            CheckFailureMode::Collect => Ok(check_batch.result),
        };
    }
    let CheckBatch {
        mut result,
        failures: legacy_failures,
    } = check_batch;
    let plan = plan_run(
        ctx,
        &catalog,
        PlanRunRequest {
            selectors: targets.iter().map(ToString::to_string).collect(),
            profile: None,
            affected_base: None,
        },
    )?;
    let execution = execute_check_run(
        ctx,
        &catalog,
        plan.clone(),
        ExecuteCheckRunRequest {
            work_plan_id: Some(plan_id.to_owned()),
            record_receipts: true,
            fail_fast: false,
        },
        &|| false,
    )?;
    let evidence_ok = execution.run.result.conclusion == Some(RunConclusion::Success);
    let failed_target_labels = execution
        .failed_targets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let object = result
        .as_object_mut()
        .ok_or_else(|| anyhow!("work check result was not a JSON object"))?;
    let legacy_ok = legacy_failures.is_empty();
    object.insert("ok".into(), json!(legacy_ok && evidence_ok));
    object.insert("plan".into(), json!(plan));
    object.insert("run".into(), json!(execution.run.result));
    object.insert("results".into(), json!(execution.results));
    object.insert("failed_targets".into(), json!(execution.failed_targets));

    if matches!(failure_mode, CheckFailureMode::ReportError) {
        let legacy_failure = failure_message(&legacy_failures);
        match (legacy_failure, evidence_ok) {
            (Some(failure), false) => bail!(
                "{}\nWork evidence targets also failed: [{failed_target_labels}]",
                failure
            ),
            (Some(failure), true) => bail!("{failure}"),
            (None, false) => bail!("Work evidence targets failed: [{failed_target_labels}]"),
            (None, true) => {}
        }
    }
    Ok(result)
}

fn configured_evidence_targets(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
) -> Result<BTreeSet<jig_contract::TargetId>> {
    let mut targets = BTreeSet::new();
    for gate in ctx.work_gates() {
        let WorkGate::Evidence(gate) = gate else {
            continue;
        };
        targets.extend(resolve_evidence_targets(catalog, &gate.selector)?);
    }
    Ok(targets)
}

struct CheckFailure {
    exit_status: i32,
    message: String,
}

struct CheckBatch {
    result: Value,
    failures: Vec<CheckFailure>,
}

impl CheckBatch {
    fn require_success(self) -> Result<Value> {
        if let Some(message) = failure_message(&self.failures) {
            bail!("{message}");
        }
        Ok(self.result)
    }
}

fn failure_message(failures: &[CheckFailure]) -> Option<String> {
    (!failures.is_empty()).then(|| {
        failures
            .iter()
            .map(|failure| failure.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn run_check_tools(
    ctx: &RepoContext,
    plan_id: &str,
    tools: Vec<String>,
    failure_mode: CheckFailureMode,
) -> Result<CheckBatch> {
    let started = now_ms();
    let before_fingerprint = current_worktree_fingerprint(ctx);
    for name in &tools {
        validate_check_tool(ctx, name, "Work check")?;
    }

    let mut results = Vec::with_capacity(tools.len());
    let mut check_failures = Vec::<CheckFailure>::new();
    for name in &tools {
        let execution = execute_manifest_tool_result_without_worktree_fingerprint(
            ctx,
            name,
            json!({}),
            Some(plan_id.to_string()),
        );
        match execution {
            Ok(result) => {
                let result_failure = manifest_tool_result_failure(&result)?;
                results.push(result);
                if let Some((exit_status, message)) = result_failure {
                    check_failures.push(CheckFailure {
                        exit_status,
                        message,
                    });
                }
            }
            Err(error) => check_failures.push(CheckFailure {
                exit_status: 1,
                message: format!("{name} could not execute:\n{error:#}"),
            }),
        }
        if !check_failures.is_empty() && !failure_mode.continues_after_failure() {
            break;
        }
    }
    let receipt_ids = results
        .iter()
        .filter_map(|result| result["receipt_id"].as_str())
        .collect::<Vec<_>>();
    let after_fingerprint = current_worktree_fingerprint(ctx);
    let worktree_fingerprint_override =
        work_check_fingerprint_evidence(&before_fingerprint, &after_fingerprint);
    let receipt_result = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": plan_id,
                "tools": tools,
                "receipt_ids": receipt_ids,
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms: started,
            ended_at_ms: now_ms(),
            exit_status: check_failures
                .iter()
                .map(|failure| failure.exit_status)
                .max()
                .unwrap_or(0),
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(worktree_fingerprint_override),
        },
    );

    let receipt_id = match receipt_result {
        Ok(receipt_id) => receipt_id,
        Err(receipt_error) => {
            if let Some(check_error) = failure_message(&check_failures) {
                bail!(
                    "{}\nwork check batch receipt recording also failed:\n{receipt_error:#}",
                    check_error
                );
            }
            return Err(receipt_error);
        }
    };

    Ok(CheckBatch {
        result: json!({
            "ok": check_failures.is_empty(),
            "plan_id": plan_id,
            "checks": results,
            "errors": check_failures.iter().map(|failure| &failure.message).collect::<Vec<_>>(),
            "receipt_id": receipt_id,
        }),
        failures: check_failures,
    })
}

fn work_check_fingerprint_evidence(
    before: &crate::state::CurrentWorktreeFingerprint,
    after: &crate::state::CurrentWorktreeFingerprint,
) -> std::result::Result<String, String> {
    let before = before
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("before work check", before.error.as_deref()))?;
    let after = after
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("after work check", after.error.as_deref()))?;

    if before == after {
        Ok(after.to_string())
    } else {
        Err(format!(
            "worktree changed during work check; before fingerprint {before}, after fingerprint {after}; rerun work check after generated changes settle"
        ))
    }
}

fn fingerprint_error(stage: &str, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("Failed to collect worktree fingerprint {stage}: {error}"),
        None => format!("Failed to collect worktree fingerprint {stage}"),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn collect_mode_runs_and_reports_every_failed_check() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(
                r#"
[commands]
first_check_command = "printf first > first-ran.txt; exit 3"
second_check_command = "printf second > second-ran.txt; exit 5"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.first_check"

[[work.gates]]
id = "second"
kind = "check"
tool = "jig.second_check"
"#,
            )
            .required_commands(["first_check_command", "second_check_command"])
            .tools([
                json!({
                    "name": "jig.first_check",
                    "kind": "command",
                    "description": "First check.",
                    "command": "first_check_command"
                }),
                json!({
                    "name": "jig.second_check",
                    "kind": "command",
                    "description": "Second check.",
                    "command": "second_check_command"
                }),
            ])
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Test", "# Test\n").unwrap();

        let result = check_configured_collect_failures(&ctx, "plan_1").unwrap();

        assert_eq!(result["ok"], false);
        assert_eq!(result["checks"].as_array().unwrap().len(), 2);
        assert_eq!(result["errors"].as_array().unwrap().len(), 2);
        assert!(temp.path().join("first-ran.txt").exists());
        assert!(temp.path().join("second-ran.txt").exists());
    }
}
