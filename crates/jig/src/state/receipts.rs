use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use time::{Date, Month};
use ulid::Ulid;

use crate::cancellation::{ensure_status_collection_active, status_collection_cancellation};
use crate::context::RepoContext;
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_without_worktree_fingerprint,
    is_worktree_fingerprint_cancellation, repo_worktree_fingerprint,
    repo_worktree_fingerprint_with_cancellation,
};
use crate::tool_defs::tool;

use super::events::{
    ReceiptRecord, append_jsonl, ensure_state_layout, new_id, now_ms, read_jsonl,
    read_jsonl_locked, read_jsonl_with_cancellation, truncate, with_jsonl_write_lock,
    write_jsonl_locked,
};
use super::sessions::current_session;

pub(crate) struct ReceiptInput<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) args: Value,
    pub(crate) invoked_command_key: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) exit_status: i32,
    pub(crate) stdout: &'a str,
    pub(crate) stderr: &'a str,
    pub(crate) evidence: Option<Value>,
    pub(crate) session_override: Option<String>,
    pub(crate) collect_git_metadata: bool,
    pub(crate) collect_worktree_fingerprint: bool,
    pub(crate) worktree_fingerprint_override: Option<std::result::Result<String, String>>,
}

pub(super) struct StateToolReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) started_at_ms: u64,
    pub(super) plan_id: Option<String>,
    pub(super) session_override: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReceiptListFilter {
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) failed_only: bool,
    // `usize::default()` is 0, but a null receipt limit should keep the
    // public default instead of asking for zero rows.
    #[serde(
        default = "crate::serde_helpers::default_receipts_limit",
        deserialize_with = "crate::serde_helpers::null_as_default_receipts_limit"
    )]
    pub(crate) limit: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ToolReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) exit_status: i32,
    pub(crate) ended_at_ms: u64,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) diff_summary: String,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkReviewReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) exit_status: i32,
    pub(crate) ended_at_ms: u64,
    pub(crate) evidence: Option<WorkReviewReceiptEvidence>,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) diff_summary: String,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkReviewReceiptEvidence {
    pub(crate) status: Option<String>,
    pub(crate) finding_count: Option<u64>,
    pub(crate) actionable_count: Option<u64>,
    pub(crate) retained_finding_count: Option<usize>,
    pub(crate) retained_actionable_count: Option<usize>,
    pub(crate) findings_truncated: Option<bool>,
    pub(crate) actionable_findings_truncated: Option<bool>,
    pub(crate) threshold: Option<String>,
    pub(crate) parse_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentWorktreeFingerprint {
    pub(crate) fingerprint: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct StateArchiveRequest {
    pub(crate) before: String,
    pub(crate) dry_run: bool,
}

pub(crate) fn receipts_list(ctx: &RepoContext, filter: ReceiptListFilter) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?
        .into_iter()
        .rev()
        .filter(|receipt| receipt_matches_filters(receipt, &filter))
        .take(filter.limit)
        .map(receipt_list_value)
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "ok": true,
        "receipts": receipts,
    }))
}

pub(crate) fn receipts_archive(ctx: &RepoContext, request: StateArchiveRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let before_ms = parse_archive_before_ms(&request.before)?;
    let receipts_path = ctx.state_file("receipts.jsonl");
    let source_path = receipts_path
        .strip_prefix(ctx.root())
        .unwrap_or(&receipts_path)
        .display()
        .to_string();
    with_jsonl_write_lock(&receipts_path, |guard| {
        let receipts = read_jsonl_locked::<ReceiptRecord>(guard, &receipts_path)?;
        let protected = protected_receipt_ids(&receipts);
        let (retained, archived): (Vec<_>, Vec<_>) = receipts.into_iter().partition(|receipt| {
            receipt.ended_at_ms >= before_ms || protected.contains(&receipt.id)
        });
        let protected_retained = retained
            .iter()
            .filter(|receipt| receipt.ended_at_ms < before_ms && protected.contains(&receipt.id))
            .count();
        let archive_path = if archived.is_empty() || request.dry_run {
            None
        } else {
            Some(
                ctx.state_dir()
                    .join("archive")
                    .join(format!("receipts-before-{before_ms}-{}.jsonl", Ulid::new())),
            )
        };

        if !request.dry_run {
            if let Some(path) = &archive_path {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create {}", parent.display()))?;
                }
                write_jsonl_locked(guard, path, &archived)?;
                if let Err(error) = write_jsonl_locked(guard, &receipts_path, &retained) {
                    let _ = fs::remove_file(path);
                    return Err(error);
                }
            }
        }

        Ok(json!({
            "ok": true,
            "command": "state archive",
            "dry_run": request.dry_run,
            "before": request.before,
            "before_ms": before_ms,
            "source_path": source_path,
            "archive_path": archive_path.map(|path| path.display().to_string()),
            "receipt_count_before": retained.len() + archived.len(),
            "receipts_archived": archived.len(),
            "receipts_retained": retained.len(),
            "protected_receipts_retained": protected_retained,
        }))
    })
}

