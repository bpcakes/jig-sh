use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;

const WORKSPACE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn issue(id: &str) -> Value {
    json!({
        "id": id,
        "title": "Example task",
        "description": "Implement the example.",
        "acceptance_criteria": "The example passes.",
        "status": "open",
        "priority": 2,
        "issue_type": "task",
        "created_at": "2026-01-02T03:04:05Z",
        "updated_at": "2026-01-03T04:05:06.123456789Z",
        "assignee": "example-agent"
    })
}

fn write_export(root: &Path, relative: &str, records: &[Value]) {
    fs::create_dir_all(root.join(".beads")).unwrap();
    let mut body = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    fs::write(root.join(relative), body).unwrap();
}

#[test]
fn reads_current_export_and_returns_exact_normalized_issue() {
    let temp = tempdir().unwrap();
    write_export(temp.path(), PRIMARY_EXPORT, &[issue("example-123")]);

    let export = BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap();
    let task = export.issue("example-123").unwrap();

    assert_eq!(export.len(), 1);
    assert_eq!(export.relative_path(), Path::new(PRIMARY_EXPORT));
    assert_eq!(task.provider, "beads");
    assert_eq!(task.workspace_id, WORKSPACE_ID);
    assert_eq!(task.id, "example-123");
    assert_eq!(task.title, "Example task");
    assert_eq!(task.description, "Implement the example.");
    assert_eq!(task.acceptance_criteria, "The example passes.");
    assert_eq!(task.status, "open");
    assert_eq!(task.assignee.as_deref(), Some("example-agent"));
    assert_eq!(task.provider_revision, "2026-01-03T04:05:06.123456789Z");
    assert!(task.semantic_revision.starts_with("sha256:"));
}

#[test]
fn reads_legacy_export_only_when_selection_is_unambiguous() {
    let temp = tempdir().unwrap();
    write_export(temp.path(), LEGACY_EXPORT, &[issue("example-123")]);
    let export = BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap();
    assert_eq!(export.relative_path(), Path::new(LEGACY_EXPORT));

    fs::write(temp.path().join(PRIMARY_EXPORT), "").unwrap();
    assert_eq!(
        BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::AmbiguousExport
    );
}

#[test]
fn missing_export_and_unsafe_workspace_fail_closed() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join(".beads")).unwrap();
    assert_eq!(
        BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::MissingExport
    );

    let other = tempdir().unwrap();
    fs::write(other.path().join(".beads"), b"not a directory").unwrap();
    assert_eq!(
        BeadsExport::open(other.path(), WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::InvalidWorkspace
    );
}

#[cfg(unix)]
#[test]
fn symlinked_and_hard_linked_exports_are_not_repository_authority() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let outside = temp.path().join("outside.jsonl");
    fs::write(&outside, format!("{}\n", issue("example-123"))).unwrap();
    fs::create_dir(temp.path().join("repo")).unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(repo.join(".beads")).unwrap();
    symlink(&outside, repo.join(PRIMARY_EXPORT)).unwrap();
    assert_eq!(
        BeadsExport::open(&repo, WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::UnsafeExport
    );

    fs::remove_file(repo.join(PRIMARY_EXPORT)).unwrap();
    fs::hard_link(&outside, repo.join(PRIMARY_EXPORT)).unwrap();
    assert_eq!(
        BeadsExport::open(&repo, WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::UnsafeExport
    );
}

#[cfg(unix)]
#[test]
fn replacing_tracker_directory_cannot_redirect_a_pinned_snapshot() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let repository = temp.path().join("repository");
    let outside = temp.path().join("outside");
    fs::create_dir(&repository).unwrap();
    fs::create_dir(repository.join(".beads")).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join("issues.jsonl"),
        format!("{}\n", issue("outside-123")),
    )
    .unwrap();

    let result = BeadsExport::open_with_hooks(
        &repository,
        WORKSPACE_ID,
        || {
            fs::rename(repository.join(".beads"), repository.join(".beads-pinned")).unwrap();
            symlink(&outside, repository.join(".beads")).unwrap();
        },
        || {},
    );

    assert_eq!(result.unwrap_err(), BeadsJsonlError::MissingExport);
    assert!(repository.join(PRIMARY_EXPORT).is_file());
}

#[cfg(unix)]
#[test]
fn replacing_validated_export_with_fifo_is_nonblocking_and_rejected() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let repository = temp.path().join("repository");
    write_export(&repository, PRIMARY_EXPORT, &[issue("example-123")]);
    let fifo = repository.join(PRIMARY_EXPORT);
    let worker_repository = repository;
    let worker_fifo = fifo.clone();
    let (hook_sender, hook_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();

    let worker = std::thread::spawn(move || {
        let result = BeadsExport::open_with_hooks(
            &worker_repository,
            WORKSPACE_ID,
            || {},
            || {
                fs::remove_file(&worker_fifo).unwrap();
                let fifo_path = CString::new(worker_fifo.as_os_str().as_bytes()).unwrap();
                // SAFETY: `fifo_path` is a live NUL-terminated string and the
                // mode contains only ordinary permission bits.
                assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
                hook_sender.send(()).unwrap();
            },
        );
        result_sender.send(result).unwrap();
    });

    hook_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    let result = match result_receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Unblock a regressed blocking read-open so the test can cleanly
            // join its worker before reporting the failure.
            let _writer = fs::OpenOptions::new().write(true).open(&fifo).unwrap();
            let _ = result_receiver.recv_timeout(Duration::from_secs(2));
            worker.join().unwrap();
            panic!("opening a post-validation FIFO blocked instead of failing promptly");
        }
        Err(error) => panic!("tracker worker disconnected: {error}"),
    };
    worker.join().unwrap();

    assert_eq!(result.unwrap_err(), BeadsJsonlError::UnsafeExport);
}

