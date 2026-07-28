use std::env;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tempfile::Builder as TempFileBuilder;

pub(super) const INVOCATION_CWD_ENV: &str = "JIG_INVOKE_CWD";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepositoryFileLeaf {
    Missing,
    RegularFile,
    Symlink,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepositoryEntryIdentity {
    platform: RepositoryEntryPlatformIdentity,
}

#[derive(Clone, Debug)]
pub(crate) struct RepositoryDirectoryCommit {
    pub(crate) identity: RepositoryEntryIdentity,
    // Retaining the directory handle prevents reuse of its device/inode or
    // volume/file-index identity for the lifetime of the transaction.
    pub(crate) handle: Arc<File>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RepositoryEntryPlatformIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial: u32,
        file_index_high: u32,
        file_index_low: u32,
    },
    #[cfg(not(any(unix, windows)))]
    Unsupported,
}

#[derive(Clone, Debug)]
pub(crate) struct RepositoryFileCommit {
    pub(crate) identity: RepositoryEntryIdentity,
    pub(crate) content_length: u64,
    pub(crate) content_sha256: [u8; 32],
    pub(crate) permission_identity: u32,
    // Keeping the published inode open prevents identity reuse during the
    // transaction. Ownership still compares the complete fingerprint after
    // quarantine, so same-inode in-place writes are never accepted.
    pub(crate) handle: Arc<File>,
}

#[derive(Clone, Debug)]
pub(crate) struct RepositorySymlinkCommit {
    pub(crate) identity: RepositoryEntryIdentity,
    pub(crate) target: PathBuf,
    pub(crate) target_is_directory: bool,
    pub(crate) handle: Arc<File>,
}

pub(crate) fn validate_portable_planned_file_collisions<I, P>(paths: I) -> Result<()>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut planned = paths
        .into_iter()
        .map(|path| {
            let original = path.as_ref().to_path_buf();
            let components = portable_planned_file_components(&original)?;
            Ok((original, components))
        })
        .collect::<Result<Vec<_>>>()?;

    planned.sort_unstable_by(|left, right| left.1.cmp(&right.1));
    for adjacent in planned.windows(2) {
        let (left_path, left_components) = &adjacent[0];
        let (right_path, right_components) = &adjacent[1];
        if component_prefix(left_components, right_components) {
            bail!(
                "Portable planned repository file collision between {} and {}: generated file paths must not be equal ignoring ASCII case, and neither file may be an ancestor of another",
                left_path.display(),
                right_path.display()
            );
        }
    }

    Ok(())
}

fn portable_planned_file_components(path: &Path) -> Result<Vec<Vec<u8>>> {
    if path.is_absolute() {
        bail!(
            "Planned repository file path must be relative: {}",
            path.display()
        );
    }

    let components = path
        .components()
        .map(|component| {
            let Component::Normal(component) = component else {
                bail!(
                    "Planned repository file path must contain only normal relative components: {}",
                    path.display()
                );
            };
            let Some(component_text) = component.to_str() else {
                bail!(
                    "Planned repository file path is not portable because every component must be valid Unicode: {}",
                    path.display()
                );
            };
            let normalized = component_text
                .as_bytes()
                .iter()
                .map(u8::to_ascii_lowercase)
                .collect::<Vec<_>>();
            if normalized.contains(&b'\\') {
                bail!(
                    "Planned repository file path is not portable to Windows because component {:?} contains a raw backslash; use '/' separators: {}",
                    component.to_string_lossy(),
                    path.display()
                );
            }
            if normalized
                .iter()
                .any(|byte| is_windows_forbidden_component_byte(*byte))
            {
                bail!(
                    "Planned repository file path is not portable to Windows because component {:?} contains a forbidden character or control byte: {}",
                    component.to_string_lossy(),
                    path.display()
                );
            }
            if normalized
                .last()
                .is_some_and(|byte| matches!(byte, b'.' | b' '))
            {
                bail!(
                    "Planned repository file path is not portable to Windows because component {:?} ends in a dot or space: {}",
                    component.to_string_lossy(),
                    path.display()
                );
            }
            if is_windows_reserved_device_component(&normalized) {
                bail!(
                    "Planned repository file path is not portable to Windows because component {:?} uses a reserved device name: {}",
                    component.to_string_lossy(),
                    path.display()
                );
            }
            Ok(normalized)
        })
        .collect::<Result<Vec<_>>>()?;
    if components.is_empty() {
        bail!("Planned repository file path cannot be empty");
    }
    Ok(components)
}

const fn is_windows_forbidden_component_byte(byte: u8) -> bool {
    byte.is_ascii_control() || matches!(byte, b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*')
}

fn is_windows_reserved_device_component(component: &[u8]) -> bool {
    let basename = component
        .split(|byte| *byte == b'.')
        .next()
        .unwrap_or(component);
    matches!(basename, b"con" | b"prn" | b"aux" | b"nul")
        || (basename.len() == 4
            && matches!(&basename[..3], b"com" | b"lpt")
            && matches!(basename[3], b'1'..=b'9'))
        || (basename.len() == 5
            && matches!(&basename[..3], b"com" | b"lpt")
            && matches!(&basename[3..], b"\xc2\xb9" | b"\xc2\xb2" | b"\xc2\xb3"))
}

fn component_prefix(prefix: &[Vec<u8>], path: &[Vec<u8>]) -> bool {
    prefix.len() <= path.len() && prefix.iter().zip(path).all(|(left, right)| left == right)
}

pub(crate) fn validate_repository_regular_file_leaf(
    root: &Path,
    relative: &Path,
) -> Result<RepositoryFileLeaf> {
    match validate_repository_relative_file_leaf(root, relative)? {
        RepositoryFileLeaf::Symlink => bail!(
            "Unsafe repository path {}: destination leaf {} is a symlink; generated files must be missing or regular files",
            relative.display(),
            root.join(relative).display()
        ),
        leaf => Ok(leaf),
    }
}

pub(crate) fn read_repository_regular_file(root: &Path, relative: &Path) -> Result<String> {
    let contents = read_repository_regular_file_bytes(root, relative)?;
    String::from_utf8(contents)
        .with_context(|| format!("Failed to read {} as UTF-8", root.join(relative).display()))
}

pub(crate) fn read_repository_regular_file_bytes(root: &Path, relative: &Path) -> Result<Vec<u8>> {
    let mut file = open_verified_repository_regular_file(root, relative)?;
    let path = root.join(relative);
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(contents)
}

fn open_verified_repository_regular_file(root: &Path, relative: &Path) -> Result<File> {
    if validate_repository_regular_file_leaf(root, relative)? != RepositoryFileLeaf::RegularFile {
        bail!(
            "Repository file changed before it could be read: {}",
            root.join(relative).display()
        );
    }

    let path = root.join(relative);
    let file = open_file_no_follow(&path)?;
    let opened = file
        .metadata()
        .with_context(|| format!("Failed to inspect opened file {}", path.display()))?;
    if !opened.is_file() {
        bail!(
            "Repository file changed while it was being opened: {}",
            path.display()
        );
    }
    if validate_repository_regular_file_leaf(root, relative)? != RepositoryFileLeaf::RegularFile {
        bail!(
            "Repository file changed while it was being opened: {}",
            path.display()
        );
    }
    let verification = open_file_no_follow(&path)?;
    if !same_open_file_identity(&file, &verification)? {
        bail!(
            "Repository file changed while it was being opened: {}",
            path.display()
        );
    }

    Ok(file)
}

fn open_file_no_follow(path: &Path) -> Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .with_context(|| format!("Failed to open {} without following links", path.display()))
}

