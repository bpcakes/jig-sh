use std::ffi::OsStr;

use tempfile::tempdir;

use super::StateDirectory;

#[test]
fn cache_and_durable_writes_reject_state_larger_than_the_read_limit() {
    let temp = tempdir().unwrap();
    let cache = StateDirectory::open(temp.path(), temp.path()).unwrap();
    let value = serde_json::json!({
        "payload": "x".repeat(super::super::bounded_json::MAX_LOOP_STATE_BYTES as usize)
    });

    for durable in [false, true] {
        let name = if durable {
            OsStr::new("durable.json")
        } else {
            OsStr::new("cache.json")
        };
        let path = temp.path().join(name);
        let error = if durable {
            cache.write_json_durable(name, &path, &value).unwrap_err()
        } else {
            cache.write_json(name, &path, &value).unwrap_err()
        };

        assert!(error.to_string().contains("8388608-byte safety limit"));
        assert!(!path.exists());
    }
}
