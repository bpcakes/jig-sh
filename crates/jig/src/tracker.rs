//! Pure, bounded reads of repository-local Beads JSONL exports.
//!
//! JSONL is the task-data boundary. This module never invokes `br`, opens its
//! database, imports or exports state, or mutates the tracker workspace.

use std::collections::BTreeMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, Metadata, OpenOptions},
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(crate) const INPUT_PROFILE: &str = "beads-rust-jsonl-v1";
pub(crate) const PRIMARY_EXPORT: &str = ".beads/issues.jsonl";
pub(crate) const LEGACY_EXPORT: &str = ".beads/beads.jsonl";
const PRIMARY_EXPORT_NAME: &str = "issues.jsonl";
const LEGACY_EXPORT_NAME: &str = "beads.jsonl";

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_ISSUES: usize = 10_000;
const MAX_JSON_DEPTH: usize = 64;
const MAX_ID_BYTES: usize = 256;
const MAX_TITLE_CHARS: usize = 500;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const SEMANTIC_REVISION_DOMAIN: &[u8] = b"jig.tracker.issue.semantic.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "the next delivery milestone consumes exact issue snapshots"
)]
pub(crate) struct TrackerIssue {
    pub(crate) provider: &'static str,
    pub(crate) workspace_id: String,
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) acceptance_criteria: String,
    pub(crate) status: String,
    pub(crate) assignee: Option<String>,
    pub(crate) provider_revision: String,
    pub(crate) semantic_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExportIssue {
    Live(Box<TrackerIssue>),
    Tombstone,
}

#[derive(Debug)]
pub(crate) struct BeadsExport {
    relative_path: PathBuf,
    issues: BTreeMap<String, ExportIssue>,
}

impl BeadsExport {
    pub(crate) fn open(root: &Path, workspace_id: &str) -> Result<Self, BeadsJsonlError> {
        Self::open_with_hooks(root, workspace_id, || {}, || {})
    }

    fn open_with_hooks(
        root: &Path,
        workspace_id: &str,
        after_directory_open: impl FnOnce(),
        before_export_open: impl FnOnce(),
    ) -> Result<Self, BeadsJsonlError> {
        validate_workspace_id(workspace_id)?;
        let repository = Dir::open_ambient_dir(root, ambient_authority())
            .map_err(|_| BeadsJsonlError::InvalidWorkspace)?;
        let tracker = repository
            .open_dir_nofollow(".beads")
            .map_err(|_| BeadsJsonlError::InvalidWorkspace)?;
        let tracker_metadata = tracker
            .dir_metadata()
            .map_err(|_| BeadsJsonlError::InvalidWorkspace)?;
        if !tracker_metadata.is_dir() {
            return Err(BeadsJsonlError::InvalidWorkspace);
        }
        after_directory_open();

        let export_name = select_export(&tracker)?;
        let bytes = read_stable_export(&tracker, export_name, before_export_open)?;
        let confirmed_name =
            select_export(&tracker).map_err(|_| BeadsJsonlError::ChangedDuringRead)?;
        if confirmed_name != export_name {
            return Err(BeadsJsonlError::ChangedDuringRead);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| BeadsJsonlError::InvalidUtf8)?;
        let issues = parse_export(text, workspace_id)?;
        Ok(Self {
            relative_path: Path::new(".beads").join(export_name),
            issues,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.issues.len()
    }

    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    #[allow(
        dead_code,
        reason = "the next delivery milestone consumes exact issue snapshots"
    )]
    pub(crate) fn issue(&self, exact_id: &str) -> Result<&TrackerIssue, BeadsJsonlError> {
        validate_issue_id(exact_id)?;
        match self.issues.get(exact_id) {
            Some(ExportIssue::Live(issue)) => Ok(issue),
            Some(ExportIssue::Tombstone) => Err(BeadsJsonlError::IssueTombstoned),
            None => Err(BeadsJsonlError::IssueMissing),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BeadsJsonlError {
    InvalidWorkspace,
    MissingExport,
    AmbiguousExport,
    UnsafeExport,
    ExportTooLarge,
    ChangedDuringRead,
    InvalidUtf8,
    LineTooLong { line: usize },
    TooManyIssues,
    InvalidRecord { line: usize, reason: &'static str },
    DuplicateIssueId { line: usize },
    InvalidIssueId,
    IssueMissing,
    IssueTombstoned,
}

impl std::fmt::Display for BeadsJsonlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidWorkspace => {
                formatter.write_str("the .beads workspace is not a safe repository-local directory")
            }
            Self::MissingExport => formatter.write_str("no supported Beads JSONL export exists"),
            Self::AmbiguousExport => {
                formatter.write_str("both .beads/issues.jsonl and .beads/beads.jsonl exist")
            }
            Self::UnsafeExport => formatter
                .write_str("the Beads JSONL export is not a safe regular repository-local file"),
            Self::ExportTooLarge => write!(
                formatter,
                "the Beads JSONL export exceeds the {MAX_INPUT_BYTES}-byte limit"
            ),
            Self::ChangedDuringRead => {
                formatter.write_str("the Beads JSONL export changed while it was being read")
            }
            Self::InvalidUtf8 => formatter.write_str("the Beads JSONL export is not UTF-8"),
            Self::LineTooLong { line } => write!(
                formatter,
                "Beads JSONL line {line} exceeds the {MAX_LINE_BYTES}-byte limit"
            ),
            Self::TooManyIssues => write!(
                formatter,
                "the Beads JSONL export exceeds the {MAX_ISSUES}-issue limit"
            ),
            Self::InvalidRecord { line, reason } => {
                write!(formatter, "Beads JSONL line {line} is invalid: {reason}")
            }
            Self::DuplicateIssueId { line } => {
                write!(formatter, "Beads JSONL line {line} repeats an issue ID")
            }
            Self::InvalidIssueId => formatter.write_str("the exact Beads issue ID is invalid"),
            Self::IssueMissing => formatter.write_str("the exact Beads issue ID was not found"),
            Self::IssueTombstoned => formatter.write_str("the exact Beads issue ID is tombstoned"),
        }
    }
}

impl std::error::Error for BeadsJsonlError {}

fn select_export(directory: &Dir) -> Result<&'static str, BeadsJsonlError> {
    let primary_exists = entry_exists(directory, PRIMARY_EXPORT_NAME)?;
    let legacy_exists = entry_exists(directory, LEGACY_EXPORT_NAME)?;
    match (primary_exists, legacy_exists) {
        (true, true) => Err(BeadsJsonlError::AmbiguousExport),
        (true, false) => Ok(PRIMARY_EXPORT_NAME),
        (false, true) => Ok(LEGACY_EXPORT_NAME),
        (false, false) => Err(BeadsJsonlError::MissingExport),
    }
}

