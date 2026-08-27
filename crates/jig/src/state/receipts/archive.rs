use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use flate2::{Compression, GzBuilder, read::GzDecoder, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use time::{Date, Month};
use ulid::Ulid;

use crate::context::{RepoContext, WorkGate};
use crate::repository::{RepositoryCatalog, resolve_evidence_targets};
use crate::tool_defs::tool;

use super::super::MAINTENANCE_WRITER_COORDINATION_NOTE;
use super::super::compression::{create_dir_all_synced, sync_directory};
use super::super::jsonl::{
    JsonlWriteGuard, RawJsonlRecord, RawJsonlRewrite, read_jsonl, rewrite_jsonl_raw_locked,
    scan_jsonl_raw, scan_jsonl_raw_locked, with_jsonl_write_lock,
};
use super::super::maintenance::create_receipts_backup;
use super::super::records::{PlanEvent, ReceiptRecord};
use super::super::support::ensure_state_layout;
use super::{
    IndexedTargetReceipts, WORK_CHECK_EVIDENCE_SCHEMA, WorkCheckBatchEvidence, parse_raw_receipt,
    receipt_arg_strings, receipt_args_has_receipt_ids, target_receipt_status,
    validate_receipt_stream,
};

#[derive(Debug)]
pub(crate) struct StateArchiveRequest {
    pub(crate) before: String,
    pub(crate) dry_run: bool,
}

pub(crate) fn receipts_archive(ctx: &RepoContext, request: StateArchiveRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let before_ms = parse_archive_before_ms(&request.before)?;
    let open_plan_ids =
        current_open_plan_ids(&read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?);
    let configured_evidence = configured_gate_evidence_keys(ctx)?;
    let receipts_path = ctx.state_file("receipts.jsonl");
    let source_path = receipts_path
        .strip_prefix(ctx.root())
        .unwrap_or(&receipts_path)
        .display()
        .to_string();
    let mut recovery_hint = None;
    let mut archive_hint = None;
    let result = with_jsonl_write_lock(&receipts_path, |guard| {
        let mut protection_index =
            ReceiptProtectionIndex::with_evidence(&open_plan_ids, &configured_evidence.targets);
        let protection_scan = scan_jsonl_raw_locked(guard, &receipts_path, &|| false, |record| {
            let receipt = parse_raw_receipt(record, &receipts_path)?;
            protection_index.observe(
                &receipt,
                &open_plan_ids,
                &configured_evidence.check_tools,
                &configured_evidence.check_gate_ids,
                &configured_evidence.review_gate_ids,
            );
            Ok(())
        })?;
        if protection_scan.unterminated_final_record {
            bail!(
                "Refusing to archive {} because its final JSONL record is not newline-terminated",
                receipts_path.display()
            );
        }
        let protected = protection_index.protected_receipt_ids()?;
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
pub(super) struct ReceiptGzipArtifact {
    pub(super) path: PathBuf,
    pub(super) receipt_count: usize,
    pub(super) uncompressed_bytes: u64,
    pub(super) compressed_bytes: u64,
    pub(super) sha256: String,
    pub(super) content_sha256: String,
}

fn write_receipt_gzip_locked(
    guard: &JsonlWriteGuard,
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

pub(super) fn write_receipt_gzip(
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

pub(super) fn refuse_unterminated_receipt_stream(path: &Path, unterminated: bool) -> Result<()> {
    if unterminated {
        bail!(
            "Refusing to process {} because its final JSONL record is not newline-terminated",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn sha256_reader(mut reader: impl Read) -> Result<String> {
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

struct ConfiguredGateEvidence {
    check_tools: BTreeSet<String>,
    check_gate_ids: BTreeSet<String>,
    review_gate_ids: BTreeSet<String>,
    targets: BTreeMap<String, BTreeSet<jig_contract::TargetId>>,
}

fn configured_gate_evidence_keys(ctx: &RepoContext) -> Result<ConfiguredGateEvidence> {
    let mut configured = ConfiguredGateEvidence {
        check_tools: BTreeSet::new(),
        check_gate_ids: BTreeSet::new(),
        review_gate_ids: BTreeSet::new(),
        targets: BTreeMap::new(),
    };
    let gates = ctx.work_gates();
    let repository = gates
        .iter()
        .any(|gate| matches!(gate, WorkGate::Evidence(_)))
        .then(|| RepositoryCatalog::from_context(ctx))
        .transpose()?;
    for gate in gates {
        match gate {
            WorkGate::Check(gate) => {
                configured.check_gate_ids.insert(gate.id);
                configured.check_tools.insert(gate.tool);
            }
            WorkGate::CodexReview(gate) => {
                configured.review_gate_ids.insert(gate.id);
            }
            WorkGate::Evidence(gate) => {
                let catalog = repository
                    .as_ref()
                    .expect("evidence gates initialize the repository catalog");
                configured
                    .targets
                    .insert(gate.id, resolve_evidence_targets(catalog, &gate.selector)?);
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    Ok(configured)
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
pub(super) struct ReceiptProtectionIndex {
    checks: BTreeMap<(String, String), ProtectedCheckReceipts>,
    latest_check_by_plan_gate: BTreeMap<(String, String), ProtectedWorkCheck>,
    latest_review_by_plan_gate: BTreeMap<(String, String), LatestReceipt>,
    target_evidence: BTreeMap<(String, String), IndexedTargetReceipts>,
}

impl ReceiptProtectionIndex {
    pub(super) fn with_evidence(
        open_plan_ids: &BTreeSet<String>,
        evidence_targets: &BTreeMap<String, BTreeSet<jig_contract::TargetId>>,
    ) -> Self {
        let target_evidence = open_plan_ids
            .iter()
            .flat_map(|plan_id| {
                evidence_targets.iter().map(move |(gate_id, targets)| {
                    (
                        (plan_id.clone(), gate_id.clone()),
                        IndexedTargetReceipts::for_archive(targets.clone()),
                    )
                })
            })
            .collect();
        Self {
            target_evidence,
            ..Self::default()
        }
    }

    pub(super) fn observe(
        &mut self,
        receipt: &ReceiptRecord,
        open_plan_ids: &BTreeSet<String>,
        check_gate_tools: &BTreeSet<String>,
        check_gate_ids: &BTreeSet<String>,
        review_gate_ids: &BTreeSet<String>,
    ) {
        let Some(plan_id) = receipt
            .plan_id
            .as_ref()
            .filter(|plan_id| open_plan_ids.contains(*plan_id))
        else {
            return;
        };
        if let (Some(run_id), Some(target)) = (receipt.run_id.as_ref(), receipt.target.as_ref()) {
            let status = target_receipt_status(receipt, run_id, target);
            for ((evidence_plan_id, _), receipts) in &mut self.target_evidence {
                if evidence_plan_id == plan_id {
                    receipts.observe(&status);
                }
            }
        }
        if check_gate_tools.contains(&receipt.tool_name) {
            self.checks.insert(
                (plan_id.clone(), receipt.tool_name.clone()),
                ProtectedCheckReceipts {
                    direct_receipt_id: Some(receipt.id.clone()),
                    ..ProtectedCheckReceipts::default()
                },
            );
        }
        if receipt.tool_name == tool::WORK_REVIEW
            && let Some(gate_id) = receipt
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
        if receipt.tool_name == tool::WORK_CHECK {
            for gate_id in receipt_arg_strings(receipt, "gates") {
                if !check_gate_ids.contains(gate_id) {
                    continue;
                }
                let key = (plan_id.clone(), gate_id.to_string());
                self.latest_check_by_plan_gate.remove(&key);
                // The batch itself is the durable supersession tombstone even
                // when its structured evidence is malformed or omits this
                // selected gate. Retaining it keeps an archived rewrite from
                // revealing an older pass when the stream is read again.
                self.latest_check_by_plan_gate.insert(
                    key,
                    ProtectedWorkCheck {
                        id: receipt.id.clone(),
                        receipt_ids: Vec::new(),
                    },
                );
            }
            if let Some(evidence) = receipt
                .evidence
                .as_ref()
                .and_then(|evidence| {
                    serde_json::from_value::<WorkCheckBatchEvidence>(evidence.clone()).ok()
                })
                .filter(|evidence| evidence.schema == WORK_CHECK_EVIDENCE_SCHEMA)
            {
                for gate in evidence.gates {
                    if !check_gate_ids.contains(&gate.gate_id) {
                        continue;
                    }
                    let receipt_ids = [
                        gate.tool_receipt_id,
                        gate.source_batch_receipt_id,
                        gate.source_tool_receipt_id,
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                    self.latest_check_by_plan_gate.insert(
                        (plan_id.clone(), gate.gate_id),
                        ProtectedWorkCheck {
                            id: receipt.id.clone(),
                            receipt_ids,
                        },
                    );
                }
            }
        }
        if receipt.tool_name == tool::WORK_CHECK && receipt.exit_status == 0 {
            let receipt_ids = receipt_arg_strings(receipt, "receipt_ids")
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

    pub(super) fn protected_receipt_ids(&self) -> Result<BTreeSet<String>> {
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
        for work_check in self.latest_check_by_plan_gate.values() {
            protected.insert(work_check.id.clone());
            protected.extend(work_check.receipt_ids.iter().cloned());
        }
        for receipt in self.latest_review_by_plan_gate.values() {
            protected.insert(receipt.id.clone());
            if let Some(worker_receipt_id) = &receipt.worker_receipt_id {
                protected.insert(worker_receipt_id.clone());
            }
        }
        for ((plan_id, gate_id), receipts) in &self.target_evidence {
            if let Some(error) = receipts.error() {
                bail!(
                    "cannot safely archive target evidence for plan '{plan_id}' gate '{gate_id}': {error}"
                );
            }
            if let Some(group) = receipts.selected() {
                protected.extend(
                    group
                        .receipts
                        .values()
                        .map(|receipt| receipt.receipt_id.clone()),
                );
            }
        }
        Ok(protected)
    }
}

pub(in crate::state) fn parse_archive_before_ms(value: &str) -> Result<u64> {
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
