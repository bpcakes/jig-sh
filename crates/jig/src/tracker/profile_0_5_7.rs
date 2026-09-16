use std::path::PathBuf;

use serde_json::Value;

use super::{
    TrackerComment, TrackerError, TrackerIssue, TrackerMutation, TrackerOperation,
    semantic_revision,
};

#[derive(Debug)]
pub(super) struct BeadsInfo {
    pub beads_dir: PathBuf,
    pub database_path: PathBuf,
    pub jsonl_path: PathBuf,
}

pub(super) fn parse_version(value: &Value) -> Result<String, TrackerError> {
    reject_error_member(value, TrackerOperation::Version)?;
    let version = required_string(value, "version", TrackerOperation::Version)?;
    if version.len() > 64
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(unsupported(TrackerOperation::Version));
    }
    Ok(version.into())
}

pub(super) fn parse_info(value: &Value) -> Result<BeadsInfo, TrackerError> {
    reject_error_member(value, TrackerOperation::Info)?;
    Ok(BeadsInfo {
        beads_dir: PathBuf::from(required_string(value, "path", TrackerOperation::Info)?),
        database_path: PathBuf::from(required_string(
            value,
            "database_path",
            TrackerOperation::Info,
        )?),
        jsonl_path: PathBuf::from(required_string(
            value,
            "jsonl_path",
            TrackerOperation::Info,
        )?),
    })
}

pub(super) fn parse_issue(
    value: &Value,
    workspace_id: &str,
    expected_id: &str,
) -> Result<TrackerIssue, TrackerError> {
    let values = value
        .as_array()
        .filter(|values| values.len() == 1)
        .ok_or_else(|| unsupported(TrackerOperation::ShowIssue))?;
    let issue = values
        .first()
        .ok_or_else(|| unsupported(TrackerOperation::ShowIssue))?;
    reject_error_member(issue, TrackerOperation::ShowIssue)?;
    let id = required_string(issue, "id", TrackerOperation::ShowIssue)?;
    if id != expected_id {
        return Err(unsupported(TrackerOperation::ShowIssue));
    }
    if optional_string(issue, "status", TrackerOperation::ShowIssue)? == Some("tombstone")
        || issue
            .get("deleted_at")
            .is_some_and(|value| !value.is_null())
    {
        return Err(TrackerError::IssueTombstoned {
            issue_id: expected_id.into(),
        });
    }
    let title = required_string(issue, "title", TrackerOperation::ShowIssue)?.to_string();
    let description = optional_string(issue, "description", TrackerOperation::ShowIssue)?
        .unwrap_or_default()
        .to_string();
    let acceptance_criteria =
        optional_string(issue, "acceptance_criteria", TrackerOperation::ShowIssue)?
            .unwrap_or_default()
            .to_string();
    let status = required_string(issue, "status", TrackerOperation::ShowIssue)?.to_string();
    let assignee =
        optional_string(issue, "assignee", TrackerOperation::ShowIssue)?.map(str::to_string);
    let provider_revision =
        optional_string(issue, "updated_at", TrackerOperation::ShowIssue)?.map(str::to_string);
    let revision = semantic_revision(workspace_id, id, &title, &description, &acceptance_criteria);
    Ok(TrackerIssue {
        provider: "beads",
        workspace_id: workspace_id.into(),
        id: id.into(),
        title,
        description,
        acceptance_criteria,
        status,
        assignee,
        provider_revision,
        semantic_revision: revision,
    })
}

pub(super) fn parse_comments(
    value: &Value,
    expected_issue_id: &str,
) -> Result<Vec<TrackerComment>, TrackerError> {
    value
        .as_array()
        .ok_or_else(|| unsupported(TrackerOperation::ListComments))?
        .iter()
        .map(|value| {
            reject_error_member(value, TrackerOperation::ListComments)?;
            parse_comment_for_operation(value, expected_issue_id, TrackerOperation::ListComments)
        })
        .collect()
}

