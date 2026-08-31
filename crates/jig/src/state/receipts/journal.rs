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

use crate::context::RepoContext;

use super::super::records::ReceiptRecord;

const RECEIPT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const RECEIPT_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) struct ReceiptJournalWriter<'a> {
    state_dir: &'a Dir,
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
    let directories = ReceiptJournalDirectories::open(ctx.root())?;
    let legacy_lock = open_optional_regular_file(
        &directories.state,
        "receipts.jsonl",
        true,
        "receipt journal",
    )?;
    if let Some(file) = &legacy_lock {
        lock_exclusive_until(file, "legacy receipt journal", deadline, cancelled)?;
    }
    let lock = open_regular_file(
        &directories.locks,
        "receipts.jsonl.lock",
        true,
        true,
        "receipt journal lock",
    )?;
    if let Err(error) = lock_exclusive_until(&lock, "receipt journal", deadline, cancelled) {
        if let Some(file) = &legacy_lock {
            let _ = FileExt::unlock(file);
        }
        return Err(error);
    }

    let writer = ReceiptJournalWriter {
        state_dir: &directories.state,
    };
    let result = operation(&writer);
    let legacy_unlock = legacy_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
    let unlock = FileExt::unlock(&lock);
    match (result, legacy_unlock, unlock) {
        (Ok(value), Ok(()), Ok(())) => Ok(value),
        (Err(error), _, _) => Err(error),
        (Ok(_), Err(error), _) => Err(error).context("Failed to unlock legacy receipt journal"),
        (Ok(_), Ok(()), Err(error)) => Err(error).context("Failed to unlock receipt journal"),
    }
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
        let mut file = open_regular_file(
            self.state_dir,
            "receipts.jsonl",
            true,
            true,
            "receipt journal",
        )?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        Ok(())
    }

    pub(crate) fn inspect<T>(
        &self,
        operation: impl FnOnce(&File) -> Result<T>,
    ) -> Result<Option<T>> {
        match open_optional_regular_file(
            self.state_dir,
            "receipts.jsonl",
            false,
            "receipt journal",
        )? {
            Some(file) => operation(&file).map(Some),
            None => Ok(None),
        }
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
    use std::os::unix::fs::symlink;

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
