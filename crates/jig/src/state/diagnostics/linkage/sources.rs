//! Read-only inspection of local run history sources outside the active
//! journal: run archives under `.agent/.cache/state-archives/` and manifested
//! run backups under `.agent/.cache/state-backups/`. Compressed sources are
//! streamed in memory; nothing is decompressed to disk.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::{display_repo_path, push_sample};
use super::LifecycleObservation;
use crate::state::compression::single_member_gzip_reader;
use crate::state::maintenance::read_run_backup_manifest;
use crate::state::records::RunEventRecord;
use crate::state::runs::lifecycle::{RunStreamValidator, is_recognized_run_event};

const STATE_ARCHIVES_DIR: &str = ".agent/.cache/state-archives";
const STATE_BACKUPS_DIR: &str = ".agent/.cache/state-backups";
const RUN_ARCHIVE_PREFIX: &str = "runs-";
const ARCHIVE_SUFFIX: &str = ".jsonl.gz";
const BACKUP_MANIFEST_FILE: &str = "manifest.json";
const MAX_SOURCE_RECORD_BYTES: usize = 8 * 1024 * 1024;
const MAX_HISTORY_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

struct ScanBudget {
    remaining: u64,
    consumed: u64,
    exhausted: bool,
}

impl Default for ScanBudget {
    fn default() -> Self {
        Self {
            remaining: MAX_HISTORY_UNCOMPRESSED_BYTES,
            consumed: 0,
            exhausted: false,
        }
    }
}

impl ScanBudget {
    fn consume(&mut self, bytes: u64) -> Result<()> {
        if bytes > self.remaining {
            self.exhausted = true;
            bail!(
                "aggregate uncompressed content exceeds the {MAX_HISTORY_UNCOMPRESSED_BYTES} byte diagnostic limit"
            );
        }
        self.remaining -= bytes;
        self.consumed += bytes;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SourceKind {
    Archive,
    Backup,
}

impl SourceKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Archive => "run_archive",
            Self::Backup => "state_backup",
        }
    }
}

#[derive(Debug)]
pub(super) struct BackupFacts {
    manifest_path: String,
    pub(super) created_at_ms: u64,
    original_bytes: u64,
    original_sha256: String,
}

#[derive(Debug)]
pub(super) struct SourceLifecycle {
    pub(super) path: String,
    pub(super) kind: SourceKind,
    pub(super) observation: LifecycleObservation,
    pub(super) backup: Option<BackupFacts>,
}

impl SourceLifecycle {
    pub(super) fn to_value(&self) -> Value {
        json!({
            "kind": self.kind.as_str(),
            "path": self.path,
            "events": self.observation.events(),
            "verified_complete": self.observation.status() == super::LifecycleStatus::Completed,
            "anomalies": self.observation.anomalies,
        })
    }

    pub(super) fn recovery_value(&self) -> Value {
        let backup = self.backup.as_ref();
        json!({
            "kind": "exact_source_backup",
            "backup_path": backup.map(|facts| facts.manifest_path.as_str()),
            "compressed_path": self.path,
            "created_at_ms": backup.map(|facts| facts.created_at_ms),
            "original_bytes": backup.map(|facts| facts.original_bytes),
            "original_sha256": backup.map(|facts| facts.original_sha256.as_str()),
            "events_for_run": self.observation.events(),
            "restore_replaces_whole_stream": true,
        })
    }
}

#[derive(Debug, Default, serde::Serialize)]
pub(in crate::state::diagnostics) struct HistorySources {
    pub(super) archives_scanned: u64,
    pub(super) backups_scanned: u64,
    pub(super) symlinks_skipped: u64,
    pub(super) error_count: u64,
    pub(super) errors: Vec<String>,
    pub(super) errors_truncated: bool,
    pub(super) uncompressed_bytes_scanned: u64,
    pub(super) budget_exhausted: bool,
    #[serde(skip)]
    pub(super) found: BTreeMap<String, Vec<SourceLifecycle>>,
}

impl HistorySources {
    fn error(&mut self, message: String) {
        self.error_count += 1;
        push_sample(&mut self.errors, message);
        self.errors_truncated = self.error_count as usize > self.errors.len();
    }

    fn merge(
        &mut self,
        path: String,
        kind: SourceKind,
        backup: Option<&BackupFacts>,
        found: BTreeMap<String, LifecycleObservation>,
    ) {
        for (run_id, observation) in found {
            self.found.entry(run_id).or_default().push(SourceLifecycle {
                path: path.clone(),
                kind,
                observation,
                backup: backup.map(|facts| BackupFacts {
                    manifest_path: facts.manifest_path.clone(),
                    created_at_ms: facts.created_at_ms,
                    original_bytes: facts.original_bytes,
                    original_sha256: facts.original_sha256.clone(),
                }),
            });
        }
    }
}

/// Scans every supported local run history source for the wanted run IDs.
pub(super) fn scan_local_history_sources(root: &Path, wanted: &BTreeSet<String>) -> HistorySources {
    let mut budget = ScanBudget::default();
    scan_local_history_sources_with_budget(root, wanted, &mut budget)
}

