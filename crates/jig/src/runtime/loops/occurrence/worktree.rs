use super::*;

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

const UNIX_PATH_PREFIX: &str = "jig-path-v1:unix-hex:";

pub(in crate::runtime::loops) fn encode_worktree_path(path: &Path) -> String {
    if let Some(path) = path.to_str() {
        return path.to_string();
    }
    #[cfg(unix)]
    {
        let mut encoded =
            String::with_capacity(UNIX_PATH_PREFIX.len() + path.as_os_str().len() * 2);
        encoded.push_str(UNIX_PATH_PREFIX);
        for byte in path.as_os_str().as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(encoded, "{byte:02x}");
        }
        encoded
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned()
    }
}

fn decode_worktree_path(encoded: &str) -> Result<PathBuf> {
    let Some(hex) = encoded.strip_prefix(UNIX_PATH_PREFIX) else {
        return Ok(PathBuf::from(encoded));
    };
    #[cfg(unix)]
    {
        if hex.len() % 2 != 0 {
            bail!("Persisted Unix worktree path has an invalid byte encoding");
        }
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).map_err(|_| {
                    anyhow::anyhow!("Persisted Unix worktree path is not hexadecimal")
                })?;
                u8::from_str_radix(pair, 16)
                    .map_err(|_| anyhow::anyhow!("Persisted Unix worktree path is not hexadecimal"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        bail!("Persisted Unix worktree paths cannot be decoded on this platform")
    }
}

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
                Some(&encode_worktree_path(path)),
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
        self.worktree.as_deref().is_some_and(|worktree| {
            decode_worktree_path(worktree)
                .and_then(|path| path.try_exists().map_err(Into::into))
                .unwrap_or(true)
        })
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
            if record.worktree.as_deref() != Some(encode_worktree_path(path).as_str()) {
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
