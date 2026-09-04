use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::bootstrap::path::{RepositoryEntryIdentity, repository_file_identity};
use crate::context::RepoContext;
use crate::execution::NoopExecutionObserver;
use crate::state::{receipt_record_id, with_receipt_journal_writer};

use super::{
    RepositoryRevisionState, WORKER_RECEIPT_PATH, git_is_dirty, git_stdout, remove_worktree,
    repo_task_has_changes,
};

const MAX_VERIFIED_RECEIPT_BASELINE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VERIFIED_RECEIPT_APPEND_BYTES: u64 = 16 * 1024 * 1024;
const RECEIPT_SNAPSHOT_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskOutcome {
    Succeeded,
    Failed,
}

pub(super) enum PreparedCheckout {
    Repo {
        path: PathBuf,
        initial_head: String,
        receipt_journal: ReceiptJournalBaseline,
    },
    Worktree {
        repo_root: PathBuf,
        path: PathBuf,
        initial_head: String,
    },
}

pub(super) struct CheckoutCompletion {
    pub(super) report: CheckoutReport,
    pub(super) error: Option<String>,
}

pub(super) enum CheckoutReport {
    Repository {
        path: PathBuf,
        dirty: Option<bool>,
        head_changed: Option<bool>,
        receipt_append_valid: Option<bool>,
    },
    Worktree {
        path: PathBuf,
        retained: bool,
        dirty: Option<bool>,
        head_changed: Option<bool>,
    },
}

impl CheckoutReport {
    pub(super) const fn is_repository(&self) -> bool {
        matches!(self, Self::Repository { .. })
    }

    pub(super) fn repository_requires_attention(&self) -> bool {
        self.repository_revision_state() == RepositoryRevisionState::Unknown
            || matches!(
                self,
                Self::Repository {
                    receipt_append_valid: Some(false) | None,
                    ..
                }
            )
    }

    pub(super) fn repository_revision_state(&self) -> RepositoryRevisionState {
        match self {
            Self::Repository {
                dirty: Some(false),
                head_changed: Some(false),
                ..
            } => RepositoryRevisionState::Unchanged,
            Self::Repository {
                dirty: Some(false),
                head_changed: Some(true),
                ..
            } => RepositoryRevisionState::Changed,
            Self::Repository { .. } => RepositoryRevisionState::Unknown,
            Self::Worktree { .. } => RepositoryRevisionState::NotApplicable,
        }
    }

    pub(super) fn retained_worktree(&self) -> Option<String> {
        match self {
            Self::Worktree {
                path,
                retained: true,
                ..
            } => Some(super::super::occurrence::encode_worktree_path(path)),
            Self::Repository { .. } | Self::Worktree { .. } => None,
        }
    }

    pub(super) fn value(&self) -> Value {
        match self {
            Self::Repository {
                path,
                dirty,
                head_changed,
                receipt_append_valid,
            } => json!({
                "mode": "repo",
                "path": super::super::occurrence::encode_worktree_path(path),
                "retained": true,
                "dirty": dirty,
                "head_changed": head_changed,
                "receipt_append_valid": receipt_append_valid,
            }),
            Self::Worktree {
                path,
                retained,
                dirty,
                head_changed,
            } => json!({
                "mode": "worktree",
                "path": super::super::occurrence::encode_worktree_path(path),
                "retained": retained,
                "dirty": dirty,
                "head_changed": head_changed,
            }),
        }
    }
}

impl PreparedCheckout {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Repo { path, .. } | Self::Worktree { path, .. } => path,
        }
    }

    pub(super) fn finish(
        self,
        outcome: TaskOutcome,
        ctx: &RepoContext,
        worker_receipt_id: Option<&str>,
    ) -> CheckoutCompletion {
        let mut cleanup_observer = NoopExecutionObserver;
        match self {
            Self::Repo {
                path,
                initial_head,
                receipt_journal,
            } => {
                let dirty = repo_task_has_changes(ctx, &path, &mut cleanup_observer);
                let final_head =
                    git_stdout(ctx, &path, ["rev-parse", "HEAD"], &mut cleanup_observer);
                let receipt_append = receipt_journal.verify(ctx, worker_receipt_id);
                let mut errors = Vec::new();
                if let Err(error) = &dirty {
                    errors.push(format!(
                        "Failed to inspect retained task checkout: {error:#}"
                    ));
                }
                if let Err(error) = &final_head {
                    errors.push(format!(
                        "Failed to inspect retained task checkout HEAD: {error:#}"
                    ));
                }
                if let Err(error) = &receipt_append {
                    errors.push(format!(
                        "Durable receipt history changed outside append-only runtime writes: {error:#}"
                    ));
                }
                CheckoutCompletion {
                    report: CheckoutReport::Repository {
                        path,
                        dirty: dirty.ok(),
                        head_changed: final_head.ok().map(|head| head != initial_head),
                        receipt_append_valid: Some(receipt_append.is_ok()),
                    },
                    error: (!errors.is_empty()).then(|| errors.join("; ")),
                }
            }
            Self::Worktree {
                repo_root,
                path,
                initial_head,
            } => {
                let dirty = git_is_dirty(ctx, &path, &mut cleanup_observer);
                let final_head =
                    git_stdout(ctx, &path, ["rev-parse", "HEAD"], &mut cleanup_observer);
                let mut errors = Vec::new();
                if let Err(error) = &dirty {
                    errors.push(format!("Failed to inspect task worktree status: {error:#}"));
                }
                if let Err(error) = &final_head {
                    errors.push(format!("Failed to inspect task worktree HEAD: {error:#}"));
                }
                let dirty = dirty.ok();
                let head_changed = final_head.ok().map(|head| head != initial_head);
                let mut retained = outcome == TaskOutcome::Failed
                    || dirty.unwrap_or(true)
                    || head_changed.unwrap_or(true);
                if !retained
                    && let Err(error) =
                        remove_worktree(ctx, &repo_root, &path, false, &mut cleanup_observer)
                {
                    retained = true;
                    errors.push(format!("Failed to remove clean task worktree: {error:#}"));
                }
                CheckoutCompletion {
                    report: CheckoutReport::Worktree {
                        path,
                        retained,
                        dirty,
                        head_changed,
                    },
                    error: (!errors.is_empty()).then(|| errors.join("; ")),
                }
            }
        }
    }
}

