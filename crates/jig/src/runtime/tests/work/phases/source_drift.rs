use super::*;

#[test]
fn iteration_rejects_source_changed_by_a_selected_action() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'; printf 'api\\n' >> .agent/launch.log\"",
        "api_test_command = \"printf 'api\\n' >> .agent/launch.log; printf '// changed during check\\n' >> api/example.go\"",
    );
    fs::write(&config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some("iteration".into()),
            explain: false,
        })),
    )
    .unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("source authority changed"), "{error}");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "api\n"
    );
}

#[test]
fn iteration_rejects_source_changed_during_selection_before_launch() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts_before = fs::read(&receipts_path).unwrap_or_default();
    let source_path = temp.path().join("api/example.go");
    let mut observer = crate::execution::NoopExecutionObserver;

    let error = crate::runtime::work::check_phase_with_pre_execution_test_hook(
        &ctx,
        crate::command::WorkCheckRequest {
            rust_focus: Default::default(),
            projection: Default::default(),
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: Some(crate::command::WorkCheckPhase::Iteration),
            explain: false,
        },
        &mut observer,
        || {
            fs::write(&source_path, "package api\n// changed during selection\n").unwrap();
        },
    )
    .unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("source authority changed"), "{error}");
    assert!(!temp.path().join(".agent/launch.log").exists());
    assert_eq!(
        fs::read(&receipts_path).unwrap_or_default(),
        receipts_before,
        "selection drift must fail before recording execution evidence"
    );
}
