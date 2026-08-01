#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use flate2::{Compression, GzBuilder, read::GzDecoder, write::GzEncoder};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use time::{Date, Month};
use ulid::Ulid;

use crate::cancellation::{ensure_status_collection_active, status_collection_cancellation};
use crate::context::{RepoContext, WorkGate};
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_without_worktree_fingerprint,
    is_worktree_fingerprint_cancellation, repo_worktree_fingerprint,
    repo_worktree_fingerprint_with_cancellation,
};
use crate::tool_defs::tool;

use super::MAINTENANCE_WRITER_COORDINATION_NOTE;
use super::compression::{create_dir_all_synced, sync_directory};
use super::jsonl::{
    RawJsonlRecord, RawJsonlRewrite, append_jsonl, read_jsonl, read_receipts_reverse,
    rewrite_jsonl_raw_locked, scan_jsonl_raw, scan_jsonl_raw_locked, with_jsonl_write_lock,
};
use super::maintenance::create_receipts_backup;
use super::records::{PlanEvent, ReceiptRecord};
use super::sessions::current_session;
use super::support::{ensure_state_layout, new_id, now_ms, truncate};

const SUCCESSFUL_RECEIPT_PREVIEW_BYTES: usize = 512;

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
    pub(crate) changed_path_count: usize,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: Option<String>,
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
    pub(crate) changed_path_count: usize,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: Option<String>,
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

#[derive(Debug, Default)]
struct IndexedCheckReceipts {
    direct: Option<ToolReceiptStatus>,
    exact_work_check: Option<ToolReceiptStatus>,
    legacy_work_check: Option<ToolReceiptStatus>,
}

/// A request-scoped view of the receipts needed to evaluate configured work
/// gates. Building it retains a bounded number of statuses per configured gate
/// while scanning the receipt stream exactly once.
#[derive(Debug, Default)]
pub(crate) struct WorkGateReceiptIndex {
    checks: BTreeMap<String, IndexedCheckReceipts>,
    reviews: BTreeMap<String, WorkReviewReceiptStatus>,
}

impl WorkGateReceiptIndex {
    pub(crate) fn tool_receipt(&self, tool_name: &str) -> Option<&ToolReceiptStatus> {
        self.checks
            .get(tool_name)
            .and_then(|receipts| receipts.direct.as_ref())
    }

    pub(crate) fn work_check_receipt(
        &self,
        tool_name: &str,
        tool_receipt_id: &str,
    ) -> Option<&ToolReceiptStatus> {
        let receipts = self.checks.get(tool_name)?;
        if receipts.direct.as_ref()?.receipt_id != tool_receipt_id {
            return None;
        }
        receipts
            .exact_work_check
            .as_ref()
            .or(receipts.legacy_work_check.as_ref())
    }

    pub(crate) fn review_receipt(&self, gate_id: &str) -> Option<&WorkReviewReceiptStatus> {
        self.reviews.get(gate_id)
    }
}

