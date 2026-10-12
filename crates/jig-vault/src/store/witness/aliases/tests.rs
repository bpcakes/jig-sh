use super::*;

fn layout() -> (tempfile::TempDir, fs::Metadata) {
    let temp = tempfile::tempdir().unwrap();
    for child in [IDS_DIR, JOURNALS_DIR, LOCKS_DIR] {
        fs::create_dir(temp.path().join(child)).unwrap();
    }
    let output = temp.path().join("output");
    fs::write(&output, b"output").unwrap();
    let metadata = fs::metadata(&output).unwrap();
    (temp, metadata)
}

#[test]
fn a_removed_entry_retries_to_a_complete_safe_scan() {
    for child in [IDS_DIR, JOURNALS_DIR] {
        let (temp, output) = layout();
        let disappearing = temp.path().join(child).join(".temporary");
        fs::write(&disappearing, b"temporary").unwrap();
        let mut removed = false;
        let result = check_with(temp.path(), &output, |path| {
            if path == disappearing && !removed {
                fs::remove_file(path).unwrap();
                removed = true;
            }
            fs::symlink_metadata(path)
        });
        assert!(!result.unwrap());
        assert!(removed);
        assert_eq!(fs::read(temp.path().join("output")).unwrap(), b"output");
    }
}

#[test]
fn a_renamed_alias_in_an_already_scanned_directory_is_still_protected() {
    let (temp, output) = layout();
    let old = temp.path().join(JOURNALS_DIR).join(".temporary");
    let new = temp.path().join(IDS_DIR).join("published.json");
    fs::hard_link(temp.path().join("output"), &old).unwrap();
    let mut renamed = false;
    let result = check_with(temp.path(), &output, |path| {
        if path == old {
            fs::rename(&old, &new).unwrap();
            renamed = true;
        }
        fs::symlink_metadata(path)
    });
    assert!(result.unwrap());
    assert!(renamed);
    assert_eq!(fs::read(new).unwrap(), b"output");
}

#[test]
fn repeated_disappearance_is_bounded_and_never_proves_absence() {
    let (temp, output) = layout();
    let entry = temp.path().join(JOURNALS_DIR).join(".temporary");
    fs::write(&entry, b"temporary").unwrap();
    let mut attempts = 0;
    let error = check_with(temp.path(), &output, |path| {
        // Remove each enumerated entry and create its successor for the
        // next attempt, as overlapping writers could do.
        fs::remove_file(path).unwrap();
        attempts += 1;
        fs::write(path.with_extension(format!("{attempts}.tmp")), b"temporary").unwrap();
        fs::symlink_metadata(path)
    })
    .unwrap_err();
    assert_eq!(attempts, SCAN_ATTEMPTS);
    assert!(error.to_string().contains("retry the operation"));
}

#[test]
fn other_metadata_and_directory_errors_still_fail_closed() {
    let (temp, output) = layout();
    fs::write(temp.path().join(IDS_DIR).join("record.json"), b"record").unwrap();
    let error = check_with(temp.path(), &output, |_| {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    })
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );

    let journals = temp.path().join(JOURNALS_DIR);
    fs::remove_dir(&journals).unwrap();
    fs::write(&journals, b"not a directory").unwrap();
    assert!(check(temp.path(), &output).is_err());
}