pub(super) fn parse_comment(
    value: &Value,
    expected_issue_id: &str,
    expected_author: &str,
    expected_text: &str,
) -> Result<TrackerComment, TrackerError> {
    reject_error_member(value, TrackerOperation::AddComment)?;
    let comment =
        parse_comment_for_operation(value, expected_issue_id, TrackerOperation::AddComment)?;
    if comment.author != expected_author || comment.text != expected_text {
        return Err(unsupported(TrackerOperation::AddComment));
    }
    Ok(comment)
}

fn parse_comment_for_operation(
    value: &Value,
    expected_issue_id: &str,
    operation: TrackerOperation,
) -> Result<TrackerComment, TrackerError> {
    let issue_id = required_string(value, "issue_id", operation)?;
    if issue_id != expected_issue_id {
        return Err(unsupported(operation));
    }
    let id = value
        .get("id")
        .and_then(|value| match value {
            Value::String(value) if !value.is_empty() => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .ok_or_else(|| unsupported(operation))?;
    Ok(TrackerComment {
        id,
        issue_id: issue_id.into(),
        author: required_string(value, "author", operation)?.into(),
        text: required_string(value, "text", operation)?.into(),
        created_at: required_string(value, "created_at", operation)?.into(),
    })
}

pub(super) fn parse_claim(
    value: &Value,
    expected_id: &str,
    expected_actor: &str,
) -> Result<TrackerMutation, TrackerError> {
    let issue = one_mutation_item(value, TrackerOperation::ClaimIssue, expected_id)?;
    let status = required_string(issue, "status", TrackerOperation::ClaimIssue)?;
    let assignee = required_string(issue, "assignee", TrackerOperation::ClaimIssue)?;
    if status != "in_progress" || assignee != expected_actor {
        return Err(unsupported(TrackerOperation::ClaimIssue));
    }
    mutation_from_issue(issue, expected_id, TrackerOperation::ClaimIssue)
}

pub(super) fn parse_close(
    value: &Value,
    expected_id: &str,
) -> Result<TrackerMutation, TrackerError> {
    let issue = one_mutation_item(value, TrackerOperation::CloseIssue, expected_id)?;
    let status = required_string(issue, "status", TrackerOperation::CloseIssue)?;
    if status != "closed" {
        return Err(unsupported(TrackerOperation::CloseIssue));
    }
    mutation_from_issue(issue, expected_id, TrackerOperation::CloseIssue)
}

fn one_mutation_item<'a>(
    value: &'a Value,
    operation: TrackerOperation,
    expected_id: &str,
) -> Result<&'a Value, TrackerError> {
    let values = value.as_array().ok_or_else(|| unsupported(operation))?;
    if values.len() != 1 {
        return Err(unsupported(operation));
    }
    let issue = values.first().ok_or_else(|| unsupported(operation))?;
    reject_error_member(issue, operation)?;
    if required_string(issue, "id", operation)? != expected_id {
        return Err(unsupported(operation));
    }
    Ok(issue)
}

fn mutation_from_issue(
    issue: &Value,
    issue_id: &str,
    operation: TrackerOperation,
) -> Result<TrackerMutation, TrackerError> {
    Ok(TrackerMutation {
        issue_id: issue_id.into(),
        status: optional_string(issue, "status", operation)?.map(str::to_string),
        assignee: optional_string(issue, "assignee", operation)?.map(str::to_string),
        provider_revision: optional_string(issue, "updated_at", operation)?.map(str::to_string),
    })
}

