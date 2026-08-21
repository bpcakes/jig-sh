use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use jig_contract::{
    ActionRunner, Finding, FindingSeverity, PlannedTarget, ResultParser, RunConclusion, RunPlan,
    RunStatus, TargetId, TargetRunResult,
};
use jig_owned_process::{
    OwnedProcessTreeError, ProcessOutputLimits, run_owned_process_tree_with_output_limits,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::repository::RepositoryCatalog;
use crate::state::{
    ReceiptInput, TargetReceiptMetadata, complete_run, mark_run_running, mark_target_started,
    now_ms, record_target_receipt, record_target_result, run_by_id, start_run,
};

use super::tool_execution::run_native_tool;

const DEFAULT_TARGET_TIMEOUT_SECONDS: u64 = 30 * 60;
const GENERIC_TARGET_TOOL: &str = "jig.target_run";

#[derive(Debug, Serialize)]
pub(in crate::runtime) struct CheckRunExecution {
    pub(in crate::runtime) run: crate::state::DurableRun,
    pub(in crate::runtime) results: Vec<Value>,
    pub(in crate::runtime) failed_targets: Vec<TargetId>,
}

pub(in crate::runtime) struct ExecuteCheckRunRequest {
    pub(in crate::runtime) work_plan_id: Option<String>,
    pub(in crate::runtime) record_receipts: bool,
    pub(in crate::runtime) fail_fast: bool,
}

pub(in crate::runtime) fn execute_check_run(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: RunPlan,
    request: ExecuteCheckRunRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<CheckRunExecution> {
    crate::repository::validate_run_plan(ctx, catalog, &plan)?;
    let run = start_run(ctx, plan, request.work_plan_id.clone())?;
    let run_id = run.result.run_id.clone();
    mark_run_running(ctx, &run_id)?;

    let mut conclusions = BTreeMap::<TargetId, RunConclusion>::new();
    let mut compatibility_results = Vec::new();
    let mut failed_targets = Vec::new();
    let mut stop_after_failure = false;
    let finisher = TargetFinisher {
        ctx,
        catalog,
        run: &run,
        work_plan_id: request.work_plan_id.as_deref(),
        record_receipts: request.record_receipts,
    };

    for layer in &run.plan.execution_layers {
        for target_id in layer {
            let planned = planned_target(&run.plan, target_id)?;
            let dependency_failed = planned.depends_on.iter().any(|dependency| {
                conclusions
                    .get(dependency)
                    .is_some_and(|conclusion| *conclusion != RunConclusion::Success)
            });
            let skip_reason = if cancelled() {
                Some((
                    RunConclusion::Cancelled,
                    "run cancellation was requested before the target started".to_owned(),
                ))
            } else if dependency_failed {
                Some((
                    RunConclusion::Skipped,
                    "a declared target dependency did not succeed".to_owned(),
                ))
            } else if stop_after_failure {
                Some((
                    RunConclusion::Skipped,
                    "the run stopped after an earlier failure because fail-fast was requested"
                        .to_owned(),
                ))
            } else {
                None
            };

            let (result, compatibility) = if let Some((conclusion, reason)) = skip_reason {
                let capture = TargetCapture::not_started(conclusion, reason);
                finisher.finish(planned, None, capture)?
            } else {
                mark_target_started(ctx, &run_id, target_id.clone())?;
                let started_at_ms = now_ms();
                let capture = run_target(ctx, catalog, planned, cancelled);
                finisher.finish(planned, Some(started_at_ms), capture)?
            };

            let conclusion = result
                .conclusion
                .expect("finished target results always have a conclusion");
            conclusions.insert(target_id.clone(), conclusion);
            if matches!(
                conclusion,
                RunConclusion::Failure | RunConclusion::TimedOut | RunConclusion::Blocked
            ) {
                failed_targets.push(target_id.clone());
                stop_after_failure |= request.fail_fast;
            }
            record_target_result(ctx, &run_id, result)?;
            if let Some(compatibility) = compatibility {
                compatibility_results.push(compatibility);
            }
        }
    }

    let conclusion = aggregate_conclusion(conclusions.values().copied());
    complete_run(ctx, &run_id, conclusion)?;
    Ok(CheckRunExecution {
        run: run_by_id(ctx, &run_id)?,
        results: compatibility_results,
        failed_targets,
    })
}

fn planned_target<'a>(plan: &'a RunPlan, target: &TargetId) -> Result<&'a PlannedTarget> {
    plan.targets
        .iter()
        .find(|planned| &planned.target == target)
        .ok_or_else(|| anyhow::anyhow!("run plan references missing target '{target}'"))
}

