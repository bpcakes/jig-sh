use super::*;
use std::collections::VecDeque;

/// Keep a small, fixed set of already verified open files. On Linux, a bounded
/// advisory read allows backing I/O to overlap sibling enumeration. Every byte
/// used as authority still passes through hash_file and its normal accounting.
#[derive(Default)]
pub(super) struct PendingReads(VecDeque<PendingFile>);

struct PendingFile {
    path: String,
    file: cap_std::fs::File,
    metadata: Metadata,
}

impl PendingReads {
    pub(super) fn push(
        &mut self,
        path: String,
        file: cap_std::fs::File,
        metadata: Metadata,
        budget: &CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        budget.ensure_active()?;
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let reserved: u64 = self
                .0
                .iter()
                .map(|file| file.metadata.len().min(65536))
                .sum();
            let remaining = budget
                .limits
                .bytes
                .saturating_sub(budget.stats.content_bytes_read)
                .saturating_sub(reserved);
            let length = metadata.len().min(65536).min(remaining);
            if length != 0 {
                // SAFETY: this live capability owns the fd, and the nonzero
                // length fits off_t. Advice grants no proof and can be ignored
                // by the filesystem, so errors merely leave synchronous reads.
                unsafe {
                    libc::posix_fadvise(
                        file.as_raw_fd(),
                        0,
                        length as libc::off_t,
                        libc::POSIX_FADV_WILLNEED,
                    );
                }
            }
        }
        budget.ensure_active()?;
        self.0.push_back(PendingFile {
            path,
            file,
            metadata,
        });
        Ok(())
    }

    pub(super) fn full(&self) -> bool {
        self.0.len() == 64
    }

    pub(super) fn finish(
        &mut self,
        projection: &mut FileProjection,
        budget: &mut CollectionBudget<'_>,
        buffer: &mut [u8],
    ) -> CollectionResult<()> {
        while !self.0.is_empty() {
            self.finish_one(projection, budget, buffer)?;
        }
        // Callers drain before descending or closing the directory. This keeps
        // its signature observation after all children, including pending files.
        Ok(())
    }

    pub(super) fn finish_one(
        &mut self,
        projection: &mut FileProjection,
        budget: &mut CollectionBudget<'_>,
        buffer: &mut [u8],
    ) -> CollectionResult<()> {
        let mut pending = self.0.pop_front().expect("pending source file exists");
        let digest = hash_file(
            &mut pending.file,
            &pending.metadata,
            budget,
            &pending.path,
            buffer,
        )?;
        projection
            .observed
            .push((pending.path.clone(), signature(&pending.metadata)));
        projection.entries.insert(
            pending.path,
            CurrentEntry {
                kind: "regular",
                mode: executable_mode(&pending.metadata),
                digest: Some(digest),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::freshness::CollectionLimits;
    use std::time::Duration;

    #[test]
    fn queued_open_file_changes_cannot_publish_a_projection() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("example.txt"), "Example original").unwrap();
        let root = open_root(temp.path()).unwrap();
        let metadata = root.symlink_metadata("example.txt").unwrap();
        let file = root.open_with("example.txt", &read_options(false)).unwrap();
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &|| false,
        );
        let mut pending = PendingReads::default();
        pending
            .push("example.txt".into(), file, metadata, &budget)
            .unwrap();
        std::fs::write(temp.path().join("example.txt"), "Changed before streaming").unwrap();
        let mut projection = FileProjection {
            entries: BTreeMap::new(),
            problems: Vec::new(),
            observed: Vec::new(),
        };
        let failure = pending
            .finish(&mut projection, &mut budget, &mut [0; 8192])
            .unwrap_err();
        assert_eq!(failure.reason.code, FreshnessReasonCode::SourceRaced);
        assert!(projection.entries.is_empty());
        assert!(projection.observed.is_empty());
    }
}
