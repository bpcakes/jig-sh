use std::ffi::OsString;
#[cfg(unix)]
use std::fs::{File, Permissions};
#[cfg(unix)]
use std::io::{Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
use anyhow::Context;
use anyhow::Result as AnyResult;
#[cfg(not(unix))]
use anyhow::bail;

use super::ResolvedBrokeredFile;

pub(super) struct BrokeredSecretFiles {
    // Secret files intentionally live on disk while the child runs. TempDir
    // cleanup removes them on normal unwind; hard process kills can leave them
    // behind for OS temp cleanup. Drop runs before field drops, so keep `_dir`
    // before `files`: the explicit Drop wipe uses the retained file handles,
    // then TempDir removes the persisted paths during field drop.
    _dir: tempfile::TempDir,
    env: Vec<(String, OsString)>,
    #[cfg(unix)]
    files: Vec<(OsString, File)>,
}

#[cfg(unix)]
impl Drop for BrokeredSecretFiles {
    fn drop(&mut self) {
        for (path, file) in &mut self.files {
            wipe_secret_file_best_effort(file, std::path::Path::new(path));
        }
    }
}

impl BrokeredSecretFiles {
    pub(super) fn create(files: &[ResolvedBrokeredFile]) -> AnyResult<Option<Self>> {
        if files.is_empty() {
            return Ok(None);
        }

        #[cfg(not(unix))]
        {
            bail!(
                "vault run --file mapping '{}={}' requires Unix-style owner-only temporary files; use --env on this platform",
                files[0].var.as_str(),
                files[0].secret_name.as_str()
            );
        }

        #[cfg(unix)]
        {
            let dir = tempfile::Builder::new()
                .prefix("jig-vault-run-")
                .permissions(Permissions::from_mode(0o700))
                .tempdir()
                .context("failed to create vault secret file temp dir")?;
            let mut env = Vec::with_capacity(files.len());
            let mut persisted_files = Vec::with_capacity(files.len());
            for mapping in files {
                // tempfile uses mkstemp on Unix and creates owner-only files;
                // keep the random path so the child can read it until TempDir cleanup.
                let mut secret_file = tempfile::Builder::new()
                    .prefix("secret-")
                    .tempfile_in(dir.path())
                    .with_context(|| {
                        format!(
                            "failed to create brokered temp file for vault secret '{}'",
                            mapping.secret_name.as_str()
                        )
                    })?;
                let path = secret_file.path().to_path_buf();
                write_secret_file(secret_file.as_file_mut(), &path, mapping.value.as_slice())
                    .with_context(|| {
                        format!(
                            "failed to write vault secret '{}' to a brokered temp file",
                            mapping.secret_name.as_str()
                        )
                    })?;
                // `keep` gives the child a stable path; the owning TempDir still
                // removes the persisted file tree when the brokered run ends.
                let (file, path) = secret_file.keep().with_context(|| {
                    format!(
                        "failed to persist brokered temp file for vault secret '{}'",
                        mapping.secret_name.as_str(),
                    )
                })?;
                let path = path.into_os_string();
                env.push((mapping.var.as_str().to_string(), path.clone()));
                persisted_files.push((path, file));
            }
            Ok(Some(Self {
                _dir: dir,
                env,
                files: persisted_files,
            }))
        }
    }

    pub(super) fn env(&self) -> &[(String, OsString)] {
        &self.env
    }
}

#[cfg(unix)]
fn write_secret_file(file: &mut File, path: &std::path::Path, value: &[u8]) -> AnyResult<()> {
    file.write_all(value)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))
}

#[cfg(unix)]
fn wipe_secret_file_best_effort(file: &mut File, path: &std::path::Path) {
    if let Err(error) = wipe_secret_file(file, path) {
        eprintln!(
            "jig vault could not wipe brokered temp secret file {} before cleanup: {error:#}",
            path.display()
        );
    }
}

#[cfg(unix)]
pub(super) fn wipe_secret_file(file: &mut File, path: &std::path::Path) -> AnyResult<()> {
    let len = file.seek(SeekFrom::End(0)).with_context(|| {
        format!(
            "failed to measure brokered temp secret file {}",
            path.display()
        )
    })?;
    file.rewind().with_context(|| {
        format!(
            "failed to seek brokered temp secret file {}",
            path.display()
        )
    })?;
    let zeros = [0_u8; 8192];
    let mut remaining = len;
    while remaining > 0 {
        let chunk_len = remaining.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..chunk_len]).with_context(|| {
            format!(
                "failed to wipe brokered temp secret file {}",
                path.display()
            )
        })?;
        remaining -= chunk_len as u64;
    }
    file.sync_all().with_context(|| {
        format!(
            "failed to sync wiped brokered temp secret file {}",
            path.display()
        )
    })
}