fn run_target(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    planned: &PlannedTarget,
    cancelled: &dyn Fn() -> bool,
) -> TargetCapture {
    match &planned.runner {
        ActionRunner::Command {
            command,
            working_directory,
            environment,
        } => run_command_target(
            ctx,
            planned,
            command,
            working_directory.as_deref(),
            environment,
            cancelled,
        ),
        ActionRunner::Native { operation } => {
            if cancelled() {
                return TargetCapture::not_started(
                    RunConclusion::Cancelled,
                    "run cancellation was requested before the native operation started",
                );
            }
            match run_native_tool(ctx, operation, &json!({})) {
                Ok(output) => TargetCapture::from_process(
                    output.exit_status,
                    output.stdout,
                    output.stderr,
                    planned.result_parser,
                ),
                Err(error) => TargetCapture::blocked(format!(
                    "native runner '{operation}' for target '{}' could not start: {error:#}",
                    planned.target
                )),
            }
        }
    }
    .with_alias(catalog.aliases_for_target(&planned.target).first().cloned())
}

fn run_command_target(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    command_key: &str,
    working_directory: Option<&str>,
    environment: &BTreeMap<String, String>,
    cancelled: &dyn Fn() -> bool,
) -> TargetCapture {
    let command_text = match ctx.command_for_key(command_key) {
        Ok(command) => command,
        Err(error) => {
            return TargetCapture::blocked(format!(
                "command runner '{command_key}' for target '{}' is unavailable: {error:#}",
                planned.target
            ))
            .with_command_key(command_key);
        }
    };
    let working_directory = match resolve_working_directory(ctx.root(), working_directory) {
        Ok(path) => path,
        Err(error) => {
            return TargetCapture::blocked(format!(
                "target '{}' has an invalid working directory: {error:#}",
                planned.target
            ))
            .with_command_key(command_key);
        }
    };
    if let Err(error) = validate_environment(environment) {
        return TargetCapture::blocked(format!(
            "target '{}' has an invalid runner environment: {error:#}",
            planned.target
        ))
        .with_command_key(command_key);
    }

    let mut command = Command::new("bash");
    command
        .current_dir(working_directory)
        .arg("-c")
        .arg(command_text)
        .envs(environment);
    let timeout = Duration::from_secs(
        planned
            .timeout_seconds
            .unwrap_or(DEFAULT_TARGET_TIMEOUT_SECONDS)
            .max(1),
    );
    match run_owned_process_tree_with_output_limits(
        &mut command,
        timeout,
        ProcessOutputLimits::default(),
        cancelled,
    ) {
        Ok(output) => TargetCapture::from_process(
            output.status.code().unwrap_or(1),
            bounded_output(output.stdout),
            bounded_output(output.stderr),
            planned.result_parser,
        )
        .with_command_key(command_key),
        Err(OwnedProcessTreeError::TimedOut) => TargetCapture::not_started(
            RunConclusion::TimedOut,
            format!(
                "target '{}' exceeded its {timeout:?} timeout",
                planned.target
            ),
        )
        .with_command_key(command_key),
        Err(OwnedProcessTreeError::Cancelled) => TargetCapture::not_started(
            RunConclusion::Cancelled,
            format!("target '{}' was cancelled", planned.target),
        )
        .with_command_key(command_key),
        Err(error) => TargetCapture::blocked(format!(
            "command runner '{command_key}' for target '{}' failed: {error}",
            planned.target
        ))
        .with_command_key(command_key),
    }
}

