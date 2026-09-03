#[cfg(test)]
mod review_thread_capability_tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn observed_witness_preserves_viewer_mutation_capabilities() {
        let pull_request = json!({
            "review_threads": {"nodes": [{
                "id": "PRRT_1",
                "is_resolved": false,
                "has_trusted_comment": true,
                "viewer_can_reply": false,
                "viewer_can_resolve": true,
                "comments": {"total_count": 0, "nodes": []},
            }]},
        });

        let witness = observed_review_thread_witnesses(&pull_request)
            .remove("PRRT_1")
            .unwrap();

        assert!(!witness.viewer_can_reply);
        assert!(witness.viewer_can_resolve);
    }

    #[test]
    fn known_viewer_capability_denials_skip_remote_mutations() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let pull_request = json!({
            "review_threads": {"nodes": [
                {
                    "id": "PRRT_REPLY",
                    "is_resolved": false,
                    "has_trusted_comment": true,
                    "viewer_can_reply": false,
                    "viewer_can_resolve": true,
                    "comments": {"total_count": 0, "nodes": []},
                },
                {
                    "id": "PRRT_RESOLVE",
                    "is_resolved": false,
                    "has_trusted_comment": true,
                    "viewer_can_reply": true,
                    "viewer_can_resolve": false,
                    "comments": {"total_count": 0, "nodes": []},
                },
            ]},
        });
        let worker_output = json!({
            "review_thread_replies": [
                {"thread_id": "PRRT_REPLY", "body": "Addressed.", "resolve": false},
                {"thread_id": "PRRT_RESOLVE", "body": "", "resolve": true},
            ],
        });

        let result = post_review_thread_updates(
            &ctx,
            &pull_request,
            &worker_output,
            "example-head",
            &mut NoopExecutionObserver,
        );

        assert!(!result.failed, "{}", result.posts);
        assert_eq!(result.posts[0]["status"], "skipped");
        assert_eq!(result.posts[0]["reason"], "viewer_cannot_reply");
        assert_eq!(result.posts[0]["reply_skip_reason"], "viewer_cannot_reply");
        assert_eq!(result.posts[1]["status"], "skipped");
        assert_eq!(result.posts[1]["reason"], "viewer_cannot_resolve");
        assert_eq!(
            result.posts[1]["resolve_skip_reason"],
            "viewer_cannot_resolve"
        );
    }
}
