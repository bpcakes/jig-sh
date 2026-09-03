use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use fs4::fs_std::FileExt;
use serde::Serialize;

use crate::bootstrap::path::repository_file_identity;
use crate::context::RepoContext;

use super::super::records::ReceiptRecord;

const RECEIPT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const RECEIPT_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) struct ReceiptJournalWriter<'a> {
    state_dir: &'a Dir,
    journal: &'a File,
}

#[derive(Debug)]
struct ReceiptAppendMayHaveLanded {
    source: io::Error,
}

impl std::fmt::Display for ReceiptAppendMayHaveLanded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Receipt journal append may have landed before publication failed: {}",
            self.source
        )
    }
}

impl std::error::Error for ReceiptAppendMayHaveLanded {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub(crate) fn receipt_append_may_have_landed(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<ReceiptAppendMayHaveLanded>().is_some())
}

#[cfg(test)]
pub(crate) fn receipt_append_may_have_landed_for_test() -> anyhow::Error {
    anyhow::Error::new(ReceiptAppendMayHaveLanded {
        source: io::Error::other("injected post-write failure"),
    })
}

pub(crate) fn with_receipt_journal_writer<T>(
    ctx: &RepoContext,
    operation: impl FnOnce(&ReceiptJournalWriter<'_>) -> Result<T>,
) -> Result<T> {
    with_receipt_journal_writer_until(
        ctx,
        Instant::now() + RECEIPT_LOCK_TIMEOUT,
        &|| false,
        operation,
    )
}

pub(crate) fn with_receipt_journal_writer_until<T>(
    ctx: &RepoContext,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    operation: impl FnOnce(&ReceiptJournalWriter<'_>) -> Result<T>,
) -> Result<T> {
    with_receipt_journal_writer_after_open_until(ctx, deadline, cancelled, |_| Ok(()), operation)
}

fn with_receipt_journal_writer_after_open_until<T>(
    ctx: &RepoContext,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    mut after_legacy_open: impl FnMut(&File) -> Result<()>,
    operation: impl FnOnce(&ReceiptJournalWriter<'_>) -> Result<T>,
) -> Result<T> {
    let directories = ReceiptJournalDirectories::open(ctx.root())?;
    loop {
        // Create the legacy-locked inode before taking either lock. Older runtimes lock the
        // journal itself, so skipping this lock on the first append would leave no common
        // serialization point during the cache-lock cutover.
        let legacy_lock = open_regular_file(
            &directories.state,
            "receipts.jsonl",
            true,
            true,
            "receipt journal",
        )?;
        after_legacy_open(&legacy_lock)?;
        lock_exclusive_until(&legacy_lock, "legacy receipt journal", deadline, cancelled)?;
        let lock = open_regular_file(
            &directories.locks,
            "receipts.jsonl.lock",
            true,
            true,
            "receipt journal lock",
        )?;
        lock_exclusive_until(&lock, "receipt journal", deadline, cancelled)?;
        if !journal_file_is_current(&directories.state, &legacy_lock)? {
            drop(lock);
            drop(legacy_lock);
            if cancelled() {
                bail!("Execution was cancelled while retrying receipt journal lock identity");
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "Timed out retrying receipt journal lock identity before its operation deadline"
                );
            }
            thread::sleep(RECEIPT_LOCK_POLL_INTERVAL.min(remaining));
            continue;
        }

        let writer = ReceiptJournalWriter {
            state_dir: &directories.state,
            journal: &legacy_lock,
        };
        // The file handles are the lock guards. Dropping them releases advisory locks on every
        // return path without letting fallible post-operation cleanup reclassify a durable append.
        return operation(&writer);
    }
}

fn journal_file_is_current(state_dir: &Dir, opened: &File) -> Result<bool> {
    let current = open_regular_file(state_dir, "receipts.jsonl", false, false, "receipt journal")?;
    Ok(repository_file_identity(opened)? == repository_file_identity(&current)?)
}

fn lock_exclusive_until(
    file: &File,
    description: &str,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    loop {
        if cancelled() {
            bail!("Execution was cancelled while waiting for {description} lock");
        }
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to lock {description}"));
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("Timed out waiting for {description} lock before its operation deadline");
        }
        thread::sleep(RECEIPT_LOCK_POLL_INTERVAL.min(remaining));
    }
}

impl ReceiptJournalWriter<'_> {
    pub(crate) fn append<T: Serialize>(&self, value: &T) -> Result<()> {
        let mut record = serde_json::to_vec(value)?;
        record.push(b'\n');
        let mut journal = self.journal;
        journal
            .write_all(&record)
            .map_err(|source| anyhow::Error::new(ReceiptAppendMayHaveLanded { source }))?;
        journal
            .sync_data()
            .map_err(|source| anyhow::Error::new(ReceiptAppendMayHaveLanded { source }))?;
        Ok(())
    }

    pub(crate) fn inspect<T>(
        &self,
        operation: impl FnOnce(&File) -> Result<T>,
    ) -> Result<Option<T>> {
        let Some(journal) =
            open_optional_regular_file(self.state_dir, "receipts.jsonl", false, "receipt journal")?
        else {
            return Ok(None);
        };
        if repository_file_identity(self.journal)? != repository_file_identity(&journal)? {
            bail!("Receipt journal changed while its writer locks were held");
        }
        operation(&journal).map(Some)
    }
}

