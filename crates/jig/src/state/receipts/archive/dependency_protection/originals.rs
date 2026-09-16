use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use super::super::{
    JsonlWriteGuard, parse_raw_receipt, scan_jsonl_raw_locked, target_receipt_status,
};
use crate::state::{TargetReceiptStatus, records::ReceiptRecord};

#[cfg(test)]
thread_local! {
    pub(super) static RECORD_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Location {
    offset: u64,
    length: usize,
    digest: [u8; 32],
    conflicting: bool,
}

/// Maintenance has no inspection quotas: retain locations, never the journal's
/// full payloads, and keep the writer lock alive throughout indexed traversal.
pub(super) struct ArchiveOriginalIndex<'a> {
    _guard: &'a JsonlWriteGuard,
    file: File,
    locations: BTreeMap<String, Location>,
    pub(super) loaded_bytes: u64,
}

impl<'a> ArchiveOriginalIndex<'a> {
    pub(super) fn open(guard: &'a JsonlWriteGuard, path: &Path) -> Result<Self> {
        let mut file = File::open(path).context("Cannot open archive original receipt journal")?;
        let mut locations: BTreeMap<String, Location> = BTreeMap::new();
        let scan = scan_jsonl_raw_locked(guard, path, &|| false, |record| {
            #[cfg(test)]
            RECORD_VISITS.set(RECORD_VISITS.get() + 1);
            let receipt = parse_raw_receipt(record, path)?;
            let location = Location {
                offset: record.start_offset,
                length: record.bytes.len(),
                digest: Sha256::digest(record.bytes).into(),
                conflicting: false,
            };
            if let Some(previous) = locations.get_mut(&receipt.id) {
                if !previous.conflicting && previous.digest != location.digest {
                    let original: serde_json::Value =
                        serde_json::from_slice(&read_at(&mut file, previous)?)?;
                    let current: serde_json::Value = serde_json::from_slice(record.bytes)?;
                    // Formatting changes are equivalent; any actual conflict
                    // permanently invalidates this ID, including unknown fields.
                    previous.conflicting = original != current;
                }
            } else {
                locations.insert(receipt.id, location);
            }
            Ok(())
        })?;
        ensure!(
            !scan.unterminated_final_record,
            "Cannot archive an unterminated original receipt journal"
        );
        Ok(Self {
            _guard: guard,
            file,
            locations,
            loaded_bytes: 0,
        })
    }

    pub(super) fn get(&mut self, id: &str) -> Result<TargetReceiptStatus> {
        let location = self
            .locations
            .get(id)
            .context("Cannot archive while an original dependency receipt is missing")?;
        ensure!(
            !location.conflicting,
            "Required original receipt has conflicting duplicate IDs"
        );
        let receipt: ReceiptRecord = serde_json::from_slice(&read_at(&mut self.file, location)?)?;
        self.loaded_bytes = self.loaded_bytes.saturating_add(location.length as u64);
        ensure!(
            receipt.id == id,
            "Original dependency receipt changed during archive protection"
        );
        let target = receipt
            .target
            .as_ref()
            .context("Cannot archive while an original dependency receipt is missing")?;
        Ok(target_receipt_status(&receipt, target))
    }
}

fn read_at(file: &mut File, location: &Location) -> Result<Vec<u8>> {
    #[cfg(test)]
    RECORD_VISITS.set(RECORD_VISITS.get() + 1);
    file.seek(SeekFrom::Start(location.offset))?;
    let mut bytes = vec![0; location.length];
    file.read_exact(&mut bytes)?;
    ensure!(
        <[u8; 32]>::from(Sha256::digest(&bytes)) == location.digest,
        "Original dependency receipt changed during archive protection"
    );
    Ok(bytes)
}