fn bounded_output(output: Option<jig_owned_process::BoundedProcessOutput>) -> String {
    output.map_or_else(String::new, |output| {
        let mut value = output.to_string_lossy();
        if output.truncated {
            value.push_str("\n[output truncated by Jig]\n");
        }
        value
    })
}

fn resolve_working_directory(root: &Path, configured: Option<&str>) -> Result<PathBuf> {
    let Some(configured) = configured else {
        return root
            .canonicalize()
            .with_context(|| format!("failed to resolve repository root {}", root.display()));
    };
    let configured_path = Path::new(configured);
    if configured_path.as_os_str().is_empty()
        || configured_path.is_absolute()
        || configured_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("working_directory must be a non-empty relative repository path");
    }
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve repository root {}", root.display()))?;
    let candidate = root.join(configured_path).canonicalize().with_context(|| {
        format!(
            "failed to resolve configured path {}",
            root.join(configured_path).display()
        )
    })?;
    if !candidate.starts_with(&canonical_root) {
        bail!("working_directory resolves outside the repository");
    }
    if !candidate.is_dir() {
        bail!("working_directory is not a directory");
    }
    Ok(candidate)
}

fn validate_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) {
            bail!("environment variable name {name:?} is invalid");
        }
        if value.contains('\0') {
            bail!("environment variable {name:?} contains a NUL byte");
        }
    }
    Ok(())
}

struct TargetFinisher<'a> {
    ctx: &'a RepoContext,
    catalog: &'a RepositoryCatalog,
    run: &'a crate::state::DurableRun,
    work_plan_id: Option<&'a str>,
    record_receipts: bool,
}

impl TargetFinisher<'_> {
    fn finish(
        &self,
        planned: &PlannedTarget,
        started_at_ms: Option<u64>,
        capture: TargetCapture,
    ) -> Result<(TargetRunResult, Option<Value>)> {
        let ended_at_ms = now_ms();
        let tool_name = capture.alias.as_deref().unwrap_or(GENERIC_TARGET_TOOL);
        let receipt_id = self
            .record_receipts
            .then(|| {
                record_target_receipt(
                    self.ctx,
                    ReceiptInput {
                        tool_name,
                        args: json!({
                            "run_id": self.run.result.run_id,
                            "target": planned.target,
                        }),
                        invoked_command_key: capture.command_key.clone(),
                        plan_id: self.work_plan_id.map(str::to_owned),
                        started_at_ms: started_at_ms.unwrap_or(ended_at_ms),
                        ended_at_ms,
                        exit_status: capture.receipt_exit_status,
                        stdout: &capture.stdout,
                        stderr: &capture.stderr,
                        evidence: None,
                        session_override: None,
                        collect_git_metadata: true,
                        collect_worktree_fingerprint: false,
                        worktree_fingerprint_override: Some(Ok(self
                            .run
                            .plan
                            .source
                            .worktree_fingerprint
                            .clone())),
                    },
                    TargetReceiptMetadata {
                        run_id: self.run.result.run_id.clone(),
                        target: planned.target.clone(),
                        config_digest: self.run.plan.config_digest.clone(),
                        input_digest: planned.input_digest.clone(),
                        findings: capture.findings.clone(),
                    },
                )
            })
            .transpose()?;

        let mut result = TargetRunResult::queued(
            planned.target.clone(),
            self.run.plan.config_digest.clone(),
            planned.input_digest.clone(),
        );
        result.status = RunStatus::Completed;
        result.conclusion = Some(capture.conclusion);
        result.started_at_ms = started_at_ms;
        result.ended_at_ms = Some(ended_at_ms);
        result.exit_code = capture.exit_code;
        result.receipt_id.clone_from(&receipt_id);
        result.findings.clone_from(&capture.findings);

        let compatibility = started_at_ms.map(|_| {
            let alias = self
                .catalog
                .aliases_for_target(&planned.target)
                .first()
                .cloned();
            json!({
                "target": planned.target,
                "tool": alias,
                "response": {
                    "ok": true,
                    "tool": alias.as_deref().unwrap_or(GENERIC_TARGET_TOOL),
                    "command_key": capture.command_key,
                    "args": {},
                    "result": {
                        "exit_status": capture.receipt_exit_status,
                        "stdout": capture.stdout,
                        "stderr": capture.stderr,
                    },
                    "receipt_id": receipt_id,
                },
            })
        });
        Ok((result, compatibility))
    }
}

