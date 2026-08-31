use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::context::RepoContext;
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

include!("github/trust.rs");

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
    let repository = repository_snapshot(ctx, observer)?;
    let pr_list_fetch_limit = PR_LIST_FETCH_LIMIT.to_string();
    let raw_prs = gh_json(
        ctx,
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
        observer,
    )?;
    let raw_prs = raw_prs
        .as_array()
        .ok_or_else(|| anyhow!("gh pr list returned non-array JSON"))?;
    let pr_list_truncated = raw_prs.len() > PR_LIST_LIMIT;

    let mut pull_requests = Vec::new();
    let mut permissions = RepositoryPermissionCache::default();
    for raw_pr in raw_prs.iter().take(PR_LIST_LIMIT) {
        if observer.cancelled() {
            return Err(ExecutionCommandError::Cancelled.into_anyhow());
        }
        pull_requests.push(pull_request_snapshot(
            ctx,
            &repository,
            raw_pr,
            &mut permissions,
            observer,
        )?);
    }

    let summary = summary_for_pull_requests(&pull_requests, PR_LIST_LIMIT, pr_list_truncated);
    Ok(json!({
            "kind": "github_pr_status_snapshot",
            "schema_version": 1,
            "observed_at_ms": observed_at_ms,
            "repository": repository.value,
            "summary": summary,
            "pull_requests": pull_requests,
    }))
}

struct RepositorySnapshot {
    owner: String,
    name: String,
    default_branch: String,
    value: Value,
}

fn repository_snapshot(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<RepositorySnapshot> {
    let raw = gh_json(
        ctx,
        os_args([
            "repo",
            "view",
            "--json",
            "nameWithOwner,name,owner,url,defaultBranchRef",
        ]),
        &[0],
        observer,
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
        .unwrap_or_else(|| ctx.default_branch())
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
    ctx: &RepoContext,
    repository: &RepositorySnapshot,
    raw_pr: &Value,
    permissions: &mut RepositoryPermissionCache,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let number = raw_pr
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("gh pr list returned a PR without a numeric number"))?;
    let checks = checks_snapshot(ctx, number, observer)?;
    let review_threads = review_threads_snapshot(ctx, repository, number, permissions, observer)?;
    let base_ref = raw_pr
        .get("baseRefName")
        .and_then(Value::as_str)
        .unwrap_or("");
    let base_is_default_branch = base_ref == repository.default_branch;

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
        "raw": {
            "pr_list": raw_pr,
            "status_check_rollup": raw_pr.get("statusCheckRollup").cloned().unwrap_or(Value::Null),
        },
    }))
}

fn checks_snapshot(
    ctx: &RepoContext,
    pr_number: u64,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let output = run_gh(
        ctx,
        os_args([
            "pr",
            "checks",
            &pr_number.to_string(),
            "--json",
            PR_CHECK_FIELDS,
        ]),
        observer,
    )?;
    let checks = match output.status_code {
        Some(0 | 8) => parse_gh_json(&output.stdout, "gh pr checks")?,
        Some(1) if output.stderr.to_lowercase().contains("no checks") => json!([]),
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

fn review_threads_snapshot(
    ctx: &RepoContext,
    repository: &RepositorySnapshot,
    pr_number: u64,
    permissions: &mut RepositoryPermissionCache,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let mut nodes = Vec::new();
    let mut has_next_page = true;
    let mut cursor = None;
    let mut page_count = 0;
    let mut truncated = false;

    while has_next_page {
        if observer.cancelled() {
            return Err(ExecutionCommandError::Cancelled.into_anyhow());
        }
        if page_count >= REVIEW_THREAD_PAGE_LIMIT {
            truncated = true;
            break;
        }
        page_count += 1;
        let page = review_thread_page(ctx, repository, pr_number, cursor.as_deref(), observer)?;
        let connection = page
            .pointer("/data/repository/pullRequest/reviewThreads")
            .ok_or_else(|| anyhow!("GitHub GraphQL response did not include reviewThreads"))?;
        let page_nodes = connection
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("GitHub GraphQL reviewThreads.nodes was not an array"))?;
        for thread in page_nodes {
            nodes.push(normalize_review_thread(
                ctx,
                repository,
                thread,
                permissions,
                observer,
            )?);
        }
        has_next_page = connection
            .pointer("/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        cursor = connection
            .pointer("/pageInfo/endCursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }

    Ok(json!({
        "summary": review_thread_summary(&nodes),
        "nodes": nodes,
        "page_info": {
            "page_count": page_count,
            "truncated": truncated,
            "has_next_page": has_next_page,
            "end_cursor": cursor,
        },
    }))
}

fn review_thread_page(
    ctx: &RepoContext,
    repository: &RepositorySnapshot,
    pr_number: u64,
    cursor: Option<&str>,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let mut args = vec![
        OsString::from("api"),
        OsString::from("graphql"),
        OsString::from("-f"),
        OsString::from(format!("query={}", review_threads_query())),
        OsString::from("-F"),
        OsString::from(format!("owner={}", repository.owner)),
        OsString::from("-F"),
        OsString::from(format!("name={}", repository.name)),
        OsString::from("-F"),
        OsString::from(format!("number={pr_number}")),
    ];
    if let Some(cursor) = cursor {
        args.push(OsString::from("-F"));
        args.push(OsString::from(format!("threadsAfter={cursor}")));
    }
    Ok(gh_json(ctx, args, &[0], observer)?)
}

const fn review_threads_query() -> &'static str {
    r"
query($owner: String!, $name: String!, $number: Int!, $threadsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $threadsAfter) {
        pageInfo {
          hasNextPage
          endCursor
        }
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          startLine
          originalLine
          originalStartLine
          subjectType
          diffSide
          startDiffSide
          viewerCanReply
          viewerCanResolve
          viewerCanUnresolve
          resolvedBy {
            login
          }
          comments(last: 10) {
            totalCount
            nodes {
              id
              url
              body
              createdAt
              updatedAt
              author {
                login
              }
            }
          }
        }
      }
    }
  }
}
"
}

