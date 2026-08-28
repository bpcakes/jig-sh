//! Deterministic gzip streams used by local state backups and exports.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::GzBuilder;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GzipWriteReport {
    pub(super) uncompressed_bytes: u64,
    pub(super) compressed_bytes: u64,
    pub(super) uncompressed_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GzipReadReport {
    pub(super) uncompressed_bytes: u64,
    pub(super) uncompressed_sha256: String,
}

pub(super) fn write_gzip_atomic(
    destination: &Path,
    producer: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<GzipWriteReport> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    create_dir_all_synced(parent)?;
    if destination.exists() {
        bail!(
            "Refusing to replace existing gzip output {}",
            destination.display()
        );
    }

    let temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create gzip output in {}", parent.display()))?;
    let encoder = GzBuilder::new()
        .mtime(0)
        .write(temp, Compression::default());
    let mut writer = DigestWriter::new(encoder);
    producer(&mut writer)?;
    let (encoder, uncompressed_bytes, uncompressed_sha256) = writer.finish();
    let mut temp = encoder
        .finish()
        .context("Failed to finish gzip state output")?;
    temp.as_file_mut()
        .sync_all()
        .context("Failed to sync gzip state output")?;
    let compressed_bytes = temp
        .as_file()
        .metadata()
        .context("Failed to inspect gzip state output")?
        .len();
    temp.persist_noclobber(destination)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to publish {}", destination.display()))?;
    sync_directory(parent)?;

    Ok(GzipWriteReport {
        uncompressed_bytes,
        compressed_bytes,
        uncompressed_sha256,
    })
}

pub(super) fn gzip_file_atomic(source: &Path, destination: &Path) -> Result<GzipWriteReport> {
    let mut source_file =
        File::open(source).with_context(|| format!("Failed to open {}", source.display()))?;
    let report = write_gzip_atomic(destination, |writer| {
        io::copy(&mut source_file, writer)
            .with_context(|| format!("Failed to back up {}", source.display()))?;
        Ok(())
    })?;
    let validation = verify_gzip_file(destination, Some(report.uncompressed_bytes)).and_then(
        |restored_report| {
            if restored_report.uncompressed_bytes == report.uncompressed_bytes
                && restored_report.uncompressed_sha256 == report.uncompressed_sha256
            {
                Ok(())
            } else {
                bail!(
                    "Gzip verification failed for {}; refusing to use the backup",
                    destination.display()
                );
            }
        },
    );
    match validation {
        Ok(()) => Ok(report),
        Err(error) => {
            remove_invalid_gzip(destination).with_context(|| {
                format!(
                    "{error:#}; additionally failed to remove invalid gzip artifact {}",
                    destination.display()
                )
            })?;
            Err(error)
        }
    }
}

pub(super) fn decompress_gzip_to_temp(
    source: &Path,
    destination_dir: &Path,
    maximum_uncompressed_bytes: Option<u64>,
) -> Result<(NamedTempFile, GzipReadReport)> {
    let source_file =
        File::open(source).with_context(|| format!("Failed to open {}", source.display()))?;
    let mut decoder = GzDecoder::new(source_file);
    let mut temp = NamedTempFile::new_in(destination_dir).with_context(|| {
        format!(
            "Failed to create restored state file in {}",
            destination_dir.display()
        )
    })?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = decoder
            .read(&mut buffer)
            .with_context(|| format!("Failed to decompress {}", source.display()))?;
        if read == 0 {
            break;
        }
        if maximum_uncompressed_bytes.is_some_and(|maximum| {
            bytes
                .checked_add(read as u64)
                .is_none_or(|next| next > maximum)
        }) {
            bail!(
                "Refusing to decompress {} beyond the expected {} bytes",
                source.display(),
                maximum_uncompressed_bytes.unwrap_or(0)
            );
        }
        temp.write_all(&buffer[..read])
            .context("Failed to write restored state file")?;
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    temp.as_file_mut()
        .sync_all()
        .context("Failed to sync restored state file")?;

    Ok((
        temp,
        GzipReadReport {
            uncompressed_bytes: bytes,
            uncompressed_sha256: digest_hex(&hasher.finalize()),
        },
    ))
}

