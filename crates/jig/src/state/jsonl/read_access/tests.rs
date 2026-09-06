use super::*;

#[test]
fn replaced_file_is_reopened_before_the_visitor_runs() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"old\n").unwrap();
    let mut locks = 0;
    let mut visits = 0;
    let bytes = with_jsonl_read(
        &path,
        &|| false,
        |file| {
            locks += 1;
            if locks == 1 {
                let replacement = temp.path().join("replacement.jsonl");
                fs::write(&replacement, b"replacement\n")?;
                fs::rename(replacement, &path)?;
            }
            FileExt::try_lock_shared(file)
        },
        ReadLockLabels::STATE,
        |access| {
            visits += 1;
            let JsonlReadAccess::Locked(mut file) = access else {
                panic!("expected a current locked file");
            };
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        },
    )
    .unwrap();
    assert_eq!(bytes, b"replacement\n");
    assert_eq!(locks, 2);
    assert_eq!(visits, 1);
}

#[test]
fn missing_read_does_not_create_data_or_cache_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    with_jsonl_read(
        &path,
        &|| false,
        |_| panic!("missing files must not be locked"),
        ReadLockLabels::STATE,
        |access| {
            assert!(matches!(access, JsonlReadAccess::Missing));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn visitor_runs_with_both_locks_and_failure_releases_them() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{}\n").unwrap();
    let cache_path = state_lock_path(&path);
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    fs::write(&cache_path, b"").unwrap();
    let data_probe = File::open(&path).unwrap();
    let cache_probe = File::open(&cache_path).unwrap();

    let error = with_jsonl_read::<()>(
        &path,
        &|| false,
        |file| {
            assert!(FileExt::try_lock_exclusive(&cache_probe)?);
            FileExt::unlock(&cache_probe)?;
            FileExt::try_lock_shared(file)
        },
        ReadLockLabels::STATE,
        |access| {
            assert!(matches!(access, JsonlReadAccess::Locked(_)));
            assert!(!FileExt::try_lock_exclusive(&data_probe)?);
            assert!(!FileExt::try_lock_exclusive(&cache_probe)?);
            bail!("visitor failed")
        },
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "visitor failed");
    assert!(FileExt::try_lock_exclusive(&data_probe).unwrap());
    assert!(FileExt::try_lock_exclusive(&cache_probe).unwrap());
}

#[test]
fn read_errors_precede_unlock_errors_and_unlock_errors_prevent_retries() {
    let failed_unlock = || Err(io::Error::other("unlock failed"));
    let error = finish_read::<()>(
        Some(Err(anyhow::anyhow!("read failed"))),
        failed_unlock(),
        failed_unlock(),
        ReadLockLabels::STATE,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "read failed");

    for (result, labels, expected) in [
        (
            Some(Ok(())),
            ReadLockLabels::STATE,
            "Failed to unlock state cache file",
        ),
        (
            None,
            ReadLockLabels::STATE,
            "Failed to unlock stale state cache file",
        ),
        (
            Some(Ok(())),
            ReadLockLabels::RECEIPT,
            "Failed to unlock receipt cache lock",
        ),
        (
            None,
            ReadLockLabels::RECEIPT,
            "Failed to unlock stale receipt cache lock",
        ),
    ] {
        let error = finish_read(result, failed_unlock(), failed_unlock(), labels).unwrap_err();
        assert_eq!(error.to_string(), expected);
    }
    for (result, expected) in [
        (Some(Ok(())), "Failed to unlock receipt state file"),
        (None, "Failed to unlock stale receipt state file"),
    ] {
        let error =
            finish_read(result, Ok(()), failed_unlock(), ReadLockLabels::RECEIPT).unwrap_err();
        assert_eq!(error.to_string(), expected);
    }
    assert_eq!(
        finish_read(Some(Ok(7)), Ok(()), Ok(()), ReadLockLabels::STATE).unwrap(),
        Some(7)
    );
    assert_eq!(
        finish_read::<()>(None, Ok(()), Ok(()), ReadLockLabels::STATE).unwrap(),
        None
    );
}