pub(super) fn require_mutation_ready(value: &Value) -> Result<(), TrackerError> {
    reject_error_member(value, TrackerOperation::SyncStatus)?;
    let jsonl_newer = required_bool(value, "jsonl_newer")?;
    let db_newer = required_bool(value, "db_newer")?;
    let coverage_drift = required_bool(value, "coverage_drift")?;
    let workspace_health =
        required_string(value, "workspace_health", TrackerOperation::SyncStatus)?;
    let audit = value
        .get("reliability_audit")
        .and_then(Value::as_object)
        .ok_or_else(|| unsupported(TrackerOperation::SyncStatus))?;
    let audit_health = audit
        .get("health")
        .and_then(Value::as_str)
        .ok_or_else(|| unsupported(TrackerOperation::SyncStatus))?;
    if audit.get("source").and_then(Value::as_str) != Some("sync.status") {
        return Err(unsupported(TrackerOperation::SyncStatus));
    }
    let anomalies = audit
        .get("anomalies")
        .and_then(Value::as_array)
        .ok_or_else(|| unsupported(TrackerOperation::SyncStatus))?;
    if audit.get("anomaly_count").and_then(Value::as_u64) != Some(anomalies.len() as u64)
        || anomalies.iter().any(|anomaly| {
            anomaly.get("code").and_then(Value::as_str).is_none()
                || anomaly.get("severity").and_then(Value::as_str).is_none()
        })
    {
        return Err(unsupported(TrackerOperation::SyncStatus));
    }
    let only_db_newer = db_newer
        && !jsonl_newer
        && !coverage_drift
        && anomalies.len() == 1
        && anomalies.first().is_some_and(|anomaly| {
            anomaly.get("code").and_then(Value::as_str) == Some("db_newer")
                && anomaly.get("severity").and_then(Value::as_str) == Some("degraded")
        });
    let healthy = !db_newer
        && !jsonl_newer
        && !coverage_drift
        && anomalies.is_empty()
        && workspace_health == "healthy"
        && audit_health == "healthy";
    let allowed_manual_pending =
        only_db_newer && workspace_health == "degraded" && audit_health == "degraded";
    if healthy || allowed_manual_pending {
        Ok(())
    } else {
        Err(TrackerError::StaleStorage)
    }
}

pub(super) fn classify_provider_error(
    stdout: &[u8],
    stderr: &[u8],
    operation: TrackerOperation,
    issue_id: Option<&str>,
) -> Option<TrackerError> {
    let value = if stderr.iter().all(u8::is_ascii_whitespace) {
        crate::strict_json::from_slice(stdout).ok()?
    } else if stdout.iter().all(u8::is_ascii_whitespace) {
        crate::strict_json::from_slice(stderr).ok()?
    } else {
        return None;
    };
    let (code, retryable) = error_envelope(&value)?;
    let expected_retryable = match code {
        "WORKFLOW_CAPACITY_EXCEEDED" | "AMBIGUOUS_ID" => true,
        "SYNC_CONFLICT"
        | "STALE_DB"
        | "STALE_STORAGE"
        | "ISSUE_NOT_FOUND"
        | "POLICY_VIOLATION"
        | "DEPENDENCY_BLOCKED"
        | "BLOCKED_TRANSITION"
        | "ASSIGNMENT_CONFLICT"
        | "ALREADY_CLAIMED" => false,
        _ => return None,
    };
    if retryable != expected_retryable {
        return None;
    }
    if matches!(code, "SYNC_CONFLICT" | "STALE_DB" | "STALE_STORAGE") {
        return Some(TrackerError::StaleStorage);
    }
    if !matches!(
        operation,
        TrackerOperation::ShowIssue
            | TrackerOperation::ListComments
            | TrackerOperation::AddComment
            | TrackerOperation::ClaimIssue
            | TrackerOperation::CloseIssue
    ) {
        return None;
    }
    let issue_id = issue_id?.to_string();
    match code {
        "ISSUE_NOT_FOUND" => Some(TrackerError::IssueMissing { issue_id }),
        "POLICY_VIOLATION" | "DEPENDENCY_BLOCKED" | "BLOCKED_TRANSITION" => {
            Some(TrackerError::BlockedTransition { issue_id })
        }
        "WORKFLOW_CAPACITY_EXCEEDED" | "ASSIGNMENT_CONFLICT" | "ALREADY_CLAIMED" => {
            Some(TrackerError::AssignmentConflict { issue_id })
        }
        "AMBIGUOUS_ID" => Some(TrackerError::AmbiguousIssueId { issue_id }),
        _ => None,
    }
}

