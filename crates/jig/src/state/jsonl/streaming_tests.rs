use std::cell::Cell;
use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;

#[test]
fn raw_scanner_visits_one_record_at_a_time_and_reports_exact_stats() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let first = serde_json::to_vec(&json!({ "payload": "x".repeat(96 * 1024) })).unwrap();
    let final_record =
        serde_json::to_vec(&json!({ "id": 2, "payload": "y".repeat(64 * 1024) })).unwrap();
    let mut source = first.clone();
    source.extend_from_slice(b"\n \t\n");
    source.extend_from_slice(&final_record);
    fs::write(&path, &source).unwrap();
    let mut visited = Vec::new();

    let stats = scan_jsonl_raw(&path, &|| false, |record| {
        visited.push((
            record.line_number,
            record.bytes.len(),
            record.bytes.len() as u64 + u64::from(record.terminated),
            record.terminated,
        ));
        Ok(())
    })
    .unwrap();

    assert_eq!(
        visited,
        vec![
            (1, first.len(), first.len() as u64 + 1, true),
            (3, final_record.len(), final_record.len() as u64, false,),
        ]
    );
    assert_eq!(stats.file_bytes, source.len() as u64);
    assert_eq!(stats.physical_lines, 3);
    assert_eq!(stats.records, 2);
    assert_eq!(stats.blank_lines, 1);
    assert_eq!(stats.max_line_bytes, first.len() as u64 + 1);
    assert_eq!(stats.max_line_number, Some(1));
    assert!(stats.unterminated_final_record);
}

#[test]
fn raw_scanner_checks_cancellation_while_building_a_large_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, vec![b'x'; JSONL_READ_CHUNK * 64]).unwrap();
    let checks = Cell::new(0usize);
    let visited = Cell::new(false);

    let error = scan_jsonl_raw(
        &path,
        &|| {
            let current = checks.get();
            checks.set(current + 1);
            current >= 12
        },
        |_| {
            visited.set(true);
            Ok(())
        },
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "status collection was cancelled");
    assert!(!visited.get());
    assert!(checks.get() > 12);
}

#[test]
fn raw_rewrite_preserves_kept_bytes_and_publishes_validated_output() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let first = br#"{"id":1,"unknown":{"nested":true}}   "#;
    let mut source = first.to_vec();
    source.extend_from_slice(b"\n\n{\"id\":2}\n{\"id\":3}\n");
    fs::write(&path, &source).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    }

    let stats = with_jsonl_write_lock(&path, |guard| {
        rewrite_jsonl_raw_locked(
            guard,
            &path,
            &|| false,
            |record| {
                let value: Value = serde_json::from_slice(record.bytes)?;
                Ok(match value["id"].as_u64().unwrap() {
                    1 => RawJsonlRewrite::Keep,
                    2 => RawJsonlRewrite::Replace(br#"{"id":2,"changed":true}"#.to_vec()),
                    3 => RawJsonlRewrite::Drop,
                    _ => unreachable!(),
                })
            },
            |temp_path| {
                let values = read_jsonl::<Value>(temp_path)?;
                assert_eq!(values.len(), 2);
                assert_eq!(values[1], json!({ "id": 2, "changed": true }));
                Ok(())
            },
        )
    })
    .unwrap();

    let mut expected = first.to_vec();
    expected.extend_from_slice(b"\n\n{\"id\":2,\"changed\":true}\n");
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(stats.input.records, 3);
    assert_eq!(stats.output.records, 2);
    assert_eq!(stats.kept_records, 1);
    assert_eq!(stats.replaced_records, 1);
    assert_eq!(stats.dropped_records, 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[test]
fn raw_rewrite_validation_failure_leaves_source_exactly_unchanged() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let source = b"{\"id\":1}\n".to_vec();
    fs::write(&path, &source).unwrap();

    let error = with_jsonl_write_lock(&path, |guard| {
        rewrite_jsonl_raw_locked(
            guard,
            &path,
            &|| false,
            |_| Ok(RawJsonlRewrite::Replace(br#"{"id":2}"#.to_vec())),
            |_| bail!("semantic validator rejected output"),
        )
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Rewritten JSONL validation failed")
    );
    assert_eq!(fs::read(&path).unwrap(), source);
}

#[test]
fn raw_rewrite_rejects_malformed_input_and_unterminated_tail_without_mutation() {
    for source in [
        b"{\"id\":1}\nnot-json\n".as_slice(),
        b"{\"id\":1}\n{\"id\":2}".as_slice(),
    ] {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        fs::write(&path, source).unwrap();

        let error = with_jsonl_write_lock(&path, |guard| {
            rewrite_jsonl_raw_locked(
                guard,
                &path,
                &|| false,
                |_| Ok(RawJsonlRewrite::Keep),
                |_| Ok(()),
            )
        })
        .unwrap_err();

        assert_eq!(fs::read(&path).unwrap(), source);
        assert!(
            error
                .to_string()
                .contains("Failed to parse source JSONL record")
                || error.to_string().contains("not newline-terminated"),
            "{error:#}"
        );
    }
}
