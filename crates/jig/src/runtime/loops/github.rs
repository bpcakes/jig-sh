use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::bootstrap::scrub_known_repository_git_environment;
use crate::context::{CommandTimeout, RepoContext};
use crate::execution::{
    ExecutionCommandError, ExecutionControl, run_authoritative_execution_command,
};
use crate::state::now_ms;

use super::workflow::WorkflowTick;

const PR_LIST_FIELDS: &str = concat!(
    "number,title,url,state,isDraft,author,baseRefName,headRefName,headRefOid,",
    "headRepository,headRepositoryOwner,isCrossRepository,mergeable,mergeStateStatus,",
    "reviewDecision,statusCheckRollup,updatedAt,createdAt"
);
const PR_CHECK_FIELDS: &str =
    "bucket,completedAt,description,event,link,name,startedAt,state,workflow";
const PR_LIST_LIMIT: usize = 100;
const PR_LIST_FETCH_LIMIT: usize = PR_LIST_LIMIT + 1;
const REVIEW_THREAD_PAGE_LIMIT: usize = 10;
const GITHUB_SNAPSHOT_REQUEST_LIMIT: usize = 256;
const GITHUB_SNAPSHOT_RESPONSE_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const GITHUB_SNAPSHOT_EVIDENCE_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const GITHUB_SNAPSHOT_REVIEW_ITEM_LIMIT: usize = 10_000;
const GITHUB_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

include!("github/trust.rs");
include!("github/review_threads.rs");

pub(super) fn github_pr_status_tick(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkflowTick> {
    Ok(WorkflowTick::from_actions(
        github_pr_status_snapshot(ctx, observer)?,
        Vec::new(),
    ))
}

pub(super) fn github_pr_status_snapshot(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let observed_at_ms = now_ms();
    let mut client = GithubSnapshotClient::new(ctx, observer);
    let repository = repository_snapshot(&mut client)?;
    let pr_list_fetch_limit = PR_LIST_FETCH_LIMIT.to_string();
    let raw_prs = client.json(
        os_args([
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            &pr_list_fetch_limit,
            "--json",
            PR_LIST_FIELDS,
        ]),
        &[0],
    )?;
    let raw_prs = raw_prs
        .as_array()
        .ok_or_else(|| anyhow!("gh pr list returned non-array JSON"))?;
    let pr_list_truncated = raw_prs.len() > PR_LIST_LIMIT;

    let mut pull_requests = Vec::new();
    let mut permissions = RepositoryPermissionCache::default();
    for raw_pr in raw_prs.iter().take(PR_LIST_LIMIT) {
        if client.cancelled() {
            return Err(ExecutionCommandError::Cancelled.into_anyhow());
        }
        pull_requests.push(pull_request_snapshot(
            &mut client,
            &repository,
            raw_pr,
            &mut permissions,
        )?);
    }

    let summary = summary_for_pull_requests(&pull_requests, PR_LIST_LIMIT, pr_list_truncated);
    let budget = client.budget_snapshot();
    let snapshot = json!({
            "kind": "github_pr_status_snapshot",
            "schema_version": 1,
            "observed_at_ms": observed_at_ms,
            "repository": repository.value,
            "summary": summary,
            "budget": budget,
            "pull_requests": pull_requests,
    });
    require_serialized_snapshot_budget(&snapshot, GITHUB_SNAPSHOT_EVIDENCE_BYTE_LIMIT)?;
    Ok(snapshot)
}

fn require_serialized_snapshot_budget(snapshot: &Value, limit: usize) -> Result<()> {
    let byte_len = serde_json::to_vec(snapshot)?.len();
    if byte_len > limit {
        bail!("GitHub PR snapshot exceeded its {limit}-byte serialized evidence budget");
    }
    Ok(())
}

struct GithubSnapshotClient<'a> {
    ctx: &'a RepoContext,
    observer: &'a mut dyn ExecutionControl,
    budget: GithubSnapshotBudget,
}

impl<'a> GithubSnapshotClient<'a> {
    fn new(ctx: &'a RepoContext, observer: &'a mut dyn ExecutionControl) -> Self {
        Self {
            ctx,
            observer,
            budget: GithubSnapshotBudget::new(ctx.command_timeout()),
        }
    }