fn open_directory_no_follow(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect directory {}", path.display()))?;
    if !repository_metadata_is_real_directory(&metadata) {
        bail!("Expected a real directory at {}", path.display());
    }

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };

        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let directory = options.open(path).with_context(|| {
        format!(
            "Failed to open directory {} without following links",
            path.display()
        )
    })?;
    let opened = directory
        .metadata()
        .with_context(|| format!("Failed to inspect opened directory {}", path.display()))?;
    if !opened.is_dir() {
        bail!(
            "Directory changed while it was being opened: {}",
            path.display()
        );
    }
    Ok(directory)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_symlink_no_follow(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW);
    options
        .open(path)
        .with_context(|| format!("Failed to retain symlink handle {}", path.display()))
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
fn open_symlink_no_follow(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_SYMLINK);
    options
        .open(path)
        .with_context(|| format!("Failed to retain symlink handle {}", path.display()))
}

#[cfg(windows)]
fn open_symlink_no_follow(path: &Path) -> Result<File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let encoded = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `encoded` is NUL terminated. Ownership of a successful handle
    // is transferred exactly once into `File` below.
    let handle = unsafe {
        CreateFileW(
            encoded.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("Failed to retain symlink handle {}", path.display()));
    }
    // SAFETY: `handle` is valid, uniquely owned here, and has the same raw
    // representation expected by `File::from_raw_handle`.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    windows
)))]
fn open_symlink_no_follow(path: &Path) -> Result<File> {
    bail!(
        "Retained symlink handles are unsupported on this platform: {}",
        path.display()
    )
}

fn repository_symlink_handle_at(path: &Path) -> Result<(RepositoryEntryIdentity, Arc<File>)> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect symlink {}", path.display()))?;
    if !metadata.file_type().is_symlink() {
        bail!("Expected a symlink at {}", path.display());
    }
    let handle = open_symlink_no_follow(path)?;
    let identity = repository_file_identity(&handle)?;
    if repository_path_identity(path)? != identity {
        bail!(
            "Symlink changed while its retained identity was being verified: {}",
            path.display()
        );
    }
    Ok((identity, Arc::new(handle)))
}

pub(crate) fn repository_symlink_commit_at(path: &Path) -> Result<RepositorySymlinkCommit> {
    let (identity, handle) = repository_symlink_handle_at(path)?;
    let target = fs::read_link(path)
        .with_context(|| format!("Failed to read symlink {}", path.display()))?;
    let target_is_directory = repository_symlink_is_directory(&metadata_for_symlink(path)?);
    if repository_path_identity(path)? != identity
        || fs::read_link(path)
            .with_context(|| format!("Failed to reread symlink {}", path.display()))?
            != target
    {
        bail!(
            "Symlink changed while its target was being inspected: {}",
            path.display()
        );
    }
    Ok(RepositorySymlinkCommit {
        identity,
        target,
        target_is_directory,
        handle,
    })
}

fn metadata_for_symlink(path: &Path) -> Result<fs::Metadata> {
    fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect symlink type {}", path.display()))
}

#[cfg(windows)]
fn repository_symlink_is_directory(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::FileTypeExt;

    metadata.file_type().is_symlink_dir()
}

#[cfg(not(windows))]
const fn repository_symlink_is_directory(_metadata: &fs::Metadata) -> bool {
    // Unix creation does not distinguish file and directory symlinks. Keeping
    // this generation property independent of the mutable target also makes
    // snapshot comparison stable for broken or concurrently changed targets.
    false
}

pub(crate) fn repository_directory_commit_at(path: &Path) -> Result<RepositoryDirectoryCommit> {
    let directory = open_directory_no_follow(path)?;
    let identity = repository_file_identity(&directory)?;
    if repository_path_identity(path)? != identity {
        bail!(
            "Directory changed while its retained identity was being verified: {}",
            path.display()
        );
    }
    Ok(RepositoryDirectoryCommit {
        identity,
        handle: Arc::new(directory),
    })
}

pub(crate) fn repository_directory_commit_matches_path(
    commit: &RepositoryDirectoryCommit,
    path: &Path,
) -> Result<bool> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "Failed to inspect retained directory path {}",
            path.display()
        )
    })?;
    Ok(repository_metadata_is_real_directory(&metadata)
        && repository_file_identity(&commit.handle)? == commit.identity
        && repository_path_identity(path)? == commit.identity)
}

fn repository_metadata_is_real_directory(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        windows_directory_attributes_are_real(
            metadata.is_dir(),
            metadata.file_type().is_symlink(),
            metadata.file_attributes(),
        )
    }
    #[cfg(not(windows))]
    {
        metadata.is_dir() && !metadata.file_type().is_symlink()
    }
}

pub(crate) fn repository_metadata_is_real_regular_file(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_file() && !metadata.file_type().is_symlink()
    }
}

#[cfg(windows)]
fn windows_directory_attributes_are_real(
    is_directory: bool,
    is_symlink: bool,
    attributes: u32,
) -> bool {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    is_directory && !is_symlink && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
}

pub(crate) fn write_repository_file_atomic(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    expected_leaf: RepositoryFileLeaf,
) -> Result<RepositoryFileCommit> {
    write_repository_file_atomic_with(
        root,
        relative,
        AtomicWriteOptions {
            expected_leaf,
            desired_permissions: None,
            allow_symlink_replacement: false,
            create_parents: true,
            temporary_directory: None,
        },
        || Ok(()),
        |temp| {
            temp.write_all(bytes).with_context(|| {
                format!(
                    "Failed to write temporary file for {}",
                    root.join(relative).display()
                )
            })
        },
    )
}

