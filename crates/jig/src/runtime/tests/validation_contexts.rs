use super::*;

#[test]
fn native_checks_record_run_history_without_receipts() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let result = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts {
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
    assert!(result["results"][0]["response"].get("receipt_id").is_none());
    assert!(!ctx.state_file("receipts.jsonl").exists());
    assert!(!fs::read(ctx.state_file("runs.jsonl")).unwrap().is_empty());
}
