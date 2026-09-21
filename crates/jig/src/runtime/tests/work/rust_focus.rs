use super::*;

fn fixture(root: &Path) -> RepoContext {
    write_v6_evidence_fixture_repo(root, "");
    fs::create_dir_all(root.join("example-api/src")).unwrap();
    fs::create_dir_all(root.join("example-api/tests")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"example-api\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    fs::write(root.join("example-api/Cargo.toml"), "[package]\nname = \"example-api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[features]\nexample = []\n").unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"example-api\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.join("example-api/src/lib.rs"), "#[test]\nfn example_one() { assert_eq!(2 + 2, 4); }\n#[test]\nfn example_two() { assert_eq!(3 + 2, 5); }\n").unwrap();
    fs::write(
        root.join("example-api/tests/sibling.rs"),
        "compile_error!(\"sibling integration target must not build during lib focus\");\n",
    )
    .unwrap();
    let actions = json!([
        {"target":{"component":"api","action":"test-focused"},"intent":"check","effects":["read_only","process"],
         "runner":{"kind":"rust_nextest_v1","configuration":{"workspace_manifest":"Cargo.toml","focused":true}},
         "arguments":{"focus":{"type":"rust_focus_v1"}},"inputs":["Cargo.toml","Cargo.lock","example-api/**"]},
        {"target":{"component":"api","action":"test-full"},"intent":"check","effects":["read_only","process"],
         "runner":{"kind":"rust_nextest_v1","configuration":{"workspace_manifest":"Cargo.toml","focused":false}},
         "inputs":["Cargo.toml","Cargo.lock","example-api/**"]}
    ]);
    let components = json!([{"id":"api","root":".","adapters":["rust"]}]);
    let profiles = json!([
        {"id":"iteration","targets":[{"component":"api","action":"test-focused"}]},
        {"id":"verify","targets":[{"component":"api","action":"test-full"}]}
    ]);
    let path = root.join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config["repository"]["components"] = toml::Value::try_from(&components).unwrap();
    config["repository"]["actions"] = toml::Value::try_from(&actions).unwrap();
    config["repository"]["profiles"] = toml::Value::try_from(&profiles).unwrap();
    config.as_table_mut().unwrap().insert(
        "work".into(),
        toml::Value::try_from(json!({
            "iteration_profile":"iteration", "gates":[
                {"id":"full","kind":"evidence","profile":"verify"},
                {"id":"same-target-default","kind":"evidence","target":"api:test-focused"}
            ]
        }))
        .unwrap(),
    );
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
    let path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["contract_version"] = json!(8);
    manifest["components"] = components;
    manifest["actions"] = actions;
    manifest["profiles"] = profiles;
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    init_git_repo(root);
    RepoContext::load_from_root(root.to_path_buf()).unwrap()
}

fn focus() -> Value {
    json!({"kind":"explicit","packages":["example-api@0.1.0"],"targets":[{"kind":"lib"}]})
}

fn check(ctx: &RepoContext, focus: &Value, explain: bool, cli: bool) -> Value {
    if cli {
        dispatch(
            ctx,
            CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
                projection: Default::default(),
                plan_id: "plan_1".into(),
                gates: vec![],
                tools: vec![],
                phase: Some("iteration".into()),
                explain,
                rust_focus: vec![format!("api:test-focused={focus}")],
            })),
        )
        .unwrap()
    } else {
        crate::runtime::call_tool(
            ctx,
            tool::WORK_CHECK,
            json!({
                "plan_id":"plan_1", "phase":"iteration", "explain":explain,
                "rust_focus":{"api:test-focused":focus},
            }),
        )
        .unwrap()
    }
}

