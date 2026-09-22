//! Bounded, read-only lookup of batch-child identities in retained receipt
//! archives. Every selected archive is validated to the same structural
//! standard as an active receipt stream before any identity from it is used.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::super::{display_repo_path, push_sample};
use super::RunLinkageCollector;
use crate::state::compression::single_member_gzip_reader;
use crate::state::records::ReceiptRecord;

const STATE_ARCHIVES_DIR: &str = ".agent/.cache/state-archives";
const RECEIPT_ARCHIVE_PREFIX: &str = "receipts-before-";
const ARCHIVE_SUFFIX: &str = ".jsonl.gz";
const MAX_SOURCE_RECORD_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECEIPT_HISTORY_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Default, serde::Serialize)]
pub(in crate::state::diagnostics) struct ReceiptHistorySources {
    pub(super) archives_scanned: u64,
    pub(super) symlinks_skipped: u64,
    pub(super) error_count: u64,
    pub(super) errors: Vec<String>,
    pub(super) errors_truncated: bool,
    pub(super) uncompressed_bytes_scanned: u64,
    pub(super) budget_exhausted: bool,
    pub(super) reference_budget_exhausted: bool,
    #[serde(skip)]
    found: BTreeMap<String, BTreeSet<String>>,
    #[serde(skip)]
    retained_reference_units: usize,
}

impl ReceiptHistorySources {
    fn error(&mut self, message: String) {
        self.error_count = self.error_count.saturating_add(1);
        push_sample(&mut self.errors, message);
        self.errors_truncated = self.error_count as usize > self.errors.len();
    }

    pub(super) fn merge_into(&self, collector: &mut RunLinkageCollector) {
        if self.retained_reference_units > 0
            && !collector.track_references(self.retained_reference_units)
        {
            return;
        }
        for (receipt_id, run_ids) in &self.found {
            collector.receipt_ids.insert(receipt_id.clone());
            for run_id in run_ids {
                let collected_run_ids = collector
                    .receipt_runs
                    .entry(receipt_id.clone())
                    .or_default();
                collected_run_ids.insert(run_id.clone());
                if collected_run_ids.len() > 1 {
                    collector
                        .conflicting_receipt_runs
                        .insert(receipt_id.clone());
                }
            }
        }
        if self.reference_budget_exhausted {
            collector.reference_budget_exceeded = true;
        }
    }
}

#[derive(Default)]
struct ScanBudget {
    remaining: u64,
    consumed: u64,
    exhausted: bool,
}

impl ScanBudget {
    fn new() -> Self {
        Self {
            remaining: MAX_RECEIPT_HISTORY_UNCOMPRESSED_BYTES,
            ..Self::default()
        }
    }

    fn consume(&mut self, bytes: u64) -> Result<()> {
        if bytes > self.remaining {
            self.exhausted = true;
            bail!(
                "aggregate uncompressed receipt history exceeds the {MAX_RECEIPT_HISTORY_UNCOMPRESSED_BYTES} byte diagnostic limit"
            );
        }
        self.remaining -= bytes;
        self.consumed += bytes;
        Ok(())
    }
}

#[derive(Clone)]
struct ReferenceBudget {
    remaining: usize,
    exhausted: bool,
}

impl ReferenceBudget {
    fn new(remaining: usize) -> Self {
        Self {
            remaining,
            exhausted: false,
        }
    }

    fn consume(&mut self) -> Result<()> {
        if self.remaining == 0 {
            self.exhausted = true;
            bail!(
                "receipt history identities and associations exceed the remaining diagnostic reference budget"
            );
        }
        self.remaining -= 1;
        Ok(())
    }
}

pub(super) fn scan_receipt_history_sources(
    root: &Path,
    wanted: &BTreeSet<String>,
    known_receipt_ids: &BTreeSet<String>,
    known_receipt_runs: &BTreeMap<String, BTreeSet<String>>,
    remaining_references: usize,
) -> ReceiptHistorySources {
    if wanted.is_empty() {
        return ReceiptHistorySources::default();
    }
    let mut sources = ReceiptHistorySources::default();
    let mut budget = ScanBudget::new();
    let mut reference_budget = ReferenceBudget::new(remaining_references);
    let directory = root.join(STATE_ARCHIVES_DIR);
    for path in directory_entries(root, &directory, &mut sources) {
        if budget.exhausted || reference_budget.exhausted {
            break;
        }
        if !is_receipt_archive(&path)
            || !path_matches_type(root, &path, &mut sources, fs::Metadata::is_file)
        {
            continue;
        }
        sources.archives_scanned = sources.archives_scanned.saturating_add(1);
        let display = display_repo_path(root, &path);
        let mut archive_reference_budget = reference_budget.clone();
        match scan_receipt_archive(
            &path,
            wanted,
            known_receipt_ids,
            known_receipt_runs,
            &sources.found,
            &mut budget,
            &mut archive_reference_budget,
        ) {
            Ok(found) => {
                reference_budget = archive_reference_budget;
                for (receipt_id, run_ids) in found {
                    sources.found.entry(receipt_id).or_default().extend(run_ids);
                }
            }
            Err(error) => {
                reference_budget.exhausted |= archive_reference_budget.exhausted;
                sources.error(format!("{display}: {error:#}"));
            }
        }
    }
    sources.uncompressed_bytes_scanned = budget.consumed;
    sources.budget_exhausted = budget.exhausted;
    sources.reference_budget_exhausted = reference_budget.exhausted;
    sources.retained_reference_units =
        remaining_references.saturating_sub(reference_budget.remaining);
    sources
}