pub(super) struct ReceiptJournalBaseline {
    path: PathBuf,
    identity: Option<RepositoryEntryIdentity>,
    byte_len: u64,
    digest: [u8; 32],
    index_entry: String,
}

impl ReceiptJournalBaseline {
    pub(super) fn capture(ctx: &RepoContext) -> Result<Self> {
        Self::capture_with(ctx, || {})
    }

    fn capture_with(ctx: &RepoContext, mut after_snapshot: impl FnMut()) -> Result<Self> {
        let path = ctx.root().join(WORKER_RECEIPT_PATH);
        let mut baseline = None;
        for _ in 0..RECEIPT_SNAPSHOT_ATTEMPTS {
            let snapshot = open_receipt_journal_snapshot(ctx, &path)?;
            after_snapshot();
            let (identity, byte_len, digest) = match snapshot {
                Some(snapshot) => {
                    let digest = capture_prefix(&path, &snapshot.file, snapshot.byte_len)?;
                    if !receipt_snapshot_is_current(ctx, &path, &snapshot)? {
                        continue;
                    }
                    (Some(snapshot.identity), snapshot.byte_len, digest)
                }
                None => (None, 0, Sha256::digest([]).into()),
            };
            baseline = Some((identity, byte_len, digest));
            break;
        }
        let Some((identity, byte_len, digest)) = baseline else {
            bail!(
                "Receipt journal changed during all {RECEIPT_SNAPSHOT_ATTEMPTS} snapshot attempts"
            );
        };
        let mut observer = NoopExecutionObserver;
        let index_entry = git_stdout(
            ctx,
            ctx.root(),
            ["ls-files", "--stage", "--", WORKER_RECEIPT_PATH],
            &mut observer,
        )?;
        Ok(Self {
            path,
            identity,
            byte_len,
            digest,
            index_entry,
        })
    }

    fn verify(&self, ctx: &RepoContext, worker_receipt_id: Option<&str>) -> Result<()> {
        self.verify_with(ctx, worker_receipt_id, || {})
    }

    fn verify_with(
        &self,
        ctx: &RepoContext,
        worker_receipt_id: Option<&str>,
        after_snapshot: impl FnOnce(),
    ) -> Result<()> {
        let mut observer = NoopExecutionObserver;
        let index_entry = git_stdout(
            ctx,
            ctx.root(),
            ["ls-files", "--stage", "--", WORKER_RECEIPT_PATH],
            &mut observer,
        )?;
        if index_entry != self.index_entry {
            bail!("Git index entry changed for {WORKER_RECEIPT_PATH}");
        }

        let Some(snapshot) = open_receipt_journal_snapshot(ctx, &self.path)? else {
            if self.identity.is_none() && worker_receipt_id.is_none() {
                return Ok(());
            }
            bail!("Receipt journal disappeared: {}", self.path.display());
        };
        if let Some(identity) = self.identity.as_ref()
            && snapshot.identity != *identity
        {
            bail!("Receipt journal identity changed: {}", self.path.display());
        }
        after_snapshot();
        verify_append(self, &snapshot.file, snapshot.byte_len, worker_receipt_id)?;
        if !receipt_snapshot_is_current(ctx, &self.path, &snapshot)? {
            bail!(
                "Receipt journal changed while it was being verified: {}",
                self.path.display()
            );
        }
        Ok(())
    }
}

struct ReceiptJournalSnapshot {
    file: File,
    identity: RepositoryEntryIdentity,
    byte_len: u64,
}