#[test]
fn focused_work_execution_preserves_scope_and_cannot_finish_full_gate() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path());
    let cli = check(&ctx, &focus(), true, true);
    let mcp = check(&ctx, &focus(), true, false);
    assert_eq!(
        cli["selected_invocations"][0]["invocation"],
        mcp["selected_invocations"][0]["invocation"]
    );
    let invocation = &cli["selected_invocations"][0]["invocation"];
    let argv = invocation["prepared_rust_input"]["args"]
        .as_array()
        .unwrap();
    assert!(argv.contains(&json!("--package")), "{argv:?}");
    assert!(argv.contains(&json!("example-api@0.1.0")));
    assert!(argv.contains(&json!("--lib")));
    assert!(!argv.contains(&json!("--workspace")));
    assert!(
        !temp.path().join("target").exists(),
        "explain must not build"
    );
    let executed = check(&ctx, &focus(), false, true);
    assert_eq!(executed["ok"], true, "{executed:#}");
    assert_eq!(executed["final_gates_ok"], false);
    assert_eq!(
        executed["selected_invocations"][0]["evidence_validity"]["status"],
        "passed"
    );
    let receipt = executed["selected_invocations"][0]["evidence_validity"]["receipt_id"]
        .as_str()
        .unwrap();
    let records = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    assert!(
        records
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(|value| value["id"] == receipt && value["exit_status"] == 0)
    );
    let gates =
        crate::runtime::call_tool(&ctx, tool::WORK_GATES, json!({"plan_id":"plan_1"})).unwrap();
    assert_eq!(gates["gates_ok"], false, "{gates:#}");
    assert_eq!(gates["gates"][0]["status"], "missing");
    let same_target = gates["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "same-target-default")
        .unwrap();
    assert_eq!(same_target["status"], "stale", "{gates:#}");
    let finish = crate::runtime::call_tool(&ctx, tool::WORK_FINISH, json!({"plan_id":"plan_1"}));
    assert!(
        finish.is_err(),
        "focused receipt must not close full validation"
    );
    let full = crate::runtime::call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id":"plan_1", "gates":["same-target-default"]
        }),
    );
    assert!(
        full.is_err(),
        "default invocation must build the excluded sibling target: {full:?}"
    );
    let records = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    assert!(
        records
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(|value| {
                value["target"] == json!({"component":"api","action":"test-focused"})
                    && value["exit_status"]
                        .as_i64()
                        .is_some_and(|status| status != 0)
                    && value["stderr_preview"].as_str().is_some_and(|output| {
                        output
                            .contains("sibling integration target must not build during lib focus")
                    })
            }),
        "forced default invocation must record the real compile-error sentinel"
    );
}

#[test]
fn focus_filter_and_feature_changes_invalidate_exact_invocation_evidence() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path());
    let first = check(&ctx, &focus(), false, false);
    assert_eq!(first["ok"], true, "{first:#}");
    let original = check(&ctx, &focus(), true, false);
    assert_eq!(original["selected_invocations"][0]["disposition"], "reused");
    let identity = &original["selected_invocations"][0]["invocation"]["target_identity"];
    assert!(identity["invocation_digest"].is_string());
    for (key, value) in [
        ("filter", json!("test(example_one)")),
        (
            "features",
            json!({"features":["example"],"no_default_features":true}),
        ),
    ] {
        let mut changed = focus();
        changed[key] = value;
        let preview = check(&ctx, &changed, true, false);
        let selected = &preview["selected_invocations"][0];
        assert_ne!(
            selected["invocation"]["target_identity"]["invocation_digest"],
            identity["invocation_digest"],
            "{preview:#}"
        );
        assert_ne!(selected["disposition"], "reused", "{preview:#}");
        assert_eq!(preview["final_gates_ok"], false);
    }
}

#[test]
fn automatic_custom_runner_falls_back_honestly_but_explicit_or_unselected_focus_rejects() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    enable_v6_iteration_profile(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let request = |target: &str, focus: Value, explain: bool| {
        json!({
            "plan_id":"plan_1", "phase":"iteration", "explain":explain,
            "rust_focus":{(target):focus},
        })
    };
    for explain in [true, false] {
        let result = crate::runtime::call_tool(
            &ctx,
            tool::WORK_CHECK,
            request("api:test", json!({"kind":"automatic"}), explain),
        )
        .unwrap();
        assert_eq!(result["ok"], true, "{result:#}");
        assert_eq!(
            result["rust_focus_fallbacks"],
            json!([{
                "target":{"component":"api","action":"test"},
                "reason":"unsupported_runner", "scope":"configured_default"
            }])
        );
        assert!(
            result["selected_invocations"][0]["invocation"]["arguments"]
                .as_object()
                .is_none_or(|args| args.is_empty())
        );
        assert_eq!(
            result["selected_invocations"][0]["invocation"]["runner"]["command"],
            "api_test_command"
        );
    }
    let before = fs::read(temp.path().join(".agent/launch.log")).unwrap();
    for (target, focus, message) in [
        ("api:test", focus(), "unsupported"),
        ("web:test", json!({"kind":"automatic"}), "not selected"),
    ] {
        let error =
            crate::runtime::call_tool(&ctx, tool::WORK_CHECK, request(target, focus, false))
                .unwrap_err();
        assert!(error.to_string().contains(message), "{error:#}");
    }
    assert_eq!(
        fs::read(temp.path().join(".agent/launch.log")).unwrap(),
        before
    );
}