    fn cancelled(&self) -> bool {
        self.observer.cancelled()
    }

    fn json(&mut self, args: Vec<OsString>, allowed_statuses: &[i32]) -> Result<Value> {
        let command_label = command_label(&args);
        let output = self.output(args)?;
        let status = output.status_code.unwrap_or(-1);
        if !allowed_statuses.contains(&status) {
            return Err(output.into_error(&command_label));
        }
        parse_gh_json(&output.stdout, &command_label)
    }

    fn output(&mut self, args: Vec<OsString>) -> Result<GhOutput> {
        let timeout = self.budget.reserve_request()?;
        let output = run_gh_with_timeout(self.ctx, args, timeout, self.observer)?;
        self.budget.record_response(&output)?;
        Ok(output)
    }

    fn reserve_review_items(&mut self, count: usize) -> Result<()> {
        self.budget.reserve_review_items(count)
    }

    fn budget_snapshot(&self) -> Value {
        self.budget.snapshot()
    }
}

struct GithubSnapshotBudget {
    started_at: Instant,
    timeout: Duration,
    request_count: usize,
    response_bytes: usize,
    review_item_count: usize,
}

impl GithubSnapshotBudget {
    fn new(command_timeout: CommandTimeout) -> Self {
        Self {
            started_at: Instant::now(),
            timeout: command_timeout.duration().min(GITHUB_SNAPSHOT_TIMEOUT),
            request_count: 0,
            response_bytes: 0,
            review_item_count: 0,
        }
    }

    fn reserve_request(&mut self) -> Result<CommandTimeout> {
        if self.request_count >= GITHUB_SNAPSHOT_REQUEST_LIMIT {
            bail!("GitHub PR snapshot exceeded its {GITHUB_SNAPSHOT_REQUEST_LIMIT}-request budget");
        }
        let remaining = self
            .timeout
            .checked_sub(self.started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                anyhow!(
                    "GitHub PR snapshot exceeded its {}-second deadline",
                    self.timeout.as_secs()
                )
            })?;
        let seconds = remaining.as_secs();
        if seconds == 0 {
            bail!(
                "GitHub PR snapshot exceeded its {}-second deadline",
                self.timeout.as_secs()
            );
        }
        let timeout = CommandTimeout::from_seconds(seconds)
            .ok_or_else(|| anyhow!("GitHub PR snapshot produced an invalid request timeout"))?;
        self.request_count += 1;
        Ok(timeout)
    }

    fn record_response(&mut self, output: &GhOutput) -> Result<()> {
        let bytes = output.stdout.len().saturating_add(output.stderr.len());
        let total = self.response_bytes.saturating_add(bytes);
        if total > GITHUB_SNAPSHOT_RESPONSE_BYTE_LIMIT {
            bail!(
                "GitHub PR snapshot exceeded its {GITHUB_SNAPSHOT_RESPONSE_BYTE_LIMIT}-byte response budget"
            );
        }
        self.response_bytes = total;
        Ok(())
    }

    fn reserve_review_items(&mut self, count: usize) -> Result<()> {
        let total = self.review_item_count.saturating_add(count);
        if total > GITHUB_SNAPSHOT_REVIEW_ITEM_LIMIT {
            bail!(
                "GitHub PR snapshot exceeded its {GITHUB_SNAPSHOT_REVIEW_ITEM_LIMIT}-item review budget"
            );
        }
        self.review_item_count = total;
        Ok(())
    }

    fn snapshot(&self) -> Value {
        json!({
            "request_count": self.request_count,
            "request_limit": GITHUB_SNAPSHOT_REQUEST_LIMIT,
            "response_bytes": self.response_bytes,
            "response_byte_limit": GITHUB_SNAPSHOT_RESPONSE_BYTE_LIMIT,
            "review_item_count": self.review_item_count,
            "review_item_limit": GITHUB_SNAPSHOT_REVIEW_ITEM_LIMIT,
            "timeout_seconds": self.timeout.as_secs(),
        })
    }
}

struct RepositorySnapshot {
    owner: String,
    name: String,
    default_branch: String,
    value: Value,
}