/// Classify the structured payload emitted by `br close` when every requested
/// issue is skipped. Version 0.5.7 writes the skipped-result document followed
/// by its `NOTHING_TO_DO` error document to stdout and leaves stderr empty. The
/// adapter closes one issue per invocation, so that exact two-document sequence
/// with one matching skipped entry and no closed entries proves no write landed.
pub(super) fn classify_close_noop(
    stdout: &[u8],
    stderr: &[u8],
    expected_id: &str,
) -> Option<TrackerError> {
    if !stderr.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let [result, envelope] = two_strict_json_documents(stdout)?;
    let (code, retryable) = error_envelope(&envelope)?;
    if code != "NOTHING_TO_DO" || retryable {
        return None;
    }
    let result = result.as_object()?;
    if result.len() != 3 {
        return None;
    }
    let closed = result.get("closed")?.as_array()?;
    let skipped = result.get("skipped")?.as_array()?;
    let warnings = result.get("warnings")?.as_array()?;
    if !closed.is_empty() || skipped.len() != 1 || !warnings.is_empty() {
        return None;
    }
    let skipped = skipped.first()?.as_object()?;
    if skipped.len() != 2 {
        return None;
    }
    if skipped.get("id")?.as_str()? != expected_id {
        return None;
    }
    let reason = skipped.get("reason")?.as_str()?;
    if reason == "issue not found" {
        Some(TrackerError::IssueMissing {
            issue_id: expected_id.into(),
        })
    } else if reason.is_empty() || reason.contains('\0') {
        None
    } else {
        Some(TrackerError::BlockedTransition {
            issue_id: expected_id.into(),
        })
    }
}

fn error_envelope(value: &Value) -> Option<(&str, bool)> {
    let root = value.as_object()?;
    if root.len() != 1 {
        return None;
    }
    let error = root.get("error")?.as_object()?;
    if error.len() != 5 {
        return None;
    }
    let code = error.get("code")?.as_str()?;
    let message = error.get("message")?.as_str()?;
    if message.is_empty() || message.contains('\0') {
        return None;
    }
    match error.get("hint")? {
        Value::Null => {}
        Value::String(hint) if !hint.contains('\0') => {}
        _ => return None,
    }
    match error.get("context")? {
        Value::Null | Value::Object(_) => {}
        _ => return None,
    }
    Some((code, error.get("retryable")?.as_bool()?))
}

fn two_strict_json_documents(bytes: &[u8]) -> Option<[Value; 2]> {
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Value>();
    stream.next()?.ok()?;
    let first_end = stream.byte_offset();
    stream.next()?.ok()?;
    let second_end = stream.byte_offset();
    if stream.next().is_some() || !bytes.get(second_end..)?.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    Some([
        crate::strict_json::from_slice(bytes.get(..first_end)?).ok()?,
        crate::strict_json::from_slice(bytes.get(first_end..second_end)?).ok()?,
    ])
}

fn required_string<'a>(
    value: &'a Value,
    field: &str,
    operation: TrackerOperation,
) -> Result<&'a str, TrackerError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| unsupported(operation))
}

fn optional_string<'a>(
    value: &'a Value,
    field: &str,
    operation: TrackerOperation,
) -> Result<Option<&'a str>, TrackerError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.contains('\0') => Ok(Some(value)),
        Some(_) => Err(unsupported(operation)),
    }
}

fn required_bool(value: &Value, field: &str) -> Result<bool, TrackerError> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| unsupported(TrackerOperation::SyncStatus))
}

fn reject_error_member(value: &Value, operation: TrackerOperation) -> Result<(), TrackerError> {
    if value
        .as_object()
        .is_some_and(|object| object.contains_key("error"))
    {
        Err(unsupported(operation))
    } else {
        Ok(())
    }
}

const fn unsupported(operation: TrackerOperation) -> TrackerError {
    TrackerError::UnsupportedResponse { operation }
}