pub(crate) fn write_repository_file_atomic_guarded(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    desired_permissions: Option<fs::Permissions>,
    temporary_directory: &Path,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositoryFileCommit> {
    write_repository_file_atomic_with(
        root,
        relative,
        AtomicWriteOptions {
            expected_leaf: RepositoryFileLeaf::Missing,
            desired_permissions,
            allow_symlink_replacement: false,
            create_parents: false,
            temporary_directory: Some(temporary_directory),
        },
        &mut validate_boundary,
        |temp| {
            temp.write_all(bytes).with_context(|| {
                format!(
                    "Failed to write temporary file for {}",
                    root.join(relative).display()
                )
            })
        },
    )
}

pub(crate) fn write_repository_file_atomic_staged(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    expected_leaf: RepositoryFileLeaf,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositoryFileCommit> {
    write_repository_file_atomic_with(
        root,
        relative,
        AtomicWriteOptions {
            expected_leaf,
            desired_permissions: None,
            allow_symlink_replacement: false,
            create_parents: false,
            temporary_directory: None,
        },
        &mut validate_boundary,
        |temp| {
            temp.write_all(bytes).with_context(|| {
                format!(
                    "Failed to write temporary file for {}",
                    root.join(relative).display()
                )
            })
        },
    )
}

pub(crate) fn copy_repository_regular_file_atomic_with_permissions(
    root: &Path,
    relative: &Path,
    source: &Path,
    permissions: fs::Permissions,
    expected_leaf: RepositoryFileLeaf,
) -> Result<RepositoryFileCommit> {
    write_repository_file_atomic_with(
        root,
        relative,
        AtomicWriteOptions {
            expected_leaf,
            desired_permissions: Some(permissions),
            allow_symlink_replacement: true,
            create_parents: true,
            temporary_directory: None,
        },
        || Ok(()),
        |temp| {
            let mut source = File::open(source)
                .with_context(|| format!("Failed to open rendered file {}", source.display()))?;
            std::io::copy(&mut source, temp).with_context(|| {
                format!(
                    "Failed to copy rendered file into temporary file for {}",
                    root.join(relative).display()
                )
            })?;
            Ok(())
        },
    )
}

pub(crate) fn copy_repository_regular_file_atomic_with_permissions_guarded(
    root: &Path,
    relative: &Path,
    source: &Path,
    permissions: fs::Permissions,
    temporary_directory: &Path,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositoryFileCommit> {
    write_repository_file_atomic_with(
        root,
        relative,
        AtomicWriteOptions {
            expected_leaf: RepositoryFileLeaf::Missing,
            desired_permissions: Some(permissions),
            allow_symlink_replacement: true,
            create_parents: false,
            temporary_directory: Some(temporary_directory),
        },
        &mut validate_boundary,
        |temp| {
            let mut source = File::open(source)
                .with_context(|| format!("Failed to open rendered file {}", source.display()))?;
            std::io::copy(&mut source, temp).with_context(|| {
                format!(
                    "Failed to copy rendered file into temporary file for {}",
                    root.join(relative).display()
                )
            })?;
            Ok(())
        },
    )
}

pub(crate) fn copy_repository_regular_file_atomic_with_permissions_staged(
    root: &Path,
    relative: &Path,
    source: &Path,
    permissions: fs::Permissions,
    expected_leaf: RepositoryFileLeaf,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositoryFileCommit> {
    write_repository_file_atomic_with(
        root,
        relative,
        AtomicWriteOptions {
            expected_leaf,
            desired_permissions: Some(permissions),
            allow_symlink_replacement: true,
            create_parents: false,
            temporary_directory: None,
        },
        &mut validate_boundary,
        |temp| {
            let mut source = File::open(source)
                .with_context(|| format!("Failed to open rendered file {}", source.display()))?;
            std::io::copy(&mut source, temp).with_context(|| {
                format!(
                    "Failed to copy rendered file into temporary file for {}",
                    root.join(relative).display()
                )
            })?;
            Ok(())
        },
    )
}

pub(crate) fn copy_repository_symlink_atomic(
    root: &Path,
    relative: &Path,
    source: &Path,
) -> Result<RepositorySymlinkCommit> {
    copy_repository_symlink_atomic_with(root, relative, source, true, None, || Ok(()))
}

pub(crate) fn copy_repository_symlink_atomic_guarded(
    root: &Path,
    relative: &Path,
    source: &Path,
    temporary_directory: &Path,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositorySymlinkCommit> {
    copy_repository_symlink_atomic_with(
        root,
        relative,
        source,
        false,
        Some(temporary_directory),
        &mut validate_boundary,
    )
}

pub(crate) fn copy_repository_symlink_atomic_staged(
    root: &Path,
    relative: &Path,
    source: &Path,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositorySymlinkCommit> {
    copy_repository_symlink_atomic_with(root, relative, source, false, None, &mut validate_boundary)
}

fn copy_repository_symlink_atomic_with(
    root: &Path,
    relative: &Path,
    source: &Path,
    create_parents: bool,
    temporary_directory: Option<&Path>,
    mut validate_boundary: impl FnMut() -> Result<()>,
) -> Result<RepositorySymlinkCommit> {
    if validate_repository_relative_file_leaf(root, relative)? != RepositoryFileLeaf::Missing {
        bail!(
            "Repository symlink destination must be missing before publication: {}",
            root.join(relative).display()
        );
    }
    if create_parents {
        create_repository_parent_directories(root, relative)?;
    } else {
        validate_repository_relative_ancestors(root, relative)?;
    }
    let source_commit = repository_symlink_commit_at(source)
        .with_context(|| format!("Failed to retain rendered symlink {}", source.display()))?;
    let target = source_commit.target.clone();
    let target_is_directory = source_commit.target_is_directory;
    let destination = root.join(relative);
    let destination_parent = destination.parent().with_context(|| {
        format!(
            "Repository symlink has no parent: {}",
            destination.display()
        )
    })?;
    let name = destination
        .file_name()
        .with_context(|| format!("Repository symlink has no name: {}", destination.display()))?
        .to_string_lossy();
    validate_boundary()?;
    let mut temporary = None;
    for index in 0_u32..1024 {
        let candidate = temporary_directory
            .unwrap_or(destination_parent)
            .join(format!(".{name}.jig-link-{}-{index}", std::process::id()));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == ErrorKind::NotFound => {
                temporary = Some(candidate);
                break;
            }
            Ok(_) => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect temporary symlink {}",
                        candidate.display()
                    )
                });
            }
        }
    }
    let temporary = temporary.context("Failed to allocate a unique temporary symlink path")?;
    create_repository_symlink(&target, &temporary, target_is_directory)?;
    let (identity, handle) = match repository_symlink_handle_at(&temporary) {
        Ok(commit) => commit,
        Err(primary) => {
            return Err(cleanup_temporary_symlink_without_identity(
                &temporary, primary,
            ));
        }
    };
    if let Err(primary) = validate_boundary() {
        return Err(cleanup_temporary_symlink(&temporary, &identity, primary));
    }
    match validate_repository_relative_file_leaf(root, relative) {
        Ok(RepositoryFileLeaf::Missing) => {}
        Ok(_) => {
            let primary = anyhow::anyhow!(
                "Repository symlink destination appeared concurrently: {}",
                destination.display()
            );
            return Err(cleanup_temporary_symlink(&temporary, &identity, primary));
        }
        Err(primary) => {
            return Err(cleanup_temporary_symlink(&temporary, &identity, primary));
        }
    }
    if let Err(error) = rename_entry_noreplace(&temporary, &destination) {
        let primary = anyhow::Error::new(error).context(format!(
            "Failed to publish symlink {}",
            destination.display()
        ));
        return Err(cleanup_temporary_symlink(&temporary, &identity, primary));
    }
    Ok(RepositorySymlinkCommit {
        identity,
        target,
        target_is_directory,
        handle,
    })
}

fn cleanup_temporary_symlink(
    temporary: &Path,
    expected_identity: &RepositoryEntryIdentity,
    primary: anyhow::Error,
) -> anyhow::Error {
    cleanup_temporary_symlink_with(temporary, expected_identity, primary, |_| {})
}

fn cleanup_temporary_symlink_with(
    temporary: &Path,
    expected_identity: &RepositoryEntryIdentity,
    primary: anyhow::Error,
    before_quarantine_identity_check: impl FnOnce(&Path),
) -> anyhow::Error {
    let Some(parent) = temporary.parent() else {
        return anyhow::anyhow!(
            "{primary:#}\nTemporary symlink has no parent for safe cleanup: {}",
            temporary.display()
        );
    };
    let name = temporary
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("jig-link"))
        .to_string_lossy();
    let mut quarantine = None;
    for index in 0_u32..1024 {
        let candidate = parent.join(format!(
            ".{name}.jig-link-cleanup-{}-{index}",
            std::process::id()
        ));
        match rename_entry_noreplace(temporary, &candidate) {
            Ok(()) => {
                quarantine = Some(candidate);
                break;
            }
            Err(error) => match fs::symlink_metadata(&candidate) {
                Ok(_) => {}
                Err(inspect_error) if inspect_error.kind() == ErrorKind::NotFound => {
                    return if error.kind() == ErrorKind::NotFound {
                        primary
                    } else {
                        anyhow::anyhow!(
                            "{primary:#}\nCould not quarantine temporary symlink {} for safe cleanup; preserving it: {error}",
                            temporary.display()
                        )
                    };
                }
                Err(inspect_error) => {
                    return anyhow::anyhow!(
                        "{primary:#}\nCould not inspect temporary symlink cleanup candidate {} after rename failed ({error}); preserving {}: {inspect_error}",
                        candidate.display(),
                        temporary.display()
                    );
                }
            },
        }
    }
    let Some(quarantine) = quarantine else {
        return anyhow::anyhow!(
            "{primary:#}\nCould not allocate an uncontended cleanup quarantine for temporary symlink {}; preserving it",
            temporary.display()
        );
    };

    before_quarantine_identity_check(&quarantine);
    match repository_path_identity(&quarantine) {
        Ok(identity) if &identity == expected_identity => match fs::remove_file(&quarantine) {
            Ok(()) => primary,
            Err(cleanup) => anyhow::anyhow!(
                "{primary:#}\nAdditionally failed to remove quarantined temporary symlink {}; it remains available for recovery: {cleanup}",
                quarantine.display()
            ),
        },
        Ok(_) => restore_changed_temporary_symlink(
            &quarantine,
            temporary,
            anyhow::anyhow!(
                "{primary:#}\nTemporary symlink changed before cleanup; refusing to unlink the replacement {}",
                quarantine.display()
            ),
        ),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == ErrorKind::NotFound) =>
        {
            primary
        }
        Err(cleanup) => anyhow::anyhow!(
            "{primary:#}\nAdditionally failed to identify quarantined temporary symlink {} for cleanup; preserving it: {cleanup:#}",
            quarantine.display()
        ),
    }
}

