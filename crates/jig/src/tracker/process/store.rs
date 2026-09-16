use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::super::{InvalidWorkspaceReason, TrackerError, TrackerOperation};
use super::budget::OperationBudget;
use authority::{
    DatabaseIdentity, JsonlIdentity, LEGACY_LOCK_FILE, SNAPSHOT_DATABASE_FAMILY_SUFFIXES,
    SnapshotIdentity, as_jsonl_error, open_authority_file, validate_live_database_family,
};

mod authority;

const COPY_BUFFER_BYTES: usize = 256 * 1024;
const SNAPSHOT_ATTEMPTS: [Option<Duration>; 3] = [
    Some(Duration::from_millis(10)),
    Some(Duration::from_millis(25)),
    None,
];
pub(crate) const MAX_STORE_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;

#[cfg(test)]
type SnapshotHook = Box<dyn FnMut(&Path)>;

#[cfg(test)]
thread_local! {
    static TEST_SNAPSHOT_HOOK: std::cell::RefCell<Option<SnapshotHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestStoreSnapshotHook {
    previous: Option<SnapshotHook>,
}

#[cfg(test)]
impl TestStoreSnapshotHook {
    pub(crate) fn set(hook: impl FnMut(&Path) + 'static) -> Self {
        let previous = TEST_SNAPSHOT_HOOK.with(|value| value.replace(Some(Box::new(hook))));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestStoreSnapshotHook {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_SNAPSHOT_HOOK.with(|value| {
            value.replace(previous);
        });
    }
}

#[cfg(test)]
fn run_test_snapshot_hook(database_path: &Path) {
    TEST_SNAPSHOT_HOOK.with(|value| {
        if let Some(hook) = value.borrow_mut().as_mut() {
            hook(database_path);
        }
    });
}

#[cfg(not(test))]
fn run_test_snapshot_hook(_database_path: &Path) {}

struct SnapshotSizeBudget {
    remaining: u64,
}

impl SnapshotSizeBudget {
    const fn new() -> Self {
        Self {
            remaining: MAX_STORE_SNAPSHOT_BYTES,
        }
    }

    fn reserve(&mut self, bytes: u64) -> Result<(), TrackerError> {
        self.remaining =
            self.remaining
                .checked_sub(bytes)
                .ok_or(TrackerError::StoreSnapshotTooLarge {
                    limit_bytes: MAX_STORE_SNAPSHOT_BYTES,
                })?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct RetainedStore {
    database_path: PathBuf,
    database: File,
    database_identity: DatabaseIdentity,
    jsonl_path: PathBuf,
}

impl RetainedStore {
    pub(crate) fn capture(
        database_path: PathBuf,
        jsonl_path: PathBuf,
        budget: OperationBudget,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Self, TrackerError> {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        let database = open_authority_file(&database_path).map_err(|_| database_error())?;
        let database_identity = DatabaseIdentity::capture(&database, &database_path)?;
        match open_authority_file(&jsonl_path) {
            Ok(file) => {
                DatabaseIdentity::capture(&file, &jsonl_path).map_err(as_jsonl_error)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(jsonl_error()),
        }
        let store = Self {
            database_path,
            database,
            database_identity,
            jsonl_path,
        };
        store.verify_paths()?;
        Ok(store)
    }

    pub(crate) fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub(crate) fn jsonl_path(&self) -> &Path {
        &self.jsonl_path
    }

    pub(crate) fn verify_paths(&self) -> Result<(), TrackerError> {
        self.database_identity
            .verify(&self.database, &self.database_path)?;
        validate_live_database_family(&self.database_path)?;
        match open_authority_file(&self.jsonl_path) {
            Ok(file) => DatabaseIdentity::capture(&file, &self.jsonl_path)
                .map(|_| ())
                .map_err(as_jsonl_error),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(jsonl_error()),
        }
    }

    pub(super) fn prepare(
        &self,
        root: &Path,
        budget: OperationBudget,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<PreparedStore, TrackerError> {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        self.verify_paths()?;
        let database = self.database.try_clone().map_err(|_| database_error())?;
        self.database_identity
            .verify(&database, &self.database_path)?;

        let snapshot = if matches!(
            operation,
            TrackerOperation::ShowIssue
                | TrackerOperation::ListComments
                | TrackerOperation::SyncStatus
        ) {
            let mut snapshot = None;
            for retry_delay in SNAPSHOT_ATTEMPTS {
                budget
                    .checkpoint_before_spawn(operation, cancelled)
                    .map_err(snapshot_preparation_error)?;
                match self.prepare_snapshot_attempt(root, budget, operation, cancelled) {
                    Ok(prepared) => {
                        snapshot = Some(prepared);
                        break;
                    }
                    Err(TrackerError::StoreChangedDuringSnapshot) => {
                        let Some(retry_delay) = retry_delay else {
                            break;
                        };
                        wait_for_snapshot_retry(retry_delay, budget, operation, cancelled)?;
                    }
                    Err(error @ TrackerError::TimedOut { .. }) => {
                        return Err(snapshot_preparation_error(error));
                    }
                    Err(error) => return Err(error),
                }
            }
            Some(snapshot.ok_or(TrackerError::StoreChangedDuringSnapshot)?)
        } else {
            None
        };
        self.verify_paths()?;
        let (snapshot_directory, database_snapshot_path, jsonl_path) = snapshot.map_or(
            (None, None, None),
            |(directory, database_path, jsonl_path)| {
                (Some(directory), Some(database_path), jsonl_path)
            },
        );
        Ok(PreparedStore {
            authority_database: database,
            live_database_path: self.database_path.clone(),
            snapshot_directory,
            database_snapshot_path,
            jsonl_path,
        })
    }

    fn prepare_snapshot_attempt(
        &self,
        root: &Path,
        budget: OperationBudget,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(tempfile::TempDir, PathBuf, Option<PathBuf>), TrackerError> {
        let mut size_budget = SnapshotSizeBudget::new();
        let jsonl = if operation == TrackerOperation::SyncStatus {
            match open_authority_file(&self.jsonl_path) {
                Ok(file) => {
                    let identity = JsonlIdentity::capture(&file, &self.jsonl_path)?;
                    size_budget.reserve(identity.len)?;
                    Some((file, identity))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err(jsonl_error()),
            }
        } else {
            None
        };
        let directory = super::private_temp_directory(root, "jig-tracker-store-", operation)?;
        let database_path = copy_database_family_snapshot(
            &self.database_path,
            directory.path(),
            budget,
            operation,
            cancelled,
            &mut size_budget,
        )?;
        let jsonl_path = if operation == TrackerOperation::SyncStatus {
            let path = directory.path().join("issues.jsonl");
            if let Some((source, expected)) = &jsonl {
                copy_jsonl_snapshot(
                    source,
                    &self.jsonl_path,
                    expected,
                    &path,
                    budget,
                    operation,
                    cancelled,
                )?;
            } else {
                create_empty_jsonl_snapshot(&path, operation)?;
            }
            Some(path)
        } else {
            None
        };
        Ok((directory, database_path, jsonl_path))
    }
}

fn wait_for_snapshot_retry(
    delay: Duration,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), TrackerError> {
    let remaining = budget
        .remaining_before_spawn(operation, cancelled)
        .map_err(snapshot_preparation_error)?;
    std::thread::sleep(delay.min(remaining));
    budget
        .checkpoint_before_spawn(operation, cancelled)
        .map_err(snapshot_preparation_error)
}

fn snapshot_preparation_error(error: TrackerError) -> TrackerError {
    if matches!(error, TrackerError::TimedOut { .. }) {
        TrackerError::StoreSnapshotTimedOut
    } else {
        error
    }
}

pub(super) struct PreparedStore {
    #[allow(
        dead_code,
        reason = "pins the validated live database inode through spawn"
    )]
    authority_database: File,
    live_database_path: PathBuf,
    #[allow(dead_code, reason = "keeps the private store snapshot paths alive")]
    snapshot_directory: Option<tempfile::TempDir>,
    database_snapshot_path: Option<PathBuf>,
    jsonl_path: Option<PathBuf>,
}

impl PreparedStore {
    pub(super) fn verify_before_spawn(
        &self,
        operation: TrackerOperation,
    ) -> Result<(), TrackerError> {
        if operation.is_mutation() {
            DatabaseIdentity::capture(&self.authority_database, &self.live_database_path)?;
            validate_live_database_family(&self.live_database_path)?;
        }
        Ok(())
    }

    pub(super) fn configure(
        &self,
        command: &mut Command,
        operation: TrackerOperation,
    ) -> Result<(), TrackerError> {
        if let Some(database_path) = &self.database_snapshot_path {
            command.arg("--db").arg(database_path);
        } else {
            command.arg("--db").arg(&self.live_database_path);
        }
        if operation == TrackerOperation::SyncStatus
            && let Some(jsonl_path) = &self.jsonl_path
        {
            command.env("BEADS_JSONL", jsonl_path);
        }
        Ok(())
    }
}

fn copy_database_family_snapshot(
    database_path: &Path,
    destination_directory: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
    size_budget: &mut SnapshotSizeBudget,
) -> Result<PathBuf, TrackerError> {
    let mut source_paths = Vec::with_capacity(1 + SNAPSHOT_DATABASE_FAMILY_SUFFIXES.len());
    source_paths.push(database_path.to_path_buf());
    for suffix in SNAPSHOT_DATABASE_FAMILY_SUFFIXES {
        let mut path = database_path.as_os_str().to_os_string();
        path.push(suffix);
        source_paths.push(PathBuf::from(path));
    }

    let mut sources = Vec::with_capacity(source_paths.len());
    for (index, path) in source_paths.into_iter().enumerate() {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        match open_authority_file(&path) {
            Ok(file) => {
                let identity = SnapshotIdentity::capture(&file, &path)?;
                sources.push((path, Some((file, identity))));
            }
            Err(error) if index != 0 && error.kind() == std::io::ErrorKind::NotFound => {
                sources.push((path, None));
            }
            Err(_) => return Err(database_error()),
        }
    }

    let lock_path = database_path
        .parent()
        .ok_or_else(database_error)?
        .join(LEGACY_LOCK_FILE);
    budget.checkpoint_before_spawn(operation, cancelled)?;
    let lock_source = match open_authority_file(&lock_path) {
        Ok(file) => {
            let identity = SnapshotIdentity::capture(&file, &lock_path)?;
            Some((file, identity))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(database_error()),
    };

    for (_, source) in &sources {
        if let Some((_, identity)) = source {
            size_budget.reserve(identity.len)?;
        }
    }
    if let Some((_, identity)) = &lock_source {
        size_budget.reserve(identity.len)?;
    }

    let database_name = database_path.file_name().ok_or_else(database_error)?;
    let snapshot_database_path = destination_directory.join(database_name);
    for (index, (source_path, source)) in sources.iter().enumerate() {
        let Some((file, identity)) = source else {
            continue;
        };
        let destination = if index == 0 {
            snapshot_database_path.clone()
        } else {
            destination_directory.join(source_path.file_name().ok_or_else(database_error)?)
        };
        copy_snapshot_file(
            file,
            source_path,
            identity,
            &destination,
            budget,
            operation,
            cancelled,
        )?;
    }
    if let Some((file, identity)) = &lock_source {
        copy_snapshot_file(
            file,
            &lock_path,
            identity,
            &destination_directory.join(LEGACY_LOCK_FILE),
            budget,
            operation,
            cancelled,
        )?;
    }
    run_test_snapshot_hook(database_path);
    for (path, source) in &sources {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        match source {
            Some((file, identity)) => identity.verify(file, path)?,
            None => match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(TrackerError::StoreChangedDuringSnapshot),
                Err(_) => return Err(database_error()),
            },
        }
    }
    budget.checkpoint_before_spawn(operation, cancelled)?;
    match &lock_source {
        Some((file, identity)) => identity.verify(file, &lock_path)?,
        None => match fs::symlink_metadata(&lock_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err(TrackerError::StoreChangedDuringSnapshot),
            Err(_) => return Err(database_error()),
        },
    }
    Ok(snapshot_database_path)
}

fn copy_snapshot_file(
    source: &File,
    source_path: &Path,
    expected: &SnapshotIdentity,
    destination_path: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), TrackerError> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    expected.verify(source, source_path)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut destination = options
        .open(destination_path)
        .map_err(|_| prestart_failure(operation))?;
    let copied = copy_file_bytes(
        source,
        &mut destination,
        expected.len,
        budget,
        operation,
        cancelled,
    )?;
    if let Some(modified) = expected.modified {
        destination
            .set_times(fs::FileTimes::new().set_modified(modified))
            .map_err(|_| prestart_failure(operation))?;
    }
    expected.verify(source, source_path)?;
    if copied != expected.len {
        return Err(TrackerError::StoreChangedDuringSnapshot);
    }
    Ok(())
}

fn copy_file_bytes(
    source: &File,
    destination: &mut File,
    expected_len: u64,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<u64, TrackerError> {
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut offset = 0_u64;
    loop {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        let read = read_file_at(source, &mut buffer, offset).map_err(|_| database_error())?;
        if read == 0 {
            break;
        }
        if read as u64 > expected_len.saturating_sub(offset) {
            return Err(TrackerError::StoreChangedDuringSnapshot);
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|_| prestart_failure(operation))?;
        offset = offset.saturating_add(read as u64);
    }
    Ok(offset)
}

fn copy_jsonl_snapshot(
    source: &File,
    source_path: &Path,
    expected: &JsonlIdentity,
    destination_path: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), TrackerError> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    expected.verify(source, source_path)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut destination = options
        .open(destination_path)
        .map_err(|_| prestart_failure(operation))?;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut offset = 0_u64;
    loop {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        let read = read_file_at(source, &mut buffer, offset).map_err(|_| jsonl_error())?;
        if read == 0 {
            break;
        }
        if read as u64 > expected.len.saturating_sub(offset) {
            return Err(TrackerError::StoreChangedDuringSnapshot);
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|_| prestart_failure(operation))?;
        offset = offset.saturating_add(read as u64);
    }
    if let Some(modified) = expected.modified {
        destination
            .set_times(fs::FileTimes::new().set_modified(modified))
            .map_err(|_| prestart_failure(operation))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        destination
            .set_permissions(fs::Permissions::from_mode(0o400))
            .map_err(|_| prestart_failure(operation))?;
    }
    expected.verify(source, source_path)?;
    if offset != expected.len {
        return Err(TrackerError::StoreChangedDuringSnapshot);
    }
    budget.checkpoint_before_spawn(operation, cancelled)
}

fn create_empty_jsonl_snapshot(
    destination_path: &Path,
    operation: TrackerOperation,
) -> Result<(), TrackerError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o400);
    }
    options
        .open(destination_path)
        .map(|_| ())
        .map_err(|_| prestart_failure(operation))
}

#[cfg(unix)]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(buffer, offset)
}

#[cfg(not(unix))]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(offset))?;
    file.read(buffer)
}

const fn database_error() -> TrackerError {
    TrackerError::InvalidWorkspace {
        reason: InvalidWorkspaceReason::Database,
    }
}

const fn jsonl_error() -> TrackerError {
    TrackerError::InvalidWorkspace {
        reason: InvalidWorkspaceReason::JsonlExport,
    }
}

const fn prestart_failure(operation: TrackerOperation) -> TrackerError {
    TrackerError::ProcessFailure {
        operation,
        exit_code: None,
    }
}