fn repository_snapshot(client: &mut GithubSnapshotClient<'_>) -> Result<RepositorySnapshot> {
    let raw = client.json(
        os_args([
            "repo",
            "view",
            "--json",
            "nameWithOwner,name,owner,url,defaultBranchRef",
        ]),
        &[0],
    )?;
    let owner = raw
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("gh repo view JSON did not include owner.login"))?
        .to_string();
    let name = raw
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("gh repo view JSON did not include name"))?
        .to_string();
    let default_branch = raw
        .pointer("/defaultBranchRef/name")
        .and_then(Value::as_str)
        .unwrap_or_else(|| client.ctx.default_branch())
        .to_string();
    let name_with_owner = raw
        .get("nameWithOwner")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{owner}/{name}"));

    Ok(RepositorySnapshot {
        owner: owner.clone(),
        name: name.clone(),
        default_branch: default_branch.clone(),
        value: json!({
            "owner": owner,
            "name": name,
            "name_with_owner": name_with_owner,
            "url": raw.get("url").cloned().unwrap_or(Value::Null),
            "default_branch": default_branch,
        }),
    })
}

fn pull_request_snapshot(
    client: &mut GithubSnapshotClient<'_>,
    repository: &RepositorySnapshot,
    raw_pr: &Value,
    permissions: &mut RepositoryPermissionCache,
) -> Result<Value> {
    let number = raw_pr
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("gh pr list returned a PR without a numeric number"))?;
    let checks = checks_snapshot(client, number)?;
    let review_threads = review_threads_snapshot(client, repository, number, permissions)?;
    let base_ref = raw_pr
        .get("baseRefName")
        .and_then(Value::as_str)
        .unwrap_or("");
    let base_is_default_branch = base_ref == repository.default_branch;
    let head_repository_name_with_owner = head_repository_name_with_owner(raw_pr);

    Ok(json!({
        "number": number,
        "title": raw_pr.get("title").cloned().unwrap_or(Value::Null),
        "url": raw_pr.get("url").cloned().unwrap_or(Value::Null),
        "state": raw_pr.get("state").cloned().unwrap_or(Value::Null),
        "is_draft": raw_pr.get("isDraft").cloned().unwrap_or(Value::Null),
        "author": {
            "login": raw_pr.pointer("/author/login").cloned().unwrap_or(Value::Null),
        },
        "base": {
            "ref": raw_pr.get("baseRefName").cloned().unwrap_or(Value::Null),
            "is_default_branch": base_is_default_branch,
        },
        "head": {
            "ref": raw_pr.get("headRefName").cloned().unwrap_or(Value::Null),
            "sha": raw_pr.get("headRefOid").cloned().unwrap_or(Value::Null),
            "repository": raw_pr.get("headRepository").cloned().unwrap_or(Value::Null),
            "owner": raw_pr.get("headRepositoryOwner").cloned().unwrap_or(Value::Null),
            "repository_name_with_owner": head_repository_name_with_owner,
            "is_cross_repository": raw_pr.get("isCrossRepository").cloned().unwrap_or(Value::Null),
        },
        "stack": {
            "is_stacked": !base_is_default_branch,
            "base_ref": raw_pr.get("baseRefName").cloned().unwrap_or(Value::Null),
            "default_branch": repository.default_branch,
        },
        "mergeability": {
            "mergeable": raw_pr.get("mergeable").cloned().unwrap_or(Value::Null),
            "merge_state_status": raw_pr.get("mergeStateStatus").cloned().unwrap_or(Value::Null),
        },
        "review_decision": raw_pr.get("reviewDecision").cloned().unwrap_or(Value::Null),
        "updated_at": raw_pr.get("updatedAt").cloned().unwrap_or(Value::Null),
        "created_at": raw_pr.get("createdAt").cloned().unwrap_or(Value::Null),
        "checks": checks,
        "review_threads": review_threads,
    }))
}

fn head_repository_name_with_owner(raw_pr: &Value) -> Option<String> {
    let owner = raw_pr
        .pointer("/headRepositoryOwner/login")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|owner| !owner.is_empty());
    let name = raw_pr
        .pointer("/headRepository/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty());
    owner
        .zip(name)
        .map(|(owner, name)| format!("{owner}/{name}"))
}