fn restore_changed_temporary_symlink(
    quarantine: &Path,
    temporary: &Path,
    primary: anyhow::Error,
) -> anyhow::Error {
    match rename_entry_noreplace(quarantine, temporary) {
        Ok(()) => anyhow::anyhow!(
            "{primary:#}\nRestored the changed entry to {}",
            temporary.display()
        ),
        Err(error) => anyhow::anyhow!(
            "{primary:#}\nPreserved the changed entry at {} because {} became occupied: {error}",
            quarantine.display(),
            temporary.display()
        ),
    }
}

fn cleanup_temporary_symlink_without_identity(
    temporary: &Path,
    primary: anyhow::Error,
) -> anyhow::Error {
    // Without a stable identity it is unsafe to unlink this name: a watcher
    // may already have substituted a foreign entry. Surface its recovery path.
    anyhow::anyhow!(
        "{primary:#}\nCould not prove ownership of temporary symlink {}; preserving it for manual recovery",
        temporary.display()
    )
}

#[cfg(unix)]
fn create_repository_symlink(target: &Path, link: &Path, _target_is_directory: bool) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("Failed to create temporary symlink {}", link.display()))
}

#[cfg(windows)]
fn create_repository_symlink(target: &Path, link: &Path, target_is_directory: bool) -> Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    if target_is_directory {
        symlink_dir(target, link)
    } else {
        symlink_file(target, link)
    }
    .with_context(|| format!("Failed to create temporary symlink {}", link.display()))
}

#[cfg(not(any(unix, windows)))]
fn create_repository_symlink(
    _target: &Path,
    link: &Path,
    _target_is_directory: bool,
) -> Result<()> {
    bail!(
        "Creating repository symlinks is unsupported: {}",
        link.display()
    )
}

struct AtomicWriteOptions<'a> {
    expected_leaf: RepositoryFileLeaf,
    desired_permissions: Option<fs::Permissions>,
    allow_symlink_replacement: bool,
    create_parents: bool,
    temporary_directory: Option<&'a Path>,
}