#[test]
fn unknown_fields_are_validated_for_bounds_but_otherwise_tolerated() {
    let temp = tempdir().unwrap();
    let mut record = issue("example-123");
    record["future_field"] = json!({"nested": [true, 7, "supported"]});
    write_export(temp.path(), PRIMARY_EXPORT, &[record]);

    assert!(BeadsExport::open(temp.path(), WORKSPACE_ID).is_ok());
}

#[test]
fn duplicate_json_keys_and_issue_ids_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join(".beads")).unwrap();
    let duplicate_key = r#"{"id":"example-123","id":"example-456","title":"Example","status":"open","priority":2,"issue_type":"task","created_at":"2026-01-02T03:04:05Z","updated_at":"2026-01-02T03:04:05Z"}"#;
    fs::write(
        temp.path().join(PRIMARY_EXPORT),
        format!("{duplicate_key}\n"),
    )
    .unwrap();
    assert!(matches!(
        BeadsExport::open(temp.path(), WORKSPACE_ID),
        Err(BeadsJsonlError::InvalidRecord { line: 1, .. })
    ));

    write_export(
        temp.path(),
        PRIMARY_EXPORT,
        &[issue("example-123"), issue("example-123")],
    );
    assert_eq!(
        BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::DuplicateIssueId { line: 2 }
    );
}

#[test]
fn exact_lookup_distinguishes_missing_and_tombstoned_issues() {
    let temp = tempdir().unwrap();
    let mut tombstone = issue("example-deleted");
    tombstone["status"] = json!("tombstone");
    tombstone["deleted_at"] = json!("2026-01-04T05:06:07Z");
    write_export(temp.path(), PRIMARY_EXPORT, &[tombstone]);
    let export = BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap();

    assert_eq!(
        export.issue("example-deleted").unwrap_err(),
        BeadsJsonlError::IssueTombstoned
    );
    assert_eq!(
        export.issue("example-missing").unwrap_err(),
        BeadsJsonlError::IssueMissing
    );
    assert_eq!(
        export.issue("not valid").unwrap_err(),
        BeadsJsonlError::InvalidIssueId
    );
}

#[test]
fn malformed_known_fields_fail_without_echoing_private_values() {
    for (field, value) in [
        ("priority", json!(9)),
        ("status", json!("private-invalid-status")),
        ("issue_type", json!("private-invalid-type")),
        ("updated_at", json!("private-invalid-time")),
        ("assignee", json!(17)),
    ] {
        let temp = tempdir().unwrap();
        let mut record = issue("example-123");
        record[field] = value;
        write_export(temp.path(), PRIMARY_EXPORT, &[record]);
        let error = BeadsExport::open(temp.path(), WORKSPACE_ID)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("private-invalid"), "{error}");
    }
}

#[test]
fn record_count_line_size_and_depth_are_bounded() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join(".beads")).unwrap();
    fs::write(
        temp.path().join(PRIMARY_EXPORT),
        format!("{}\n", " ".repeat(MAX_LINE_BYTES + 1)),
    )
    .unwrap();
    assert_eq!(
        BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::LineTooLong { line: 1 }
    );

    let mut record = issue("example-123");
    let mut nested = json!(true);
    for _ in 0..MAX_JSON_DEPTH {
        nested = json!([nested]);
    }
    record["future_field"] = nested;
    write_export(temp.path(), PRIMARY_EXPORT, &[record]);
    assert!(matches!(
        BeadsExport::open(temp.path(), WORKSPACE_ID),
        Err(BeadsJsonlError::InvalidRecord { .. })
    ));

    let records = (0..=MAX_ISSUES)
        .map(|index| issue(&format!("example-{index}")))
        .collect::<Vec<_>>();
    write_export(temp.path(), PRIMARY_EXPORT, &records);
    assert_eq!(
        BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap_err(),
        BeadsJsonlError::TooManyIssues
    );
}

#[test]
fn blank_records_are_rejected_but_empty_exports_are_valid() {
    let temp = tempdir().unwrap();
    write_export(temp.path(), PRIMARY_EXPORT, &[]);
    assert_eq!(
        BeadsExport::open(temp.path(), WORKSPACE_ID).unwrap().len(),
        0
    );

    fs::write(temp.path().join(PRIMARY_EXPORT), "\n").unwrap();
    assert!(matches!(
        BeadsExport::open(temp.path(), WORKSPACE_ID),
        Err(BeadsJsonlError::InvalidRecord { line: 1, .. })
    ));
}