pub(crate) fn latest_plan_tool_receipt(
    ctx: &RepoContext,
    plan_id: &str,
    tool_name: &str,
) -> Result<Option<ToolReceiptStatus>> {
    latest_plan_tool_receipt_with_cancellation(ctx, plan_id, tool_name, &|| false)
}

pub(crate) fn latest_plan_tool_receipt_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    tool_name: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<ToolReceiptStatus>> {
    ensure_receipt_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;
    let receipts = read_jsonl_with_cancellation::<ReceiptRecord>(
        &ctx.state_file("receipts.jsonl"),
        cancelled,
    )?;
    for receipt in receipts.into_iter().rev() {
        ensure_receipt_scan_active(cancelled)?;
        if receipt.plan_id.as_deref() == Some(plan_id) && receipt.tool_name == tool_name {
            return Ok(Some(tool_receipt_status(receipt)));
        }
    }
    ensure_receipt_scan_active(cancelled)?;
    Ok(None)
}

fn protected_receipt_ids(receipts: &[ReceiptRecord]) -> BTreeSet<String> {
    let mut latest_by_plan_tool = BTreeMap::<(Option<String>, String), usize>::new();
    let mut latest_review_by_plan_gate = BTreeMap::<(Option<String>, String), usize>::new();
    let mut latest_work_check_by_plan_tool = BTreeMap::<(Option<String>, String), usize>::new();

    for (index, receipt) in receipts.iter().enumerate() {
        insert_latest_receipt_index(
            &mut latest_by_plan_tool,
            (receipt.plan_id.clone(), receipt.tool_name.clone()),
            receipts,
            index,
        );
        if receipt.tool_name == tool::WORK_REVIEW {
            if let Some(gate_id) = receipt.args.get("gate_id").and_then(Value::as_str) {
                insert_latest_receipt_index(
                    &mut latest_review_by_plan_gate,
                    (receipt.plan_id.clone(), gate_id.to_string()),
                    receipts,
                    index,
                );
            }
        }
        if receipt.tool_name == tool::WORK_CHECK {
            for tool_name in receipt_arg_strings(receipt, "tools") {
                insert_latest_receipt_index(
                    &mut latest_work_check_by_plan_tool,
                    (receipt.plan_id.clone(), tool_name.to_string()),
                    receipts,
                    index,
                );
            }
        }
    }

    let mut protected = BTreeSet::new();
    for index in latest_by_plan_tool
        .values()
        .chain(latest_review_by_plan_gate.values())
        .chain(latest_work_check_by_plan_tool.values())
    {
        protected.insert(receipts[*index].id.clone());
    }

    loop {
        let before_len = protected.len();
        for receipt in receipts {
            if receipt.tool_name != tool::WORK_CHECK {
                continue;
            }
            let receipt_ids = receipt_arg_strings(receipt, "receipt_ids")
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if protected.contains(&receipt.id) {
                protected.extend(receipt_ids.iter().cloned());
            } else if receipt_ids.iter().any(|id| protected.contains(id)) {
                protected.insert(receipt.id.clone());
            }
        }
        if protected.len() == before_len {
            break;
        }
    }

    protected
}

fn insert_latest_receipt_index<K: Ord>(
    latest: &mut BTreeMap<K, usize>,
    key: K,
    receipts: &[ReceiptRecord],
    index: usize,
) {
    if latest
        .get(&key)
        .is_none_or(|existing| receipt_is_newer(receipts, index, *existing))
    {
        latest.insert(key, index);
    }
}

fn receipt_is_newer(receipts: &[ReceiptRecord], candidate: usize, existing: usize) -> bool {
    receipts[candidate]
        .ended_at_ms
        .cmp(&receipts[existing].ended_at_ms)
        .then_with(|| candidate.cmp(&existing))
        .is_gt()
}

