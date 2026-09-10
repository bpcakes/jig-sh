use std::collections::BTreeMap;
use std::fs::{File, Metadata};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use jig_contract::freshness::FreshnessReasonCode;
use sha2::{Digest, Sha256};

use super::{TargetReceiptStatus, target_receipt_status};
use crate::repository::freshness::{CollectionBudget, CollectionFailure, CollectionResult};
use crate::state::records::ReceiptRecord;

// A stricter proof-reader limit, reported explicitly. A larger proof must become
// incomplete rather than letting an inspection allocate unbounded history.
const MAX_ORIGINAL_RECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
struct OriginalLocation {
    offset: u64,
    length: usize,
    digest: [u8; 32],
    conflicting: bool,
}

/// A bounded location index, not a copy of the receipt journal. Proof traversal
/// loads each required original once and charges its repeated read explicitly.
pub(crate) struct OriginalReceiptIndex {
    selection_plan: Option<String>,
    latest: BTreeMap<jig_contract::TargetId, (u64, String)>,
    path: PathBuf,
    file: Option<File>,
    metadata: Option<Metadata>,
    locations: BTreeMap<String, OriginalLocation>,
}

impl OriginalReceiptIndex {
    pub(crate) fn open(path: &Path, budget: &mut CollectionBudget<'_>) -> CollectionResult<Self> {
        Self::open_inner(path, None, budget)
    }

    pub(crate) fn open_for_plan(
        path: &Path,
        plan_id: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        Self::open_inner(path, Some(plan_id), budget)
    }