fn checks_snapshot(client: &mut GithubSnapshotClient<'_>, pr_number: u64) -> Result<Value> {
    let output = client.output(os_args([
        "pr",
        "checks",
        &pr_number.to_string(),
        "--json",
        PR_CHECK_FIELDS,
    ]))?;
    let checks = match output.status_code {
        Some(0 | 8) => parse_gh_json(&output.stdout, "gh pr checks")?,
        Some(1) => match parse_gh_json(&output.stdout, "gh pr checks") {
            Ok(checks) if checks.is_array() => checks,
            _ if output.stderr.to_lowercase().contains("no checks") => json!([]),
            _ => return Err(output.into_error("gh pr checks")),
        },
        _ => return Err(output.into_error("gh pr checks")),
    };
    let checks = checks
        .as_array()
        .ok_or_else(|| anyhow!("gh pr checks returned non-array JSON"))?;

    Ok(json!({
        "summary": check_summary(checks),
        "runs": checks.iter().map(normalize_check_run).collect::<Vec<_>>(),
    }))
}

fn check_summary(checks: &[Value]) -> Value {
    let mut pass = 0;
    let mut fail = 0;
    let mut pending = 0;
    let mut skipping = 0;
    let mut cancel = 0;
    let mut unknown = 0;

    for check in checks {
        match check
            .get("bucket")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
        {
            "pass" => pass += 1,
            "fail" => fail += 1,
            "pending" => pending += 1,
            "skipping" => skipping += 1,
            "cancel" => cancel += 1,
            _ => unknown += 1,
        }
    }

    json!({
        "total": checks.len(),
        "pass": pass,
        "fail": fail,
        "pending": pending,
        "skipping": skipping,
        "cancel": cancel,
        "unknown": unknown,
    })
}

fn normalize_check_run(check: &Value) -> Value {
    json!({
        "name": check.get("name").cloned().unwrap_or(Value::Null),
        "workflow": check.get("workflow").cloned().unwrap_or(Value::Null),
        "state": check.get("state").cloned().unwrap_or(Value::Null),
        "bucket": check.get("bucket").cloned().unwrap_or(Value::Null),
        "event": check.get("event").cloned().unwrap_or(Value::Null),
        "description": check.get("description").cloned().unwrap_or(Value::Null),
        "link": check.get("link").cloned().unwrap_or(Value::Null),
        "started_at": check.get("startedAt").cloned().unwrap_or(Value::Null),
        "completed_at": check.get("completedAt").cloned().unwrap_or(Value::Null),
    })
}

fn review_thread_summary(threads: &[Value]) -> Value {
    let mut resolved = 0;
    let mut unresolved = 0;
    let mut outdated = 0;
    let mut viewer_can_reply = 0;
    let mut viewer_can_resolve = 0;
    let mut trusted_unresolved = 0;

    for thread in threads {
        if thread
            .get("is_resolved")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            resolved += 1;
        } else {
            unresolved += 1;
            if thread
                .get("has_trusted_comment")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                trusted_unresolved += 1;
            }
        }
        if thread
            .get("is_outdated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            outdated += 1;
        }
        if thread
            .get("viewer_can_reply")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            viewer_can_reply += 1;
        }
        if thread
            .get("viewer_can_resolve")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            viewer_can_resolve += 1;
        }
    }

    json!({
        "total": threads.len(),
        "resolved": resolved,
        "unresolved": unresolved,
        "trusted_unresolved": trusted_unresolved,
        "outdated": outdated,
        "viewer_can_reply": viewer_can_reply,
        "viewer_can_resolve": viewer_can_resolve,
    })
}

