use std::collections::BTreeSet;
use std::path::PathBuf;

use tempfile::TempDir;

pub(super) const FILE_BUDGET_POLICY_PATH: &str = ".jig/file-budget.toml";

pub(super) struct StagedRender {
    pub(super) _root: TempDir,
    pub(super) destination: PathBuf,
    pub(super) active_paths: BTreeSet<PathBuf>,
    pub(super) retirement_paths: BTreeSet<PathBuf>,
}

impl StagedRender {
    pub(super) fn operation_count(&self) -> usize {
        self.active_paths.len() + self.retirement_paths.len() + self.authored_seed_paths().len()
    }

    /// Seed-once authored files staged beside managed output but deliberately
    /// excluded from the managed-path manifest and future replacement.
    pub(super) fn authored_seed_paths(&self) -> Vec<PathBuf> {
        let path = PathBuf::from(FILE_BUDGET_POLICY_PATH);
        if !self.active_paths.contains(&path) && self.destination.join(&path).is_file() {
            vec![path]
        } else {
            Vec::new()
        }
    }
}
