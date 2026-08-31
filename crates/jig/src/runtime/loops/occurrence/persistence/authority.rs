use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::runtime::loops::authority::resolve_protected_loop_authority;

#[derive(Clone, Debug)]
pub(super) struct ProtectedScheduleAuthority {
    pub(super) root: PathBuf,
    pub(super) dir: PathBuf,
    pub(super) path: PathBuf,
    pub(super) initialized_path: PathBuf,
    pub(super) lock_path: PathBuf,
}

pub(super) fn resolve_protected_schedule_authority(
    repo_root: &Path,
) -> Result<Option<ProtectedScheduleAuthority>> {
    let Some(authority) = resolve_protected_loop_authority(repo_root)? else {
        return Ok(None);
    };
    let dir = authority.dir;
    Ok(Some(ProtectedScheduleAuthority {
        root: authority.root,
        path: dir.join("schedule.json"),
        initialized_path: dir.join("schedule.initialized"),
        lock_path: dir.join("schedule.lock"),
        dir,
    }))
}
