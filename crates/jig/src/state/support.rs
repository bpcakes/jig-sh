//! State helpers that are independent of durable record schemas and JSONL mechanics.

use std::fs::{self, File};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use ulid::Ulid;

use crate::context::RepoContext;

#[cfg(test)]
thread_local! {
    static TEST_NOW_MS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestNowGuard(Option<u64>);

#[cfg(test)]
impl Drop for TestNowGuard {
    fn drop(&mut self) {
        TEST_NOW_MS.set(self.0);
    }
}

#[cfg(test)]
pub(crate) fn set_test_now_ms(value: u64) -> TestNowGuard {
    let previous = TEST_NOW_MS.replace(Some(value));
    TestNowGuard(previous)
}

pub(super) struct AdvisoryLeaseFile(File);

impl AdvisoryLeaseFile {
    pub(super) fn new(file: File) -> Self {
        Self(file)
    }

    #[cfg(test)]
    pub(super) fn try_clone(&self) -> std::io::Result<File> {
        self.0.try_clone()
    }
}

impl Drop for AdvisoryLeaseFile {
    fn drop(&mut self) {
        // flock locks survive dup/fork as references to the same lock. Unlock
        // explicitly so a short-lived inherited descriptor cannot extend a
        // finished owner's lease until the child closes or execs it.
        let _ = FileExt::unlock(&self.0);
    }
}

pub(crate) fn now_ms() -> u64 {
    #[cfg(test)]
    if let Some(value) = TEST_NOW_MS.get() {
        return value;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn truncate(value: &str) -> String {
    const LIMIT: usize = 4000;
    if value.len() <= LIMIT {
        value.to_string()
    } else {
        let mut end = LIMIT;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

pub(super) fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Ulid::new())
}

pub(super) fn ensure_state_layout(ctx: &RepoContext) -> Result<()> {
    fs::create_dir_all(ctx.state_dir())?;
    fs::create_dir_all(ctx.root().join(".agent/plans"))?;
    if let Some(parent) = ctx.current_session_path().parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub(super) fn rel_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .display()
        .to_string())
}
