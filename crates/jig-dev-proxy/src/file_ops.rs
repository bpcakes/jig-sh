use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
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
    replace_file(&tmp, &path)
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

pub(crate) fn replace_file(tmp: &Path, path: &Path) -> Result<()> {
    fs::rename(tmp, path)?;
    sync_parent_dir(path)
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
