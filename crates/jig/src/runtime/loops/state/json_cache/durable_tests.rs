#[test]
fn compensating_cache_starts_a_fresh_deadline_after_state_commit() {
    let temp = tempdir().unwrap();
    let location = JsonLocation::new(
        temp.path().to_path_buf(),
        temp.path().to_path_buf(),
        "attempts",
        JsonWriteMode::Cache,
    );
    let initial_deadline = Instant::now() + Duration::from_secs(1);

    let (_, followup_remaining) = with_json_cache_lock_compensating_until(
        &location,
        initial_deadline,
        &|| false,
        |state: &mut BTreeMap<String, String>| {
            state.insert("ExampleProject".into(), "cleared".into());
            Ok(())
        },
        |_, deadline| Ok(deadline.saturating_duration_since(Instant::now())),
    )
    .unwrap();

    assert!(followup_remaining > Duration::from_secs(25));
}

#[test]
fn durable_publish_classifies_a_post_replace_sync_failure_as_ambiguous() {
    let temp = tempdir().unwrap();
    let data_path = temp.path().join("attempts.json");
    let steps = std::cell::RefCell::new(Vec::new());

    let error = publish_durable_json(
        &data_path,
        || {
            steps.borrow_mut().push("sync_file");
            Ok(())
        },
        || {
            steps.borrow_mut().push("replace");
            Ok(())
        },
        || {
            steps.borrow_mut().push("sync_publication");
            anyhow::bail!("injected publication sync failure")
        },
    )
    .unwrap_err();

    assert_eq!(
        steps.into_inner(),
        ["sync_file", "replace", "sync_publication"]
    );
    assert!(durable_json_commit_may_have_landed(&error));
    assert!(
        error
            .to_string()
            .contains("was replaced, but its durable publication is unconfirmed"),
        "{error:#}"
    );
}