fn open_receipt_journal_snapshot(
    ctx: &RepoContext,
    path: &Path,
) -> Result<Option<ReceiptJournalSnapshot>> {
    with_receipt_journal_writer(ctx, |writer| {
        writer.inspect(|file| {
            require_regular_receipt(path, file)?;
            let file = file
                .try_clone()
                .with_context(|| format!("Failed to inspect {}", path.display()))?;
            let identity = repository_file_identity(&file)?;
            let byte_len = file
                .metadata()
                .with_context(|| format!("Failed to inspect {}", path.display()))?
                .len();
            Ok(ReceiptJournalSnapshot {
                file,
                identity,
                byte_len,
            })
        })
    })
}

fn receipt_snapshot_is_current(
    ctx: &RepoContext,
    path: &Path,
    snapshot: &ReceiptJournalSnapshot,
) -> Result<bool> {
    with_receipt_journal_writer(ctx, |writer| {
        let Some(current) = writer.inspect(|file| {
            require_regular_receipt(path, file)?;
            Ok((
                repository_file_identity(file)?,
                file.metadata()
                    .with_context(|| format!("Failed to inspect {}", path.display()))?
                    .len(),
            ))
        })?
        else {
            return Ok(false);
        };
        Ok(current.0 == snapshot.identity && current.1 == snapshot.byte_len)
    })
}

fn require_regular_receipt(path: &Path, file: &File) -> Result<()> {
    let path_metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    let file_metadata = file
        .metadata()
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    if !path_metadata.file_type().is_file() || !file_metadata.file_type().is_file() {
        bail!("Receipt journal is not a regular file: {}", path.display());
    }
    Ok(())
}

fn capture_prefix(path: &Path, source: &File, byte_len: u64) -> Result<[u8; 32]> {
    if byte_len > MAX_VERIFIED_RECEIPT_BASELINE_BYTES {
        bail!(
            "receipt journal exceeds the {MAX_VERIFIED_RECEIPT_BASELINE_BYTES} byte verification limit; archive old receipts before retrying"
        );
    }
    let mut file = source
        .try_clone()
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    let mut hasher = Sha256::new();
    let copied = std::io::copy(&mut (&mut file).take(byte_len), &mut hasher)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if copied != byte_len {
        bail!("Receipt journal changed while it was being inspected");
    }
    let digest = hasher.finalize().into();
    if byte_len > 0 {
        file.seek(SeekFrom::Start(byte_len - 1))
            .with_context(|| format!("Failed to inspect {}", path.display()))?;
        let mut last = [0_u8; 1];
        file.read_exact(&mut last)
            .with_context(|| format!("Failed to inspect {}", path.display()))?;
        if last[0] != b'\n' {
            bail!(
                "Receipt journal has an unterminated record: {}",
                path.display()
            );
        }
    }
    Ok(digest)
}

fn verify_append(
    baseline: &ReceiptJournalBaseline,
    source: &File,
    current_len: u64,
    worker_receipt_id: Option<&str>,
) -> Result<()> {
    let mut file = source
        .try_clone()
        .with_context(|| format!("Failed to inspect {}", baseline.path.display()))?;
    if current_len < baseline.byte_len {
        bail!(
            "receipt journal shrank from {} to {} bytes",
            baseline.byte_len,
            current_len
        );
    }
    let append_bytes = current_len - baseline.byte_len;
    if append_bytes > MAX_VERIFIED_RECEIPT_APPEND_BYTES {
        bail!(
            "receipt journal append exceeds the {MAX_VERIFIED_RECEIPT_APPEND_BYTES} byte verification limit"
        );
    }
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("Failed to inspect {}", baseline.path.display()))?;
    let mut prefix = (&mut file).take(baseline.byte_len);
    let mut hasher = Sha256::new();
    std::io::copy(&mut prefix, &mut hasher)
        .with_context(|| format!("Failed to verify {}", baseline.path.display()))?;
    let digest: [u8; 32] = hasher.finalize().into();
    if digest != baseline.digest {
        bail!("receipt journal prefix was rewritten");
    }

    file.seek(SeekFrom::Start(baseline.byte_len))
        .with_context(|| format!("Failed to inspect {}", baseline.path.display()))?;
    let mut reader = BufReader::new(file.take(append_bytes));
    let mut appended_records = 0_u64;
    let mut worker_receipt_matches = 0_u64;
    loop {
        let mut record = Vec::new();
        let read = reader
            .read_until(b'\n', &mut record)
            .with_context(|| format!("Failed to inspect {}", baseline.path.display()))?;
        if read == 0 {
            break;
        }
        if record.last() != Some(&b'\n') {
            bail!("appended receipt record is unterminated");
        }
        record.pop();
        let receipt_id = receipt_record_id(&record)?;
        appended_records += 1;
        if worker_receipt_id == Some(receipt_id.as_str()) {
            worker_receipt_matches += 1;
        }
    }
    match worker_receipt_id {
        Some(_) if appended_records != 1 || worker_receipt_matches != 1 => bail!(
            "expected the worker receipt to be the only appended record; found {appended_records} records and {worker_receipt_matches} matching worker receipts"
        ),
        None if appended_records != 0 => bail!(
            "expected no receipt append without a worker receipt; found {appended_records} records"
        ),
        Some(_) | None => {}
    }
    Ok(())
}

#[cfg(test)]
include!("checkout_tests.rs");