fn write_repository_file_atomic_with(
    root: &Path,
    relative: &Path,
    options: AtomicWriteOptions<'_>,
    mut validate_boundary: impl FnMut() -> Result<()>,
    write_contents: impl FnOnce(&mut File) -> Result<()>,
) -> Result<RepositoryFileCommit> {
    let AtomicWriteOptions {
        expected_leaf,
        desired_permissions,
        allow_symlink_replacement,
        create_parents,
        temporary_directory,
    } = options;
    if expected_leaf == RepositoryFileLeaf::Symlink && !allow_symlink_replacement {
        bail!(
            "Unsafe repository path {}: generated files cannot replace symlinks",
            relative.display()
        );
    }
    let validate_leaf = || {
        if allow_symlink_replacement {
            validate_repository_relative_file_leaf(root, relative)
        } else {
            validate_repository_regular_file_leaf(root, relative)
        }
    };
    let observed = validate_leaf()?;
    if observed != expected_leaf {
        bail!(
            "Repository file changed before it could be written: {}",
            root.join(relative).display()
        );
    }

    if create_parents {
        create_repository_parent_directories(root, relative)?;
    } else {
        validate_repository_relative_ancestors(root, relative)?;
    }
    let observed = validate_leaf()?;
    if observed != expected_leaf {
        bail!(
            "Repository file changed before it could be written: {}",
            root.join(relative).display()
        );
    }

    let path = root.join(relative);
    let parent = path
        .parent()
        .with_context(|| format!("Repository file has no parent: {}", path.display()))?;
    let existing_permissions = match expected_leaf {
        RepositoryFileLeaf::Missing | RepositoryFileLeaf::Symlink => None,
        RepositoryFileLeaf::RegularFile => {
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                bail!(
                    "Repository file changed before it could be written: {}",
                    path.display()
                );
            }
            Some(metadata.permissions())
        }
    };
    let final_permissions = desired_permissions.or(existing_permissions);

    validate_boundary()?;
    let mut builder = TempFileBuilder::new();
    builder.prefix(".jig-write-").suffix(".tmp");
    #[cfg(unix)]
    if final_permissions.is_none() {
        use std::os::unix::fs::PermissionsExt;

        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temp = builder
        .tempfile_in(temporary_directory.unwrap_or(parent))
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;
    if let Some(permissions) = final_permissions {
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| format!("Failed to preserve permissions for {}", path.display()))?;
    }
    write_contents(temp.as_file_mut())?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary file for {}", path.display()))?;

    let fingerprint = repository_file_fingerprint(temp.as_file_mut()).with_context(|| {
        format!(
            "Failed to identify completed temporary file for {}",
            path.display()
        )
    })?;

    validate_boundary()?;

    if validate_leaf()? != expected_leaf {
        bail!(
            "Repository file changed while it was being written: {}",
            path.display()
        );
    }
    let published = match expected_leaf {
        RepositoryFileLeaf::Missing => temp
            .persist_noclobber(&path)
            .map_err(|error| error.error)
            .with_context(|| format!("Failed to create {}", path.display())),
        RepositoryFileLeaf::RegularFile | RepositoryFileLeaf::Symlink => temp
            .persist(&path)
            .map_err(|error| error.error)
            .with_context(|| format!("Failed to replace {}", path.display())),
    }?;
    Ok(RepositoryFileCommit {
        identity: fingerprint.identity,
        content_length: fingerprint.content_length,
        content_sha256: fingerprint.content_sha256,
        permission_identity: fingerprint.permission_identity,
        handle: Arc::new(published),
    })
}

struct RepositoryFileFingerprint {
    identity: RepositoryEntryIdentity,
    content_length: u64,
    content_sha256: [u8; 32],
    permission_identity: u32,
}

fn repository_file_fingerprint(file: &mut File) -> Result<RepositoryFileFingerprint> {
    let metadata = file
        .metadata()
        .context("Failed to inspect repository file")?;
    let identity = repository_file_identity(file)?;
    let content_sha256 = hash_open_repository_file(file)?;
    let verification_sha256 = hash_open_repository_file(file)?;
    let after = file
        .metadata()
        .context("Failed to reinspect repository file after hashing")?;
    if content_sha256 != verification_sha256
        || metadata.len() != after.len()
        || repository_permission_identity(&metadata.permissions())
            != repository_permission_identity(&after.permissions())
        || metadata.modified().ok() != after.modified().ok()
    {
        bail!("Repository file changed while its stable fingerprint was being read");
    }
    Ok(RepositoryFileFingerprint {
        identity,
        content_length: metadata.len(),
        content_sha256,
        permission_identity: repository_permission_identity(&metadata.permissions()),
    })
}

fn hash_open_repository_file(file: &mut File) -> Result<[u8; 32]> {
    file.seek(SeekFrom::Start(0))
        .context("Failed to rewind repository file")?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .context("Failed to hash repository file")?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

pub(crate) fn repository_file_fingerprint_at(path: &Path) -> Result<RepositoryFileCommit> {
    let mut file = open_file_no_follow(path)?;
    let fingerprint = repository_file_fingerprint(&mut file)?;
    let path_identity = repository_path_identity(path)?;
    if path_identity != fingerprint.identity {
        bail!(
            "Repository file changed while its path identity was being verified: {}",
            path.display()
        );
    }
    Ok(RepositoryFileCommit {
        identity: fingerprint.identity,
        content_length: fingerprint.content_length,
        content_sha256: fingerprint.content_sha256,
        permission_identity: fingerprint.permission_identity,
        handle: Arc::new(file),
    })
}

pub(crate) fn repository_file_commits_match(
    left: &RepositoryFileCommit,
    right: &RepositoryFileCommit,
) -> bool {
    left.identity == right.identity
        && left.content_length == right.content_length
        && left.content_sha256 == right.content_sha256
        && left.permission_identity == right.permission_identity
}

pub(crate) fn repository_file_commit_matches_path(
    commit: &RepositoryFileCommit,
    path: &Path,
) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect retained file path {}", path.display()))?;
    if !repository_metadata_is_real_regular_file(&metadata) {
        return Ok(false);
    }
    let current = repository_file_fingerprint_at(path)?;
    Ok(repository_file_commits_match(commit, &current))
}

fn create_repository_parent_directories(root: &Path, relative: &Path) -> Result<()> {
    validate_repository_relative_ancestors(root, relative)?;
    let mut current = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(component) = component else {
                unreachable!("repository-relative components were validated above");
            };
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if repository_metadata_is_real_directory(&metadata) => {}
                Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                    "Unsafe repository path {}: ancestor {} is a symlink",
                    relative.display(),
                    current.display()
                ),
                Ok(_) => bail!(
                    "Unsafe repository path {}: ancestor {} is not a directory",
                    relative.display(),
                    current.display()
                ),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                        Err(error) => {
                            return Err(error).with_context(|| {
                                format!("Failed to create {}", current.display())
                            });
                        }
                    }
                    let metadata = fs::symlink_metadata(&current)
                        .with_context(|| format!("Failed to stat {}", current.display()))?;
                    if !repository_metadata_is_real_directory(&metadata) {
                        bail!(
                            "Unsafe repository path {}: newly created ancestor {} is not a real directory",
                            relative.display(),
                            current.display()
                        );
                    }
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to stat {}", current.display()));
                }
            }
        }
    }
    validate_repository_relative_ancestors(root, relative)
}