pub(super) fn verify_gzip_file(
    source: &Path,
    maximum_uncompressed_bytes: Option<u64>,
) -> Result<GzipReadReport> {
    let source_file =
        File::open(source).with_context(|| format!("Failed to open {}", source.display()))?;
    let mut decoder = GzDecoder::new(source_file);
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = decoder
            .read(&mut buffer)
            .with_context(|| format!("Failed to decompress {}", source.display()))?;
        if read == 0 {
            break;
        }
        if maximum_uncompressed_bytes.is_some_and(|maximum| {
            bytes
                .checked_add(read as u64)
                .is_none_or(|next| next > maximum)
        }) {
            bail!(
                "Refusing to verify {} beyond the expected {} bytes",
                source.display(),
                maximum_uncompressed_bytes.unwrap_or(0)
            );
        }
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    Ok(GzipReadReport {
        uncompressed_bytes: bytes,
        uncompressed_sha256: digest_hex(&hasher.finalize()),
    })
}

pub(super) fn sha256_file(path: &Path) -> Result<GzipReadReport> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    Ok(GzipReadReport {
        uncompressed_bytes: bytes,
        uncompressed_sha256: digest_hex(&hasher.finalize()),
    })
}

pub(super) fn create_dir_all_synced(path: &Path) -> Result<()> {
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        missing.push(cursor.to_path_buf());
        cursor = cursor.parent().with_context(|| {
            format!(
                "Cannot find an existing ancestor while creating {}",
                path.display()
            )
        })?;
    }
    for directory in missing.into_iter().rev() {
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to create {}", directory.display()));
            }
        }
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
    }
    Ok(())
}

pub(super) fn remove_invalid_gzip(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {
            sync_directory(path.parent().unwrap_or_else(|| Path::new(".")))?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to remove invalid {}", path.display()))
        }
    }
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<()> {
    let directory =
        File::open(path).with_context(|| format!("Failed to open directory {}", path.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("Failed to sync directory {}", path.display()))
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

struct DigestWriter<W> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
}

impl<W> DigestWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    fn finish(self) -> (W, u64, String) {
        (self.inner, self.bytes, digest_hex(&self.hasher.finalize()))
    }
}

impl<W: Write> Write for DigestWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        self.bytes = self.bytes.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn gzip_output_is_deterministic_and_round_trips() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("first.jsonl.gz");
        let second = temp.path().join("second.jsonl.gz");
        let payload = b"{\"id\":\"one\"}\n{\"id\":\"two\"}\n";

        let first_report = write_gzip_atomic(&first, |writer| {
            writer.write_all(payload)?;
            Ok(())
        })
        .unwrap();
        let second_report = write_gzip_atomic(&second, |writer| {
            writer.write_all(payload)?;
            Ok(())
        })
        .unwrap();

        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
        assert_eq!(first_report, second_report);
        let (restored, restored_report) =
            decompress_gzip_to_temp(&first, temp.path(), Some(payload.len() as u64)).unwrap();
        assert_eq!(fs::read(restored.path()).unwrap(), payload);
        assert_eq!(
            restored_report.uncompressed_sha256,
            first_report.uncompressed_sha256
        );
    }

    #[test]
    fn gzip_output_refuses_to_replace_existing_file() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("state.jsonl.gz");
        fs::write(&output, b"existing").unwrap();

        let error = write_gzip_atomic(&output, |_| Ok(())).unwrap_err();

        assert!(error.to_string().contains("Refusing to replace"));
        assert_eq!(fs::read(&output).unwrap(), b"existing");
    }

    #[test]
    fn bounded_decompression_stops_at_the_manifest_limit() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("large.jsonl.gz");
        let payload = vec![b'x'; 128 * 1024];
        write_gzip_atomic(&output, |writer| {
            writer.write_all(&payload)?;
            Ok(())
        })
        .unwrap();

        let error = decompress_gzip_to_temp(&output, temp.path(), Some(1024))
            .unwrap_err()
            .to_string();

        assert!(error.contains("beyond the expected 1024 bytes"));
    }
}
