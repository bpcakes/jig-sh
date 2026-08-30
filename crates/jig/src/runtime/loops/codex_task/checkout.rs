use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::context::RepoContext;
use crate::execution::NoopExecutionObserver;
use crate::state::with_stable_jsonl_file;

use super::{
    WORKER_RECEIPT_PATH, git_is_dirty, git_stdout, remove_worktree, repo_task_has_changes,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskOutcome {
    Succeeded,
    Failed,
}

pub(super) enum PreparedCheckout {
    Repo {
        path: PathBuf,
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
    pub(super) fn repository_requires_attention(&self) -> bool {
        matches!(
            self,
            Self::Repository {
                dirty: Some(true) | None,
                ..
            } | Self::Repository {
                receipt_append_valid: Some(false) | None,
                ..
            }
        )
    }

    pub(super) fn retained_worktree(&self) -> Option<String> {
        match self {
            Self::Worktree {
                path,
                retained: true,
                ..
            } => Some(path.display().to_string()),
            Self::Repository { .. } | Self::Worktree { .. } => None,
        }
    }

    pub(super) fn value(&self) -> Value {
        match self {
            Self::Repository {
                path,
                dirty,
                receipt_append_valid,
            } => json!({
                "mode": "repo",
                "path": path,
                "retained": true,
                "dirty": dirty,
                "receipt_append_valid": receipt_append_valid,
            }),
            Self::Worktree {
                path,
                retained,
                dirty,
                head_changed,
            } => json!({
                "mode": "worktree",
                "path": path,
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
                receipt_journal,
            } => {
                let dirty = repo_task_has_changes(ctx, &path, &mut cleanup_observer);
                let receipt_append = receipt_journal.verify(ctx, worker_receipt_id);
                let mut errors = Vec::new();
                if let Err(error) = &dirty {
                    errors.push(format!(
                        "Failed to inspect retained task checkout: {error:#}"
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
    path_existed: bool,
    byte_len: u64,
    digest: [u8; 32],
    index_entry: Option<String>,
}

impl ReceiptJournalBaseline {
    pub(super) fn capture(ctx: &RepoContext) -> Result<Self> {
        let path = ctx.root().join(WORKER_RECEIPT_PATH);
        let snapshot = with_stable_jsonl_file(&path, |file| capture_prefix(&path, file))?;
        let path_existed = snapshot.is_some();
        let (byte_len, digest) = snapshot.unwrap_or_else(|| (0, Sha256::digest([]).into()));
        let mut observer = NoopExecutionObserver;
        let index_entry = match git_stdout(
            ctx,
            ctx.root(),
            ["ls-files", "--stage", "--", WORKER_RECEIPT_PATH],
            &mut observer,
        ) {
            Ok(entry) => Some(entry),
            Err(_) if !path_existed => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            path_existed,
            byte_len,
            digest,
            index_entry,
        })
    }

    fn verify(&self, ctx: &RepoContext, worker_receipt_id: Option<&str>) -> Result<()> {
        let mut observer = NoopExecutionObserver;
        if let Some(baseline_entry) = self.index_entry.as_deref() {
            let index_entry = git_stdout(
                ctx,
                ctx.root(),
                ["ls-files", "--stage", "--", WORKER_RECEIPT_PATH],
                &mut observer,
            )?;
            if index_entry != baseline_entry {
                bail!("Git index entry changed for {WORKER_RECEIPT_PATH}");
            }
        }

        let verified = with_stable_jsonl_file(&self.path, |file| {
            verify_append(self, file, worker_receipt_id)
        })?;
        if verified.is_none() {
            if !self.path_existed && worker_receipt_id.is_none() {
                return Ok(());
            }
            bail!("Receipt journal disappeared: {}", self.path.display());
        }
        Ok(())
    }
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

fn capture_prefix(path: &Path, source: &File) -> Result<(u64, [u8; 32])> {
    require_regular_receipt(path, source)?;
    let mut file = source
        .try_clone()
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    let byte_len = file
        .metadata()
        .with_context(|| format!("Failed to inspect {}", path.display()))?
        .len();
    let mut hasher = Sha256::new();
    let copied = std::io::copy(&mut (&mut file).take(byte_len), &mut hasher)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if copied != byte_len {
        bail!("Receipt journal changed while it was being inspected");
    }
    let digest = hasher.finalize().into();
    if byte_len > 0 {
        file.seek(SeekFrom::End(-1))
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
    Ok((byte_len, digest))
}

fn verify_append(
    baseline: &ReceiptJournalBaseline,
    source: &File,
    worker_receipt_id: Option<&str>,
) -> Result<()> {
    require_regular_receipt(&baseline.path, source)?;
    let mut file = source
        .try_clone()
        .with_context(|| format!("Failed to inspect {}", baseline.path.display()))?;
    let current_len = file
        .metadata()
        .with_context(|| format!("Failed to inspect {}", baseline.path.display()))?
        .len();
    if current_len < baseline.byte_len {
        bail!(
            "receipt journal shrank from {} to {} bytes",
            baseline.byte_len,
            current_len
        );
    }
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
    let mut reader = BufReader::new(file);
    let mut found_worker_receipt = false;
    let mut appended_records = 0_u64;
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
        appended_records += 1;
        record.pop();
        let value = serde_json::from_slice::<Value>(&record)
            .context("Appended receipt record is not valid JSON")?;
        if worker_receipt_id.is_some_and(|expected| value["id"].as_str() == Some(expected)) {
            found_worker_receipt = true;
        }
    }
    if worker_receipt_id.is_some() && !found_worker_receipt {
        return Err(anyhow!(
            "worker receipt was not appended to the durable journal"
        ));
    }
    if worker_receipt_id.is_none() && appended_records != 0 {
        bail!("receipt records were appended without an expected worker receipt");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Write as _;

    use tempfile::tempdir;

    use super::*;
    use crate::bootstrap::GIT_BIN_ENV;
    use crate::test_env::{EnvVarGuard, lock_env};

    fn fixture() -> (tempfile::TempDir, RepoContext, PathBuf) {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = std::process::Command::new("git")
            .current_dir(temp.path())
            .arg("init")
            .output()
            .unwrap();
        assert!(git.status.success(), "{git:?}");
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let path = temp.path().join(WORKER_RECEIPT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        (temp, ctx, path)
    }

    #[test]
    fn receipt_baseline_rejects_same_length_prefix_rewrite() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        fs::write(&path, b"{\"id\":\"old-a\"}\n").unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();

        fs::write(&path, b"{\"id\":\"old-b\"}\n{\"id\":\"receipt-worker\"}\n").unwrap();

        let error = baseline.verify(&ctx, Some("receipt-worker")).unwrap_err();
        assert!(error.to_string().contains("prefix was rewritten"));
    }

    #[test]
    fn receipt_baseline_rejects_unattributed_appends() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        fs::write(&path, b"{\"id\":\"old\"}\n").unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"id\":\"unexpected\"}\n")
            .unwrap();

        let error = baseline.verify(&ctx, None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without an expected worker receipt")
        );
    }

    #[test]
    fn absent_receipt_journal_remains_valid_without_a_worker_receipt() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();

        baseline.verify(&ctx, None).unwrap();
    }
}