#[cfg(unix)]
fn same_open_file_identity(left: &File, right: &File) -> Result<bool> {
    Ok(repository_file_identity(left)? == repository_file_identity(right)?)
}

#[cfg(windows)]
fn same_open_file_identity(left: &File, right: &File) -> Result<bool> {
    Ok(repository_file_identity(left)? == repository_file_identity(right)?)
}

#[cfg(not(any(unix, windows)))]
fn same_open_file_identity(left: &File, right: &File) -> Result<bool> {
    let left = left.metadata().context("Failed to inspect opened file")?;
    let right = right.metadata().context("Failed to inspect opened file")?;
    Ok(left.is_file() && right.is_file() && left.len() == right.len())
}

#[cfg(unix)]
pub(crate) fn repository_file_identity(file: &File) -> Result<RepositoryEntryIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata().context("Failed to inspect opened file")?;
    Ok(RepositoryEntryIdentity {
        platform: RepositoryEntryPlatformIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
    })
}

#[cfg(windows)]
pub(crate) fn repository_file_identity(file: &File) -> Result<RepositoryEntryIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `info` is writable for the duration of the call and the raw
    // handle remains owned by `file`, which outlives the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("Failed to identify opened repository file");
    }
    Ok(RepositoryEntryIdentity {
        platform: RepositoryEntryPlatformIdentity::Windows {
            volume_serial: info.dwVolumeSerialNumber,
            file_index_high: info.nFileIndexHigh,
            file_index_low: info.nFileIndexLow,
        },
    })
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn repository_file_identity(_file: &File) -> Result<RepositoryEntryIdentity> {
    bail!("stable repository file identity is unsupported on this platform")
}

#[cfg(unix)]
pub(crate) fn repository_path_identity(path: &Path) -> Result<RepositoryEntryIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to identify {}", path.display()))?;
    Ok(RepositoryEntryIdentity {
        platform: RepositoryEntryPlatformIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
    })
}

#[cfg(windows)]
pub(crate) fn repository_path_identity(path: &Path) -> Result<RepositoryEntryIdentity> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, OPEN_EXISTING,
    };

    let encoded = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `encoded` is a valid NUL-terminated path buffer. The returned
    // handle is closed on every path below.
    let handle = unsafe {
        CreateFileW(
            encoded.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("Failed to identify {}", path.display()));
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is valid and `info` is writable for the call.
    let result = unsafe { GetFileInformationByHandle(handle as HANDLE, &mut info) };
    // SAFETY: `handle` is owned by this function and is closed exactly once.
    unsafe { CloseHandle(handle as HANDLE) };
    if result == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("Failed to identify {}", path.display()));
    }
    Ok(RepositoryEntryIdentity {
        platform: RepositoryEntryPlatformIdentity::Windows {
            volume_serial: info.dwVolumeSerialNumber,
            file_index_high: info.nFileIndexHigh,
            file_index_low: info.nFileIndexLow,
        },
    })
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn repository_path_identity(_path: &Path) -> Result<RepositoryEntryIdentity> {
    bail!("stable repository path identity is unsupported on this platform")
}

#[cfg(unix)]
pub(crate) fn repository_permission_identity(permissions: &fs::Permissions) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    permissions.mode()
}

pub(crate) fn repository_paths_same_filesystem(left: &Path, right: &Path) -> Result<bool> {
    let left = repository_path_identity(left)?;
    let right = repository_path_identity(right)?;
    Ok(match (&left.platform, &right.platform) {
        #[cfg(unix)]
        (
            RepositoryEntryPlatformIdentity::Unix { device: left, .. },
            RepositoryEntryPlatformIdentity::Unix { device: right, .. },
        ) => left == right,
        #[cfg(windows)]
        (
            RepositoryEntryPlatformIdentity::Windows {
                volume_serial: left,
                ..
            },
            RepositoryEntryPlatformIdentity::Windows {
                volume_serial: right,
                ..
            },
        ) => left == right,
        #[cfg(not(any(unix, windows)))]
        _ => false,
    })
}

#[cfg(not(unix))]
pub(crate) fn repository_permission_identity(permissions: &fs::Permissions) -> u32 {
    u32::from(permissions.readonly())
}

pub(super) fn validate_no_reserved_git_metadata_components(relative: &Path) -> Result<()> {
    if let Some(component) = reserved_git_metadata_component(relative) {
        bail!(
            "Unsafe repository path {}: component {component:?} aliases the reserved Git metadata component \".git\" under Git's NTFS/HFS path rules",
            relative.display()
        );
    }
    Ok(())
}

fn reserved_git_metadata_component(relative: &Path) -> Option<&str> {
    relative.components().find_map(|component| {
        let Component::Normal(component) = component else {
            return None;
        };
        let component = component.to_str()?;
        component.split('\\').find(|segment| {
            is_ntfs_git_metadata_alias(segment) || is_hfs_git_metadata_alias(segment)
        })
    })
}

// Behavioral reference only; this is an independent Rust implementation of Git's
// protections pinned at f60db8d575adb79761d363e026fb49bddf330c73:
// https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/path.c#L1394-L1449
// https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/utf8.c#L698-L787
// Upstream cases: https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/t/t0060-path-utils.sh#L438-L527
fn is_ntfs_git_metadata_alias(component: &str) -> bool {
    [".git", "git~1"].into_iter().any(|prefix| {
        let Some(remainder) = strip_ascii_case_prefix(component, prefix) else {
            return false;
        };
        remainder
            .split_once(':')
            .map_or(remainder, |(before_stream, _)| before_stream)
            .bytes()
            .all(|byte| byte == b'.' || byte == b' ')
    })
}

fn strip_ascii_case_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .then(|| &value[prefix.len()..])
}

fn is_hfs_git_metadata_alias(component: &str) -> bool {
    let mut normalized = component
        .chars()
        .filter(|character| !is_hfs_ignored(*character));
    let expected = ['.', 'g', 'i', 't'];
    expected.into_iter().all(|expected| {
        normalized
            .next()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
    }) && normalized.next().is_none()
}

const fn is_hfs_ignored(character: char) -> bool {
    matches!(
        character,
        '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
    )
}

