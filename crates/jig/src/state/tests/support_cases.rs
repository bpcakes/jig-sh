use super::*;

#[test]
fn truncate_handles_multibyte_boundaries() {
    let value = format!("{}{}", "a".repeat(3999), "é");
    let truncated = truncate(&value);

    assert!(truncated.ends_with('…'));
    assert!(truncated.starts_with(&"a".repeat(3999)));
    assert_eq!(truncated.chars().last(), Some('…'));
}

#[test]
fn repository_execution_lease_allows_readers_and_excludes_a_writer() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let first_reader =
        acquire_repository_execution_lease(&ctx, &[jig_contract::ActionEffect::ReadOnly]).unwrap();
    assert!(first_reader.permits(&[jig_contract::ActionEffect::ReadOnly]));
    assert!(!first_reader.permits(&[jig_contract::ActionEffect::Worktree]));
    let second_reader = acquire_repository_execution_lease(
        &ctx,
        &[
            jig_contract::ActionEffect::ReadOnly,
            jig_contract::ActionEffect::Process,
        ],
    )
    .unwrap();
    let root = temp.path().to_path_buf();
    let (attempting_tx, attempting_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            let ctx = RepoContext::load_from(&root).unwrap();
            attempting_tx.send(()).unwrap();
            let _writer =
                acquire_repository_execution_lease(&ctx, &[jig_contract::ActionEffect::Worktree])
                    .unwrap();
            acquired_tx.send(()).unwrap();
        });

        attempting_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "worktree execution acquired its writer lease while readers were active"
        );
        drop(first_reader);
        drop(second_reader);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
}
