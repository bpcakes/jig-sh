use super::*;

fn enable_v8_iteration_fixture(root: &Path, profile: &str) {
    enable_v6_iteration_profile(root);
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["work"]["iteration_profile"] = toml::Value::String(profile.into());
    for action in config["repository"]["actions"].as_array_mut().unwrap() {
        action.as_table_mut().unwrap().insert(
            "inputs_policy".into(),
            toml::Value::String("exhaustive".into()),
        );
        action["runner"]["kind"] = toml::Value::String("shell".into());
    }
    fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(8);
    for action in manifest["actions"].as_array_mut().unwrap() {
        action["inputs_policy"] = json!("exhaustive");
        action["runner"]["kind"] = json!("shell");
    }
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn selected_phase_keeps_prepared_authority_when_receipt_inspection_times_out() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v8_iteration_fixture(temp.path(), "iteration");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let selection = crate::repository::plan_run_with_cancellation(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: Vec::new(),
            profile: Some("iteration".into()),
            affected_base: None,
            comparison: None,
            work_plan_id: Some("plan_1".into()),
        },
        &|| false,
    )
    .unwrap();
    assert!(
        selection
            .targets
            .iter()
            .all(|target| target.target_identity.is_some())
    );

    let (passing, unavailable, collection) =
        crate::runtime::work::selected_invocation_snapshot_with_test_timeout(
            &ctx,
            "plan_1",
            &catalog,
            &selection.targets,
            std::time::Duration::ZERO,
        )
        .unwrap();

    assert!(passing.is_empty());
    assert!(unavailable.is_empty(), "prepared authority was lost");
    assert_eq!(
        collection.unwrap().limit,
        Some(jig_contract::freshness::FreshnessCollectionLimit::Deadline)
    );
}

#[test]
fn iteration_preview_and_execution_include_declared_prerequisites() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replacen(
        "inputs = [\"api/**\"]",
        "inputs = [\"api/**\"]\ndepends_on = [{ component = \"web\", action = \"test\" }]",
        1,
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["actions"][0]["depends_on"] = json!([{"component": "web", "action": "test"}]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let preview = work_check(&ctx, "iteration", true);
    assert_eq!(preview["selected_invocations"].as_array().unwrap().len(), 2);
    let prerequisite = preview["selected_invocations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|invocation| invocation["target"]["component"] == "web")
        .unwrap();
    assert_eq!(
        prerequisite["selection_reasons"][0]["kind"],
        "action_dependency"
    );
    assert_eq!(prerequisite["disposition"], "selected");

    let executed = work_check(&ctx, "iteration", false);
    assert_eq!(executed["ok"], true, "{executed:#}");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "web\napi\n"
    );
}

#[test]
fn iteration_reruns_a_target_skipped_after_its_prerequisite_failed() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let passing_config = fs::read_to_string(&config_path).unwrap().replacen(
        "inputs = [\"api/**\"]",
        "inputs = [\"api/**\"]\ndepends_on = [{ component = \"web\", action = \"test\" }]",
        1,
    );
    let failing_config = passing_config.replace(
        "web_test_command = \"printf 'web tests passed\\n'; printf 'web\\n' >> .agent/launch.log\"",
        "web_test_command = \"printf 'web\\n' >> .agent/launch.log; exit 7\"",
    );
    assert_ne!(failing_config, passing_config);
    fs::write(&config_path, failing_config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["actions"][0]["depends_on"] = json!([{"component": "web", "action": "test"}]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let failing = RepoContext::load_from(temp.path()).unwrap();

    let first = work_check(&failing, "iteration", false);
    assert_eq!(first["ok"], false, "{first:#}");
    let skipped = first["selected_invocations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|invocation| invocation["target"]["component"] == "api")
        .unwrap();
    assert_eq!(
        skipped["evidence_validity"]["freshness"], "unknown",
        "{first:#}"
    );
    assert!(
        skipped["evidence_validity"]["receipt_worktree_fingerprint_error"]
            .as_str()
            .is_some_and(|error| error.contains("did not start")),
        "{first:#}"
    );

    fs::write(&config_path, passing_config).unwrap();
    let repaired = RepoContext::load_from(temp.path()).unwrap();
    let second = work_check(&repaired, "iteration", false);

    assert_eq!(second["ok"], true, "{second:#}");
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        "web\nweb\napi\n"
    );
}

#[test]
fn iteration_rejects_unobservable_v8_current_authority_before_launch() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v8_iteration_fixture(temp.path(), "iteration");
    fs::write(temp.path().join(".gitignore"), "api/ignored\n").unwrap();
    fs::write(temp.path().join("api/ignored"), "unobservable input\n").unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let preview = work_check(&ctx, "iteration", true);
    assert_eq!(preview["selected_ok"], false, "{preview:#}");
    assert_eq!(
        preview["selected_invocations"][0]["disposition"],
        "unavailable"
    );
    assert!(
        preview["selected_invocations"][0]["evidence_validity"]["freshness_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "unobservable_input")
    );

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
    .unwrap_err()
    .to_string();
    assert!(error.contains("authority is unavailable"), "{error}");
    assert!(!temp.path().join(".agent/launch.log").exists());
}

#[test]
fn scheduled_v8_subset_plan_remains_revalidatable() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v8_iteration_fixture(temp.path(), "verify");
    init_git_repo(temp.path());
    let first_ctx = RepoContext::load_from(temp.path()).unwrap();
    assert_eq!(work_check(&first_ctx, "iteration", false)["ok"], true);
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let second = work_check(&ctx, "iteration", false);

    assert_eq!(second["ok"], true, "{second:#}");
    let plan: jig_contract::RunPlan = serde_json::from_value(second["plan"].clone()).unwrap();
    assert_eq!(plan.selectors, ["api:test"]);
    assert!(plan.profile.is_none());
    assert_eq!(plan.targets.len(), 1);
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    crate::repository::validate_run_plan(&ctx, &catalog, &plan).unwrap();
    let run_id = second["run"]["run_id"].as_str().unwrap();
    let durable = crate::state::run_by_id(&ctx, run_id).unwrap();
    crate::repository::validate_run_plan(&ctx, &catalog, &durable.plan).unwrap();
}
