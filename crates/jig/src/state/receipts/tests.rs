use super::*;
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::io::Read;
use tempfile::tempdir;

#[test]
fn receipt_gzip_export_preserves_selected_raw_records_and_unknown_fields() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("receipts.jsonl");
    let destination = temp.path().join("export/receipts.jsonl.gz");
    let second_destination = temp.path().join("export/receipts-copy.jsonl.gz");
    let old = raw_receipt("receipt_old", 10, r#","future":{"nested":true}"#);
    let new = raw_receipt("receipt_new", 100, "");
    let source_bytes = format!("{old}\n{new}\n");
    fs::write(&source, &source_bytes).unwrap();

    let artifact =
        write_receipt_gzip(&source, &destination, |receipt| receipt.ended_at_ms < 50).unwrap();
    let second_artifact = write_receipt_gzip(&source, &second_destination, |receipt| {
        receipt.ended_at_ms < 50
    })
    .unwrap();

    assert_eq!(artifact.receipt_count, 1);
    assert_eq!(artifact.uncompressed_bytes, (old.len() + 1) as u64);
    assert_eq!(fs::read_to_string(&source).unwrap(), source_bytes);
    let mut decoded = String::new();
    GzDecoder::new(File::open(&destination).unwrap())
        .read_to_string(&mut decoded)
        .unwrap();
    assert_eq!(decoded, format!("{old}\n"));
    assert_eq!(
        artifact.sha256,
        sha256_reader(File::open(&destination).unwrap()).unwrap()
    );
    assert_eq!(
        artifact.content_sha256,
        sha256_reader(std::io::Cursor::new(decoded.as_bytes())).unwrap()
    );
    assert_eq!(artifact.sha256, second_artifact.sha256);
    assert_eq!(
        fs::read(destination).unwrap(),
        fs::read(second_destination).unwrap()
    );
}

#[test]
fn receipt_gzip_export_refuses_to_replace_an_existing_output() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("receipts.jsonl");
    let destination = temp.path().join("receipts.jsonl.gz");
    fs::write(&source, format!("{}\n", raw_receipt("receipt_old", 10, ""))).unwrap();
    fs::write(&destination, "keep me").unwrap();

    let error = write_receipt_gzip(&source, &destination, |_| true)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Refusing to replace existing receipt export"));
    assert_eq!(fs::read_to_string(destination).unwrap(), "keep me");
}

fn raw_receipt(id: &str, ended_at_ms: u64, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","session_id":null,"plan_id":null,"tool_name":"jig.test","args":{{}},"started_at_ms":0,"ended_at_ms":{ended_at_ms},"exit_status":0,"stdout_preview":"","stderr_preview":"","changed_paths":[],"diff_stat":{{"files":0,"insertions":0,"deletions":0}}{extra}}}"#
    )
}

#[test]
fn legacy_check_receipt_fields_are_ignored_on_read() {
    let receipt: ReceiptRecord = serde_json::from_str(&raw_receipt(
        "legacy",
        42,
        r#","run_id":"run_legacy","target":"api:test","config_digest":"sha256:a","input_digest":"sha256:b","findings":[],"finding_count":0,"findings_truncated":false,"evaluated_at_ms":1,"valid_until_ms":2,"target_freshness":{"status":"complete"}"#,
    ))
    .unwrap();
    assert_eq!(receipt.id, "legacy");
    assert_eq!(receipt.run_id.as_deref(), Some("run_legacy"));
    assert_eq!(receipt.ended_at_ms, 42);
}