#[cfg(test)]
thread_local! {
    static WORK_GATE_RECEIPT_INDEX_SCAN_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_work_gate_receipt_index_scan_count() {
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.set(0);
}

#[cfg(test)]
pub(crate) fn work_gate_receipt_index_scan_count() -> usize {
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.get()
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
    let (receipts, _) =
        read_receipts_reverse(&ctx.state_file("receipts.jsonl"), filter.limit, |receipt| {
            receipt_matches_filters(receipt, &filter)
        })?;
    let receipts = receipts
        .into_iter()
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
    let open_plan_ids =
        current_open_plan_ids(&read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?);
    let (check_gate_tools, review_gate_ids) = configured_gate_evidence_keys(ctx);
    let receipts_path = ctx.state_file("receipts.jsonl");
    let source_path = receipts_path
        .strip_prefix(ctx.root())
        .unwrap_or(&receipts_path)
        .display()
        .to_string();
    let mut recovery_hint = None;
    let mut archive_hint = None;
    let result = with_jsonl_write_lock(&receipts_path, |guard| {
        let mut protection_index = ReceiptProtectionIndex::default();
        let protection_scan = scan_jsonl_raw_locked(guard, &receipts_path, &|| false, |record| {
            let receipt = parse_raw_receipt(record, &receipts_path)?;
            protection_index.observe(
                &receipt,
                &open_plan_ids,
                &check_gate_tools,
                &review_gate_ids,
            );
            Ok(())
        })?;
        if protection_scan.unterminated_final_record {
            bail!(
                "Refusing to archive {} because its final JSONL record is not newline-terminated",
                receipts_path.display()
            );
        }
        let protected = protection_index.protected_receipt_ids();
        let mut receipt_count_before = 0usize;
        let mut receipts_archived = 0usize;
        let mut protected_retained = 0usize;
        let count_scan = scan_jsonl_raw_locked(guard, &receipts_path, &|| false, |record| {
            let receipt = parse_raw_receipt(record, &receipts_path)?;
            receipt_count_before = receipt_count_before.saturating_add(1);
            if receipt.ended_at_ms < before_ms {
                if protected.contains(&receipt.id) {
                    protected_retained = protected_retained.saturating_add(1);
                } else {
                    receipts_archived = receipts_archived.saturating_add(1);
                }
            }
            Ok(())
        })?;
        refuse_unterminated_receipt_stream(&receipts_path, count_scan.unterminated_final_record)?;
        let receipts_retained = receipt_count_before - receipts_archived;
        let recovery_backup_path = if receipts_archived > 0 && !request.dry_run {
            Some(create_receipts_backup(ctx, &receipts_path, "receipts-archive-recovery", None)?.0)
        } else {
            None
        };
        recovery_hint = recovery_backup_path.clone();
        let archive_path = (receipts_archived > 0 && !request.dry_run).then(|| {
            ctx.root()
                .join(".agent/.cache/state-archives")
                .join(format!(
                    "receipts-before-{before_ms}-{}.jsonl.gz",
                    Ulid::new()
                ))
        });
        archive_hint = archive_path.clone();
        let artifact = match &archive_path {
            Some(path) => Some(write_receipt_gzip_locked(
                guard,
                &receipts_path,
                path,
                |receipt| receipt.ended_at_ms < before_ms && !protected.contains(&receipt.id),
            )?),
            None => None,
        };

        if !request.dry_run && receipts_archived > 0 {
            let rewrite = rewrite_jsonl_raw_locked(
                guard,
                &receipts_path,
                &|| false,
                |record| {
                    let receipt = parse_raw_receipt(record, &receipts_path)?;
                    Ok(
                        if receipt.ended_at_ms < before_ms && !protected.contains(&receipt.id) {
                            RawJsonlRewrite::Drop
                        } else {
                            RawJsonlRewrite::Keep
                        },
                    )
                },
                validate_receipt_stream,
            )?;
            if rewrite.dropped_records as usize != receipts_archived {
                bail!(
                    "Receipt archive selected {receipts_archived} records but rewrote {}",
                    rewrite.dropped_records
                );
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
            "recovery_backup_path": recovery_backup_path
                .map(|path| path.display().to_string()),
            "receipt_count_before": receipt_count_before,
            "receipts_archived": receipts_archived,
            "receipts_retained": receipts_retained,
            "protected_receipts_retained": protected_retained,
            "uncompressed_bytes": artifact.as_ref().map(|artifact| artifact.uncompressed_bytes),
            "compressed_bytes": artifact.as_ref().map(|artifact| artifact.compressed_bytes),
            "sha256": artifact.as_ref().map(|artifact| artifact.sha256.as_str()),
            "content_sha256": artifact.as_ref().map(|artifact| artifact.content_sha256.as_str()),
            "writer_coordination_note": MAINTENANCE_WRITER_COORDINATION_NOTE,
        }))
    });
    result.map_err(|error| {
        let recovery = recovery_hint.as_ref().map_or_else(
            || "no exact recovery backup was completed".into(),
            |path| format!("exact recovery backup: {}", path.display()),
        );
        let archive = archive_hint.as_ref().map_or_else(
            || "no selected-record archive was completed".into(),
            |path| format!("selected-record archive: {}", path.display()),
        );
        anyhow!("{error:#}\nReceipt archive recovery context: {recovery}; {archive}")
    })
}

pub(crate) fn receipts_export(ctx: &RepoContext, before: &str, output: &Path) -> Result<Value> {
    let before_ms = parse_archive_before_ms(before)?;
    let receipts_path = ctx.state_file("receipts.jsonl");
    let source_path = receipts_path
        .strip_prefix(ctx.root())
        .unwrap_or(&receipts_path)
        .display()
        .to_string();
    let artifact = write_receipt_gzip(&receipts_path, output, |receipt| {
        receipt.ended_at_ms < before_ms
    })?;

    Ok(json!({
        "ok": true,
        "command": "state export receipts",
        "before": before,
        "before_ms": before_ms,
        "source_path": source_path,
        "output_path": artifact.path.display().to_string(),
        "receipts_exported": artifact.receipt_count,
        "uncompressed_bytes": artifact.uncompressed_bytes,
        "compressed_bytes": artifact.compressed_bytes,
        "sha256": artifact.sha256,
        "content_sha256": artifact.content_sha256,
    }))
}

#[derive(Debug)]
struct ReceiptGzipArtifact {
    path: PathBuf,
    receipt_count: usize,
    uncompressed_bytes: u64,
    compressed_bytes: u64,
    sha256: String,
    content_sha256: String,
}

