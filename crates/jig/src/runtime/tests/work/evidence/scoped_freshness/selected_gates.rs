use super::*;

fn selected_fixture(root: &Path, dependency: bool) -> RepoContext {
    let ctx = fixture(root, dependency, true);
    let path = root.join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config["commands"]["api_test_command"] =
        toml::Value::String("printf 'api\\n' >> .scratch/launches; test ! -f .scratch/fail".into());
    config["commands"]["web_test_command"] =
        toml::Value::String("printf 'web\\n' >> .scratch/launches".into());
    let gates = config["work"]["gates"].as_array_mut().unwrap();
    gates.push(
        toml::from_str::<toml::Value>(
            "id = 'optional'\nkind = 'evidence'\ntarget = 'web:test'\nrequired = false",
        )
        .unwrap(),
    );
    gates.push(
        toml::from_str::<toml::Value>("id = 'api'\nkind = 'evidence'\ntarget = 'api:test'")
            .unwrap(),
    );
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(root.join(".gitignore"), ".scratch/\n").unwrap();
    fs::create_dir(root.join(".scratch")).unwrap();
    RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap()
}

fn selected(ctx: &RepoContext, gates: &[&str]) -> Result<Value> {
    crate::runtime::call_tool(
        ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id": "plan_1", "gates": gates}),
    )
}