    fn open_inner(
        path: &Path,
        plan_id: Option<&str>,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        budget.ensure_active()?;
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    selection_plan: plan_id.map(str::to_owned),
                    latest: BTreeMap::new(),
                    path: path.into(),
                    file: None,
                    metadata: None,
                    locations: BTreeMap::new(),
                });
            }
            Err(_) => return Err(failed("original receipt journal could not be opened")),
        };
        let metadata = file
            .metadata()
            .map_err(|_| failed("receipt metadata is unavailable"))?;
        if !metadata.is_file() {
            return Err(failed("original receipt journal is not a regular file"));
        }
        let mut reader = BufReader::new(file);
        let mut locations: BTreeMap<String, OriginalLocation> = BTreeMap::new();
        let mut latest = BTreeMap::new();
        let mut offset = 0;
        let mut record = Vec::new();
        while offset < metadata.len() {
            budget.entries(1)?;
            record.clear();
            loop {
                budget.ensure_active()?;
                let available = reader
                    .fill_buf()
                    .map_err(|_| failed("receipt journal read failed"))?;
                if available.is_empty() {
                    return Err(raced());
                }
                let remaining = (metadata.len() - offset - record.len() as u64) as usize;
                let available = &available[..available.len().min(remaining)];
                let count = available
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(available.len(), |end| end + 1);
                if record.len().saturating_add(count) > MAX_ORIGINAL_RECORD_BYTES {
                    return Err(CollectionFailure::new(
                        FreshnessReasonCode::CollectionLimit,
                        "original receipt record exceeds the 16 MiB record limit",
                    ));
                }
                budget.bytes(count as u64)?;
                record.extend_from_slice(&available[..count]);
                reader.consume(count);
                if record.last() == Some(&b'\n') {
                    break;
                }
                if offset + record.len() as u64 == metadata.len() {
                    return Err(failed(
                        "original receipt journal has an unterminated record",
                    ));
                }
            }
            if !record.iter().all(u8::is_ascii_whitespace) {
                let envelope: ReceiptRecord = serde_json::from_slice(&record)
                    .map_err(|_| failed("original receipt journal contains an invalid record"))?;
                if envelope.id.is_empty() {
                    return Err(failed(
                        "original receipt journal contains an empty receipt ID",
                    ));
                }
                if (plan_id.is_none() || envelope.plan_id.as_deref() == plan_id)
                    && let Some(target) = envelope.target
                {
                    let candidate = (envelope.ended_at_ms, envelope.id.clone());
                    if latest
                        .get(&target)
                        .is_none_or(|previous| &candidate > previous)
                    {
                        latest.insert(target, candidate);
                    }
                }
                let location = OriginalLocation {
                    offset,
                    length: record.len(),
                    digest: Sha256::digest(&record).into(),
                    conflicting: false,
                };
                if let Some(previous) = locations.get_mut(&envelope.id) {
                    if !previous.conflicting && previous.digest != location.digest {
                        // Whitespace/key ordering is not a conflicting original.
                        let position = reader
                            .stream_position()
                            .map_err(|_| failed("receipt seek failed"))?;
                        let original = read_at(reader.get_mut(), previous, budget)?;
                        reader
                            .seek(SeekFrom::Start(position))
                            .map_err(|_| failed("receipt seek failed"))?;
                        let original: serde_json::Value = serde_json::from_slice(&original)
                            .map_err(|_| failed("original receipt changed during lookup"))?;
                        let current: serde_json::Value = serde_json::from_slice(&record)
                            .map_err(|_| failed("original receipt is invalid"))?;
                        if original != current {
                            // Ambiguity is permanent for this ID, but unrelated
                            // history cannot invalidate another original.
                            previous.conflicting = true;
                        }
                    }
                } else {
                    locations.insert(envelope.id, location);
                }
            }
            offset += record.len() as u64;
        }
        let index = Self {
            selection_plan: plan_id.map(str::to_owned),
            latest,
            path: path.into(),
            file: Some(reader.into_inner()),
            metadata: Some(metadata),
            locations,
        };
        index.revalidate(budget)?;
        Ok(index)
    }

    pub(crate) fn selected_is_current(&self, receipt: &TargetReceiptStatus) -> bool {
        self.selection_plan.as_deref().is_none_or(|plan| {
            receipt.plan_id.as_deref() == Some(plan)
                && self
                    .latest
                    .get(&receipt.target)
                    .is_some_and(|(_, id)| id == &receipt.receipt_id)
        })
    }

    fn get_record(
        &mut self,
        id: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Option<ReceiptRecord>> {
        budget.ensure_active()?;
        let Some(location) = self.locations.get(id) else {
            return Ok(None);
        };
        if location.conflicting {
            return Err(failed(
                "required original receipt has conflicting duplicate IDs",
            ));
        }
        let bytes = read_at(
            self.file.as_mut().expect("indexed file exists"),
            location,
            budget,
        )?;
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != location.digest {
            return Err(raced());
        }
        let receipt: ReceiptRecord = serde_json::from_slice(&bytes)
            .map_err(|_| failed("referenced original receipt is malformed"))?;
        Ok(Some(receipt))
    }

    pub(crate) fn get(
        &mut self,
        id: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Option<TargetReceiptStatus>> {
        Ok(self.get_record(id, budget)?.and_then(|receipt| {
            receipt
                .target
                .as_ref()
                .map(|target| target_receipt_status(&receipt, target))
        }))
    }

    pub(crate) fn latest_lifecycle_receipt(
        &mut self,
        target: &jig_contract::TargetId,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Option<super::FileBudgetLifecycleReceipt>> {
        let Some((_, id)) = self.latest.get(target) else {
            return Ok(None);
        };
        let id = id.clone();
        Ok(self
            .get_record(&id, budget)?
            .map(super::FileBudgetLifecycleReceipt::from_record))
    }

    pub(crate) fn revalidate(&self, budget: &CollectionBudget<'_>) -> CollectionResult<()> {
        budget.ensure_active()?;
        match (&self.file, &self.metadata) {
            (Some(file), Some(before)) => {
                let retained = file.metadata().map_err(|_| raced())?;
                let current = std::fs::metadata(&self.path).map_err(|_| raced())?;
                if !same_metadata(before, &retained) || !same_metadata(before, &current) {
                    return Err(raced());
                }
            }
            (None, None) => match std::fs::metadata(&self.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(raced()),
            },
            _ => unreachable!("receipt file and metadata are captured together"),
        }
        budget.ensure_active()
    }
}

fn read_at(
    file: &mut File,
    location: &OriginalLocation,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<Vec<u8>> {
    file.seek(SeekFrom::Start(location.offset))
        .map_err(|_| failed("original receipt seek failed"))?;
    let mut bytes = vec![0; location.length];
    let mut offset = 0;
    while offset < bytes.len() {
        budget.ensure_active()?;
        let end = bytes.len().min(offset + 64 * 1024);
        let read = file.read(&mut bytes[offset..end]).map_err(|_| raced())?;
        if read == 0 {
            return Err(raced());
        }
        budget.bytes(read as u64)?;
        offset += read;
    }
    budget.ensure_active()?;
    Ok(bytes)
}

fn same_metadata(before: &Metadata, after: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.len() == after.len()
            && before.mode() == after.mode()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        before.len() == after.len()
            && before.modified().ok() == after.modified().ok()
            && before.is_file() == after.is_file()
    }
}

fn failed(message: &str) -> CollectionFailure {
    CollectionFailure::new(FreshnessReasonCode::CollectionFailed, message)
}

fn raced() -> CollectionFailure {
    CollectionFailure::new(
        FreshnessReasonCode::SourceRaced,
        "original receipt journal changed during proof resolution",
    )
}