fn scan_local_history_sources_with_budget(
    root: &Path,
    wanted: &BTreeSet<String>,
    budget: &mut ScanBudget,
) -> HistorySources {
    let mut sources = HistorySources::default();
    let mut remaining = wanted.clone();
    scan_run_archives(root, &mut remaining, budget, &mut sources);
    if !remaining.is_empty() && !budget.exhausted {
        scan_run_backups(root, &mut remaining, budget, &mut sources);
    }
    sources.uncompressed_bytes_scanned = budget.consumed;
    sources.budget_exhausted = budget.exhausted;
    sources
}

fn directory_entries(
    root: &Path,
    directory: &Path,
    sources: &mut HistorySources,
    is_history_symlink: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            sources.error(format!("{}: {error}", display_repo_path(root, directory)));
            return Vec::new();
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(file_type) if file_type.is_symlink() => {
                    if is_history_symlink(&entry.path()) {
                        sources.symlinks_skipped += 1;
                    }
                }
                Ok(_) => paths.push(entry.path()),
                Err(error) => sources.error(format!(
                    "{}: {error}",
                    display_repo_path(root, &entry.path())
                )),
            },
            Err(error) => sources.error(format!("{}: {error}", display_repo_path(root, directory))),
        }
    }
    paths.sort();
    paths
}

fn is_run_archive(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(RUN_ARCHIVE_PREFIX) && name.ends_with(ARCHIVE_SUFFIX))
}

fn scan_run_archives(
    root: &Path,
    wanted: &mut BTreeSet<String>,
    budget: &mut ScanBudget,
    sources: &mut HistorySources,
) {
    let directory = root.join(STATE_ARCHIVES_DIR);
    for path in directory_entries(root, &directory, sources, is_run_archive) {
        if wanted.is_empty() || budget.exhausted {
            break;
        }
        if !is_run_archive(&path) || !path.is_file() {
            continue;
        }
        sources.archives_scanned += 1;
        let display = display_repo_path(root, &path);
        match scan_gzip_run_events(&path, wanted, None, budget) {
            Ok(found) => {
                let resolved = completed_run_ids(&found);
                sources.merge(display, SourceKind::Archive, None, found);
                wanted.retain(|run_id| !resolved.contains(run_id));
            }
            Err(error) => sources.error(format!("{display}: {error:#}")),
        }
    }
}

fn scan_run_backups(
    root: &Path,
    wanted: &mut BTreeSet<String>,
    budget: &mut ScanBudget,
    sources: &mut HistorySources,
) {
    let directory = root.join(STATE_BACKUPS_DIR);
    let mut candidates = Vec::new();
    for backup_dir in directory_entries(root, &directory, sources, |_| true) {
        let manifest_path = backup_dir.join(BACKUP_MANIFEST_FILE);
        if !backup_dir.is_dir() || !manifest_path.is_file() {
            continue;
        }
        let display = display_repo_path(root, &backup_dir);
        let manifest = match read_run_backup_manifest(&manifest_path) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => continue,
            Err(error) => {
                sources.backups_scanned += 1;
                sources.error(format!("{display}: {error:#}"));
                continue;
            }
        };
        candidates.push((manifest.created_at_ms, backup_dir, display, manifest));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    for (_, backup_dir, display, manifest) in candidates {
        if wanted.is_empty() || budget.exhausted {
            break;
        }
        sources.backups_scanned += 1;
        let compressed_path = backup_dir.join(&manifest.compressed_file);
        let facts = BackupFacts {
            manifest_path: display.clone(),
            created_at_ms: manifest.created_at_ms,
            original_bytes: manifest.original_bytes,
            original_sha256: manifest.original_sha256.clone(),
        };
        let result =
            verify_backup_size(&compressed_path, manifest.compressed_bytes).and_then(|()| {
                scan_gzip_run_events(
                    &compressed_path,
                    wanted,
                    Some((manifest.original_bytes, manifest.original_sha256.as_str())),
                    budget,
                )
            });
        match result {
            Ok(found) => {
                let resolved = completed_run_ids(&found);
                sources.merge(
                    display_repo_path(root, &compressed_path),
                    SourceKind::Backup,
                    Some(&facts),
                    found,
                );
                wanted.retain(|run_id| !resolved.contains(run_id));
            }
            Err(error) => sources.error(format!("{display}: {error:#}")),
        }
    }
}

fn completed_run_ids(found: &BTreeMap<String, LifecycleObservation>) -> BTreeSet<String> {
    found
        .iter()
        .filter(|(_, observation)| observation.status() == super::LifecycleStatus::Completed)
        .map(|(run_id, _)| run_id.clone())
        .collect()
}