fn write_receipt_gzip_locked(
    guard: &super::jsonl::JsonlWriteGuard,
    source: &Path,
    destination: &Path,
    mut selected: impl FnMut(&ReceiptRecord) -> bool,
) -> Result<ReceiptGzipArtifact> {
    let mut writer = ReceiptGzipWriter::new(destination)?;
    let scan = scan_jsonl_raw_locked(guard, source, &|| false, |record| {
        let receipt = parse_raw_receipt(record, source)?;
        if selected(&receipt) {
            writer.write_record(record)?;
        }
        Ok(())
    })?;
    refuse_unterminated_receipt_stream(source, scan.unterminated_final_record)?;
    let artifact = writer.finish()?;
    validate_published_receipt_artifact(artifact)
}

fn write_receipt_gzip(
    source: &Path,
    destination: &Path,
    mut selected: impl FnMut(&ReceiptRecord) -> bool,
) -> Result<ReceiptGzipArtifact> {
    let mut writer = ReceiptGzipWriter::new(destination)?;
    let scan = scan_jsonl_raw(source, &|| false, |record| {
        let receipt = parse_raw_receipt(record, source)?;
        if selected(&receipt) {
            writer.write_record(record)?;
        }
        Ok(())
    })?;
    refuse_unterminated_receipt_stream(source, scan.unterminated_final_record)?;
    let artifact = writer.finish()?;
    validate_published_receipt_artifact(artifact)
}

struct ReceiptGzipWriter {
    destination: PathBuf,
    encoder: GzEncoder<NamedTempFile>,
    content_digest: Sha256,
    receipt_count: usize,
    uncompressed_bytes: u64,
}

impl ReceiptGzipWriter {
    fn new(destination: &Path) -> Result<Self> {
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        create_dir_all_synced(parent)?;
        let temp = NamedTempFile::new_in(parent)
            .with_context(|| format!("Failed to create gzip temp file in {}", parent.display()))?;
        let encoder = GzBuilder::new()
            .mtime(0)
            .operating_system(255)
            .write(temp, Compression::default());
        Ok(Self {
            destination: destination.to_path_buf(),
            encoder,
            content_digest: Sha256::new(),
            receipt_count: 0,
            uncompressed_bytes: 0,
        })
    }

    fn write_record(&mut self, record: RawJsonlRecord<'_>) -> Result<()> {
        self.encoder.write_all(record.bytes)?;
        self.content_digest.update(record.bytes);
        self.uncompressed_bytes += record.bytes.len() as u64;
        if record.terminated {
            self.encoder.write_all(b"\n")?;
            self.content_digest.update(b"\n");
            self.uncompressed_bytes += 1;
        }
        self.receipt_count += 1;
        Ok(())
    }

    fn finish(self) -> Result<ReceiptGzipArtifact> {
        let Self {
            destination,
            encoder,
            content_digest,
            receipt_count,
            uncompressed_bytes,
        } = self;
        let mut temp = encoder
            .finish()
            .context("Failed to finish receipt gzip stream")?;
        temp.as_file_mut().flush()?;
        temp.as_file_mut().sync_all()?;
        let compressed_bytes = temp.as_file().metadata()?.len();
        let sha256 = sha256_reader(
            temp.reopen()
                .context("Failed to reopen receipt gzip for checksum")?,
        )?;
        temp.persist_noclobber(&destination)
            .map_err(|error| error.error)
            .with_context(|| {
                format!(
                    "Refusing to replace existing receipt export {}",
                    destination.display()
                )
            })?;
        sync_directory(destination.parent().unwrap_or_else(|| Path::new(".")))?;

        Ok(ReceiptGzipArtifact {
            path: destination,
            receipt_count,
            uncompressed_bytes,
            compressed_bytes,
            sha256,
            content_sha256: format!("sha256:{:x}", content_digest.finalize()),
        })
    }
}

fn validate_receipt_gzip_artifact(artifact: &ReceiptGzipArtifact) -> Result<()> {
    let compressed_sha256 = sha256_reader(
        File::open(&artifact.path)
            .with_context(|| format!("Failed to reopen {}", artifact.path.display()))?,
    )?;
    if compressed_sha256 != artifact.sha256 {
        bail!(
            "Receipt gzip checksum changed for {}; refusing to rewrite active state",
            artifact.path.display()
        );
    }
    let source_file = File::open(&artifact.path)
        .with_context(|| format!("Failed to reopen {}", artifact.path.display()))?;
    let mut reader = BufReader::new(GzDecoder::new(source_file));
    let mut content_digest = Sha256::new();
    let mut receipt_count = 0usize;
    let mut uncompressed_bytes = 0u64;
    let mut line_number = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .with_context(|| format!("Failed to verify {}", artifact.path.display()))?;
        if read == 0 {
            break;
        }
        line_number = line_number.saturating_add(1);
        uncompressed_bytes = uncompressed_bytes
            .checked_add(read as u64)
            .context("Receipt artifact byte count overflow")?;
        if uncompressed_bytes > artifact.uncompressed_bytes {
            bail!(
                "Receipt gzip expanded beyond its expected size for {}",
                artifact.path.display()
            );
        }
        content_digest.update(&line);
        if line.last() != Some(&b'\n') {
            bail!(
                "Receipt gzip {} has an unterminated final JSONL record",
                artifact.path.display()
            );
        }
        line.pop();
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        serde_json::from_slice::<ReceiptRecord>(&line).with_context(|| {
            format!(
                "Failed to parse receipt artifact record {line_number} in {}",
                artifact.path.display()
            )
        })?;
        receipt_count = receipt_count.saturating_add(1);
    }
    let content_sha256 = format!("sha256:{:x}", content_digest.finalize());
    if uncompressed_bytes != artifact.uncompressed_bytes
        || content_sha256 != artifact.content_sha256
    {
        bail!(
            "Receipt gzip verification failed for {}; refusing to rewrite active state",
            artifact.path.display()
        );
    }
    if receipt_count != artifact.receipt_count {
        bail!(
            "Receipt gzip record count mismatch for {}; refusing to rewrite active state",
            artifact.path.display()
        );
    }
    Ok(())
}

