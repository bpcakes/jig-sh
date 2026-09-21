use super::*;

#[test]
fn final_phase_reloads_pre_v6_authority_between_legacy_checks() {
    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "first_check_command = \"printf 'first ran\\n'\"",
            "first_check_command = \"sed 's/second ran/replacement ran/' .jig.toml > .jig.toml.next && mv .jig.toml.next .jig.toml && printf 'first ran\\n' >> .agent/launch.log\"",
        )
        .replace(
            "mutating_check_command = \"printf 'generated\\n' > generated.txt\"",
            "mutating_check_command = \"printf 'second ran\\n' >> .agent/launch.log\"",
        );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = crate::runtime::dispatch(
        &ctx,
        crate::command::RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                rust_focus: Default::default(),
                projection: Default::default(),
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
                phase: Some(crate::command::WorkCheckPhase::Final),
                explain: false,
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("execution authority changed"), "{error}");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "first ran\n"
    );
}

#[test]
fn final_phase_rejects_legacy_authority_changed_while_waiting_for_a_lease() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "legacy"
kind = "check"
tool = "jig.web_test"
"#,
    );
    enable_v6_legacy_web_tool(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let original_config = fs::read_to_string(&config_path).unwrap().replace(
        "web_test_command = \"printf 'web tests passed\\n'\"",
        "web_test_command = \"printf 'original\\n' >> .agent/original.log\"",
    );
    fs::write(&config_path, &original_config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let (wait_tx, wait_rx) = std::sync::mpsc::sync_channel(0);
    let root = temp.path().to_path_buf();
    let mut observer = IterationWaitObserver {
        wait_notice: Some(wait_tx),
    };

    let changer = std::thread::spawn(move || {
        wait_rx.recv().unwrap();
        let replacement_config = original_config.replace(
            "web_test_command = \"printf 'original\\n' >> .agent/original.log\"",
            "web_test_command = \"printf 'replacement\\n' >> .agent/replacement.log\"",
        );
        fs::write(root.join(".jig.toml"), replacement_config).unwrap();
        drop(held);
    });

    let error = crate::runtime::dispatch_with_observer(
        &ctx,
        crate::command::RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                rust_focus: Default::default(),
                projection: Default::default(),
                plan_id: "plan_1".into(),
                gates: Vec::new(),
                tools: Vec::new(),
                phase: Some(crate::command::WorkCheckPhase::Final),
                explain: false,
            },
        )),
        &mut observer,
    )
    .unwrap_err()
    .to_string();
    changer.join().unwrap();

    assert!(error.contains("execution authority changed"), "{error}");
    assert!(!temp.path().join(".agent/original.log").exists());
    assert!(!temp.path().join(".agent/replacement.log").exists());
}

#[test]
fn final_phase_preserves_legacy_results_when_a_check_changes_source() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "legacy"
kind = "check"
tool = "jig.web_test"
"#,
    );
    enable_v6_legacy_web_tool(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "web_test_command = \"printf 'web tests passed\\n'\"",
        "web_test_command = \"printf 'web tests passed\\n'; printf '// generated\\n' >> web/example.ts\"",
    );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = work_check(&ctx, "final", false);

    assert_eq!(result["ok"], false, "{result:#}");
    assert_eq!(result["selected_ok"], false, "{result:#}");
    assert_eq!(result["checks"][0]["ok"], true, "{result:#}");
    assert!(result["receipt_id"].is_string(), "{result:#}");
    assert!(
        result["error"]
            .as_str()
            .is_some_and(|error| error.contains("legacy final checks changed repository source")),
        "{result:#}"
    );
    assert!(
        fs::read_to_string(temp.path().join("web/example.ts"))
            .unwrap()
            .contains("// generated")
    );
}

#[test]
fn final_phase_defers_native_targets_selected_before_a_legacy_source_change() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "legacy"
kind = "check"
tool = "jig.web_test"

[[work.gates]]
id = "native"
kind = "evidence"
profile = "verify"
"#,
    );
    enable_v6_iteration_profile(temp.path());
    enable_v6_legacy_web_tool(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "web_test_command = \"printf 'web tests passed\\n'; printf 'web\\n' >> .agent/launch.log\"",
        "web_test_command = \"printf 'web\\n' >> .agent/launch.log; printf '// generated\\n' >> web/example.ts\"",
    );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = work_check(&ctx, "final", false);

    assert_eq!(result["ok"], false, "{result:#}");
    assert!(result["run"].is_null(), "{result:#}");
    assert!(result["target_validation_receipt_id"].is_null());
    assert!(
        result["selected_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|invocation| invocation["disposition"] == "deferred")
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "web\n"
    );
    assert!(
        !read_receipts(temp.path())
            .iter()
            .any(|receipt| receipt["target"].is_object())
    );
}