struct ReceiptJournalDirectories {
    state: Dir,
    locks: Dir,
}

impl ReceiptJournalDirectories {
    fn open(root: &Path) -> Result<Self> {
        let repository = Dir::open_ambient_dir(root, ambient_authority())
            .with_context(|| format!("Failed to open repository root {}", root.display()))?;
        let agent = open_or_create_directory(&repository, ".agent", "agent state root")?;
        let state = open_or_create_directory(&agent, "state", "state directory")?;
        let cache = open_or_create_directory(&agent, ".cache", "state cache directory")?;
        let locks = open_or_create_directory(&cache, "state-locks", "state lock directory")?;
        Ok(Self { state, locks })
    }
}

fn open_or_create_directory(parent: &Dir, name: &str, description: &str) -> Result<Dir> {
    match parent.open_dir_nofollow(name) {
        Ok(directory) => Ok(directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("Failed to create {description}"));
                }
            }
            parent
                .open_dir_nofollow(name)
                .with_context(|| format!("Failed to open {description} without following links"))
        }
        Err(error) => Err(error)
            .with_context(|| format!("Failed to open {description} without following links")),
    }
}

fn open_optional_regular_file(
    directory: &Dir,
    name: &str,
    writable: bool,
    description: &str,
) -> Result<Option<File>> {
    match open_regular_file(directory, name, writable, false, description) {
        Ok(file) => Ok(Some(file)),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn open_regular_file(
    directory: &Dir,
    name: &str,
    writable: bool,
    create: bool,
    description: &str,
) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(writable)
        .append(writable)
        .create(create)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = directory
        .open_with(name, &options)
        .map(cap_std::fs::File::into_std)
        .with_context(|| format!("Failed to open {description} without following links"))?;
    if !file.metadata()?.is_file() {
        bail!("{description} is not a regular file");
    }
    Ok(file)
}

pub(crate) fn receipt_record_id(record: &[u8]) -> Result<String> {
    let receipt = serde_json::from_slice::<ReceiptRecord>(record)
        .context("Appended receipt record does not match the durable receipt schema")?;
    Ok(receipt.id)
}

#[cfg(all(test, unix))]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::fs::OpenOptions as StdOpenOptions;
    use std::io::Write as _;
    use std::os::unix::fs::symlink;
    use std::sync::mpsc;

    use serde_json::json;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn receipt_lock_wait_honors_its_operation_deadline() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let lock_path = temp
            .path()
            .join(".agent/.cache/state-locks/receipts.jsonl.lock");
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let owner = StdOpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(owner.try_lock_exclusive().unwrap());
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let error = with_receipt_journal_writer_until(&ctx, Instant::now(), &|| false, |_| Ok(()))
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("Timed out waiting for receipt journal lock"),
            "{error}"
        );
    }

    #[test]
    fn receipt_lock_wait_honors_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let lock_path = temp
            .path()
            .join(".agent/.cache/state-locks/receipts.jsonl.lock");
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let owner = StdOpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(owner.try_lock_exclusive().unwrap());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let checks = Cell::new(0);

        let error = with_receipt_journal_writer_until(
            &ctx,
            Instant::now() + Duration::from_secs(1),
            &|| {
                let observed = checks.get();
                checks.set(observed + 1);
                observed > 0
            },
            |_| Ok(()),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("cancelled while waiting"), "{error}");
    }

    #[test]
    fn first_receipt_append_holds_and_releases_both_cutover_locks() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let journal_path = temp.path().join(".agent/state/receipts.jsonl");
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        with_receipt_journal_writer(&ctx, |writer| {
            let competing_legacy = StdOpenOptions::new()
                .read(true)
                .write(true)
                .open(&journal_path)
                .unwrap();
            assert!(!competing_legacy.try_lock_exclusive().unwrap());
            writer.append(&json!({"id": 1}))
        })
        .unwrap();

        let journal = StdOpenOptions::new()
            .read(true)
            .write(true)
            .open(&journal_path)
            .unwrap();
        let lock = StdOpenOptions::new()
            .read(true)
            .write(true)
            .open(
                temp.path()
                    .join(".agent/.cache/state-locks/receipts.jsonl.lock"),
            )
            .unwrap();
        assert!(journal.try_lock_exclusive().unwrap());
        assert!(lock.try_lock_exclusive().unwrap());
        assert_eq!(fs::read(&journal_path).unwrap(), b"{\"id\":1}\n");
    }

    #[test]
    fn receipt_writer_reopens_a_journal_replaced_before_lock_acquisition() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let journal_path = temp.path().join(".agent/state/receipts.jsonl");
        fs::create_dir_all(journal_path.parent().unwrap()).unwrap();
        fs::write(&journal_path, b"{\"id\":\"old\"}\n").unwrap();
        let legacy_owner = StdOpenOptions::new()
            .read(true)
            .write(true)
            .open(&journal_path)
            .unwrap();
        assert!(legacy_owner.try_lock_exclusive().unwrap());
        let lock_path = temp
            .path()
            .join(".agent/.cache/state-locks/receipts.jsonl.lock");
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let cutover_owner = StdOpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(cutover_owner.try_lock_exclusive().unwrap());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let (opened_tx, opened_rx) = mpsc::channel();
        let (replaced_tx, replaced_rx) = mpsc::channel();
        let writer_journal_path = journal_path.clone();

        std::thread::scope(|scope| {
            let writer = scope.spawn(move || {
                let mut open_count = 0;
                with_receipt_journal_writer_after_open_until(
                    &ctx,
                    Instant::now() + Duration::from_secs(2),
                    &|| false,
                    |_| {
                        open_count += 1;
                        if open_count == 1 {
                            opened_tx.send(()).unwrap();
                            replaced_rx.recv().unwrap();
                        }
                        Ok(())
                    },
                    |writer| {
                        let competing_legacy = StdOpenOptions::new()
                            .read(true)
                            .write(true)
                            .open(&writer_journal_path)
                            .unwrap();
                        assert!(
                            !competing_legacy.try_lock_exclusive().unwrap(),
                            "the replacement inode must hold the legacy lock"
                        );
                        writer.append(&json!({"id": "current"}))
                    },
                )
                .unwrap();
                assert!(open_count >= 2, "the detached inode must be reopened");
            });

            opened_rx.recv().unwrap();
            let mut replacement =
                tempfile::NamedTempFile::new_in(journal_path.parent().unwrap()).unwrap();
            replacement
                .write_all(b"{\"id\":\"replacement\"}\n")
                .unwrap();
            replacement.persist(&journal_path).unwrap();
            FileExt::unlock(&cutover_owner).unwrap();
            FileExt::unlock(&legacy_owner).unwrap();
            replaced_tx.send(()).unwrap();
            writer.join().unwrap();
        });

        assert_eq!(
            fs::read(&journal_path).unwrap(),
            b"{\"id\":\"replacement\"}\n{\"id\":\"current\"}\n"
        );
    }

    #[test]
    fn receipt_writer_backs_off_across_repeated_inode_replacements() {
        let temp = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let journal_path = temp.path().join(".agent/state/receipts.jsonl");
        fs::create_dir_all(journal_path.parent().unwrap()).unwrap();
        fs::write(&journal_path, b"{\"id\":\"initial\"}\n").unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut open_count = 0_u32;
        let replacement_count = 3_u32;
        let started = Instant::now();

        with_receipt_journal_writer_after_open_until(
            &ctx,
            Instant::now() + Duration::from_secs(2),
            &|| false,
            |_| {
                open_count += 1;
                if open_count <= replacement_count {
                    let mut replacement =
                        tempfile::NamedTempFile::new_in(journal_path.parent().unwrap()).unwrap();
                    writeln!(replacement, "{{\"id\":\"replacement-{open_count}\"}}").unwrap();
                    replacement.persist(&journal_path).unwrap();
                }
                Ok(())
            },
            |writer| writer.append(&json!({"id": "current"})),
        )
        .unwrap();

        assert_eq!(open_count, replacement_count + 1);
        assert!(
            started.elapsed() >= RECEIPT_LOCK_POLL_INTERVAL * replacement_count,
            "identity retries must use the bounded poll interval"
        );
        assert_eq!(
            fs::read(&journal_path).unwrap(),
            b"{\"id\":\"replacement-3\"}\n{\"id\":\"current\"}\n"
        );
    }

    #[test]
    fn receipt_append_rejects_a_symlinked_journal() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let journal = temp.path().join(".agent/state/receipts.jsonl");
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        symlink(outside.path(), &journal).unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let error = with_receipt_journal_writer(&ctx, |writer| writer.append(&json!({"id": 1})))
            .unwrap_err();

        assert!(
            error.to_string().contains("without following links"),
            "{error:#}"
        );
        assert_eq!(fs::read(outside.path()).unwrap(), b"");
    }

    #[test]
    fn receipt_append_rejects_a_symlinked_state_directory() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
        fs::remove_dir(temp.path().join(".agent/state")).unwrap();
        symlink(outside.path(), temp.path().join(".agent/state")).unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let error = with_receipt_journal_writer(&ctx, |writer| writer.append(&json!({"id": 1})))
            .unwrap_err();

        assert!(
            error.to_string().contains("without following links"),
            "{error:#}"
        );
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn receipt_append_rejects_a_symlinked_lock_file() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let lock = temp
            .path()
            .join(".agent/.cache/state-locks/receipts.jsonl.lock");
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        symlink(outside.path(), &lock).unwrap();
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        let error = with_receipt_journal_writer(&ctx, |writer| writer.append(&json!({"id": 1})))
            .unwrap_err();

        assert!(
            error.to_string().contains("without following links"),
            "{error:#}"
        );
        assert_eq!(fs::read(outside.path()).unwrap(), b"");
    }
}