fn aggregate_conclusion(conclusions: impl Iterator<Item = RunConclusion>) -> RunConclusion {
    let conclusions = conclusions.collect::<Vec<_>>();
    if conclusions.contains(&RunConclusion::Cancelled) {
        RunConclusion::Cancelled
    } else if conclusions.contains(&RunConclusion::Failure) {
        RunConclusion::Failure
    } else if conclusions.contains(&RunConclusion::TimedOut) {
        RunConclusion::TimedOut
    } else if conclusions.contains(&RunConclusion::Blocked) {
        RunConclusion::Blocked
    } else {
        RunConclusion::Success
    }
}

struct TargetCapture {
    conclusion: RunConclusion,
    exit_code: Option<i32>,
    receipt_exit_status: i32,
    stdout: String,
    stderr: String,
    findings: Vec<Finding>,
    command_key: Option<String>,
    alias: Option<String>,
}

impl TargetCapture {
    fn from_process(
        exit_status: i32,
        stdout: String,
        stderr: String,
        parser: ResultParser,
    ) -> Self {
        let mut findings = parse_findings(parser, &stdout);
        let conclusion = if exit_status == 0 && findings_parse_succeeded(&findings) {
            RunConclusion::Success
        } else {
            if exit_status != 0 {
                findings.push(finding(
                    format!("target process exited with status {exit_status}"),
                    "exit_code",
                ));
            }
            RunConclusion::Failure
        };
        let receipt_exit_status = if conclusion == RunConclusion::Success {
            exit_status
        } else {
            exit_status.max(1)
        };
        Self {
            conclusion,
            exit_code: Some(exit_status),
            receipt_exit_status,
            stdout,
            stderr,
            findings,
            command_key: None,
            alias: None,
        }
    }

    fn not_started(conclusion: RunConclusion, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            conclusion,
            exit_code: None,
            receipt_exit_status: 1,
            stdout: String::new(),
            stderr: message.clone(),
            findings: vec![finding(message, "jig")],
            command_key: None,
            alias: None,
        }
    }

    fn blocked(message: impl Into<String>) -> Self {
        Self::not_started(RunConclusion::Blocked, message)
    }

    fn with_command_key(mut self, command_key: impl Into<String>) -> Self {
        self.command_key = Some(command_key.into());
        self
    }

    fn with_alias(mut self, alias: Option<String>) -> Self {
        self.alias = alias;
        self
    }
}

fn parse_findings(parser: ResultParser, stdout: &str) -> Vec<Finding> {
    if parser == ResultParser::ExitCode {
        return Vec::new();
    }
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Finding>(line).unwrap_or_else(|error| {
                finding(
                    format!("result parser rejected JSON line: {error}"),
                    "result_parser",
                )
            })
        })
        .collect()
}

fn findings_parse_succeeded(findings: &[Finding]) -> bool {
    findings
        .iter()
        .all(|finding| finding.source.as_deref() != Some("result_parser"))
}

fn finding(message: impl Into<String>, source: &str) -> Finding {
    let mut finding = Finding::new(FindingSeverity::Error, message);
    finding.source = Some(source.into());
    finding
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn working_directory_rejects_parent_escape() {
        let temp = tempdir().unwrap();
        assert!(
            resolve_working_directory(temp.path(), Some("../outside"))
                .unwrap_err()
                .to_string()
                .contains("relative repository path")
        );
    }

    #[test]
    fn json_lines_parser_normalizes_findings_and_rejects_bad_lines() {
        let valid = r#"{"severity":"warning","message":"unused","source":"lint"}"#;
        let findings = parse_findings(ResultParser::JsonLines, valid);
        assert_eq!(findings[0].severity, FindingSeverity::Warning);
        assert!(findings_parse_succeeded(&findings));

        let findings = parse_findings(ResultParser::JsonLines, "not-json");
        assert!(!findings_parse_succeeded(&findings));
    }
}