fn summary_for_pull_requests(
    pull_requests: &[Value],
    pr_list_limit: usize,
    pr_list_truncated: bool,
) -> Value {
    let mut merge_conflict = 0;
    let mut check_fail = 0;
    let mut check_pending = 0;
    let mut unresolved_threads = 0;
    let mut stacked = 0;

    for pull_request in pull_requests {
        if pull_request
            .pointer("/mergeability/mergeable")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("CONFLICTING"))
        {
            merge_conflict += 1;
        }
        if pull_request
            .pointer("/checks/summary/fail")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        {
            check_fail += 1;
        }
        if pull_request
            .pointer("/checks/summary/pending")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        {
            check_pending += 1;
        }
        unresolved_threads += pull_request
            .pointer("/review_threads/summary/unresolved")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if pull_request
            .pointer("/stack/is_stacked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            stacked += 1;
        }
    }

    json!({
        "open_pr_count": pull_requests.len(),
        "pr_list_limit": pr_list_limit,
        "pr_list_truncated": pr_list_truncated,
        "merge_conflict_pr_count": merge_conflict,
        "failing_check_pr_count": check_fail,
        "pending_check_pr_count": check_pending,
        "unresolved_review_thread_count": unresolved_threads,
        "stacked_pr_count": stacked,
    })
}

struct GhOutput {
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl GhOutput {
    fn into_error(self, command: &str) -> anyhow::Error {
        anyhow!(
            "{command} failed with status {}. stderr: {}",
            self.status_code
                .map(|status| status.to_string())
                .unwrap_or_else(|| "signal".into()),
            self.stderr.trim()
        )
    }
}

pub(super) fn gh_json_with_timeout(
    ctx: &RepoContext,
    args: Vec<OsString>,
    allowed_statuses: &[i32],
    timeout: crate::context::CommandTimeout,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    let command_label = command_label(&args);
    let output = run_gh_with_timeout(ctx, args, timeout, observer)?;
    let status = output.status_code.unwrap_or(-1);
    if !allowed_statuses.contains(&status) {
        return Err(ExecutionCommandError::failed(
            output.into_error(&command_label),
        ));
    }
    parse_gh_json(&output.stdout, &command_label).map_err(ExecutionCommandError::failed)
}

fn run_gh_with_timeout(
    ctx: &RepoContext,
    args: Vec<OsString>,
    timeout: crate::context::CommandTimeout,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<GhOutput, ExecutionCommandError> {
    let gh = std::env::var_os("JIG_GH_BIN").unwrap_or_else(|| OsString::from("gh"));
    run_gh_with_program_timeout(ctx, args, &gh, timeout, observer)
}

#[cfg(test)]
fn run_gh_with_program(
    ctx: &RepoContext,
    args: Vec<OsString>,
    gh: &OsStr,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<GhOutput, ExecutionCommandError> {
    run_gh_with_program_timeout(ctx, args, gh, ctx.command_timeout(), observer)
}

fn run_gh_with_program_timeout(
    ctx: &RepoContext,
    args: Vec<OsString>,
    gh: &OsStr,
    timeout: crate::context::CommandTimeout,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<GhOutput, ExecutionCommandError> {
    let command_label = command_label(&args);
    let execution_label = format!("{} {command_label}", gh.to_string_lossy());
    let mut command = Command::new(gh);
    command
        .args(&args)
        .current_dir(ctx.root())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    scrub_github_repository_environment(&mut command);
    let output = run_authoritative_execution_command(
        &mut command,
        timeout,
        crate::execution::internal_execution_output_limit(),
        &execution_label,
        observer,
    )?;

    Ok(GhOutput {
        status_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn scrub_github_repository_environment(command: &mut Command) {
    scrub_known_repository_git_environment(command);
    let explicitly_configured = command
        .get_envs()
        .map(|(name, _)| name.to_os_string())
        .collect::<Vec<_>>();
    for name in std::env::vars_os()
        .map(|(name, _)| name)
        .chain(explicitly_configured)
    {
        if name.to_string_lossy().eq_ignore_ascii_case("GH_REPO") {
            command.env_remove(name);
        }
    }
}

fn parse_gh_json(stdout: &str, command: &str) -> Result<Value> {
    let stdout = stdout.trim();
    if stdout.is_empty() {
        bail!("{command} returned empty stdout");
    }
    serde_json::from_str(stdout).with_context(|| format!("Failed to parse JSON from {command}"))
}

fn command_label(args: &[OsString]) -> String {
    let mut parts = vec!["gh".to_string()];
    parts.extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
    parts.join(" ")
}

fn os_args<const N: usize>(args: [&str; N]) -> Vec<OsString> {
    args.into_iter().map(OsString::from).collect()
}

#[cfg(test)]
include!("github/tests.rs");
