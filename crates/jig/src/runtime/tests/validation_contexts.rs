use super::*;

#[test]
fn native_standalone_diagnostics_and_linked_checks_keep_distinct_evidence_contracts() {
    for linked in [false, true] {
        let temp = tempdir().unwrap();
        write_v6_evidence_fixture_repo(temp.path(), "");
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let receipts = ctx.state_file("receipts.jsonl");
        let before = fs::read(&receipts).unwrap_or_default();
        let result = dispatch(
            &ctx,
            CommandKind::Check(crate::cli::CheckOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: linked.then(|| "plan_1".into()),
                    no_receipt: !linked,
                },
                profile: None,
                affected: None,
                explain: false,
                fail_fast: false,
                comparison: crate::cli::CheckComparisonOpts::default(),
                command: Some(crate::cli::CheckCommand::Selectors(vec!["api:test".into()])),
            }),
        )
        .unwrap();
        assert_eq!(result["ok"], true, "{result:#}");
        assert_eq!(
            result["results"][0]["response"]["result"]["stdout"],
            "api tests passed\n"
        );
        let after = fs::read(&receipts).unwrap_or_default();
        assert!(after.starts_with(&before));
        if linked {
            let appended: serde_json::Value =
                serde_json::from_slice(&after[before.len()..]).unwrap();
            assert_eq!(appended["plan_id"], "plan_1");
            assert_eq!(
                appended["id"],
                result["results"][0]["response"]["receipt_id"]
            );
        } else {
            assert_eq!(after, before);
            assert!(result["results"][0]["response"]["receipt_id"].is_null());
        }
        assert!(
            !fs::read(ctx.state_file("runs.jsonl")).unwrap().is_empty(),
            "native --no-receipt suppresses receipts, not run history"
        );
    }
}