fn receipt_arg_strings<'a>(receipt: &'a ReceiptRecord, key: &str) -> Vec<&'a str> {
    receipt
        .args
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn parse_archive_before_ms(value: &str) -> Result<u64> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--before must not be empty");
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return value
            .parse::<u64>()
            .with_context(|| format!("Invalid --before millisecond timestamp: {value}"));
    }

    let (year, month, day) = parse_utc_date(value)?;
    let month = Month::try_from(month as u8)
        .with_context(|| format!("Invalid --before month in {value}"))?;
    let date = Date::from_calendar_date(year, month, day as u8)
        .with_context(|| format!("Invalid --before date: {value}"))?;
    let timestamp_ms = date.midnight().assume_utc().unix_timestamp() * 1_000;
    if timestamp_ms < 0 {
        bail!("--before date must be on or after 1970-01-01: {value}");
    }
    Ok(timestamp_ms as u64)
}

fn parse_utc_date(value: &str) -> Result<(i32, u32, u32)> {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 3 {
        bail!(
            "Unsupported --before value '{value}'. Use YYYY-MM-DD or a Unix millisecond timestamp."
        );
    }
    let year = parts[0]
        .parse::<i32>()
        .with_context(|| format!("Invalid --before year in {value}"))?;
    if year < 1970 {
        bail!("--before date must be on or after 1970-01-01: {value}");
    }
    let month = parts[1]
        .parse::<u32>()
        .with_context(|| format!("Invalid --before month in {value}"))?;
    let day = parts[2]
        .parse::<u32>()
        .with_context(|| format!("Invalid --before day in {value}"))?;
    if !(1..=12).contains(&month) {
        bail!("Invalid --before month in {value}");
    }
    if day == 0 {
        bail!("Invalid --before day in {value}");
    }
    Ok((year, month, day))
}

pub(crate) fn latest_plan_work_check_receipt_for_tool(
    ctx: &RepoContext,
    plan_id: &str,
    tool_name: &str,
    tool_receipt_id: &str,
) -> Result<Option<ToolReceiptStatus>> {
    latest_plan_work_check_receipt_for_tool_with_cancellation(
        ctx,
        plan_id,
        tool_name,
        tool_receipt_id,
        &|| false,
    )
}

pub(crate) fn latest_plan_work_check_receipt_for_tool_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    tool_name: &str,
    tool_receipt_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<ToolReceiptStatus>> {
    ensure_receipt_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;
    let receipts = read_jsonl_with_cancellation::<ReceiptRecord>(
        &ctx.state_file("receipts.jsonl"),
        cancelled,
    )?;
    let mut tool_receipt_index = None;
    for (index, receipt) in receipts.iter().enumerate() {
        ensure_receipt_scan_active(cancelled)?;
        if receipt.id == tool_receipt_id {
            tool_receipt_index = Some(index);
            break;
        }
    }
    let Some(tool_receipt_index) = tool_receipt_index else {
        return Ok(None);
    };

    let mut exact_batch = None;
    let mut legacy_batch = None;
    for receipt in receipts.iter().skip(tool_receipt_index + 1).rev() {
        ensure_receipt_scan_active(cancelled)?;
        if receipt.plan_id.as_deref() != Some(plan_id)
            || receipt.tool_name != tool::WORK_CHECK
            || receipt.exit_status != 0
            || !receipt_args_include_tool(receipt, tool_name)
        {
            continue;
        }
        if exact_batch.is_none() && receipt_args_include_receipt_id(receipt, tool_receipt_id) {
            exact_batch = Some(receipt);
        }
        if legacy_batch.is_none() && !receipt_args_has_receipt_ids(receipt) {
            legacy_batch = Some(receipt);
        }
    }
    ensure_receipt_scan_active(cancelled)?;
    Ok(exact_batch
        .or(legacy_batch)
        .cloned()
        .map(tool_receipt_status))
}

pub(crate) fn latest_plan_work_review_receipt_for_gate(
    ctx: &RepoContext,
    plan_id: &str,
    gate_id: &str,
) -> Result<Option<WorkReviewReceiptStatus>> {
    latest_plan_work_review_receipt_for_gate_with_cancellation(ctx, plan_id, gate_id, &|| false)
}