fn normalize_review_thread(
    ctx: &RepoContext,
    repository: &RepositorySnapshot,
    thread: &Value,
    permissions: &mut RepositoryPermissionCache,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let mut comments = Vec::new();
    for comment in thread
        .pointer("/comments/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let author = permissions.author_snapshot(
            ctx,
            repository,
            comment.pointer("/author/login").and_then(Value::as_str),
            observer,
        )?;
        comments.push(json!({
            "id": comment.get("id").cloned().unwrap_or(Value::Null),
            "url": comment.get("url").cloned().unwrap_or(Value::Null),
            "body": comment.get("body").cloned().unwrap_or(Value::Null),
            "createdAt": comment.get("createdAt").cloned().unwrap_or(Value::Null),
            "updatedAt": comment.get("updatedAt").cloned().unwrap_or(Value::Null),
            "author": author,
        }));
    }
    let has_trusted_comment = comments.iter().any(|comment| {
        comment
            .pointer("/author/trusted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    Ok(json!({
        "id": thread.get("id").cloned().unwrap_or(Value::Null),
        "is_resolved": thread.get("isResolved").cloned().unwrap_or(Value::Null),
        "is_outdated": thread.get("isOutdated").cloned().unwrap_or(Value::Null),
        "path": thread.get("path").cloned().unwrap_or(Value::Null),
        "line": thread.get("line").cloned().unwrap_or(Value::Null),
        "start_line": thread.get("startLine").cloned().unwrap_or(Value::Null),
        "original_line": thread.get("originalLine").cloned().unwrap_or(Value::Null),
        "original_start_line": thread.get("originalStartLine").cloned().unwrap_or(Value::Null),
        "subject_type": thread.get("subjectType").cloned().unwrap_or(Value::Null),
        "diff_side": thread.get("diffSide").cloned().unwrap_or(Value::Null),
        "start_diff_side": thread.get("startDiffSide").cloned().unwrap_or(Value::Null),
        "viewer_can_reply": thread.get("viewerCanReply").cloned().unwrap_or(Value::Null),
        "viewer_can_resolve": thread.get("viewerCanResolve").cloned().unwrap_or(Value::Null),
        "viewer_can_unresolve": thread.get("viewerCanUnresolve").cloned().unwrap_or(Value::Null),
        "resolved_by": {
            "login": thread.pointer("/resolvedBy/login").cloned().unwrap_or(Value::Null),
        },
        "comments": {
            "total_count": thread.pointer("/comments/totalCount").cloned().unwrap_or(Value::Null),
            "nodes": comments,
        },
        "has_trusted_comment": has_trusted_comment,
        "raw": thread,
    }))
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

pub(super) fn gh_json(
    ctx: &RepoContext,
    args: Vec<OsString>,
    allowed_statuses: &[i32],
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    gh_json_with_timeout(ctx, args, allowed_statuses, ctx.command_timeout(), observer)
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

fn run_gh(
    ctx: &RepoContext,
    args: Vec<OsString>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<GhOutput, ExecutionCommandError> {
    run_gh_with_timeout(ctx, args, ctx.command_timeout(), observer)
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