fn entry_exists(directory: &Dir, name: &str) -> Result<bool, BeadsJsonlError> {
    match directory.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(BeadsJsonlError::UnsafeExport),
    }
}

fn read_stable_export(
    directory: &Dir,
    name: &str,
    before_open: impl FnOnce(),
) -> Result<Vec<u8>, BeadsJsonlError> {
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| BeadsJsonlError::UnsafeExport)?;
    if named.file_type().is_symlink() || !named.is_file() || has_multiple_links(&named) {
        return Err(BeadsJsonlError::UnsafeExport);
    }
    if named.len() > MAX_INPUT_BYTES as u64 {
        return Err(BeadsJsonlError::ExportTooLarge);
    }
    before_open();

    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| BeadsJsonlError::UnsafeExport)?;
    let opened = file.metadata().map_err(|_| BeadsJsonlError::UnsafeExport)?;
    if !opened.is_file()
        || has_multiple_links(&opened)
        || !same_file_state(&named, &opened)
        || opened.len() > MAX_INPUT_BYTES as u64
    {
        return Err(if opened.len() > MAX_INPUT_BYTES as u64 {
            BeadsJsonlError::ExportTooLarge
        } else {
            BeadsJsonlError::UnsafeExport
        });
    }

    let first = read_capped(&mut file)?;
    file.rewind()
        .map_err(|_| BeadsJsonlError::ChangedDuringRead)?;
    let second = read_capped(&mut file)?;
    let after = file
        .metadata()
        .map_err(|_| BeadsJsonlError::ChangedDuringRead)?;
    let named_after = directory
        .symlink_metadata(name)
        .map_err(|_| BeadsJsonlError::ChangedDuringRead)?;
    if first != second
        || !same_file_state(&opened, &after)
        || !same_file_state(&opened, &named_after)
    {
        return Err(BeadsJsonlError::ChangedDuringRead);
    }
    Ok(first)
}

fn read_capped(file: &mut File) -> Result<Vec<u8>, BeadsJsonlError> {
    let mut bytes = Vec::new();
    file.take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| BeadsJsonlError::UnsafeExport)?;
    if bytes.len() > MAX_INPUT_BYTES {
        Err(BeadsJsonlError::ExportTooLarge)
    } else {
        Ok(bytes)
    }
}

#[cfg(unix)]
fn has_multiple_links(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    metadata.nlink() != 1
}

#[cfg(not(unix))]
const fn has_multiple_links(_metadata: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn same_file_state(before: &Metadata, after: &Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
}

#[cfg(not(unix))]
fn same_file_state(before: &Metadata, after: &Metadata) -> bool {
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}

fn parse_export(
    input: &str,
    workspace_id: &str,
) -> Result<BTreeMap<String, ExportIssue>, BeadsJsonlError> {
    let mut issues = BTreeMap::new();
    for (index, raw_line) in input.split_terminator('\n').enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.len() > MAX_LINE_BYTES {
            return Err(BeadsJsonlError::LineTooLong { line: line_number });
        }
        if line.is_empty() {
            return Err(invalid(line_number, "blank record"));
        }
        if issues.len() == MAX_ISSUES {
            return Err(BeadsJsonlError::TooManyIssues);
        }
        let value = crate::strict_json::from_slice(line.as_bytes())
            .map_err(|_| invalid(line_number, "malformed JSON or duplicate object key"))?;
        validate_json_value(&value, 1, line_number)?;
        let (id, issue) = parse_issue(value, workspace_id, line_number)?;
        if issues.insert(id, issue).is_some() {
            return Err(BeadsJsonlError::DuplicateIssueId { line: line_number });
        }
    }
    Ok(issues)
}