fn verify_backup_size(compressed_path: &Path, expected_bytes: u64) -> Result<()> {
    let metadata = fs::symlink_metadata(compressed_path)
        .with_context(|| format!("Failed to inspect {}", compressed_path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", compressed_path.display());
    }
    if metadata.len() != expected_bytes {
        bail!(
            "{} has {} bytes but its manifest records {expected_bytes}",
            compressed_path.display(),
            metadata.len()
        );
    }
    Ok(())
}

/// Streams a gzip JSONL run-event file and folds lifecycles for wanted runs.
///
/// Every source receives authoritative whole-stream lifecycle validation.
/// `expected` additionally verifies the uncompressed size and SHA-256 recorded
/// by a backup manifest, so only an exact artifact accepted by restore is ever
/// reported as recoverable.
fn scan_gzip_run_events(
    path: &Path,
    wanted: &BTreeSet<String>,
    expected: Option<(u64, &str)>,
    budget: &mut ScanBudget,
) -> Result<BTreeMap<String, LifecycleObservation>> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut reader = BufReader::new(single_member_gzip_reader(file));
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = Vec::new();
    let mut found = BTreeMap::<String, LifecycleObservation>::new();
    let mut stream_validator = RunStreamValidator::default();
    let mut unrecognized_events = 0u64;
    loop {
        buffer.clear();
        let read_limit =
            (MAX_SOURCE_RECORD_BYTES as u64 + 1).min(budget.remaining.saturating_add(1));
        let read = (&mut reader)
            .take(read_limit)
            .read_until(b'\n', &mut buffer)
            .context("Failed to decompress run history")?;
        if read == 0 {
            break;
        }
        budget.consume(read as u64)?;
        if buffer.len() > MAX_SOURCE_RECORD_BYTES {
            bail!("a record exceeds the {MAX_SOURCE_RECORD_BYTES} byte diagnostic read limit");
        }
        hasher.update(&buffer);
        total = total.saturating_add(read as u64);
        let terminated = buffer.last() == Some(&b'\n');
        let line = &buffer[..buffer.len() - usize::from(terminated)];
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if !terminated {
            bail!("final record is not newline-terminated");
        }
        let event: RunEventRecord =
            serde_json::from_slice(line).context("Failed to parse run event")?;
        stream_validator.observe(&event)?;
        if !is_recognized_run_event(&event.event) {
            unrecognized_events = unrecognized_events.saturating_add(1);
        }
        if wanted.contains(&event.run_id) {
            found
                .entry(event.run_id.clone())
                .or_default()
                .observe(&event);
        }
    }
    stream_validator
        .finish()
        .context("run history fails whole-stream lifecycle validation")?;
    if unrecognized_events > 0 {
        bail!("run history contains {unrecognized_events} unrecognized event(s)");
    }
    if let Some((expected_bytes, expected_sha256)) = expected {
        let digest = hasher.finalize();
        let actual = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if total != expected_bytes || actual != expected_sha256 {
            bail!(
                "backup content does not match its manifest (bytes {total} vs {expected_bytes}); it is not an exact source"
            );
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tempfile::tempdir;

    use super::*;

    fn write_gzip(path: &Path, bytes: &[u8]) -> u64 {
        let file = File::create(path).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap();
        fs::metadata(path).unwrap().len()
    }

    #[test]
    fn archive_and_backup_scans_share_one_decompression_budget() {
        let temp = tempdir().unwrap();
        let archives = temp.path().join(STATE_ARCHIVES_DIR);
        let backup = temp.path().join(STATE_BACKUPS_DIR).join("example-backup");
        fs::create_dir_all(&archives).unwrap();
        fs::create_dir_all(&backup).unwrap();

        let archive_record =
            b"{\"id\":\"event_archive\",\"run_id\":\"run_other\",\"event\":\"note\",\"timestamp_ms\":1}\n";
        write_gzip(
            &archives.join("runs-before-1-EXAMPLE.jsonl.gz"),
            archive_record,
        );

        let backup_record =
            b"{\"id\":\"event_backup\",\"run_id\":\"run_other\",\"event\":\"note\",\"timestamp_ms\":2}\n";
        let backup_path = backup.join("runs.jsonl.gz");
        let compressed_bytes = write_gzip(&backup_path, backup_record);
        fs::write(
            backup.join(BACKUP_MANIFEST_FILE),
            serde_json::to_vec(&json!({
                "version": 1,
                "stream": "runs",
                "source_path": ".agent/state/runs.jsonl",
                "compressed_file": "runs.jsonl.gz",
                "created_at_ms": 1,
                "original_bytes": backup_record.len(),
                "original_sha256": "unused-after-budget-exhaustion",
                "compressed_bytes": compressed_bytes,
            }))
            .unwrap(),
        )
        .unwrap();

        let wanted = BTreeSet::from(["run_wanted".to_string()]);
        let mut budget = ScanBudget {
            remaining: archive_record.len() as u64,
            consumed: 0,
            exhausted: false,
        };
        let sources = scan_local_history_sources_with_budget(temp.path(), &wanted, &mut budget);

        assert_eq!(sources.archives_scanned, 1);
        assert_eq!(sources.backups_scanned, 1);
        assert_eq!(
            sources.uncompressed_bytes_scanned,
            archive_record.len() as u64
        );
        assert!(sources.budget_exhausted);
        assert_eq!(sources.error_count, 2);
        assert!(
            sources
                .errors
                .iter()
                .any(|error| error.contains("unrecognized event"))
        );
        assert!(
            sources
                .errors
                .iter()
                .any(|error| error.contains("aggregate uncompressed content exceeds"))
        );
    }
}
