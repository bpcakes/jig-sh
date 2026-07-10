use std::collections::BTreeSet;
use std::path::PathBuf;

use tempfile::TempDir;

pub(super) struct StagedRender {
    pub(super) _root: TempDir,
    pub(super) destination: PathBuf,
    pub(super) active_paths: BTreeSet<PathBuf>,
    pub(super) retirement_paths: BTreeSet<PathBuf>,
}

impl StagedRender {
    pub(super) fn operation_count(&self) -> usize {
        self.active_paths.len() + self.retirement_paths.len()
    }
}