pub(super) fn validate_repository_relative_ancestors(root: &Path, relative: &Path) -> Result<()> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "Repository path must be a contained relative path: {}",
            relative.display()
        );
    }
    validate_no_reserved_git_metadata_components(relative)?;

    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("Failed to stat repository root {}", root.display()))?;
    if !repository_metadata_is_real_directory(&root_metadata) {
        bail!(
            "Repository root must be a real directory, not a symlink or non-directory: {}",
            root.display()
        );
    }

    let destination = root.join(relative);
    if !destination.starts_with(root) {
        bail!(
            "Repository path escapes root {}: {}",
            root.display(),
            relative.display()
        );
    }

    let mut ancestor = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(component) = component else {
                unreachable!("relative path components were validated above");
            };
            ancestor.push(component);
            let metadata = match fs::symlink_metadata(&ancestor) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to stat {}", ancestor.display()));
                }
            };
            if metadata.file_type().is_symlink() {
                bail!(
                    "Unsafe repository path {}: ancestor {} is a symlink",
                    relative.display(),
                    ancestor.display()
                );
            }
            if !repository_metadata_is_real_directory(&metadata) {
                bail!(
                    "Unsafe repository path {}: ancestor {} is not a directory",
                    relative.display(),
                    ancestor.display()
                );
            }
        }
    }

    Ok(())
}

pub(super) fn validate_repository_relative_file_leaf(
    root: &Path,
    relative: &Path,
) -> Result<RepositoryFileLeaf> {
    validate_repository_relative_ancestors(root, relative)?;

    let path = root.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(RepositoryFileLeaf::Missing);
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to stat {}", path.display()));
        }
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Ok(RepositoryFileLeaf::Symlink);
    }
    if file_type.is_file() {
        return Ok(RepositoryFileLeaf::RegularFile);
    }
    if file_type.is_dir() {
        bail!(
            "Unsafe repository path {}: destination leaf {} is a directory; managed paths must be missing, regular files, or symlinks",
            relative.display(),
            path.display()
        );
    }
    bail!(
        "Unsafe repository path {}: destination leaf {} has an unsupported file type; managed paths must be missing, regular files, or symlinks",
        relative.display(),
        path.display()
    )
}

pub(super) fn absolute_path_from(path: &Path, base: &Path) -> Result<PathBuf> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    if resolved.exists() {
        fs::canonicalize(&resolved)
            .with_context(|| format!("Failed to canonicalize {}", resolved.display()))
    } else {
        Ok(resolved)
    }
}

pub(super) fn split_existing_ancestor(destination: &Path) -> Result<(PathBuf, Vec<PathBuf>)> {
    let mut existing = destination.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(&existing) {
            Ok(_) => {
                let canonical = fs::canonicalize(&existing).with_context(|| {
                    format!(
                        "Failed to canonicalize existing init destination ancestor {}",
                        existing.display()
                    )
                })?;
                missing.reverse();
                return Ok((canonical, missing));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let name = existing.file_name().with_context(|| {
                    format!(
                        "Init destination has no existing ancestor: {}",
                        destination.display()
                    )
                })?;
                missing.push(PathBuf::from(name));
                existing = existing
                    .parent()
                    .with_context(|| {
                        format!(
                            "Init destination has no existing ancestor: {}",
                            destination.display()
                        )
                    })?
                    .to_path_buf();
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect init destination ancestor {}",
                        existing.display()
                    )
                });
            }
        }
    }
}

pub(super) fn resolve_init_destination(path: &Path, base: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("Init destination must not be empty");
    }

    #[cfg(windows)]
    if (path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_))))
        && !path.is_absolute()
    {
        bail!(
            "Init destination must be a normal relative path or a complete absolute drive/UNC path: {}",
            path.display()
        );
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                bail!(
                    "Init destination must not contain '..' path components: {}",
                    path.display()
                );
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }

    // `.` is the one useful destination whose normalized relative spelling is
    // empty. It deliberately means the invocation directory.
    let normalized = if normalized.as_os_str().is_empty() {
        base.to_path_buf()
    } else if normalized.is_absolute() {
        normalized
    } else {
        base.join(normalized)
    };
    if let Ok(metadata) = fs::symlink_metadata(&normalized) {
        if metadata.file_type().is_symlink() {
            // Preserve the requested final leaf so destination validation can
            // reject it. Only symlink ancestors of a genuinely missing tail
            // are canonicalized below.
            return Ok(normalized);
        }
    }
    let (existing, missing) = split_existing_ancestor(&normalized)?;
    let mut resolved = existing;
    for component in missing {
        resolved.push(component);
    }
    Ok(resolved)
}

// The shared publication boundary is fallible on platforms without a hardened
// no-replace primitive even though supported targets return immediately.
#[allow(clippy::unnecessary_wraps)]
const fn ensure_atomic_noreplace_publication_supported_on_platform() -> Result<()> {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        windows
    ))]
    {
        Ok(())
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        windows
    )))]
    {
        bail!("atomic no-replace init publication is unsupported on this platform")
    }
}

pub(crate) fn ensure_atomic_noreplace_publication_supported(parent: &Path) -> Result<()> {
    ensure_atomic_noreplace_publication_supported_on_platform()?;
    ensure_atomic_noreplace_publication_supported_with(parent, rename_entry_noreplace)
}