pub(crate) fn latest_plan_work_review_receipt_for_gate_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    gate_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<WorkReviewReceiptStatus>> {
    ensure_receipt_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;
    let receipts = read_jsonl_with_cancellation::<ReceiptRecord>(
        &ctx.state_file("receipts.jsonl"),
        cancelled,
    )?;
    for receipt in receipts.into_iter().rev() {
        ensure_receipt_scan_active(cancelled)?;
        if receipt.plan_id.as_deref() == Some(plan_id)
            && receipt.tool_name == tool::WORK_REVIEW
            && receipt.args.get("gate_id").and_then(Value::as_str) == Some(gate_id)
        {
            return Ok(Some(work_review_receipt_status(receipt)));
        }
    }
    ensure_receipt_scan_active(cancelled)?;
    Ok(None)
}

fn ensure_receipt_scan_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

pub(crate) fn current_worktree_fingerprint(ctx: &RepoContext) -> CurrentWorktreeFingerprint {
    current_worktree_fingerprint_from_result(repo_worktree_fingerprint(ctx.root()))
        .expect("blocking worktree fingerprint collection cannot be cancelled")
}

pub(crate) fn current_worktree_fingerprint_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<CurrentWorktreeFingerprint> {
    current_worktree_fingerprint_from_result(repo_worktree_fingerprint_with_cancellation(
        ctx.root(),
        cancelled,
    ))
}

fn current_worktree_fingerprint_from_result(
    result: Result<String>,
) -> Result<CurrentWorktreeFingerprint> {
    match result {
        Ok(fingerprint) => Ok(CurrentWorktreeFingerprint {
            fingerprint: Some(fingerprint),
            error: None,
        }),
        Err(error) if is_worktree_fingerprint_cancellation(&error) => {
            Err(status_collection_cancellation())
        }
        Err(error) => Ok(CurrentWorktreeFingerprint {
            fingerprint: None,
            error: Some(format!("{error:#}")),
        }),
    }
}

pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    ensure_state_layout(ctx)?;
    let mut git_metadata = receipt_git_metadata(
        ctx,
        input.collect_git_metadata,
        input.collect_worktree_fingerprint,
    );
    if let Some(override_result) = input.worktree_fingerprint_override {
        match override_result {
            Ok(fingerprint) => {
                git_metadata.worktree_fingerprint = Some(fingerprint);
                git_metadata.worktree_fingerprint_error = None;
            }
            Err(error) => {
                git_metadata.worktree_fingerprint = None;
                git_metadata.worktree_fingerprint_error = Some(error);
            }
        }
    }
    let receipt = ReceiptRecord {
        id: new_id("receipt"),
        session_id: match input.session_override {
            Some(session_id) => Some(session_id),
            None => current_session(ctx)?,
        },
        plan_id: input.plan_id,
        tool_name: input.tool_name.to_string(),
        args: input.args,
        invoked_command_key: input.invoked_command_key,
        started_at_ms: input.started_at_ms,
        ended_at_ms: input.ended_at_ms,
        exit_status: input.exit_status,
        stdout_preview: truncate(input.stdout),
        stderr_preview: truncate(input.stderr),
        evidence: input.evidence,
        changed_paths: git_metadata.changed_paths,
        diff_stat: git_metadata.diff_stat,
        git_status_error: git_metadata.git_status_error,
        git_diff_stat_error: git_metadata.git_diff_stat_error,
        worktree_fingerprint: git_metadata.worktree_fingerprint,
        worktree_fingerprint_error: git_metadata.worktree_fingerprint_error,
    };
    let receipt_id = receipt.id.clone();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &receipt)?;
    Ok(receipt_id)
}

pub(super) fn record_successful_state_tool(
    ctx: &RepoContext,
    input: StateToolReceipt<'_>,
) -> Result<String> {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: input.tool_name,
            args: input.args,
            invoked_command_key: None,
            plan_id: input.plan_id,
            started_at_ms: input.started_at_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: input.session_override,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
}

fn receipt_matches_filters(receipt: &ReceiptRecord, filter: &ReceiptListFilter) -> bool {
    let session_matches = filter
        .session_id
        .as_ref()
        .is_none_or(|session_id| receipt.session_id.as_ref() == Some(session_id));
    let plan_matches = filter
        .plan_id
        .as_ref()
        .is_none_or(|plan_id| receipt.plan_id.as_ref() == Some(plan_id));
    let tool_matches = filter
        .tool_name
        .as_ref()
        .is_none_or(|tool_name| receipt.tool_name == *tool_name);
    let failure_matches = !filter.failed_only || receipt.exit_status != 0;

    session_matches && plan_matches && tool_matches && failure_matches
}

