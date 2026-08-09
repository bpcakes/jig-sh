use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;

use crate::state::now_ms;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_atomic_text(path: PathBuf, contents: &str, fallback_name: &str) -> Result<()> {
    let tmp = temp_path(&path, fallback_name);
    let mut file = create_new_file(&tmp, 0o600)?;
    file.write_all(contents.as_bytes())?;
    file.sync_data()?;
    drop(file);
    replace_file(&tmp, &path, fallback_name)
}

pub(crate) fn create_new_file(path: &Path, unix_mode: u32) -> Result<File> {
    #[cfg(unix)]
    {
        Ok(OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(unix_mode)
            .custom_flags(libc::O_NOFOLLOW | libc::O_EXCL)
            .open(path)?)
    }
    #[cfg(not(unix))]
    {
        let _ = unix_mode;
        Ok(File::create_new(path)?)
    }
}

pub(crate) fn open_read_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file is not a regular file",
        ));
    }
    Ok(file)
}

pub(crate) fn read_text_no_follow(path: &Path) -> io::Result<Option<String>> {
    let mut file = match open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}

pub(crate) fn temp_path(path: &Path, fallback_name: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback_name);
    // `create_new_file` fails instead of replacing an unexpected collision;
    // callers treat that as a conservative write failure.
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(
        "{file_name}.{}.{}.{}.tmp",
        std::process::id(),
        now_ms(),
        counter
    ))
}

pub(crate) fn replace_file(tmp: &Path, path: &Path, fallback_name: &str) -> Result<()> {
    replace_file_inner(tmp, path, fallback_name)?;
    sync_parent_dir(path)
}

fn replace_file_inner(tmp: &Path, path: &Path, fallback_name: &str) -> Result<()> {
    match fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(error) if cfg!(windows) && error.kind() == io::ErrorKind::AlreadyExists => {
            replace_existing_file_windows(tmp, path, fallback_name)
        }
        Err(error) => Err(error.into()),
    }
}

fn replace_existing_file_windows(tmp: &Path, path: &Path, fallback_name: &str) -> Result<()> {
    #[cfg(windows)]
    {
        replace_existing_file_windows_native(tmp, path, fallback_name)
    }
    #[cfg(not(windows))]
    {
        let _ = (tmp, path, fallback_name);
        unreachable!("replace_existing_file_windows is only called on Windows");
    }
}

#[cfg(windows)]
fn replace_existing_file_windows_native(
    tmp: &Path,
    path: &Path,
    fallback_name: &str,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let backup = backup_path(path, fallback_name);
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    let path_wide = wide_path(path);
    let tmp_wide = wide_path(tmp);
    let backup_wide = wide_path(&backup);
    let replaced = unsafe {
        // SAFETY: The UTF-16 buffers are nul-terminated and live for the entire
        // call. ReplaceFileW atomically swaps an existing target with the temp
        // file and writes a best-effort backup without leaving a missing-file
        // gap for readers.
        ReplaceFileW(
            path_wide.as_ptr(),
            tmp_wide.as_ptr(),
            backup_wide.as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if replaced == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
pub(crate) fn backup_path(path: &Path, fallback_name: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback_name);
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(
        "{file_name}.{}.{}.{}.replace-backup",
        std::process::id(),
        now_ms(),
        counter
    ))
}

pub(crate) fn replace_backup_for_path_exists(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    fs::read_dir(parent).is_ok_and(|entries| {
        entries.filter_map(std::result::Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(replace_backup_parts)
                .is_some_and(|(original_name, _)| original_name == file_name)
        })
    })
}

pub(crate) fn replace_backup_parts(file_name: &str) -> Option<(&str, &str)> {
    let stem = file_name.strip_suffix(".replace-backup")?;
    let mut parts = stem.rsplitn(4, '.');
    let counter = parts.next()?;
    let millis = parts.next()?;
    let pid = parts.next()?;
    let original_name = parts.next()?;
    if original_name.is_empty()
        || !pid.bytes().all(|byte| byte.is_ascii_digit())
        || !millis.bytes().all(|byte| byte.is_ascii_digit())
        || !counter.bytes().all(|byte| byte.is_ascii_digit())
    {
        return legacy_replace_backup_parts(stem);
    }
    Some((original_name, pid))
}

fn legacy_replace_backup_parts(stem: &str) -> Option<(&str, &str)> {
    let (original_name, pid) = stem.rsplit_once('.')?;
    if original_name.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((original_name, pid))
}

fn sync_parent_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
