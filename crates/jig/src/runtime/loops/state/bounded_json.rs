use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;

use super::ensure_status_active;

pub(super) const MAX_LOOP_STATE_BYTES: u64 = 16 * 1024 * 1024;
const LOOP_STATE_READ_CHUNK_BYTES: usize = 64 * 1024;

pub(super) fn read_bounded_json<T>(
    file: &mut File,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let initial_len = file
        .metadata()
        .with_context(|| format!("Failed to inspect {}", path.display()))?
        .len();
    read_bounded_json_from(file, path, initial_len, cancelled)
}

fn read_bounded_json_from<T>(
    source: &mut impl Read,
    path: &Path,
    initial_len: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<T>
where
    T: DeserializeOwned,
{
    require_state_size_within_limit(path, initial_len)?;
    let mut bytes = Vec::with_capacity(initial_len as usize);
    let mut chunk = vec![0_u8; LOOP_STATE_READ_CHUNK_BYTES].into_boxed_slice();
    loop {
        ensure_status_active(cancelled)?;
        let read = source
            .read(&mut chunk)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        let observed_len = bytes.len().saturating_add(read) as u64;
        require_state_size_within_limit(path, observed_len)?;
        bytes.extend_from_slice(&chunk[..read]);
    }
    ensure_status_active(cancelled)?;
    serde_json::from_slice(&bytes).with_context(|| format!("Failed to parse {}", path.display()))
}

fn require_state_size_within_limit(path: &Path, bytes: u64) -> Result<()> {
    if bytes > MAX_LOOP_STATE_BYTES {
        bail!(
            "Loop coordination state {} exceeds the {MAX_LOOP_STATE_BYTES}-byte read limit",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::Value;

    use super::*;

    fn valid_json_with_size(size: usize) -> Vec<u8> {
        let mut bytes = b"{}".to_vec();
        bytes.resize(size, b' ');
        bytes
    }

    #[test]
    fn loop_state_reader_accepts_the_exact_byte_limit() {
        let bytes = valid_json_with_size(MAX_LOOP_STATE_BYTES as usize);
        let mut source = Cursor::new(bytes);

        let value = read_bounded_json_from::<Value>(
            &mut source,
            Path::new("attempts.json"),
            MAX_LOOP_STATE_BYTES,
            &|| false,
        )
        .unwrap();

        assert_eq!(value, serde_json::json!({}));
    }

    #[test]
    fn loop_state_reader_rejects_initial_size_over_the_limit() {
        let mut source = Cursor::new(b"{}".to_vec());

        let error = read_bounded_json_from::<Value>(
            &mut source,
            Path::new("leases.json"),
            MAX_LOOP_STATE_BYTES + 1,
            &|| false,
        )
        .unwrap_err();

        assert!(error.to_string().contains("16777216-byte read limit"));
    }

    #[test]
    fn loop_state_reader_rejects_growth_past_the_limit() {
        let bytes = valid_json_with_size(MAX_LOOP_STATE_BYTES as usize + 1);
        let mut source = Cursor::new(bytes);

        let error =
            read_bounded_json_from::<Value>(&mut source, Path::new("schedule.json"), 2, &|| false)
                .unwrap_err();

        assert!(error.to_string().contains("16777216-byte read limit"));
    }
}
