use super::*;
use crate::state::json_cache::durable::durable_json_commit_may_have_landed;

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