fn validate_published_receipt_artifact(
    artifact: ReceiptGzipArtifact,
) -> Result<ReceiptGzipArtifact> {
    match validate_receipt_gzip_artifact(&artifact) {
        Ok(()) => Ok(artifact),
        Err(error) => {
            remove_invalid_receipt_artifact(&artifact.path).with_context(|| {
                format!(
                    "{error:#}; additionally failed to remove invalid receipt artifact {}",
                    artifact.path.display()
                )
            })?;
            Err(error)
        }
    }
}

fn remove_invalid_receipt_artifact(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_directory(path.parent().unwrap_or_else(|| Path::new("."))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to remove invalid {}", path.display()))
        }
    }
}

fn refuse_unterminated_receipt_stream(path: &Path, unterminated: bool) -> Result<()> {
    if unterminated {
        bail!(
            "Refusing to process {} because its final JSONL record is not newline-terminated",
            path.display()
        );
    }
    Ok(())
}

fn sha256_reader(mut reader: impl Read) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn parse_raw_receipt(record: RawJsonlRecord<'_>, path: &Path) -> Result<ReceiptRecord> {
    serde_json::from_slice(record.bytes).with_context(|| {
        format!(
            "Failed to parse receipt record {} in {}",
            record.line_number,
            path.display()
        )
    })
}

pub(super) fn validate_receipt_stream(path: &Path) -> Result<()> {
    let scan = scan_jsonl_raw(path, &|| false, |record| {
        parse_raw_receipt(record, path).map(|_| ())
    })?;
    refuse_unterminated_receipt_stream(path, scan.unterminated_final_record)
}

pub(crate) fn work_gate_receipt_index(
    ctx: &RepoContext,
    plan_id: &str,
    check_tools: &BTreeSet<String>,
    review_gate_ids: &BTreeSet<String>,
) -> Result<WorkGateReceiptIndex> {
    work_gate_receipt_index_with_cancellation(ctx, plan_id, check_tools, review_gate_ids, &|| false)
}

pub(crate) fn work_gate_receipt_index_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    check_tools: &BTreeSet<String>,
    review_gate_ids: &BTreeSet<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<WorkGateReceiptIndex> {
    ensure_receipt_scan_active(cancelled)?;
    ensure_state_layout(ctx)?;

    let mut index = WorkGateReceiptIndex {
        checks: check_tools
            .iter()
            .map(|tool_name| (tool_name.clone(), IndexedCheckReceipts::default()))
            .collect(),
        reviews: BTreeMap::new(),
    };
    if check_tools.is_empty() && review_gate_ids.is_empty() {
        return Ok(index);
    }

    let path = ctx.state_file("receipts.jsonl");
    #[cfg(test)]
    WORK_GATE_RECEIPT_INDEX_SCAN_COUNT
        .set(WORK_GATE_RECEIPT_INDEX_SCAN_COUNT.get().saturating_add(1));
    scan_jsonl_raw(&path, cancelled, |record| {
        ensure_receipt_scan_active(cancelled)?;
        let receipt = parse_raw_receipt(record, &path)?;
        if receipt.plan_id.as_deref() != Some(plan_id) {
            return Ok(());
        }

        let direct_tool_name = index
            .checks
            .contains_key(&receipt.tool_name)
            .then(|| receipt.tool_name.clone());
        if let Some(tool_name) = direct_tool_name.as_deref() {
            let receipts = index
                .checks
                .get_mut(tool_name)
                .expect("configured check tool should be indexed");
            receipts.direct = Some(tool_receipt_status(receipt.clone()));
            // A batch can only provide freshness for the latest direct
            // receipt when it appears physically after that receipt.
            receipts.exact_work_check = None;
            receipts.legacy_work_check = None;
        }

        if receipt.tool_name == tool::WORK_CHECK && receipt.exit_status == 0 {
            let batch_status = tool_receipt_status(receipt.clone());
            let has_receipt_ids = receipt_args_has_receipt_ids(&receipt);
            for tool_name in receipt_arg_strings(&receipt, "tools") {
                // If jig.work_check itself is configured as a check gate, the
                // receipt that becomes the direct anchor is not its own batch.
                if direct_tool_name.as_deref() == Some(tool_name) {
                    continue;
                }
                let Some(receipts) = index.checks.get_mut(tool_name) else {
                    continue;
                };
                let Some(direct) = receipts.direct.as_ref() else {
                    continue;
                };
                if receipt_args_include_receipt_id(&receipt, &direct.receipt_id) {
                    receipts.exact_work_check = Some(batch_status.clone());
                } else if !has_receipt_ids {
                    receipts.legacy_work_check = Some(batch_status.clone());
                }
            }
        }

        if receipt.tool_name == tool::WORK_REVIEW {
            if let Some(gate_id) = receipt
                .args
                .get("gate_id")
                .and_then(Value::as_str)
                .filter(|gate_id| review_gate_ids.contains(*gate_id))
            {
                index
                    .reviews
                    .insert(gate_id.to_string(), work_review_receipt_status(receipt));
            }
        }
        Ok(())
    })?;
    ensure_receipt_scan_active(cancelled)?;
    Ok(index)
}

