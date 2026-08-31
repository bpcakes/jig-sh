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
    WORKER_RECEIPT_PATH, git_is_dirty, git_stdout, remove_worktree, repo_task_has_changes,
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
    pub(super) const fn is_repository(&self) -> bool {
        matches!(self, Self::Repository { .. })
    }

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
                "Receipt journal changed identity during all {RECEIPT_SNAPSHOT_ATTEMPTS} snapshot attempts"
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
                "Receipt journal changed identity while it was being verified: {}",
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
        Ok(current.0 == snapshot.identity && current.1 >= snapshot.byte_len)
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
        if worker_receipt_id == Some(receipt_id.as_str()) {
            worker_receipt_matches += 1;
        }
    }
    if worker_receipt_id.is_some() && worker_receipt_matches != 1 {
        bail!(
            "expected exactly one matching worker receipt append, found {worker_receipt_matches}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::thread;

    use tempfile::tempdir;

    use super::*;
    use crate::bootstrap::GIT_BIN_ENV;
    use crate::state::{ReceiptInput, now_ms, record_receipt};
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

    fn append_runtime_receipt(ctx: &RepoContext) -> Result<String> {
        record_receipt(ctx, receipt_input())
    }

    fn receipt_input() -> ReceiptInput<'static> {
        let now = now_ms();
        ReceiptInput {
            tool_name: "worker_run",
            args: json!({"purpose": "scheduled_codex_task"}),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: now,
            ended_at_ms: now,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: Some(json!({"kind": "worker_run"})),
            session_override: Some("session-example".into()),
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        }
    }

    #[test]
    fn short_journal_windows_accept_the_worker_receipt() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();

        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let receipt_id = append_runtime_receipt(&ctx).unwrap();
        baseline.verify(&ctx, Some(&receipt_id)).unwrap();
    }

    #[test]
    fn unrelated_runtime_receipt_can_finish_during_repo_worker_execution() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let competing_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let competing_writer =
            thread::spawn(move || record_receipt(&competing_ctx, receipt_input()));
        competing_writer.join().unwrap().unwrap();
        let receipt_id = append_runtime_receipt(&ctx).unwrap();
        baseline.verify(&ctx, Some(&receipt_id)).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn git_index_probe_does_not_hold_the_receipt_writer_lock() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let _env_lock = lock_env();
        let (temp, ctx, _path) = fixture();
        let marker = temp.path().join("git-probe-started");
        let release = temp.path().join("release-git-probe");
        let git = temp.path().join("git-probe-stub");
        fs::write(
            &git,
            format!(
                "#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.01; done\n",
                marker.display(),
                release.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
        let capture_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let capture = thread::spawn(move || ReceiptJournalBaseline::capture(&capture_ctx));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !marker.exists() {
            fs::write(&release, b"").unwrap();
            let detail = match capture.join().unwrap() {
                Ok(_) => "probe completed without creating its marker".to_string(),
                Err(error) => format!("probe failed: {error:#}"),
            };
            panic!("Git index probe did not start: {detail}");
        }

        let writer_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (sent, received) = mpsc::channel();
        let writer = thread::spawn(move || {
            let result = append_runtime_receipt(&writer_ctx);
            let _ = sent.send(());
            result
        });
        let wrote_during_probe = received.recv_timeout(Duration::from_secs(1)).is_ok();
        fs::write(&release, b"").unwrap();
        let baseline = capture.join().unwrap().unwrap();
        let receipt_id = writer.join().unwrap().unwrap();

        assert!(
            wrote_during_probe,
            "receipt append was blocked by the Git index probe"
        );
        baseline.verify(&ctx, Some(&receipt_id)).unwrap();
    }

    #[test]
    fn receipt_prefix_capture_does_not_hold_the_writer_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let first_receipt = append_runtime_receipt(&ctx).unwrap();
        let capture_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let capture = thread::spawn(move || {
            ReceiptJournalBaseline::capture_with(&capture_ctx, || {
                snapshot_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            })
        });
        snapshot_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let writer_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (writer_tx, writer_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            let receipt = append_runtime_receipt(&writer_ctx);
            writer_tx.send(()).unwrap();
            receipt
        });
        writer_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        resume_tx.send(()).unwrap();

        let baseline = capture.join().unwrap().unwrap();
        let second_receipt = writer.join().unwrap().unwrap();
        baseline.verify(&ctx, None).unwrap();
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.contains(&first_receipt));
        assert!(contents.contains(&second_receipt));
    }

    #[test]
    fn receipt_append_verification_does_not_hold_the_writer_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, _path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let worker_receipt = append_runtime_receipt(&ctx).unwrap();
        let verify_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let verify = thread::spawn(move || {
            baseline.verify_with(&verify_ctx, Some(&worker_receipt), || {
                snapshot_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            })
        });
        snapshot_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let writer_ctx = RepoContext::load_from(ctx.root()).unwrap();
        let (writer_tx, writer_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            let receipt = append_runtime_receipt(&writer_ctx);
            writer_tx.send(()).unwrap();
            receipt
        });
        writer_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        resume_tx.send(()).unwrap();

        verify.join().unwrap().unwrap();
        writer.join().unwrap().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn absent_receipt_journal_still_requires_a_successful_index_probe() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = lock_env();
        let (temp, ctx, _path) = fixture();
        let git = temp.path().join("git-fails");
        fs::write(&git, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());

        let Err(error) = ReceiptJournalBaseline::capture(&ctx) else {
            panic!("an index probe failure must reject an absent receipt journal");
        };

        assert!(
            error
                .to_string()
                .contains("Git command failed with status 1"),
            "{error:#}"
        );
    }

    #[test]
    fn worker_injection_before_the_runtime_receipt_is_rejected() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();

        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{}\n")
            .unwrap();
        let receipt_id = append_runtime_receipt(&ctx).unwrap();
        let error = baseline.verify(&ctx, Some(&receipt_id)).unwrap_err();

        assert!(error.to_string().contains("durable receipt schema"));
    }

    #[test]
    fn oversized_worker_append_is_rejected_before_record_allocation() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();

        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&vec![b'x'; MAX_VERIFIED_RECEIPT_APPEND_BYTES as usize + 1])
            .unwrap();
        let error = baseline.verify(&ctx, None).unwrap_err();

        assert!(error.to_string().contains("byte verification limit"));
    }

    #[test]
    fn oversized_receipt_baseline_requires_archival_before_worker_start() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .unwrap();
        file.set_len(MAX_VERIFIED_RECEIPT_BASELINE_BYTES + 1)
            .unwrap();

        let Err(error) = ReceiptJournalBaseline::capture(&ctx) else {
            panic!("an oversized active receipt journal must be rejected");
        };

        assert!(error.to_string().contains("archive old receipts"));
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
    fn receipt_baseline_accepts_other_schema_valid_runtime_appends() {
        let _env_lock = lock_env();
        let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
        let (_temp, ctx, path) = fixture();
        let baseline = ReceiptJournalBaseline::capture(&ctx).unwrap();
        record_receipt(&ctx, receipt_input()).unwrap();

        baseline.verify(&ctx, None).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 1);
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