fn ensure_atomic_noreplace_publication_supported_with(
    parent: &Path,
    mut rename_noreplace: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PROBE_CLEANUP: AtomicU64 = AtomicU64::new(0);

    let mut builder = TempFileBuilder::new();
    builder.prefix(".jig-noreplace-probe-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let probe = builder.tempdir_in(parent).with_context(|| {
        format!(
            "Failed to create an atomic-publication capability probe in {}",
            parent.display()
        )
    })?;
    let probe_path = probe.path().to_path_buf();
    let probe_identity = match repository_directory_commit_at(&probe_path) {
        Ok(identity) => identity,
        Err(primary) => {
            let preserved = probe.keep();
            bail!(
                "{primary:#}\nCould not prove ownership of the atomic-publication capability probe; preserving it at {}",
                preserved.display()
            );
        }
    };
    let source = probe_path.join("source");
    let occupied_destination = probe_path.join("occupied-destination");
    let published_destination = probe_path.join("published-destination");
    if let Err(primary) = fs::create_dir(&source).with_context(|| {
        format!(
            "Failed to prepare atomic-publication capability probe {}",
            source.display()
        )
    }) {
        let preserved = probe.keep();
        bail!(
            "{primary:#}\nPreserving the incomplete capability probe at {}",
            preserved.display()
        );
    }
    let source_identity = match repository_directory_commit_at(&source) {
        Ok(identity) => identity,
        Err(primary) => {
            let preserved = probe.keep();
            bail!(
                "{primary:#}\nCould not retain the atomic-publication source identity; preserving the probe at {}",
                preserved.display()
            );
        }
    };

    if let Err(primary) = fs::create_dir(&occupied_destination).with_context(|| {
        format!(
            "Failed to prepare occupied atomic-publication probe destination {}",
            occupied_destination.display()
        )
    }) {
        let preserved = probe.keep();
        bail!(
            "{primary:#}\nPreserving the incomplete capability probe at {}",
            preserved.display()
        );
    }
    let occupied_identity = match repository_directory_commit_at(&occupied_destination) {
        Ok(identity) => identity,
        Err(primary) => {
            let preserved = probe.keep();
            bail!(
                "{primary:#}\nCould not retain the occupied atomic-publication destination identity; preserving the probe at {}",
                preserved.display()
            );
        }
    };

    let collision_error = match rename_noreplace(&source, &occupied_destination) {
        Ok(()) => {
            let preserved = probe.keep();
            bail!(
                "The filesystem containing {} replaced an occupied directory during an operation that required atomic no-replace semantics; preserving the capability probe at {}",
                parent.display(),
                preserved.display()
            );
        }
        Err(error) => error,
    };
    let source_survived_collision =
        repository_directory_commit_matches_path(&source_identity, &source);
    let occupied_survived_collision =
        repository_directory_commit_matches_path(&occupied_identity, &occupied_destination);
    let probe_survived_collision =
        repository_directory_commit_matches_path(&probe_identity, &probe_path);
    if !source_survived_collision.unwrap_or(false)
        || !occupied_survived_collision.unwrap_or(false)
        || !probe_survived_collision.unwrap_or(false)
    {
        let preserved = probe.keep();
        bail!(
            "Atomic no-replace collision probing in {} returned {collision_error}, but did not preserve both directory identities; preserving the complete capability probe at {}",
            parent.display(),
            preserved.display()
        );
    }

    if let Err(error) = rename_noreplace(&source, &published_destination) {
        let preserved = probe.keep();
        bail!(
            "The filesystem containing {} does not provide the atomic no-replace directory rename required for transactional init: {error}. No repository output was written; preserving the capability probe at {} for manual recovery.",
            parent.display(),
            preserved.display()
        );
    }
    let probe_is_intact = repository_directory_commit_matches_path(&probe_identity, &probe_path);
    let destination_is_source =
        repository_directory_commit_matches_path(&source_identity, &published_destination);
    let occupied_is_intact =
        repository_directory_commit_matches_path(&occupied_identity, &occupied_destination);
    let source_is_absent = matches!(
        fs::symlink_metadata(&source),
        Err(error) if error.kind() == ErrorKind::NotFound
    );
    if !probe_is_intact.unwrap_or(false)
        || !destination_is_source.unwrap_or(false)
        || !occupied_is_intact.unwrap_or(false)
        || !source_is_absent
    {
        let preserved = probe.keep();
        bail!(
            "Atomic no-replace directory rename produced an unverifiable result in {}; preserving the complete capability probe at {}",
            parent.display(),
            preserved.display()
        );
    }

    let cleanup_parent = probe_path.parent().unwrap_or(parent);
    let mut cleanup_path = None;
    for attempt in 0_u64..128 {
        let sequence = NEXT_PROBE_CLEANUP.fetch_add(1, Ordering::Relaxed);
        let candidate = cleanup_parent.join(format!(
            ".jig-noreplace-cleanup-{}-{sequence:x}-{attempt:x}",
            std::process::id()
        ));
        match rename_noreplace(&probe_path, &candidate) {
            Ok(()) => {
                cleanup_path = Some(candidate);
                break;
            }
            Err(error) => match fs::symlink_metadata(&candidate) {
                Ok(_) => {}
                Err(inspect_error) if inspect_error.kind() == ErrorKind::NotFound => {
                    let preserved = probe.keep();
                    bail!(
                        "Atomic-publication support was verified in {}, but the owned capability probe could not be quarantined for cleanup ({error}); preserving it at {}",
                        parent.display(),
                        preserved.display()
                    );
                }
                Err(inspect_error) => {
                    let preserved = probe.keep();
                    bail!(
                        "Atomic-publication support was verified in {}, but cleanup candidate {} could not be inspected after quarantine failed ({error}; {inspect_error}); preserving the probe at {}",
                        parent.display(),
                        candidate.display(),
                        preserved.display()
                    );
                }
            },
        }
    }
    let Some(cleanup_path) = cleanup_path else {
        let preserved = probe.keep();
        bail!(
            "Atomic-publication support was verified in {}, but no uncontended cleanup quarantine was available; preserving the capability probe at {}",
            parent.display(),
            preserved.display()
        );
    };
    let _ = probe.keep();
    if !repository_directory_commit_matches_path(&probe_identity, &cleanup_path)? {
        bail!(
            "Atomic-publication capability probe changed before cleanup; preserving its replacement at {}",
            cleanup_path.display()
        );
    }
    if let Err(error) = fs::remove_dir_all(&cleanup_path) {
        bail!(
            "Failed to remove the identity-checked atomic-publication capability probe {}; it remains available for recovery: {error}",
            cleanup_path.display()
        );
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn rename_entry_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "destination path contains NUL"))?;
    // SAFETY: both strings are NUL-terminated and live for the duration of the call.
    if unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
pub(crate) fn rename_entry_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "destination path contains NUL"))?;
    // SAFETY: both strings are NUL-terminated and live for the duration of the call.
    if unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn rename_entry_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // MOVEFILE_REPLACE_EXISTING is deliberately absent.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    windows
)))]
pub(crate) fn rename_entry_noreplace(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "atomic no-replace init publication is unsupported on this platform",
    ))
}

pub(super) fn bootstrap_invocation_cwd() -> Result<PathBuf> {
    let Some(value) = env::var_os(INVOCATION_CWD_ENV) else {
        let cwd = env::current_dir().context("Failed to resolve current directory")?;
        return fs::canonicalize(&cwd).with_context(|| {
            format!(
                "Failed to canonicalize current directory: {}",
                cwd.display()
            )
        });
    };

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        bail!(
            "{INVOCATION_CWD_ENV} must be an absolute path: {}",
            path.display()
        );
    }
    if !path.is_dir() {
        bail!(
            "{INVOCATION_CWD_ENV} is not a directory: {}",
            path.display()
        );
    }
    fs::canonicalize(&path).with_context(|| {
        format!(
            "Failed to canonicalize {INVOCATION_CWD_ENV}: {}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests;
