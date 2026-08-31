use super::*;

#[derive(Clone)]
pub(in crate::runtime::loops) struct OccurrenceWorktreeReservation {
    pub(super) store: OccurrenceStore,
    pub(super) occurrence_id: String,
    pub(super) owner: String,
}

impl OccurrenceGuard {
    pub(in crate::runtime::loops) fn worktree_reservation(&self) -> OccurrenceWorktreeReservation {
        OccurrenceWorktreeReservation {
            store: self.store.clone(),
            occurrence_id: self.occurrence_id.clone(),
            owner: self.owner.clone(),
        }
    }
}

impl OccurrenceWorktreeReservation {
    pub(in crate::runtime::loops) fn reserve(&self, path: &Path) -> Result<()> {
        let mut store = self.store.clone();
        store
            .set_running_worktree(
                &self.occurrence_id,
                &self.owner,
                Some(path.to_string_lossy().as_ref()),
            )
            .map(|_| ())
    }

    pub(in crate::runtime::loops) fn clear(&self, path: &Path) -> Result<()> {
        let mut store = self.store.clone();
        store
            .clear_running_worktree(&self.occurrence_id, &self.owner, path)
            .map(|_| ())
    }
}

impl ScheduleOccurrence {
    pub(super) fn has_retained_worktree(&self) -> bool {
        self.worktree
            .as_deref()
            .is_some_and(|worktree| Path::new(worktree).try_exists().unwrap_or(true))
    }
}

impl OccurrenceStore {
    fn set_running_worktree(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        worktree: Option<&str>,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            record.worktree = worktree.map(str::to_string);
            Ok(record.clone())
        })
    }

    fn clear_running_worktree(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        path: &Path,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let record = store.occurrences.get_mut(occurrence_id).ok_or_else(|| {
                anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}")
            })?;
            require_running_owner(record, owner)?;
            if record.worktree.as_deref() != Some(path.to_string_lossy().as_ref()) {
                bail!(
                    "Scheduled occurrence worktree reservation changed before cleanup: {}",
                    path.display()
                );
            }
            record.worktree = None;
            Ok(record.clone())
        })
    }
}