fn validate_json_value(value: &Value, depth: usize, line: usize) -> Result<(), BeadsJsonlError> {
    if depth > MAX_JSON_DEPTH {
        return Err(invalid(line, "JSON nesting exceeds the supported limit"));
    }
    match value {
        Value::String(text) if text.len() > MAX_TEXT_BYTES => {
            Err(invalid(line, "a text field exceeds the supported limit"))
        }
        Value::Array(values) => values
            .iter()
            .try_for_each(|value| validate_json_value(value, depth + 1, line)),
        Value::Object(values) => values
            .values()
            .try_for_each(|value| validate_json_value(value, depth + 1, line)),
        _ => Ok(()),
    }
}

fn parse_issue(
    value: Value,
    workspace_id: &str,
    line: usize,
) -> Result<(String, ExportIssue), BeadsJsonlError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(line, "record is not an object"))?;
    let id = required_text(object, "id", line)?;
    validate_issue_id(id).map_err(|_| invalid(line, "invalid issue identity"))?;
    let title = required_text(object, "title", line)?;
    if title.trim().is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(invalid(line, "invalid issue title"));
    }
    let status = required_text(object, "status", line)?;
    if !matches!(
        status,
        "open"
            | "in_progress"
            | "blocked"
            | "deferred"
            | "draft"
            | "closed"
            | "tombstone"
            | "pinned"
    ) {
        return Err(invalid(line, "unsupported issue status"));
    }
    if !matches!(
        required_text(object, "issue_type", line)?,
        "task" | "bug" | "feature" | "epic" | "chore" | "docs" | "question"
    ) {
        return Err(invalid(line, "unsupported issue type"));
    }
    if !object
        .get("priority")
        .and_then(Value::as_u64)
        .is_some_and(|priority| priority <= 4)
    {
        return Err(invalid(line, "invalid issue priority"));
    }
    let created_at = required_text(object, "created_at", line)?;
    let updated_at = required_text(object, "updated_at", line)?;
    validate_timestamp(created_at, line)?;
    validate_timestamp(updated_at, line)?;
    let deleted_at = optional_text(object, "deleted_at", line)?;
    if let Some(timestamp) = deleted_at {
        validate_timestamp(timestamp, line)?;
    }
    let id = id.to_string();
    if status == "tombstone" || deleted_at.is_some() {
        return Ok((id, ExportIssue::Tombstone));
    }

    let description = optional_text(object, "description", line)?
        .unwrap_or_default()
        .to_string();
    let acceptance_criteria = optional_text(object, "acceptance_criteria", line)?
        .unwrap_or_default()
        .to_string();
    let assignee = optional_text(object, "assignee", line)?.map(str::to_string);
    let semantic_revision =
        semantic_revision(workspace_id, &id, title, &description, &acceptance_criteria);
    Ok((
        id.clone(),
        ExportIssue::Live(Box::new(TrackerIssue {
            provider: "beads",
            workspace_id: workspace_id.to_string(),
            id,
            title: title.to_string(),
            description,
            acceptance_criteria,
            status: status.to_string(),
            assignee,
            provider_revision: updated_at.to_string(),
            semantic_revision,
        })),
    ))
}

fn required_text<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<&'a str, BeadsJsonlError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| invalid(line, "missing or invalid required field"))
}

fn optional_text<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<Option<&'a str>, BeadsJsonlError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.contains('\0') => Ok(Some(value)),
        Some(_) => Err(invalid(line, "invalid optional field")),
    }
}

fn validate_timestamp(value: &str, line: usize) -> Result<(), BeadsJsonlError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| invalid(line, "invalid RFC3339 timestamp"))
}

fn validate_workspace_id(workspace_id: &str) -> Result<(), BeadsJsonlError> {
    let parsed = workspace_id
        .parse::<ulid::Ulid>()
        .map_err(|_| BeadsJsonlError::InvalidWorkspace)?;
    if workspace_id.len() == 26 && parsed.to_string() == workspace_id {
        Ok(())
    } else {
        Err(BeadsJsonlError::InvalidWorkspace)
    }
}

fn validate_issue_id(issue_id: &str) -> Result<(), BeadsJsonlError> {
    if issue_id.is_empty()
        || issue_id.len() > MAX_ID_BYTES
        || !issue_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        Err(BeadsJsonlError::InvalidIssueId)
    } else {
        Ok(())
    }
}

fn semantic_revision(
    workspace_id: &str,
    issue_id: &str,
    title: &str,
    description: &str,
    acceptance_criteria: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(SEMANTIC_REVISION_DOMAIN);
    for field in [
        workspace_id,
        issue_id,
        title,
        description,
        acceptance_criteria,
    ] {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field.as_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

const fn invalid(line: usize, reason: &'static str) -> BeadsJsonlError {
    BeadsJsonlError::InvalidRecord { line, reason }
}

#[cfg(test)]
mod tests;