fn directory_entries(
    root: &Path,
    directory: &Path,
    sources: &mut ReceiptHistorySources,
) -> Vec<PathBuf> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            sources.error(format!("{}: {error}", display_repo_path(root, directory)));
            return Vec::new();
        }
    };
    if metadata.file_type().is_symlink() {
        sources.symlinks_skipped = sources.symlinks_skipped.saturating_add(1);
        return Vec::new();
    }
    if !metadata.is_dir() {
        sources.error(format!(
            "{} is not a directory",
            display_repo_path(root, directory)
        ));
        return Vec::new();
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
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
                    if is_receipt_archive(&entry.path()) {
                        sources.symlinks_skipped = sources.symlinks_skipped.saturating_add(1);
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

fn is_receipt_archive(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(RECEIPT_ARCHIVE_PREFIX) && name.ends_with(ARCHIVE_SUFFIX)
        })
}

fn path_matches_type(
    root: &Path,
    path: &Path,
    sources: &mut ReceiptHistorySources,
    matches: impl FnOnce(&fs::Metadata) -> bool,
) -> bool {
    match fs::metadata(path) {
        Ok(metadata) => matches(&metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            sources.error(format!("{}: {error}", display_repo_path(root, path)));
            false
        }
    }
}

fn scan_receipt_archive(
    path: &Path,
    wanted: &BTreeSet<String>,
    known_receipt_ids: &BTreeSet<String>,
    known_receipt_runs: &BTreeMap<String, BTreeSet<String>>,
    previously_found: &BTreeMap<String, BTreeSet<String>>,
    budget: &mut ScanBudget,
    reference_budget: &mut ReferenceBudget,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut reader = BufReader::new(single_member_gzip_reader(file));
    let mut buffer = Vec::new();
    let mut found = BTreeMap::<String, BTreeSet<String>>::new();
    loop {
        buffer.clear();
        let read_limit =
            (MAX_SOURCE_RECORD_BYTES as u64 + 1).min(budget.remaining.saturating_add(1));
        let read = (&mut reader)
            .take(read_limit)
            .read_until(b'\n', &mut buffer)
            .context("Failed to decompress receipt history")?;
        if read == 0 {
            break;
        }
        budget.consume(read as u64)?;
        if buffer.len() > MAX_SOURCE_RECORD_BYTES {
            bail!("a record exceeds the {MAX_SOURCE_RECORD_BYTES} byte diagnostic read limit");
        }
        let terminated = buffer.last() == Some(&b'\n');
        let line = &buffer[..buffer.len() - usize::from(terminated)];
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if !terminated {
            bail!("final record is not newline-terminated");
        }
        let receipt: ReceiptRecord =
            serde_json::from_slice(line).context("Failed to parse receipt record")?;
        if wanted.contains(&receipt.id) {
            let receipt_id = receipt.id;
            let identity_known = known_receipt_ids.contains(&receipt_id)
                || previously_found.contains_key(&receipt_id)
                || found.contains_key(&receipt_id);
            let association_known = receipt.run_id.as_ref().is_some_and(|run_id| {
                known_receipt_runs
                    .get(&receipt_id)
                    .is_some_and(|run_ids| run_ids.contains(run_id))
                    || previously_found
                        .get(&receipt_id)
                        .is_some_and(|run_ids| run_ids.contains(run_id))
                    || found
                        .get(&receipt_id)
                        .is_some_and(|run_ids| run_ids.contains(run_id))
            });
            if !identity_known || receipt.run_id.is_some() && !association_known {
                reference_budget.consume()?;
            }
            found.entry(receipt_id.clone()).or_default();
            if let Some(run_id) = receipt.run_id
                && !association_known
            {
                found.entry(receipt_id).or_default().insert(run_id);
            }
        }
    }
    Ok(found)
}