fn receipt_args_include_tool(receipt: &ReceiptRecord, tool_name: &str) -> bool {
    receipt
        .args
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.iter().any(|tool| tool.as_str() == Some(tool_name)))
}

fn receipt_args_include_receipt_id(receipt: &ReceiptRecord, receipt_id: &str) -> bool {
    receipt
        .args
        .get("receipt_ids")
        .and_then(Value::as_array)
        .is_some_and(|receipt_ids| {
            receipt_ids
                .iter()
                .any(|candidate| candidate.as_str() == Some(receipt_id))
        })
}

fn receipt_args_has_receipt_ids(receipt: &ReceiptRecord) -> bool {
    receipt
        .args
        .get("receipt_ids")
        .and_then(Value::as_array)
        .is_some()
}

pub(super) fn receipt_list_value(receipt: ReceiptRecord) -> Result<Value> {
    let diff_summary = receipt_diff_summary(&receipt);
    let mut value = serde_json::to_value(receipt)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("diff_summary".to_string(), Value::String(diff_summary));
    }
    Ok(value)
}

fn tool_receipt_status(receipt: ReceiptRecord) -> ToolReceiptStatus {
    let diff_summary = receipt_diff_summary(&receipt);
    ToolReceiptStatus {
        receipt_id: receipt.id,
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        changed_paths: receipt.changed_paths,
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint,
        worktree_fingerprint_error: receipt.worktree_fingerprint_error,
    }
}

fn work_review_receipt_status(receipt: ReceiptRecord) -> WorkReviewReceiptStatus {
    let diff_summary = receipt_diff_summary(&receipt);
    WorkReviewReceiptStatus {
        receipt_id: receipt.id,
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        evidence: receipt.evidence.as_ref().map(work_review_receipt_evidence),
        changed_paths: receipt.changed_paths,
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint,
        worktree_fingerprint_error: receipt.worktree_fingerprint_error,
    }
}

fn work_review_receipt_evidence(evidence: &Value) -> WorkReviewReceiptEvidence {
    let retained_finding_count = evidence["findings"].as_array().map(Vec::len);
    let retained_actionable_count = evidence["actionable_findings"].as_array().map(Vec::len);
    let mut parse_error = evidence["parse_error"].as_str().map(str::to_string);
    if parse_error.is_none() && evidence["status"].as_str().is_none() {
        parse_error = Some("review evidence is missing status".into());
    }
    if parse_error.is_none()
        && evidence.get("findings").is_some()
        && retained_finding_count.is_none()
    {
        parse_error = Some("review evidence findings is not an array".into());
    }
    if parse_error.is_none()
        && evidence.get("actionable_findings").is_some()
        && retained_actionable_count.is_none()
    {
        parse_error = Some("review evidence actionable_findings is not an array".into());
    }
    WorkReviewReceiptEvidence {
        status: evidence["status"].as_str().map(str::to_string),
        finding_count: evidence["raw_finding_count"]
            .as_u64()
            .or_else(|| retained_finding_count.map(|count| count as u64)),
        actionable_count: evidence["raw_actionable_count"]
            .as_u64()
            .or_else(|| retained_actionable_count.map(|count| count as u64)),
        retained_finding_count,
        retained_actionable_count,
        findings_truncated: evidence["findings_truncated"].as_bool(),
        actionable_findings_truncated: evidence["actionable_findings_truncated"].as_bool(),
        threshold: evidence["threshold"].as_str().map(str::to_string),
        parse_error,
    }
}

pub(super) fn receipt_diff_summary(receipt: &ReceiptRecord) -> String {
    if receipt.git_status_error.is_some() || receipt.git_diff_stat_error.is_some() {
        return "git metadata unavailable".to_string();
    }

    let stat = &receipt.diff_stat;
    if stat.files == 0 && stat.insertions == 0 && stat.deletions == 0 {
        "no changes".to_string()
    } else {
        let file_count = if stat.files == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", stat.files)
        };
        format!("{file_count}, +{} -{}", stat.insertions, stat.deletions)
    }
}

fn receipt_git_metadata(
    ctx: &RepoContext,
    collect_git_metadata: bool,
    collect_worktree_fingerprint: bool,
) -> GitReceiptMetadata {
    if !collect_git_metadata {
        return GitReceiptMetadata::default();
    }

    if collect_worktree_fingerprint {
        collect_git_receipt_metadata(ctx.root())
    } else {
        collect_git_receipt_metadata_without_worktree_fingerprint(ctx.root())
    }
}
