use super::*;

// Apple rejects invalid-byte path components with EILSEQ before these
// filesystem-backed fixtures can exercise Jig's path handling.
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_ENV: &str = "JIG_TEST_NON_UTF8_TMPDIR_HELPER";

#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_ROOT_ENV: &str = "JIG_TEST_NON_UTF8_TMPDIR_ROOT";

#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_TEST: &str = "source_identity::tests::non_utf8_paths::canonical_diff_order_file_preserves_non_utf8_temporary_directory_helper";

#[cfg(unix)]
#[test]
fn porcelain_z_parser_preserves_non_utf8_path_bytes() {
    let entries = parse_porcelain_status_z(b"?? bad\xFFname\0").unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].path.as_os_str().as_encoded_bytes(),
        b"bad\xFFname"
    );
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn whole_worktree_fingerprint_preserves_non_utf8_tracked_paths() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    let file_name = OsString::from_vec(b"tracked-\xff.rs".to_vec());
    let path = temp.path().join(&file_name);
    fs::write(&path, "one\n").unwrap();
    let added = Command::new("git")
        .current_dir(temp.path())
        .arg("add")
        .arg("--")
        .arg(&file_name)
        .output()
        .unwrap();
    assert!(added.status.success(), "{:?}", added.stderr);
    run_git(temp.path(), &["commit", "-m", "non-UTF-8 fixture"]);

    let clean = repo_worktree_fingerprint(temp.path()).unwrap();
    fs::write(&path, "two\n").unwrap();
    let changed = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(clean, changed);
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn canonical_diff_order_file_preserves_non_utf8_temporary_directory() {
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("source.txt"), "baseline\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "baseline"]);
    std::fs::write(temp.path().join("source.txt"), "changed\n").unwrap();

    let raw_temp = temp
        .path()
        .join(OsString::from_vec(b"proof-temp-\xff".to_vec()));
    std::fs::create_dir(&raw_temp).unwrap();

    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", NON_UTF8_TMPDIR_HELPER_TEST, "--nocapture"])
        .env(NON_UTF8_TMPDIR_HELPER_ENV, "1")
        .env(NON_UTF8_TMPDIR_HELPER_ROOT_ENV, temp.path())
        .env("TMPDIR", raw_temp);
    configure_read_only_git_environment(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "non-UTF-8 TMPDIR helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn canonical_diff_order_file_preserves_non_utf8_temporary_directory_helper() {
    if std::env::var_os(NON_UTF8_TMPDIR_HELPER_ENV).is_none() {
        return;
    }
    let root = PathBuf::from(std::env::var_os(NON_UTF8_TMPDIR_HELPER_ROOT_ENV).unwrap());

    let whole = repo_worktree_fingerprint(&root).unwrap();
    assert!(whole.starts_with("sha256:"));
}

#[cfg(unix)]
#[test]
fn literal_os_path_chunking_uses_raw_encoded_byte_lengths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let paths = [
        PathBuf::from(OsString::from_vec(vec![0xff; 40 * 1024])),
        PathBuf::from(OsString::from_vec(vec![0xfe; 40 * 1024])),
    ];

    let chunks = literal_os_path_chunks(&paths);

    assert_eq!(chunks, [&paths[..1], &paths[1..]]);
    assert_eq!(
        chunks.into_iter().flatten().cloned().collect::<Vec<_>>(),
        paths
    );
}
