use super::*;

fn fixture(root: &Path, contract_version: u64) -> RepoContext {
    write_v6_evidence_fixture_repo(
        root,
        "[[work.gates]]\nid = \"full\"\nkind = \"evidence\"\nprofile = \"verify\"\n",
    );
    enable_v6_iteration_profile(root);
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    for (_, command) in config["commands"].as_table_mut().unwrap().iter_mut() {
        *command = toml::Value::String(format!(
            "{}; test ! -f .agent/example-failure",
            command.as_str().unwrap()
        ));
    }
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(contract_version);
    if contract_version >= 8 {
        for action in config["repository"]["actions"].as_array_mut().unwrap() {
            action.as_table_mut().unwrap().insert(
                "inputs_policy".into(),
                toml::Value::String("exhaustive".into()),
            );
            action["runner"]["kind"] = toml::Value::String("shell".into());
        }
        for action in manifest["actions"].as_array_mut().unwrap() {
            action["inputs_policy"] = json!("exhaustive");
            action["runner"]["kind"] = json!("shell");
        }
    }
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    init_git_repo(root);
    RepoContext::load_from(root).unwrap()
}

fn open_plan(ctx: &RepoContext) -> String {
    crate::state::plans_open(
        ctx,
        crate::state::PlanOpenRequest {
            title: "Example phase consumer".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap()["plan_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn target_receipts(ctx: &RepoContext) -> Vec<Value> {
    fs::read_to_string(ctx.state_file("receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|record| record["target"].is_object())
        .collect()
}

fn initial_launch_log(root: &Path) -> String {
    let log = fs::read_to_string(root.join(".agent/launch.log")).unwrap();
    let mut launches = log.lines().collect::<Vec<_>>();
    launches.sort_unstable();
    assert_eq!(launches, ["api", "web"]);
    log
}

fn assert_originals_reused(report: &Value, originals: &[Value], expected: usize) {
    assert_eq!(report["ok"], true, "{report:#}");
    assert_eq!(report["selected_ok"], true, "{report:#}");
    assert_eq!(report["final_gates_ok"], true, "{report:#}");
    let invocations = report["selected_invocations"].as_array().unwrap();
    assert_eq!(invocations.len(), expected, "{report:#}");
    for invocation in invocations {
        let original = originals
            .iter()
            .find(|original| original["target"] == invocation["target"])
            .unwrap();
        assert_eq!(invocation["disposition"], "reused", "{report:#}");
        let validity = &invocation["evidence_validity"];
        assert_eq!(validity["status"], "passed", "{report:#}");
        assert_eq!(validity["receipt_id"], original["id"], "{report:#}");
        assert_eq!(validity["run_id"], original["run_id"], "{report:#}");
        assert_eq!(validity["original_plan_id"], "plan_1", "{report:#}");
    }
}

#[test]
fn cross_plan_phase_preview_and_execution_reuse_original_receipts() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), 8);
    let first =
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id":"plan_1"})).unwrap();
    assert_eq!(first["ok"], true, "{first:#}");
    let originals = target_receipts(&ctx);
    assert_eq!(originals.len(), 2);
    let launches = initial_launch_log(temp.path());
    let consumer = open_plan(&ctx);
    let snapshot = || {
        [
            "receipts.jsonl",
            "runs.jsonl",
            "plans.jsonl",
            "sessions.jsonl",
        ]
        .map(|name| fs::read(ctx.state_file(name)).unwrap_or_default())
    };
    let before = snapshot();
    let ordinary = crate::runtime::call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({"plan_id":consumer, "explain":true}),
    )
    .unwrap();
    assert_originals_reused(&ordinary, &originals, 2);
    for (phase, count) in [("final", 2), ("iteration", 1)] {
        let preview = work_check_for_plan(&ctx, &consumer, phase, true);
        assert_originals_reused(&preview, &originals, count);
    }
    assert_eq!(snapshot(), before, "previews must not write journals");
    for (phase, count) in [("final", 2), ("iteration", 1)] {
        let execution = work_check_for_plan(&ctx, &consumer, phase, false);
        assert_originals_reused(&execution, &originals, count);
        assert_eq!(target_receipts(&ctx), originals);
    }
    let ordinary =
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id":consumer})).unwrap();
    assert_eq!(ordinary["ok"], true, "{ordinary:#}");
    assert!(ordinary["run"].is_null(), "{ordinary:#}");
    assert_eq!(target_receipts(&ctx), originals);
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        launches
    );
}

#[test]
fn cross_plan_phase_preview_does_not_resurrect_pass_before_newer_failure() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), 8);
    let first =
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id":"plan_1"})).unwrap();
    assert_eq!(first["ok"], true, "{first:#}");
    let failing_plan = open_plan(&ctx);
    fs::write(
        temp.path().join(".agent/example-failure"),
        "Example failure\n",
    )
    .unwrap();
    let failure = crate::runtime::call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({"plan_id":failing_plan, "gates":["full"]}),
    )
    .unwrap_err();
    assert!(
        failure.to_string().contains("concluded failure"),
        "{failure:#}"
    );
    let receipts = target_receipts(&ctx);
    let latest = receipts
        .iter()
        .rev()
        .find(|record| record["target"]["component"] == "api")
        .unwrap();
    assert_eq!(latest["exit_status"], 1, "{latest:#}");
    let consumer = open_plan(&ctx);
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    let launches = fs::read(temp.path().join(".agent/launch.log")).unwrap();
    for phase in [None, Some("final"), Some("iteration")] {
        let report = crate::runtime::call_tool(
            &ctx,
            tool::WORK_CHECK,
            json!({"plan_id":consumer, "phase":phase, "explain":true}),
        )
        .unwrap();
        assert_eq!(report["selected_ok"], false, "{report:#}");
        let api = report["selected_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|invocation| invocation["target"]["component"] == "api")
            .unwrap();
        assert_eq!(api["disposition"], "selected", "{report:#}");
        assert_eq!(api["evidence_validity"]["status"], "failed", "{report:#}");
        assert_eq!(api["evidence_validity"]["receipt_id"], latest["id"]);
        assert_eq!(api["evidence_validity"]["original_plan_id"], failing_plan);
    }
    assert_eq!(fs::read(ctx.state_file("receipts.jsonl")).unwrap(), before);
    assert_eq!(
        fs::read(temp.path().join(".agent/launch.log")).unwrap(),
        launches
    );
}

#[test]
fn legacy_contract_phase_preview_keeps_other_plan_receipts_local() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), 6);
    let first =
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id":"plan_1"})).unwrap();
    assert_eq!(first["ok"], true, "{first:#}");
    let launches = initial_launch_log(temp.path());
    let consumer = open_plan(&ctx);
    for phase in ["final", "iteration"] {
        let report = work_check_for_plan(&ctx, &consumer, phase, true);
        assert_eq!(report["selected_ok"], false, "{report:#}");
        assert!(
            report["selected_invocations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|invocation| invocation["disposition"] == "selected"
                    && invocation["evidence_validity"]["status"] == "missing"),
            "{report:#}"
        );
    }
    assert_eq!(
        fs::read_to_string(temp.path().join(".agent/launch.log")).unwrap(),
        launches
    );
}
