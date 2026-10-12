use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};

use super::ensure_status_active;

pub(super) const MAX_LOOP_STATE_BYTES: u64 = 8 * 1024 * 1024;
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

pub(super) fn encode_bounded_json<T: Serialize>(value: &T, path: &Path) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(value).context("Failed to encode loop state JSON")?;
    require_state_size_within_limit(path, bytes.len() as u64)?;
    Ok(bytes)
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
            "Loop coordination state {} exceeds the {MAX_LOOP_STATE_BYTES}-byte safety limit; stop loop dispatchers and inspect or repair this file before retrying",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
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

        assert!(error.to_string().contains("8388608-byte safety limit"));
    }

    #[test]
    fn loop_state_reader_rejects_a_preexisting_twelve_mib_file_without_allocating_it() {
        let mut source = Cursor::new(b"{}".to_vec());

        let error = read_bounded_json_from::<Value>(
            &mut source,
            Path::new("attempts.json"),
            12 * 1024 * 1024,
            &|| false,
        )
        .unwrap_err();

        assert!(error.to_string().contains("8388608-byte safety limit"));
        assert!(error.to_string().contains("inspect or repair"));
    }

    #[test]
    fn loop_state_reader_rejects_growth_past_the_limit() {
        let bytes = valid_json_with_size(MAX_LOOP_STATE_BYTES as usize + 1);
        let mut source = Cursor::new(bytes);

        let error =
            read_bounded_json_from::<Value>(&mut source, Path::new("schedule.json"), 2, &|| false)
                .unwrap_err();

        assert!(error.to_string().contains("8388608-byte safety limit"));
    }

    #[test]
    fn loop_state_reader_polls_cancellation_between_chunks() {
        let bytes = valid_json_with_size(LOOP_STATE_READ_CHUNK_BYTES * 8);
        let mut source = Cursor::new(bytes);
        let checks = Cell::new(0_usize);

        let error = read_bounded_json_from::<Value>(
            &mut source,
            Path::new("attempts.json"),
            (LOOP_STATE_READ_CHUNK_BYTES * 8) as u64,
            &|| {
                checks.set(checks.get() + 1);
                checks.get() > 3
            },
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "status collection was cancelled");
        assert!(checks.get() > 3);
    }

    #[test]
    fn loop_state_encoder_rejects_output_over_the_read_limit() {
        let value = serde_json::json!({ "payload": "x".repeat(MAX_LOOP_STATE_BYTES as usize) });

        let error = encode_bounded_json(&value, Path::new("attempts.json")).unwrap_err();

        assert!(error.to_string().contains("8388608-byte safety limit"));
    }
}
