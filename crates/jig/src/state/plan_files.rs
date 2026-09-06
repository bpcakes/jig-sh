//! Confined, descriptor-relative storage for work-plan Markdown bodies.
//!
//! Reads retain and validate only the prefix needed to display 20,000 Unicode
//! scalars. Bytes beyond that bounded prefix are deliberately unobserved; a
//! body that is valid through the display boundary returns marked truncated.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use fs4::fs_std::FileExt;

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;

pub(crate) const PLAN_BODY_VISIBLE_CHARS: usize = 20_000;
const PLAN_BODY_PREFIX_BYTES: usize = PLAN_BODY_VISIBLE_CHARS * 4;
pub(crate) const PLAN_BODY_INPUT_BYTES: usize = PLAN_BODY_PREFIX_BYTES + 4;
const PLAN_BODY_READ_CHUNK: usize = 16 * 1024;
const PLAN_BODY_LOCK_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(10);
const PLAN_BODY_LOCK_WAIT_LIMIT: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanFileErrorKind {
    InvalidId,
    NotFound,
    UnsafePath,
    UnsafeType,
    InvalidUtf8,
    Read,
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    UnsupportedPlatform,
}

#[derive(Debug)]
pub(crate) struct PlanFileError {
    kind: PlanFileErrorKind,
    message: String,
}

impl PlanFileError {
    pub(crate) const fn kind(&self) -> PlanFileErrorKind {
        self.kind
    }
}

impl std::fmt::Display for PlanFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PlanFileError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlanBodyRead {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

pub(crate) fn validate_plan_id(plan_id: &str) -> Result<()> {
    if plan_id.is_empty()
        || plan_id.len() > 128
        || !plan_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(plan_error(
            PlanFileErrorKind::InvalidId,
            "plan id must contain 1 through 128 ASCII alphanumeric, underscore, or hyphen bytes",
        ));
    }
    Ok(())
}

pub(crate) fn plan_body_path(ctx: &RepoContext, plan_id: &str) -> Result<PathBuf> {
    validate_plan_id(plan_id)?;
    Ok(ctx
        .root()
        .join(".agent/plans")
        .join(plan_body_name(plan_id)))
}

pub(crate) fn create_plan_body(ctx: &RepoContext, plan_id: &str, body: &str) -> Result<PathBuf> {
    validate_plan_id(plan_id)?;
    let path = plan_body_path(ctx, plan_id)?;
    let directory = open_plan_directory(ctx.root(), true, &|| false)?
        .expect("creating the plan directory always returns a handle");
    let mut options = regular_options(true, true, true);
    let mut file = open_regular(&directory, &plan_body_name(plan_id), &mut options, &path)?;
    file.write_all(body.as_bytes())
        .with_context(|| format!("Failed to write plan body {}", path.display()))?;
    file.sync_data()
        .with_context(|| format!("Failed to sync plan body {}", path.display()))?;
    Ok(path)
}

pub(crate) fn append_plan_body(ctx: &RepoContext, plan_id: &str, body: &[u8]) -> Result<()> {
    validate_plan_id(plan_id)?;
    let path = plan_body_path(ctx, plan_id)?;
    let directory = open_plan_directory(ctx.root(), false, &|| false)?.ok_or_else(|| {
        plan_error(
            PlanFileErrorKind::NotFound,
            format!("Plan body directory does not exist for {}", path.display()),
        )
    })?;
    let mut body_options = regular_options(true, false, false);
    body_options.append(true);
    let mut file = open_regular(
        &directory,
        &plan_body_name(plan_id),
        &mut body_options,
        &path,
    )
    .map_err(|error| missing_body_append_error(error, &path))?;
    file.lock_exclusive()
        .with_context(|| format!("Failed to lock plan body {}", path.display()))?;

    let lock_path = path.with_extension("md.lock");
    let mut lock_options = regular_options(false, true, false);
    lock_options.read(true).write(true);
    let lock = match open_regular(
        &directory,
        &plan_lock_name(plan_id),
        &mut lock_options,
        &lock_path,
    ) {
        Ok(lock) => lock,
        Err(error) => {
            let _ = FileExt::unlock(&file);
            return Err(error);
        }
    };
    if let Err(error) = lock.lock_exclusive() {
        let _ = FileExt::unlock(&file);
        return Err(error).with_context(|| format!("Failed to lock {}", lock_path.display()));
    }

    let result = file
        .write_all(body)
        .with_context(|| format!("Failed to append plan body {}", path.display()))
        .and_then(|()| {
            file.sync_data()
                .with_context(|| format!("Failed to sync plan body {}", path.display()))
        });
    let lock_unlock =
        FileExt::unlock(&lock).with_context(|| format!("Failed to unlock {}", lock_path.display()));
    let body_unlock = FileExt::unlock(&file)
        .with_context(|| format!("Failed to unlock plan body {}", path.display()));
    match (result, lock_unlock, body_unlock) {
        (Ok(()), Ok(()), Ok(())) => Ok(()),
        (Err(error), _, _) | (Ok(()), Err(error), _) | (Ok(()), Ok(()), Err(error)) => Err(error),
    }
}

