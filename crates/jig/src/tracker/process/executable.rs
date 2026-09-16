use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::{TrackerError, TrackerOperation};
use super::budget::OperationBudget;

pub(crate) const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static TEST_BR_OVERRIDE: std::cell::RefCell<Option<Option<PathBuf>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestBrOverride {
    previous: Option<Option<PathBuf>>,
}

#[cfg(test)]
impl TestBrOverride {
    pub(crate) fn set(path: Option<&Path>) -> Self {
        let replacement = Some(path.map(Path::to_path_buf));
        let previous = TEST_BR_OVERRIDE.with(|value| value.replace(replacement));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestBrOverride {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_BR_OVERRIDE.with(|value| {
            value.replace(previous);
        });
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedExecutable {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    path: PathBuf,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    identity: ExecutableIdentity,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    source: File,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    snapshot: ImmutableExecutableSnapshot,
}

impl ResolvedExecutable {
    fn capture(
        root: &Path,
        path: PathBuf,
        budget: OperationBudget,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Self, TrackerError> {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let source =
                open_executable(&path).map_err(|_| TrackerError::ExecutableCandidateInvalid)?;
            let (snapshot, identity) =
                immutable_snapshot(&source, &path, root, budget, operation, cancelled)?;
            Ok(Self {
                path,
                identity,
                source,
                snapshot,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (root, path, budget, operation, cancelled);
            Err(TrackerError::UnsupportedPlatform)
        }
    }

    pub(super) fn prepare(
        &self,
        root: &Path,
        budget: OperationBudget,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<PreparedExecutable, TrackerError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let _ = (root, budget, operation, cancelled);
            self.identity.verify_metadata(&self.source, &self.path)?;
            let file = self
                .snapshot
                .file
                .try_clone()
                .map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
            Ok(PreparedExecutable {
                file,
                #[cfg(target_os = "macos")]
                path: self.snapshot.path.clone(),
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (root, budget, operation, cancelled);
            Err(TrackerError::UnsupportedPlatform)
        }
    }
}

fn open_executable(path: &Path) -> std::io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path)
}

pub(super) struct PreparedExecutable {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[allow(
        dead_code,
        reason = "pins the prepared executable snapshot through spawn"
    )]
    file: File,
    #[cfg(target_os = "macos")]
    path: PathBuf,
}

impl PreparedExecutable {
    pub(super) fn command(&self) -> Command {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::process::CommandExt;

            let descriptor = self.file.as_raw_fd();
            let mut command = Command::new(super::descriptor_path(descriptor));
            // SAFETY: this closure runs after fork and calls only an
            // async-signal-safe `fcntl` operation on the live snapshot.
            unsafe {
                command.pre_exec(move || super::make_descriptor_inheritable(descriptor));
            }
            command
        }
        #[cfg(target_os = "macos")]
        {
            Command::new(&self.path)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        unreachable!("unsupported tracker platforms fail before command construction")
    }
}

#[derive(Debug)]
struct ImmutableExecutableSnapshot {
    file: File,
    #[cfg(target_os = "macos")]
    path: PathBuf,
    #[cfg(target_os = "macos")]
    #[allow(dead_code, reason = "keeps the private executable pathname alive")]
    directory: tempfile::TempDir,
}

#[derive(Debug, Eq, PartialEq)]
struct ExecutableIdentity {
    metadata: ExecutableMetadata,
}

impl ExecutableIdentity {
    #[allow(clippy::too_many_arguments)]
    fn capture(
        file: &File,
        path: &Path,
        budget: OperationBudget,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
        mut destination: Option<&mut File>,
    ) -> Result<Self, TrackerError> {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        if path.canonicalize().ok().as_deref() != Some(path) {
            return Err(TrackerError::ExecutableCandidateInvalid);
        }
        let before = executable_metadata_identity(
            &file
                .metadata()
                .map_err(|_| TrackerError::ExecutableCandidateInvalid)?,
        );
        if !before.is_file || !before.is_executable || before.len > MAX_EXECUTABLE_BYTES {
            return Err(TrackerError::ExecutableCandidateInvalid);
        }

        let mut buffer = [0_u8; 16 * 1024];
        let mut total = 0_u64;
        loop {
            budget.checkpoint_before_spawn(operation, cancelled)?;
            let read = read_executable_at(file, &mut buffer, total)
                .map_err(|_| TrackerError::ExecutableCandidateInvalid)?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            if total > MAX_EXECUTABLE_BYTES {
                return Err(TrackerError::ExecutableCandidateInvalid);
            }
            if let Some(destination) = destination.as_deref_mut() {
                destination
                    .write_all(&buffer[..read])
                    .map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
            }
        }

        budget.checkpoint_before_spawn(operation, cancelled)?;
        let after = executable_metadata_identity(
            &file
                .metadata()
                .map_err(|_| TrackerError::ExecutableCandidateInvalid)?,
        );
        let visible = executable_metadata_identity(
            &fs::symlink_metadata(path).map_err(|_| TrackerError::ExecutableCandidateInvalid)?,
        );
        if before != after || before != visible || total != before.len {
            return Err(TrackerError::ExecutableCandidateInvalid);
        }
        Ok(Self { metadata: before })
    }

    fn verify_metadata(&self, file: &File, path: &Path) -> Result<(), TrackerError> {
        let opened = executable_metadata_identity(
            &file
                .metadata()
                .map_err(|_| TrackerError::ExecutableChanged)?,
        );
        let visible = executable_metadata_identity(
            &fs::symlink_metadata(path).map_err(|_| TrackerError::ExecutableChanged)?,
        );
        if opened == self.metadata && visible == self.metadata {
            Ok(())
        } else {
            Err(TrackerError::ExecutableChanged)
        }
    }
}

#[cfg(target_os = "linux")]
fn immutable_snapshot(
    source: &File,
    source_path: &Path,
    _root: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(ImmutableExecutableSnapshot, ExecutableIdentity), TrackerError> {
    let mut snapshot =
        create_snapshot_file().map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
    let identity = ExecutableIdentity::capture(
        source,
        source_path,
        budget,
        operation,
        cancelled,
        Some(&mut snapshot),
    )?;
    budget.checkpoint_before_spawn(operation, cancelled)?;
    finalize_snapshot(snapshot)
        .map(|file| (ImmutableExecutableSnapshot { file }, identity))
        .map_err(|_| TrackerError::ExecutableSnapshotUnavailable)
}

#[cfg(target_os = "macos")]
fn immutable_snapshot(
    source: &File,
    source_path: &Path,
    root: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(ImmutableExecutableSnapshot, ExecutableIdentity), TrackerError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let directory = super::private_temp_directory(root, "jig-br-profile-", operation)?;
    let snapshot_path = directory.path().join("br");
    let mut snapshot = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&snapshot_path)
        .map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
    let identity = ExecutableIdentity::capture(
        source,
        source_path,
        budget,
        operation,
        cancelled,
        Some(&mut snapshot),
    )?;
    budget.checkpoint_before_spawn(operation, cancelled)?;
    snapshot
        .sync_all()
        .map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
    snapshot
        .set_permissions(fs::Permissions::from_mode(0o500))
        .map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
    drop(snapshot);

    let read_only =
        File::open(&snapshot_path).map_err(|_| TrackerError::ExecutableSnapshotUnavailable)?;
    let flags = unsafe { libc::fcntl(read_only.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 || flags & libc::O_ACCMODE != libc::O_RDONLY {
        return Err(TrackerError::ExecutableSnapshotUnavailable);
    }
    Ok((
        ImmutableExecutableSnapshot {
            file: read_only,
            path: snapshot_path,
            directory,
        },
        identity,
    ))
}

#[cfg(target_os = "linux")]
fn create_snapshot_file() -> std::io::Result<File> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;

    let name = CString::new("jig-br-profile").expect("static memfd name is valid");
    let preferred = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | libc::MFD_EXEC;
    // SAFETY: `name` is NUL-terminated and the flags are valid memfd flags.
    let mut descriptor = unsafe { libc::memfd_create(name.as_ptr(), preferred) };
    if descriptor == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
        // Older kernels predate `MFD_EXEC` and create executable memfds by
        // default. Retry only the documented unsupported-flag failure.
        // SAFETY: the same valid name is reused with the legacy flag set.
        descriptor = unsafe {
            libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
        };
    }
    if descriptor == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: ownership of the newly created descriptor transfers to `File`.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(target_os = "linux")]
fn finalize_snapshot(snapshot: File) -> std::io::Result<File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;

    snapshot.sync_all()?;
    snapshot.set_permissions(fs::Permissions::from_mode(0o500))?;
    let seals = libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_WRITE | libc::F_SEAL_SEAL;
    // SAFETY: the snapshot owns this live memfd and has no writable mappings.
    if unsafe { libc::fcntl(snapshot.as_raw_fd(), libc::F_ADD_SEALS, seals) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    let read_only = File::open(super::descriptor_path(snapshot.as_raw_fd()))?;
    drop(snapshot);
    Ok(read_only)
}

#[cfg(unix)]
fn read_executable_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(buffer, offset)
}

#[cfg(not(unix))]
fn read_executable_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(offset))?;
    file.read(buffer)
}

#[derive(Debug, Eq, PartialEq)]
struct ExecutableMetadata {
    is_file: bool,
    is_executable: bool,
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

fn executable_metadata_identity(metadata: &fs::Metadata) -> ExecutableMetadata {
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    ExecutableMetadata {
        is_file: metadata.is_file(),
        #[cfg(unix)]
        is_executable: metadata.permissions().mode() & 0o111 != 0,
        #[cfg(not(unix))]
        is_executable: true,
        len: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(unix)]
        changed_seconds: metadata.ctime(),
        #[cfg(unix)]
        changed_nanoseconds: metadata.ctime_nsec(),
    }
}

pub(crate) fn resolve_br(
    root: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<ResolvedExecutable, TrackerError> {
    resolve_br_on_platform(
        root,
        budget,
        operation,
        cancelled,
        cfg!(any(target_os = "linux", target_os = "macos")),
    )
}

fn resolve_br_on_platform(
    root: &Path,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
    platform_supported: bool,
) -> Result<ResolvedExecutable, TrackerError> {
    budget.checkpoint_before_spawn(operation, cancelled)?;
    if !platform_supported {
        return Err(TrackerError::UnsupportedPlatform);
    }
    #[cfg(test)]
    if let Some(path) = TEST_BR_OVERRIDE.with(|value| value.borrow().clone()) {
        return path.map_or(Err(TrackerError::BinaryMissing), |path| {
            resolve_candidate(root, path, budget, operation, cancelled)
        });
    }

    let search_path = env::var_os("PATH").ok_or(TrackerError::BinaryMissing)?;
    let mut invalid_candidate = None;
    for entry in env::split_paths(&search_path) {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        // The tracker provider is an external executable authority. Empty and
        // relative PATH entries inherit the caller's current directory, and an
        // absolute entry can still point back into the repository. Neither may
        // promote checkout-controlled bytes into the supported provider profile.
        if entry.as_os_str().is_empty() || !entry.is_absolute() {
            continue;
        }
        match resolve_candidate(root, entry.join("br"), budget, operation, cancelled) {
            Ok(executable) => return Ok(executable),
            Err(TrackerError::BinaryMissing) => continue,
            Err(error @ TrackerError::ExecutableCandidateInvalid) => {
                invalid_candidate = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(invalid_candidate.unwrap_or(TrackerError::BinaryMissing))
}

fn resolve_candidate(
    root: &Path,
    candidate: PathBuf,
    budget: OperationBudget,
    operation: TrackerOperation,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<ResolvedExecutable, TrackerError> {
    budget.checkpoint_before_spawn(operation, cancelled)?;
    if !executable_file(&candidate) {
        return Err(TrackerError::BinaryMissing);
    }
    let path = candidate
        .canonicalize()
        .map_err(|_| TrackerError::BinaryMissing)?;
    if path.starts_with(root) {
        return Err(TrackerError::BinaryMissing);
    }
    ResolvedExecutable::capture(root, path, budget, operation, cancelled)
}

fn executable_file(path: &Path) -> bool {
    executable_metadata(path).is_some_and(|metadata| metadata.is_file())
}

#[cfg(unix)]
fn executable_metadata(path: &Path) -> Option<fs::Metadata> {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_metadata(path: &Path) -> Option<fs::Metadata> {
    fs::metadata(path).ok()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn unsupported_platform_fails_before_provider_lookup() {
        let directory = tempfile::tempdir().unwrap();
        let budget = OperationBudget::new(Duration::from_secs(5));
        let mut never_cancelled = || false;

        assert_eq!(
            resolve_br_on_platform(
                directory.path(),
                budget,
                TrackerOperation::Version,
                &mut never_cancelled,
                false,
            )
            .unwrap_err(),
            TrackerError::UnsupportedPlatform
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::*;

    #[test]
    fn prepared_snapshot_is_read_only_and_executable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("br");
        fs::write(&path, "#!/bin/sh\nprintf 'snapshot-ok\\n'\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let path = path.canonicalize().unwrap();
        let budget = OperationBudget::new(Duration::from_secs(5));
        let mut never_cancelled = || false;
        let executable = ResolvedExecutable::capture(
            directory.path(),
            path,
            budget,
            TrackerOperation::Version,
            &mut never_cancelled,
        )
        .unwrap();
        let prepared = executable
            .prepare(
                directory.path(),
                budget,
                TrackerOperation::Version,
                &mut never_cancelled,
            )
            .unwrap();

        let flags = unsafe { libc::fcntl(prepared.file.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags, -1);
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
        let output = prepared.command().output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"snapshot-ok\n");
    }
}
