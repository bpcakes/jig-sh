use super::*;

fn retry_fixture(root: &Path) -> RepoContext {
    write_v6_evidence_fixture_repo(
        root,
        r#"
[[work.gates]]
id = "verify"
kind = "evidence"
profile = "verify"
"#,
    );
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "printf 'api tests passed\\n'",
            "printf 'api\\n' >> .scratch/invocations; test ! -f .scratch/fail",
        )
        .replace(
            "printf 'web tests passed\\n'",
            "printf 'web\\n' >> .scratch/invocations",
        );
    fs::write(config_path, config).unwrap();
    fs::write(root.join(".gitignore"), ".scratch/\n").unwrap();
    fs::create_dir(root.join(".scratch")).unwrap();
    init_git_repo(root);
    RepoContext::load_from(root).unwrap()
}

fn check(ctx: &RepoContext) -> anyhow::Result<Value> {
    dispatch(
        ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
}

#[test]
fn opted_in_tracker_updates_reuse_receipts_but_documentation_changes_rerun() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    retry_fixture(root);
    let config = root.join(".jig.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        text.replace(
            "[[work.gates]]",
            "[work]\nreceipt_metadata = [\"beads\"]\n\n[[work.gates]]",
        ),
    )
    .unwrap();
    fs::create_dir(root.join(".beads")).unwrap();
    fs::write(root.join(".beads/issues.jsonl"), "ExampleOpen\n").unwrap();
    let ctx = RepoContext::load_from(root).unwrap();
    let first = check(&ctx).unwrap();
    fs::write(root.join(".beads/issues.jsonl"), "ExampleClosed\n").unwrap();
    let second = check(&ctx).unwrap();
    assert!(second["run"].is_null(), "{second:#}");
    for (before, after) in first["target_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .zip(second["target_evidence"].as_array().unwrap())
    {
        assert_eq!(before["receipt_id"], after["receipt_id"]);
        assert_eq!(after["disposition"], "reused");
    }
    fs::write(
        root.join("guide.md"),
        "Example changed packaged instructions\n",
    )
    .unwrap();
    let third = check(&ctx).unwrap();
    assert_eq!(third["run"]["targets"].as_array().unwrap().len(), 2);
    assert!(
        third["target_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .all(|evidence| evidence["disposition"] == "executed")
    );
}

#[test]
fn targeted_retry_preserves_other_targets_original_receipts() {
    let temp = tempdir().unwrap();
    let ctx = retry_fixture(temp.path());
    fs::write(temp.path().join(".scratch/fail"), "fail").unwrap();
    assert!(check(&ctx).is_err());
    let before = work_gates(&ctx);
    let web = before["gates"][0]["targets"][1].clone();
    assert_eq!(web["status"], "passed");
    fs::remove_file(temp.path().join(".scratch/fail")).unwrap();
    let retry = run_repository_target(&ctx, "api:test");
    assert_eq!(retry["ok"], true);
    let after = work_gates(&ctx);
    assert_eq!(after["overall"], "passed", "{after:#}");
    assert_eq!(
        after["gates"][0]["targets"][1]["receipt_id"],
        web["receipt_id"]
    );
    assert_eq!(after["gates"][0]["targets"][1]["run_id"], web["run_id"]);
    assert_eq!(
        after["gates"][0]["targets"][0]["run_id"],
        retry["run"]["run_id"]
    );
    assert!(after["gates"][0]["run_id"].is_null());
    let invocations = fs::read_to_string(temp.path().join(".scratch/invocations")).unwrap();
    assert_eq!(invocations.lines().filter(|line| *line == "api").count(), 2);
    assert_eq!(invocations.lines().filter(|line| *line == "web").count(), 1);
}

#[test]
fn default_retry_executes_only_failure_then_validates_without_execution() {
    let temp = tempdir().unwrap();
    let ctx = retry_fixture(temp.path());
    fs::write(temp.path().join(".scratch/fail"), "fail").unwrap();
    assert!(check(&ctx).is_err());
    let web = work_gates(&ctx)["gates"][0]["targets"][1].clone();
    fs::remove_file(temp.path().join(".scratch/fail")).unwrap();
    let retry = check(&ctx).unwrap();
    assert_eq!(retry["ok"], true);
    assert_eq!(retry["run"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(retry["target_evidence"][1]["receipt_id"], web["receipt_id"]);
    assert_eq!(retry["target_evidence"][1]["disposition"], "reused");
    let validated = check(&ctx).unwrap();
    assert_eq!(validated["ok"], true);
    assert!(validated["run"].is_null());
    assert!(validated["results"].as_array().unwrap().is_empty());
    assert_ne!(
        validated["target_validation_receipt_id"],
        retry["target_validation_receipt_id"]
    );
    for (prior, current) in retry["target_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .zip(validated["target_evidence"].as_array().unwrap())
    {
        assert_eq!(prior["receipt_id"], current["receipt_id"]);
        assert_eq!(prior["run_id"], current["run_id"]);
        assert_eq!(current["disposition"], "reused");
    }
    let receipts = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    let validation = receipts
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|receipt| receipt["id"] == validated["target_validation_receipt_id"])
        .unwrap();
    assert_eq!(
        validation["evidence"]["targets"],
        validated["target_evidence"]
    );
    assert!(validation["target"].is_null());
    assert!(validation["run_id"].is_null());
    let invocations = fs::read_to_string(temp.path().join(".scratch/invocations")).unwrap();
    assert_eq!(invocations.lines().filter(|line| *line == "api").count(), 2);
    assert_eq!(invocations.lines().filter(|line| *line == "web").count(), 1);
}

#[test]
fn newer_failed_target_blocks_old_profile_pass_and_source_change_prevents_reuse() {
    let temp = tempdir().unwrap();
    let ctx = retry_fixture(temp.path());
    assert_eq!(check(&ctx).unwrap()["ok"], true);
    fs::write(temp.path().join(".scratch/fail"), "fail").unwrap();
    assert_eq!(run_repository_target(&ctx, "api:test")["ok"], false);
    assert_eq!(
        work_gates(&ctx)["gates"][0]["targets"][0]["status"],
        "failed"
    );
    fs::remove_file(temp.path().join(".scratch/fail")).unwrap();
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();
    let checked = check(&ctx).unwrap();
    assert_eq!(checked["ok"], true);
    assert_eq!(checked["run"]["targets"].as_array().unwrap().len(), 2);
}

#[test]
fn retry_repairs_dependents_when_their_dependency_runs_again() {
    let temp = tempdir().unwrap();
    retry_fixture(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "inputs = [\"web/**\"]",
        "inputs = [\"web/**\"]\ndepends_on = [{ component = \"api\", action = \"test\" }]",
    );
    fs::write(config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["actions"][1]["depends_on"] = json!([{"component": "api", "action": "test"}]);
    fs::write(manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    fs::write(temp.path().join(".scratch/fail"), "fail").unwrap();
    assert!(check(&ctx).is_err());
    let records = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    let validation = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|row| row["evidence"]["schema"] == "jig.work_check_targets/v1")
        .unwrap();
    assert_eq!(validation["exit_status"], 1);
    assert_eq!(
        validation["evidence"]["targets"][0]["disposition"],
        "executed"
    );
    assert_eq!(
        validation["evidence"]["targets"][1]["disposition"],
        "not_started"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(".scratch/invocations"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["api"]
    );
    fs::remove_file(temp.path().join(".scratch/fail")).unwrap();
    fs::remove_file(temp.path().join(".scratch/invocations")).unwrap();
    assert_eq!(check(&ctx).unwrap()["ok"], true);
    assert_eq!(run_repository_target(&ctx, "api:test")["ok"], true);
    let gates = work_gates(&ctx);
    assert_eq!(gates["gates"][0]["targets"][1]["status"], "stale");
    let repaired = check(&ctx).unwrap();
    assert_eq!(repaired["ok"], true, "{repaired:#}");
    assert_eq!(repaired["run"]["targets"].as_array().unwrap().len(), 2);
    let invocations = fs::read_to_string(temp.path().join(".scratch/invocations")).unwrap();
    assert_eq!(
        invocations.lines().collect::<Vec<_>>(),
        ["api", "web", "api", "api", "web"]
    );
    assert!(check(&ctx).unwrap()["run"].is_null());
}

#[test]
fn newest_unknown_or_expired_receipt_never_reveals_an_older_pass() {
    use std::io::Write;
    let temp = tempdir().unwrap();
    let ctx = retry_fixture(temp.path());
    assert_eq!(check(&ctx).unwrap()["ok"], true);
    let path = temp.path().join(".agent/state/receipts.jsonl");
    let original = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|row| row["target"]["component"] == "api")
        .unwrap();
    let mut newer = original.clone();
    newer["id"] = json!("receipt_foreign");
    newer["plan_id"] = json!("plan_other");
    newer["ended_at_ms"] = json!(original["ended_at_ms"].as_u64().unwrap() + 1);
    newer["exit_status"] = json!(1);
    writeln!(
        fs::OpenOptions::new().append(true).open(&path).unwrap(),
        "{newer}"
    )
    .unwrap();
    assert_eq!(work_gates(&ctx)["overall"], "passed");
    for (index, (field, value, expected)) in [
        ("input_digest", Value::Null, "unknown"),
        ("config_digest", json!("different"), "stale"),
        ("valid_until_ms", json!(0), "stale"),
        (
            "evidence",
            json!({"requires_time_validity": true}),
            "unknown",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut newer = original.clone();
        newer["id"] = json!(format!("receipt_invalid_{index}"));
        newer["ended_at_ms"] = json!(original["ended_at_ms"].as_u64().unwrap() + 2 + index as u64);
        newer[field] = value;
        writeln!(
            fs::OpenOptions::new().append(true).open(&path).unwrap(),
            "{newer}"
        )
        .unwrap();
        let gates = work_gates(&ctx);
        assert_eq!(gates["overall"], "blocked");
        assert_eq!(gates["gates"][0]["targets"][0]["receipt_id"], newer["id"]);
        assert_eq!(
            gates["gates"][0]["targets"][0]["freshness"], expected,
            "{gates:#}"
        );
    }
}

#[test]
fn cancelled_validation_does_not_record_a_reused_pass() {
    struct Cancelled;
    impl crate::execution::ExecutionObserver for Cancelled {}
    impl crate::execution::ExecutionCancellation for Cancelled {
        fn cancelled(&self) -> bool {
            true
        }
    }
    let temp = tempdir().unwrap();
    let ctx = retry_fixture(temp.path());
    assert_eq!(check(&ctx).unwrap()["ok"], true);
    let path = temp.path().join(".agent/state/receipts.jsonl");
    let before = fs::read(&path).unwrap();
    let error = crate::runtime::work::check_from_args_with_observer(
        &ctx,
        json!({"plan_id": "plan_1"}),
        &mut Cancelled,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("cancelled"));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn explicit_native_retry_preserves_prepared_work_plan_authority() {
    let temp = tempdir().unwrap();
    retry_fixture(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "runner = { kind = \"command\", command = \"api_test_command\" }",
        "runner = { kind = \"native\", operation = \"jig.file_budget\" }",
    );
    fs::write(
        config_path,
        config.replace("inputs = [\"api/**\"]", "inputs = [\"**\"]"),
    )
    .unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(7);
    manifest["actions"][0]["inputs"] = json!(["**"]);
    manifest["actions"][0]["runner"] = json!({"kind": "native", "operation": "jig.file_budget"});
    fs::write(manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
    fs::create_dir(temp.path().join(".jig")).unwrap();
    fs::write(
        temp.path().join(".jig/file-budget.toml"),
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"api/**\"]\nmax_lines=100\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example native retry".into(),
            body: Some("Verify the captured Git comparison baseline.".into()),
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    let result = run_repository_target_for_plan(&ctx, "api:test", plan_id);
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(
        result["plan"]["targets"][0]["prepared_native_input"]["work_plan_id"],
        plan_id
    );
    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some(plan_id.into()),
        })),
    )
    .unwrap();
    assert_eq!(gates["gates"][0]["targets"][0]["status"], "passed");
}