pub(crate) fn read_plan_body(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanBodyRead> {
    validate_plan_id(plan_id)?;
    ensure_status_collection_active(cancelled)?;
    let path = plan_body_path(ctx, plan_id)?;
    let directory = open_plan_directory(ctx.root(), false, cancelled)?.ok_or_else(|| {
        plan_error(
            PlanFileErrorKind::NotFound,
            format!("Plan body directory does not exist for {}", path.display()),
        )
    })?;
    ensure_status_collection_active(cancelled)?;
    let mut options = regular_options(false, false, false);
    options.read(true);
    let file = open_regular(&directory, &plan_body_name(plan_id), &mut options, &path)?;
    read_plan_body_file(file, &path, cancelled)
}

fn read_plan_body_file(
    mut file: File,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanBodyRead> {
    let lock_deadline = std::time::Instant::now() + PLAN_BODY_LOCK_WAIT_LIMIT;
    loop {
        ensure_status_collection_active(cancelled)?;
        match FileExt::try_lock_shared(&file) {
            Ok(true) => break,
            Ok(false) if std::time::Instant::now() >= lock_deadline => {
                return Err(plan_error(
                    PlanFileErrorKind::Read,
                    format!(
                        "Timed out waiting for a shared lock on plan body {}",
                        path.display()
                    ),
                ));
            }
            Ok(false) => std::thread::sleep(PLAN_BODY_LOCK_RETRY_DELAY),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(plan_error(
                    PlanFileErrorKind::Read,
                    format!(
                        "Failed to shared-lock plan body {}: {error}",
                        path.display()
                    ),
                ));
            }
        }
    }
    let file_len = file
        .metadata()
        .map_err(|error| {
            plan_error(
                PlanFileErrorKind::Read,
                format!("Failed to inspect plan body {}: {error}", path.display()),
            )
        })?
        .len();
    let read_len = usize::try_from(file_len.min(PLAN_BODY_INPUT_BYTES as u64))
        .expect("bounded plan body length fits usize");
    let mut bytes = Vec::with_capacity(read_len);
    let mut chunk = [0_u8; PLAN_BODY_READ_CHUNK];
    while bytes.len() < read_len {
        ensure_status_collection_active(cancelled)?;
        let requested = (read_len - bytes.len()).min(chunk.len());
        let read = file.read(&mut chunk[..requested]).map_err(|error| {
            plan_error(
                PlanFileErrorKind::Read,
                format!("Failed to read plan body {}: {error}", path.display()),
            )
        })?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        ensure_status_collection_active(cancelled)?;
    }

    let read_result = complete_utf8_prefix(&bytes, file_len, path).map(|complete_prefix| {
        let text = std::str::from_utf8(complete_prefix).expect("prefix was validated as UTF-8");
        let mut chars = text.chars();
        let visible = chars
            .by_ref()
            .take(PLAN_BODY_VISIBLE_CHARS)
            .collect::<String>();
        let truncated = chars.next().is_some() || file_len > complete_prefix.len() as u64;
        PlanBodyRead {
            text: visible,
            truncated,
        }
    });
    let unlock_result = FileExt::unlock(&file).map_err(|error| {
        plan_error(
            PlanFileErrorKind::Read,
            format!("Failed to unlock plan body {}: {error}", path.display()),
        )
    });
    match (read_result, unlock_result) {
        (Ok(body), Ok(())) => Ok(body),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn complete_utf8_prefix<'a>(bytes: &'a [u8], file_len: u64, path: &Path) -> Result<&'a [u8]> {
    if file_len <= PLAN_BODY_PREFIX_BYTES as u64 {
        return std::str::from_utf8(bytes).map(|_| bytes).map_err(|error| {
            plan_error(
                PlanFileErrorKind::InvalidUtf8,
                format!(
                    "Plan body {} stream did not contain valid UTF-8: {error}",
                    path.display()
                ),
            )
        });
    }

    if let Err(error) = std::str::from_utf8(bytes)
        && error.valid_up_to() < PLAN_BODY_PREFIX_BYTES
    {
        return Err(plan_error(
            PlanFileErrorKind::InvalidUtf8,
            format!(
                "Plan body {} stream did not contain valid UTF-8: {error}",
                path.display()
            ),
        ));
    }
    let mut end = bytes.len().min(PLAN_BODY_PREFIX_BYTES);
    while std::str::from_utf8(&bytes[..end]).is_err() {
        end = end.checked_sub(1).ok_or_else(|| {
            plan_error(
                PlanFileErrorKind::InvalidUtf8,
                format!("Plan body {} has no valid UTF-8 prefix", path.display()),
            )
        })?;
    }
    Ok(&bytes[..end])
}

fn missing_body_append_error(error: anyhow::Error, path: &Path) -> anyhow::Error {
    if error
        .downcast_ref::<PlanFileError>()
        .is_some_and(|error| error.kind() == PlanFileErrorKind::NotFound)
    {
        return plan_error(
            PlanFileErrorKind::NotFound,
            format!(
                "Plan body {} is missing; restore the original body before appending so prior progress is not silently lost",
                path.display()
            ),
        );
    }
    error
}

fn plan_body_name(plan_id: &str) -> OsString {
    OsString::from(format!("{plan_id}.md"))
}

fn plan_lock_name(plan_id: &str) -> OsString {
    OsString::from(format!("{plan_id}.md.lock"))
}

fn regular_options(writable: bool, create: bool, create_new: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .write(writable)
        .create(create)
        .create_new(create_new)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options
}

fn open_regular(
    directory: &Dir,
    name: &OsStr,
    options: &mut OpenOptions,
    path: &Path,
) -> Result<File> {
    let file = directory
        .open_with(name, options)
        .map(cap_std::fs::File::into_std)
        .map_err(|error| classify_open_error(error, path))?;
    let metadata = file.metadata().map_err(|error| {
        plan_error(
            PlanFileErrorKind::UnsafeType,
            format!("Failed to verify plan file {}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(plan_error(
            PlanFileErrorKind::UnsafeType,
            format!("Plan path is not a regular file: {}", path.display()),
        ));
    }
    Ok(file)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_plan_directory(
    root: &Path,
    create: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Dir>> {
    open_plan_directory_with_hook(root, create, cancelled, |_, _| {})
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_plan_directory_with_hook(
    root: &Path,
    create: bool,
    cancelled: &dyn Fn() -> bool,
    mut after_create: impl FnMut(&Path, &OsStr),
) -> Result<Option<Dir>> {
    ensure_status_collection_active(cancelled)?;
    let mut directory = Dir::open_ambient_dir(root, ambient_authority())
        .with_context(|| format!("Failed to open repository root {}", root.display()))?;
    let mut opened = root.to_path_buf();
    for name in [OsStr::new(".agent"), OsStr::new("plans")] {
        ensure_status_collection_active(cancelled)?;
        opened.push(name);
        directory = match directory.open_dir_nofollow(name) {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match directory.create_dir(name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(plan_error(
                            PlanFileErrorKind::UnsafePath,
                            format!(
                                "Failed to create plan directory {}: {error}",
                                opened.display()
                            ),
                        ));
                    }
                }
                after_create(&opened, name);
                ensure_status_collection_active(cancelled)?;
                directory.open_dir_nofollow(name).map_err(|error| {
                    plan_error(
                        PlanFileErrorKind::UnsafePath,
                        format!(
                            "Failed to open plan directory {} without following links: {error}",
                            opened.display()
                        ),
                    )
                })?
            }
            Err(error) => {
                return Err(plan_error(
                    PlanFileErrorKind::UnsafePath,
                    format!(
                        "Failed to open plan directory {} without following links: {error}",
                        opened.display()
                    ),
                ));
            }
        };
    }
    Ok(Some(directory))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_plan_directory(
    _root: &Path,
    _create: bool,
    _cancelled: &dyn Fn() -> bool,
) -> Result<Option<Dir>> {
    Err(plan_error(
        PlanFileErrorKind::UnsupportedPlatform,
        "Safe plan-body access is supported only on Linux and macOS",
    ))
}

fn classify_open_error(error: io::Error, path: &Path) -> anyhow::Error {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => PlanFileErrorKind::NotFound,
        _ => PlanFileErrorKind::UnsafePath,
    };
    plan_error(
        kind,
        format!(
            "Failed to open plan file {} without following links: {error}",
            path.display()
        ),
    )
}

fn plan_error(kind: PlanFileErrorKind, message: impl Into<String>) -> anyhow::Error {
    PlanFileError {
        kind,
        message: message.into(),
    }
    .into()
}

#[cfg(test)]
mod tests;
