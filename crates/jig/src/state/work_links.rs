//! Immutable plan-to-issue links stored independently from historical plan events.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::context::RepoContext;

use self::projection::{scan_journal, scan_journal_locked};
use super::jsonl::{
    append_jsonl_durable_locked, confirm_jsonl_durable_locked, with_jsonl_write_lock,
};

mod projection;

pub(crate) const WORK_LINKS_FILE: &str = "work-links.jsonl";
pub(crate) const WORK_LINK_SCHEMA_VERSION: u32 = 1;

use super::tracker_identity::{
    PROVIDER_BEADS, TRACKER_ROOT_BEADS, validate_portable_identifier,
    validate_portable_tracker_issue,
};

const SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"jig-work-link-snapshot-v1\0";
const MAX_EVENT_ID_BYTES: usize = 128;
const MAX_TITLE_BYTES: usize = 16 * 1024;
pub(crate) const MAX_WORK_LINK_RECORD_BYTES: usize = 2 * 1024 * 1024;
const MAX_JOURNAL_DIAGNOSTIC_SAMPLES: usize = 20;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkLinkEstablishedBy {
    Start,
    Attach,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkLinkIssueV1 {
    pub(crate) provider: String,
    pub(crate) workspace_id: String,
    pub(crate) issue_id: String,
    pub(crate) tracker_root: String,
}

impl WorkLinkIssueV1 {
    pub(crate) fn beads(
        workspace_id: impl Into<String>,
        issue_id: impl Into<String>,
    ) -> Result<Self> {
        let issue = Self {
            provider: PROVIDER_BEADS.into(),
            workspace_id: workspace_id.into(),
            issue_id: issue_id.into(),
            tracker_root: TRACKER_ROOT_BEADS.into(),
        };
        issue.validate()?;
        Ok(issue)
    }

    fn validate(&self) -> Result<()> {
        validate_portable_tracker_issue(
            &self.provider,
            &self.workspace_id,
            &self.issue_id,
            &self.tracker_root,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkLinkSnapshotV1 {
    pub(crate) observed_at_ms: u64,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) acceptance_criteria: String,
    pub(crate) context_digest: String,
}

impl WorkLinkSnapshotV1 {
    pub(crate) fn new(
        observed_at_ms: u64,
        title: impl Into<String>,
        description: impl Into<String>,
        acceptance_criteria: impl Into<String>,
    ) -> Result<Self> {
        let title = title.into();
        let description = description.into();
        let acceptance_criteria = acceptance_criteria.into();
        validate_snapshot_text(observed_at_ms, &title, &description, &acceptance_criteria)?;
        Ok(Self {
            observed_at_ms,
            context_digest: snapshot_context_digest(&title, &description, &acceptance_criteria),
            title,
            description,
            acceptance_criteria,
        })
    }

    fn validate(&self) -> Result<()> {
        validate_snapshot_text(
            self.observed_at_ms,
            &self.title,
            &self.description,
            &self.acceptance_criteria,
        )?;
        let expected =
            snapshot_context_digest(&self.title, &self.description, &self.acceptance_criteria);
        if self.context_digest != expected {
            bail!(
                "work-link snapshot context_digest does not match its title, description, and acceptance criteria"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkLinkRecordV1 {
    pub(crate) id: String,
    pub(crate) schema_version: u32,
    pub(crate) plan_id: String,
    pub(crate) issue: WorkLinkIssueV1,
    pub(crate) snapshot: WorkLinkSnapshotV1,
    pub(crate) established_by: WorkLinkEstablishedBy,
}

impl WorkLinkRecordV1 {
    fn from_request(id: String, request: &WorkLinkRequest) -> Self {
        Self {
            id,
            schema_version: WORK_LINK_SCHEMA_VERSION,
            plan_id: request.plan_id.clone(),
            issue: request.issue.clone(),
            snapshot: request.snapshot.clone(),
            established_by: request.established_by,
        }
    }

    fn validate(&self) -> Result<()> {
        validate_event_id(&self.id)?;
        if self.schema_version != WORK_LINK_SCHEMA_VERSION {
            bail!(
                "unsupported work-link schema version {}",
                self.schema_version
            );
        }
        super::plan_files::validate_plan_id(&self.plan_id)?;
        self.issue.validate()?;
        self.snapshot.validate()?;
        Ok(())
    }

    fn same_link(&self, request: &WorkLinkRequest) -> bool {
        self.plan_id == request.plan_id && self.issue == request.issue
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkLinkRequest {
    pub(crate) plan_id: String,
    pub(crate) issue: WorkLinkIssueV1,
    pub(crate) snapshot: WorkLinkSnapshotV1,
    pub(crate) established_by: WorkLinkEstablishedBy,
}

impl WorkLinkRequest {
    pub(crate) fn new(
        plan_id: impl Into<String>,
        issue: WorkLinkIssueV1,
        snapshot: WorkLinkSnapshotV1,
        established_by: WorkLinkEstablishedBy,
    ) -> Result<Self> {
        let request = Self {
            plan_id: plan_id.into(),
            issue,
            snapshot,
            established_by,
        };
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<()> {
        super::plan_files::validate_plan_id(&self.plan_id)?;
        self.issue.validate()?;
        self.snapshot.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SupportedWorkLink {
    pub(crate) record: WorkLinkRecordV1,
    /// All distinct event IDs that encoded the same immutable link.
    pub(crate) event_ids: Vec<String>,
    /// Exact full-record replays (same event ID and all JSON fields).
    pub(crate) replayed_records: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkLinkDiagnostic {
    pub(crate) line_number: Option<u64>,
    pub(crate) event_id: Option<String>,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkLinkProjection {
    Unlinked,
    Supported(Box<SupportedWorkLink>),
    Conflict(Vec<WorkLinkDiagnostic>),
    Unsupported(Vec<WorkLinkDiagnostic>),
    Corrupt(Vec<WorkLinkDiagnostic>),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkLinkJournalAuthority {
    #[default]
    Empty,
    Supported,
    Unsupported,
    Conflicting,
    Corrupt,
    Torn,
    Unreadable,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct WorkLinkJournalDiagnostics {
    pub(crate) authority: WorkLinkJournalAuthority,
    pub(crate) known_plans: u64,
    pub(crate) supported_plans: u64,
    pub(crate) unsupported_plans: u64,
    pub(crate) conflicting_plans: u64,
    pub(crate) corrupt_plans: u64,
    pub(crate) supported_records: u64,
    pub(crate) replayed_records: u64,
    pub(crate) torn_tail: bool,
    pub(crate) error_count: u64,
    pub(crate) errors: Vec<WorkLinkJournalDiagnosticSample>,
    pub(crate) errors_truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct WorkLinkJournalDiagnosticSample {
    pub(crate) plan_id: Option<String>,
    pub(crate) line_number: Option<u64>,
    pub(crate) event_id: Option<String>,
    pub(crate) message: String,
}

/// Return bounded semantic facts from the same fail-closed projection used by
/// work-link readers. Missing journals remain an empty, read-only authority.
pub(crate) fn work_link_journal_diagnostics_from_path(path: &Path) -> WorkLinkJournalDiagnostics {
    match scan_journal(path, None) {
        Ok(journal) => journal.diagnostics(),
        Err(error) => WorkLinkJournalDiagnostics {
            authority: if error
                .downcast_ref::<super::jsonl::JsonlRecordTooLarge>()
                .is_some()
            {
                WorkLinkJournalAuthority::Corrupt
            } else if error
                .downcast_ref::<projection::WorkLinkProjectionLimit>()
                .is_some()
            {
                WorkLinkJournalAuthority::Unsupported
            } else {
                WorkLinkJournalAuthority::Unreadable
            },
            error_count: 1,
            errors: vec![WorkLinkJournalDiagnosticSample {
                plan_id: None,
                line_number: None,
                event_id: None,
                message: super::support::truncate(&format!("{error:#}")),
            }],
            ..WorkLinkJournalDiagnostics::default()
        },
    }
}

/// Project one plan's work link without creating a missing journal.
pub(crate) fn project_work_link(ctx: &RepoContext, plan_id: &str) -> Result<WorkLinkProjection> {
    super::plan_files::validate_plan_id(plan_id)?;
    let path = ctx.state_file(WORK_LINKS_FILE);
    let journal = scan_journal(&path, Some(plan_id))?;
    Ok(journal.for_plan(plan_id))
}

/// Append an immutable link, or return the existing record for an exact retry.
///
/// The epoch check and plan lookup occur before any work-link file or lock is
/// created. The authoritative scan and optional append share one JSONL lock.
pub(crate) fn attach_work_link(
    ctx: &RepoContext,
    request: &WorkLinkRequest,
) -> Result<WorkLinkRecordV1> {
    attach_work_link_with_limits(ctx, request, projection::PRODUCTION_LIMITS)
}

fn attach_work_link_with_limits(
    ctx: &RepoContext,
    request: &WorkLinkRequest,
    limits: projection::ProjectionLimits,
) -> Result<WorkLinkRecordV1> {
    if !crate::context::supports_work_links(ctx.contract_version()) {
        bail!(
            "work-link writes require repository contract epoch {} (found {})",
            crate::context::WORK_LINK_CONTRACT_VERSION,
            ctx.contract_version()
        );
    }
    request.validate()?;
    super::plans::ensure_plan_exists(ctx, &request.plan_id)?;

    let path = ctx.state_file(WORK_LINKS_FILE);
    with_jsonl_write_lock(&path, |guard| {
        let mut journal = scan_journal_locked(guard, &path, Some(&request.plan_id), limits)?;
        journal.ensure_authoritative_write_safe()?;
        match journal.for_plan(&request.plan_id) {
            WorkLinkProjection::Unlinked => {
                let record =
                    WorkLinkRecordV1::from_request(super::support::new_id("work-link"), request);
                record.validate()?;
                let encoded = serde_json::to_vec(&record)?;
                if encoded.len() > MAX_WORK_LINK_RECORD_BYTES {
                    bail!("work-link record exceeds the {MAX_WORK_LINK_RECORD_BYTES}-byte limit");
                }
                journal.admit_candidate(&encoded)?;
                append_jsonl_durable_locked(guard, &path, &record)?;
                Ok(record)
            }
            WorkLinkProjection::Supported(existing) if existing.record.same_link(request) => {
                // The first append may have made the record visible before a
                // durability sync reported failure. Exact retry success must
                // re-confirm the file and its directory entry.
                confirm_jsonl_durable_locked(guard, &path)?;
                Ok(existing.record)
            }
            WorkLinkProjection::Supported(existing) => bail!(
                "plan {} already has immutable work link {}:{} in workspace {}; refusing a distinct link",
                request.plan_id,
                existing.record.issue.provider,
                existing.record.issue.issue_id,
                existing.record.issue.workspace_id
            ),
            WorkLinkProjection::Conflict(diagnostics) => bail!(
                "work-link authority for plan {} is conflicting: {}",
                request.plan_id,
                diagnostic_summary(&diagnostics)
            ),
            WorkLinkProjection::Unsupported(diagnostics) => bail!(
                "work-link authority for plan {} is unsupported: {}",
                request.plan_id,
                diagnostic_summary(&diagnostics)
            ),
            WorkLinkProjection::Corrupt(diagnostics) => bail!(
                "work-link authority for plan {} is corrupt: {}",
                request.plan_id,
                diagnostic_summary(&diagnostics)
            ),
        }
    })
}

fn validate_event_id(id: &str) -> Result<()> {
    validate_portable_identifier("work-link event id", id, MAX_EVENT_ID_BYTES)
}

fn validate_snapshot_text(
    observed_at_ms: u64,
    title: &str,
    description: &str,
    acceptance_criteria: &str,
) -> Result<()> {
    if observed_at_ms == 0 {
        bail!("work-link snapshot observed_at_ms must be greater than zero");
    }
    if title.is_empty() || title.len() > MAX_TITLE_BYTES || title.contains('\0') {
        bail!("work-link snapshot title must contain 1 through {MAX_TITLE_BYTES} bytes and no NUL");
    }
    if description.contains('\0') {
        bail!("work-link description must contain no NUL");
    }
    if acceptance_criteria.contains('\0') {
        bail!("work-link acceptance criteria must contain no NUL");
    }
    // Reject text that cannot possibly fit before hashing it. The writer also
    // checks the full serialized record, including JSON escaping and metadata.
    // Separate field limits must not reject content from a supported Beads record.
    if title
        .len()
        .saturating_add(description.len())
        .saturating_add(acceptance_criteria.len())
        > MAX_WORK_LINK_RECORD_BYTES
    {
        bail!("work-link snapshot text exceeds the {MAX_WORK_LINK_RECORD_BYTES}-byte record limit");
    }
    Ok(())
}

fn snapshot_context_digest(title: &str, description: &str, acceptance_criteria: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(SNAPSHOT_DIGEST_DOMAIN);
    hash_field(&mut digest, title.as_bytes());
    hash_field(&mut digest, description.as_bytes());
    hash_field(&mut digest, acceptance_criteria.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn hash_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn diagnostic_summary(diagnostics: &[WorkLinkDiagnostic]) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "no diagnostic details".into())
}

#[cfg(test)]
mod tests;