fn current_open_plan_ids(events: &[PlanEvent]) -> BTreeSet<String> {
    let mut open = BTreeMap::<String, bool>::new();
    for event in events {
        match event {
            PlanEvent::Open { plan_id, .. } => {
                open.insert(plan_id.clone(), true);
            }
            PlanEvent::Close { plan_id, .. } => {
                open.insert(plan_id.clone(), false);
            }
            PlanEvent::Append { .. } | PlanEvent::Unknown { .. } => {}
        }
    }
    open.into_iter()
        .filter_map(|(plan_id, is_open)| is_open.then_some(plan_id))
        .collect()
}

fn configured_gate_evidence_keys(ctx: &RepoContext) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut check_tools = BTreeSet::new();
    let mut review_gate_ids = BTreeSet::new();
    for gate in ctx.work_gates() {
        match gate {
            WorkGate::Check(gate) => {
                check_tools.insert(gate.tool);
            }
            WorkGate::CodexReview(gate) => {
                review_gate_ids.insert(gate.id);
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    (check_tools, review_gate_ids)
}

#[derive(Clone, Debug)]
struct LatestReceipt {
    id: String,
    worker_receipt_id: Option<String>,
}

#[derive(Clone, Debug)]
struct ProtectedWorkCheck {
    id: String,
    receipt_ids: Vec<String>,
}

#[derive(Debug, Default)]
struct ProtectedCheckReceipts {
    direct_receipt_id: Option<String>,
    exact_work_check: Option<ProtectedWorkCheck>,
    legacy_work_check: Option<ProtectedWorkCheck>,
}

#[derive(Debug, Default)]
struct ReceiptProtectionIndex {
    checks: BTreeMap<(String, String), ProtectedCheckReceipts>,
    latest_review_by_plan_gate: BTreeMap<(String, String), LatestReceipt>,
}

impl ReceiptProtectionIndex {
    fn observe(
        &mut self,
        receipt: &ReceiptRecord,
        open_plan_ids: &BTreeSet<String>,
        check_gate_tools: &BTreeSet<String>,
        review_gate_ids: &BTreeSet<String>,
    ) {
        let Some(plan_id) = receipt
            .plan_id
            .as_ref()
            .filter(|plan_id| open_plan_ids.contains(*plan_id))
        else {
            return;
        };
        if check_gate_tools.contains(&receipt.tool_name) {
            self.checks.insert(
                (plan_id.clone(), receipt.tool_name.clone()),
                ProtectedCheckReceipts {
                    direct_receipt_id: Some(receipt.id.clone()),
                    ..ProtectedCheckReceipts::default()
                },
            );
        }
        if receipt.tool_name == tool::WORK_REVIEW {
            if let Some(gate_id) = receipt
                .args
                .get("gate_id")
                .and_then(Value::as_str)
                .filter(|gate_id| review_gate_ids.contains(*gate_id))
            {
                self.latest_review_by_plan_gate.insert(
                    (plan_id.clone(), gate_id.to_string()),
                    LatestReceipt {
                        id: receipt.id.clone(),
                        worker_receipt_id: receipt
                            .evidence
                            .as_ref()
                            .and_then(|evidence| evidence.get("worker_receipt_id"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    },
                );
            }
        }
        if receipt.tool_name == tool::WORK_CHECK && receipt.exit_status == 0 {
            let receipt_ids = receipt_arg_strings(receipt, "receipt_ids")
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let has_receipt_ids = receipt_args_has_receipt_ids(receipt);
            for tool_name in receipt_arg_strings(receipt, "tools") {
                if !check_gate_tools.contains(tool_name) {
                    continue;
                }
                let Some(check) = self
                    .checks
                    .get_mut(&(plan_id.clone(), tool_name.to_string()))
                else {
                    continue;
                };
                let Some(direct_receipt_id) = check.direct_receipt_id.as_ref() else {
                    continue;
                };
                // A work-check receipt configured as its own direct gate
                // cannot also be the later batch proving itself.
                if direct_receipt_id == &receipt.id {
                    continue;
                }
                let batch = ProtectedWorkCheck {
                    id: receipt.id.clone(),
                    receipt_ids: receipt_ids.clone(),
                };
                if receipt_ids
                    .iter()
                    .any(|receipt_id| receipt_id == direct_receipt_id)
                {
                    check.exact_work_check = Some(batch);
                } else if !has_receipt_ids {
                    check.legacy_work_check = Some(batch);
                }
            }
        }
    }

    fn protected_receipt_ids(&self) -> BTreeSet<String> {
        let mut protected = BTreeSet::new();
        for check in self.checks.values() {
            let Some(direct_receipt_id) = &check.direct_receipt_id else {
                continue;
            };
            protected.insert(direct_receipt_id.clone());
            if let Some(work_check) = check
                .exact_work_check
                .as_ref()
                .or(check.legacy_work_check.as_ref())
            {
                protected.insert(work_check.id.clone());
                protected.extend(work_check.receipt_ids.iter().cloned());
            }
        }
        for receipt in self.latest_review_by_plan_gate.values() {
            protected.insert(receipt.id.clone());
            if let Some(worker_receipt_id) = &receipt.worker_receipt_id {
                protected.insert(worker_receipt_id.clone());
            }
        }
        protected
    }
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
        stdout_preview: receipt_output_preview(input.stdout, input.exit_status),
        stderr_preview: receipt_output_preview(input.stderr, input.exit_status),
        evidence: input.evidence,
        changed_paths: git_metadata.changed_paths,
        changed_path_count: git_metadata.changed_path_count,
        changed_paths_truncated: git_metadata.changed_paths_truncated,
        changed_paths_digest: git_metadata.changed_paths_digest,
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
    let changed_path_count = receipt
        .changed_path_count
        .unwrap_or(receipt.changed_paths.len());
    let changed_paths_truncated =
        receipt.changed_paths_truncated || changed_path_count > receipt.changed_paths.len();
    ToolReceiptStatus {
        receipt_id: receipt.id,
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        changed_paths: receipt.changed_paths,
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest: receipt.changed_paths_digest,
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint,
        worktree_fingerprint_error: receipt.worktree_fingerprint_error,
    }
}

fn work_review_receipt_status(receipt: ReceiptRecord) -> WorkReviewReceiptStatus {
    let diff_summary = receipt_diff_summary(&receipt);
    let changed_path_count = receipt
        .changed_path_count
        .unwrap_or(receipt.changed_paths.len());
    let changed_paths_truncated =
        receipt.changed_paths_truncated || changed_path_count > receipt.changed_paths.len();
    WorkReviewReceiptStatus {
        receipt_id: receipt.id,
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        evidence: receipt.evidence.as_ref().map(work_review_receipt_evidence),
        changed_paths: receipt.changed_paths,
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest: receipt.changed_paths_digest,
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

fn receipt_output_preview(value: &str, exit_status: i32) -> String {
    if exit_status != 0 {
        return truncate(value);
    }
    truncate_to_bytes(value, SUCCESSFUL_RECEIPT_PREVIEW_BYTES)
}

fn truncate_to_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
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

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use serde_json::json;
    use std::process::Command;
    use tempfile::tempdir;

    use crate::test_env::TestRepoBuilder;

    #[test]
    fn successful_receipt_previews_are_small_but_failures_keep_diagnostics() {
        let output = "x".repeat(5_000);

        let successful = receipt_output_preview(&output, 0);
        let failed = receipt_output_preview(&output, 1);

        assert_eq!(
            successful.strip_suffix('…').unwrap().len(),
            SUCCESSFUL_RECEIPT_PREVIEW_BYTES
        );
        assert_eq!(failed.strip_suffix('…').unwrap().len(), 4_000);
    }

    #[test]
    fn successful_receipt_preview_preserves_utf8_boundaries() {
        let output = "a".repeat(SUCCESSFUL_RECEIPT_PREVIEW_BYTES - 1) + "é-tail";

        let preview = receipt_output_preview(&output, 0);

        assert!(preview.ends_with('…'));
        assert_eq!(
            preview.strip_suffix('…').unwrap(),
            "a".repeat(SUCCESSFUL_RECEIPT_PREVIEW_BYTES - 1)
        );
    }

    #[test]
    fn receipt_protection_is_limited_to_open_configured_gate_evidence() {
        let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
        let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
        let review_gate_ids = BTreeSet::from(["rust-review".to_string()]);
        let mut index = ReceiptProtectionIndex::default();
        let receipts = [
            test_receipt("receipt_direct", "plan_open", tool::TEST, 10, json!({})),
            test_receipt(
                "receipt_batch",
                "plan_open",
                tool::WORK_CHECK,
                20,
                json!({
                    "tools": [tool::TEST],
                    "receipt_ids": ["receipt_direct"],
                }),
            ),
            test_receipt(
                "receipt_review",
                "plan_open",
                tool::WORK_REVIEW,
                30,
                json!({"gate_id": "rust-review"}),
            ),
            test_receipt("receipt_non_gate", "plan_open", tool::CLIPPY, 40, json!({})),
            test_receipt("receipt_closed", "plan_closed", tool::TEST, 50, json!({})),
        ];
        for receipt in &receipts {
            index.observe(receipt, &open_plan_ids, &check_gate_tools, &review_gate_ids);
        }

        let protected = index.protected_receipt_ids();

        assert_eq!(
            protected,
            BTreeSet::from([
                "receipt_batch".to_string(),
                "receipt_direct".to_string(),
                "receipt_review".to_string(),
            ])
        );
    }

    #[test]
    fn receipt_protection_matches_successful_legacy_batch_lookup() {
        let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
        let check_gate_tools = BTreeSet::from([tool::TEST.to_string()]);
        let review_gate_ids = BTreeSet::new();
        let direct = test_receipt("receipt_direct", "plan_open", tool::TEST, 10, json!({}));
        let successful_legacy = test_receipt(
            "receipt_legacy_success",
            "plan_open",
            tool::WORK_CHECK,
            20,
            json!({"tools": [tool::TEST]}),
        );
        let mut failed_legacy = test_receipt(
            "receipt_legacy_failed",
            "plan_open",
            tool::WORK_CHECK,
            30,
            json!({"tools": [tool::TEST]}),
        );
        failed_legacy.exit_status = 1;
        let unrelated_exact_schema = test_receipt(
            "receipt_exact_other",
            "plan_open",
            tool::WORK_CHECK,
            40,
            json!({"tools": [tool::TEST], "receipt_ids": []}),
        );
        let mut index = ReceiptProtectionIndex::default();
        for receipt in [
            direct,
            successful_legacy,
            failed_legacy,
            unrelated_exact_schema,
        ] {
            index.observe(
                &receipt,
                &open_plan_ids,
                &check_gate_tools,
                &review_gate_ids,
            );
        }

        let protected = index.protected_receipt_ids();

        assert_eq!(
            protected,
            BTreeSet::from([
                "receipt_direct".to_string(),
                "receipt_legacy_success".to_string(),
            ])
        );
    }

    #[test]
    fn newest_review_protects_its_worker_receipt_by_physical_order() {
        let open_plan_ids = BTreeSet::from(["plan_open".to_string()]);
        let check_gate_tools = BTreeSet::new();
        let review_gate_ids = BTreeSet::from(["rust-review".to_string()]);
        let old_worker = test_receipt(
            "receipt_worker_old",
            "plan_open",
            crate::tool_defs::WORKER_RUN_TOOL,
            400,
            json!({}),
        );
        let mut old_review = test_receipt(
            "receipt_review_old",
            "plan_open",
            tool::WORK_REVIEW,
            500,
            json!({"gate_id": "rust-review"}),
        );
        old_review.evidence = Some(json!({"worker_receipt_id": "receipt_worker_old"}));
        let latest_worker = test_receipt(
            "receipt_worker_latest",
            "plan_open",
            crate::tool_defs::WORKER_RUN_TOOL,
            200,
            json!({}),
        );
        let mut latest_review = test_receipt(
            "receipt_review_latest",
            "plan_open",
            tool::WORK_REVIEW,
            100,
            json!({"gate_id": "rust-review"}),
        );
        latest_review.exit_status = 1;
        latest_review.evidence = Some(json!({"worker_receipt_id": "receipt_worker_latest"}));
        let mut index = ReceiptProtectionIndex::default();
        for receipt in [old_worker, old_review, latest_worker, latest_review] {
            index.observe(
                &receipt,
                &open_plan_ids,
                &check_gate_tools,
                &review_gate_ids,
            );
        }

        let protected = index.protected_receipt_ids();

        assert_eq!(
            protected,
            BTreeSet::from([
                "receipt_review_latest".to_string(),
                "receipt_worker_latest".to_string(),
            ])
        );
    }

    #[test]
    fn receipt_gzip_export_preserves_selected_raw_records_and_unknown_fields() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("receipts.jsonl");
        let destination = temp.path().join("export/receipts.jsonl.gz");
        let second_destination = temp.path().join("export/receipts-copy.jsonl.gz");
        let old = raw_receipt("receipt_old", 10, r#","future":{"nested":true}"#);
        let new = raw_receipt("receipt_new", 100, "");
        let source_bytes = format!("{old}\n{new}\n");
        fs::write(&source, &source_bytes).unwrap();

        let artifact =
            write_receipt_gzip(&source, &destination, |receipt| receipt.ended_at_ms < 50).unwrap();
        let second_artifact = write_receipt_gzip(&source, &second_destination, |receipt| {
            receipt.ended_at_ms < 50
        })
        .unwrap();

        assert_eq!(artifact.receipt_count, 1);
        assert_eq!(artifact.uncompressed_bytes, (old.len() + 1) as u64);
        assert_eq!(fs::read_to_string(&source).unwrap(), source_bytes);
        let mut decoded = String::new();
        GzDecoder::new(File::open(&destination).unwrap())
            .read_to_string(&mut decoded)
            .unwrap();
        assert_eq!(decoded, format!("{old}\n"));
        assert_eq!(
            artifact.sha256,
            sha256_reader(File::open(&destination).unwrap()).unwrap()
        );
        assert_eq!(
            artifact.content_sha256,
            sha256_reader(std::io::Cursor::new(decoded.as_bytes())).unwrap()
        );
        assert_eq!(artifact.sha256, second_artifact.sha256);
        assert_eq!(
            fs::read(destination).unwrap(),
            fs::read(second_destination).unwrap()
        );
    }

    #[test]
    fn receipt_gzip_export_refuses_to_replace_an_existing_output() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("receipts.jsonl");
        let destination = temp.path().join("receipts.jsonl.gz");
        fs::write(&source, format!("{}\n", raw_receipt("receipt_old", 10, ""))).unwrap();
        fs::write(&destination, "keep me").unwrap();

        let error = write_receipt_gzip(&source, &destination, |_| true)
            .unwrap_err()
            .to_string();

        assert!(error.contains("Refusing to replace existing receipt export"));
        assert_eq!(fs::read_to_string(destination).unwrap(), "keep me");
    }

    #[test]
    fn recorded_receipt_persists_bounded_change_set_metadata() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "fixture"]);
        fs::create_dir_all(temp.path().join("changed")).unwrap();
        for index in 0..105 {
            fs::write(
                temp.path().join(format!("changed/file-{index:03}.txt")),
                "changed\n",
            )
            .unwrap();
        }
        fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
        fs::write(
            temp.path().join(".agent/state/metadata-noise.jsonl"),
            "noise\n",
        )
        .unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        record_receipt(
            &ctx,
            ReceiptInput {
                tool_name: tool::TEST,
                args: json!({}),
                invoked_command_key: None,
                plan_id: None,
                started_at_ms: 1,
                ended_at_ms: 2,
                exit_status: 0,
                stdout: &"success output ".repeat(100),
                stderr: "",
                evidence: None,
                session_override: None,
                collect_git_metadata: true,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();

        let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
        let receipt = receipts.last().unwrap();
        assert_eq!(receipt.changed_paths.len(), 100);
        assert_eq!(receipt.changed_path_count, Some(105));
        assert!(receipt.changed_paths_truncated);
        assert!(
            receipt
                .changed_paths_digest
                .as_deref()
                .is_some_and(|digest| digest.starts_with("sha256:"))
        );
        assert!(
            receipt
                .changed_paths
                .iter()
                .all(|path| !path.starts_with(".agent/"))
        );
        assert_eq!(
            receipt.stdout_preview.strip_suffix('…').unwrap().len(),
            SUCCESSFUL_RECEIPT_PREVIEW_BYTES
        );
    }

    fn test_receipt(
        id: &str,
        plan_id: &str,
        tool_name: &str,
        ended_at_ms: u64,
        args: Value,
    ) -> ReceiptRecord {
        ReceiptRecord {
            id: id.to_string(),
            session_id: None,
            plan_id: Some(plan_id.to_string()),
            tool_name: tool_name.to_string(),
            args,
            invoked_command_key: None,
            started_at_ms: 0,
            ended_at_ms,
            exit_status: 0,
            stdout_preview: String::new(),
            stderr_preview: String::new(),
            evidence: None,
            changed_paths: Vec::new(),
            changed_path_count: None,
            changed_paths_truncated: false,
            changed_paths_digest: None,
            diff_stat: crate::git_receipts::DiffStat::default(),
            git_status_error: None,
            git_diff_stat_error: None,
            worktree_fingerprint: None,
            worktree_fingerprint_error: None,
        }
    }

    fn raw_receipt(id: &str, ended_at_ms: u64, extra: &str) -> String {
        format!(
            r#"{{"id":"{id}","session_id":null,"plan_id":null,"tool_name":"jig.test","args":{{}},"started_at_ms":0,"ended_at_ms":{ended_at_ms},"exit_status":0,"stdout_preview":"","stderr_preview":"","changed_paths":[],"diff_stat":{{"files":0,"insertions":0,"deletions":0}}{extra}}}"#
        )
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
