//! Complete, bounded-retry scans for output aliases of witness files.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{IDS_DIR, JOURNALS_DIR, LOCKS_DIR};

const SCAN_ATTEMPTS: usize = 3;

pub(super) fn check(root: &Path, output: &fs::Metadata) -> Result<bool> {
    check_with(root, output, |path| fs::symlink_metadata(path))
}

fn check_with(
    root: &Path,
    output: &fs::Metadata,
    mut inspect: impl FnMut(&Path) -> io::Result<fs::Metadata>,
) -> Result<bool> {
    'scan: for _ in 0..SCAN_ATTEMPTS {
        for child in [IDS_DIR, JOURNALS_DIR, LOCKS_DIR] {
            let directory = root.join(child);
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect vault witness {}", directory.display())
                    });
                }
            };
            for entry in entries {
                let path = entry?.path();
                let metadata = match inspect(&path) {
                    Ok(metadata) => metadata,
                    // Atomic writes rename temporaries and completed
                    // transactions unlink journals. Re-enumerate the whole
                    // tree: a renamed inode may now have a different name
                    // in a directory we already visited. Skipping this entry
                    // would mistake an incomplete scan for proof of absence.
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue 'scan,
                    Err(error) => return Err(error.into()),
                };
                if metadata.dev() == output.dev() && metadata.ino() == output.ino() {
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    }
    bail!("vault witness kept changing during output alias inspection; retry the operation")
}

#[cfg(test)]
mod tests;