fn launches(ctx: &RepoContext) -> Vec<String> {
    fs::read_to_string(ctx.root().join(".scratch/launches"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn selected_native_gates_force_deduplicate_and_preserve_default_reuse() {
    let temp = tempdir().unwrap();
    let ctx = selected_fixture(temp.path(), true);
    for expected in [2, 4] {
        let result = selected(&ctx, &["verify", "optional", "verify"]).unwrap();
        assert_eq!(result["ok"], true, "{result:#}");
        assert_eq!(launches(&ctx).len(), expected);
        assert_eq!(&launches(&ctx)[expected - 2..], ["api", "web"]);
        assert_eq!(result["run"]["targets"].as_array().unwrap().len(), 2);
        for target in result["target_evidence"].as_array().unwrap() {
            assert_eq!(target["disposition"], "executed");
            assert_eq!(target["original_plan_id"], "plan_1");
        }
    }
    let reused = selected(&ctx, &[]).unwrap();
    assert_eq!(reused["ok"], true, "{reused:#}");
    assert!(reused["run"].is_null());
    assert_eq!(launches(&ctx).len(), 4);
}

#[test]
fn optional_native_selection_does_not_require_unselected_gates_or_allow_finish() {
    let temp = tempdir().unwrap();
    let ctx = selected_fixture(temp.path(), false);
    let result = selected(&ctx, &["optional"]).unwrap();
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(launches(&ctx), ["web"]);
    assert_eq!(result["target_evidence"].as_array().unwrap().len(), 1);
    let finish = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_FINISH,
        json!({"plan_id": "plan_1"}),
    );
    assert!(finish.is_err(), "{finish:#?}");
}

#[test]
fn invalid_gate_selection_starts_nothing_and_writes_no_receipts() {
    let temp = tempdir().unwrap();
    let ctx = selected_fixture(temp.path(), false);
    let journal = ctx.state_file("receipts.jsonl");
    let before = fs::read(&journal).ok();
    let error = selected(&ctx, &["verify", "missing"]).unwrap_err();
    assert!(error.to_string().contains("Unknown configured check gate"));
    assert!(launches(&ctx).is_empty());
    assert_eq!(fs::read(&journal).ok(), before);
}

#[test]
fn mixed_native_and_legacy_gates_share_cli_and_mcp_selection() {
    for cli in [false, true] {
        let temp = tempdir().unwrap();
        let ctx = selected_fixture(temp.path(), false);
        let config_path = ctx.root().join(".jig.toml");
        let mut config: toml::Value =
            toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        config["work"]["gates"].as_array_mut().unwrap().push(
            toml::from_str::<toml::Value>(
                "id = 'legacy'\nkind = 'check'\ntool = 'jig.example_check'",
            )
            .unwrap(),
        );
        config["repository"]["actions"][0]
            .as_table_mut()
            .unwrap()
            .insert(
                "legacy_aliases".into(),
                toml::Value::try_from(vec!["jig.example_check"]).unwrap(),
            );
        let manifest_path = ctx.root().join(".agent/jig-contract.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["tools"] = json!([{"name": "jig.example_check", "kind": "command", "description": "Example check", "command": "api_test_command"}]);
        manifest["actions"][0]["legacy_aliases"] = json!(["jig.example_check"]);
        fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
        fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
        let result = if cli {
            dispatch(
                &ctx,
                CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
                    projection: crate::surface::ResponseSurface::Standard,
                    plan_id: "plan_1".into(),
                    gates: vec!["legacy".into(), "optional".into()],
                    tools: vec![],
                })),
            )
        } else {
            selected(&ctx, &["legacy", "optional"])
        }
        .unwrap();
        assert_eq!(result["ok"], true, "{result:#}");
        assert_eq!(launches(&ctx), ["api", "web"]);
        assert_eq!(result["checks"].as_array().unwrap().len(), 1);
        assert_eq!(result["target_evidence"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn review_and_unsupported_gates_are_rejected_before_native_execution() {
    for (kind, extra, message) in [
        ("codex_review", "skill = 'example-review'", "work review"),
        ("future_gate", "", "Unsupported"),
    ] {
        let temp = tempdir().unwrap();
        let ctx = selected_fixture(temp.path(), false);
        let path = ctx.root().join(".jig.toml");
        let mut config = fs::read_to_string(&path).unwrap();
        config.push_str(&format!(
            "\n[[work.gates]]\nid = 'invalid'\nkind = '{kind}'\n{extra}\n"
        ));
        fs::write(path, config).unwrap();
        let result = RepoContext::load_from_root(ctx.root().to_path_buf())
            .and_then(|ctx| selected(&ctx, &["verify", "invalid"]));
        let error = result.unwrap_err();
        assert!(error.to_string().contains(message), "{error:#}");
        assert!(launches(&ctx).is_empty());
    }
}

#[test]
fn closed_or_cancelled_native_selection_starts_nothing() {
    struct Cancelled;
    impl crate::execution::ExecutionObserver for Cancelled {}
    impl crate::execution::ExecutionCancellation for Cancelled {
        fn cancelled(&self) -> bool {
            true
        }
    }
    let temp = tempdir().unwrap();
    let ctx = selected_fixture(temp.path(), false);
    let cancelled = crate::runtime::call_tool_with_observer(
        &ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id": "plan_1", "gates": ["verify"]}),
        &mut Cancelled,
    );
    assert!(cancelled.is_err());
    assert!(launches(&ctx).is_empty());
    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_1".into(),
            resolution: Some("Example closed plan".into()),
        },
    )
    .unwrap();
    assert!(
        selected(&ctx, &["verify"])
            .unwrap_err()
            .to_string()
            .contains("already closed")
    );
    assert!(launches(&ctx).is_empty());
}

#[test]
fn failed_prerequisite_skips_dependent_and_default_retry_only_repairs_failure() {
    for dependency in [false, true] {
        let temp = tempdir().unwrap();
        let ctx = selected_fixture(temp.path(), dependency);
        fs::write(ctx.root().join(".scratch/fail"), "").unwrap();
        assert!(selected(&ctx, &["verify"]).is_err());
        let initial = launches(&ctx);
        assert_eq!(initial.len(), if dependency { 1 } else { 2 });
        fs::remove_file(ctx.root().join(".scratch/fail")).unwrap();
        let repaired = selected(&ctx, &[]).unwrap();
        assert_eq!(repaired["ok"], true, "{repaired:#}");
        assert_eq!(
            launches(&ctx).len() - initial.len(),
            if dependency { 2 } else { 1 }
        );
    }
}
