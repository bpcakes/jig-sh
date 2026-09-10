use super::*;

fn configure_native(ctx: &RepoContext, policy: &str) {
    let runner = jig_contract::ActionRunner::native_configured(
        jig_contract::tool::FILE_BUDGET,
        jig_contract::NativeActionConfigurationV1::file_budget(
            jig_contract::NativeFileBudgetConfigV1::default(),
        ),
    );
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"][0]["runner"] = toml::Value::try_from(&runner).unwrap();
    config["repository"]["actions"][0]["inputs"] = toml::Value::try_from(vec!["**"]).unwrap();
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let manifest_path = ctx.root().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["actions"][0]["runner"] = json!(runner);
    manifest["actions"][0]["inputs"] = json!(["**"]);
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::create_dir_all(ctx.root().join(".jig")).unwrap();
    fs::write(ctx.root().join(".jig/file-budget.toml"), policy).unwrap();
}

#[test]
fn native_dependency_preserves_prepared_plan_authority_and_inherits_waiver_expiry() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    configure_native(
        &ctx,
        r#"version=1
[[rules]]
id="source"
include=["api/**"]
max_lines=1
[[waivers]]
id="example-waiver"
rule="source"
path="api/example.go"
ceiling_lines=4
reason="Example staged split"
expires=2099-12-31
"#,
    );
    fs::write(
        ctx.root().join("api/example.go"),
        "package example\n// initial\n",
    )
    .unwrap();
    run_git(ctx.root(), &["add", "."]);
    run_git(ctx.root(), &["commit", "-qm", "Example native authority"]);
    let ctx = RepoContext::load_freshness_fixture(ctx.root().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example native inherited validity".into(),
            body: Some("Verify prepared comparison authority and inherited expiry.".into()),
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    fs::write(
        ctx.root().join("api/example.go"),
        "package example\n// initial\n// bounded addition\n",
    )
    .unwrap();
    let result = run_repository_target_for_plan(&ctx, "web:test", plan_id);
    assert_eq!(result["ok"], true, "{result:#}");
    let targets = result["run"]["targets"].as_array().unwrap();
    let native = targets
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    let parent = targets
        .iter()
        .find(|target| target["target"]["component"] == "web")
        .unwrap();
    assert_eq!(
        native["target_freshness"]["state"], "complete",
        "{result:#}"
    );
    assert_eq!(
        parent["target_freshness"]["state"], "complete",
        "{result:#}"
    );
    let boundary = native["valid_until_ms"]
        .as_u64()
        .expect("active native waiver has an expiry");
    assert!(parent["valid_until_ms"].is_null());
    assert_eq!(
        parent["target_freshness"]["effective_valid_until_ms"],
        boundary
    );
    assert_eq!(
        parent["target_freshness"]["effective_requires_time_validity"],
        true
    );
    let gates = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_GATES,
        json!({"plan_id": plan_id, "freshness_timeout_ms": 30_000}),
    )
    .unwrap();
    assert_eq!(gates["overall"], "passed", "{gates:#}");
    assert_eq!(gates["gates"][0]["effective_valid_until_ms"], boundary);
    assert!(gates["gates"][0]["targets"][0]["valid_until_ms"].is_null());
    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert!(checked["run"].is_null(), "{checked:#}");
    assert_eq!(checked["effective_valid_until_ms"], boundary);
}

#[test]
fn completed_native_failure_has_complete_freshness_and_blocks_dependents() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, true);
    configure_native(
        &ctx,
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"api/**\"]\nmax_lines=1\n",
    );
    run_git(ctx.root(), &["add", "."]);
    run_git(
        ctx.root(),
        &["commit", "-qm", "Example native failure authority"],
    );
    let ctx = RepoContext::load_freshness_fixture(ctx.root().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example native failure".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    fs::write(
        ctx.root().join("api/example.go"),
        "package example\n// excessive addition\n",
    )
    .unwrap();
    let result = run_repository_target_for_plan(&ctx, "web:test", plan_id);
    assert_eq!(result["ok"], false, "{result:#}");
    let targets = result["run"]["targets"].as_array().unwrap();
    let native = targets
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert_eq!(native["conclusion"], "failure", "{result:#}");
    assert_eq!(
        native["target_freshness"]["state"], "complete",
        "{result:#}"
    );
    let parent = targets
        .iter()
        .find(|target| target["target"]["component"] == "web")
        .unwrap();
    assert_eq!(parent["conclusion"], "skipped", "{result:#}");
    assert_eq!(
        parent["target_freshness"]["state"], "incomplete",
        "{result:#}"
    );
    let gates = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_GATES,
        json!({"plan_id": plan_id, "freshness_timeout_ms": 30_000}),
    )
    .unwrap();
    assert_eq!(gates["overall"], "blocked", "{gates:#}");
    assert_eq!(gates["gates"][0]["status"], "failed", "{gates:#}");
    let native_gate = gates["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert_eq!(native_gate["freshness"], "fresh", "{gates:#}");
    assert_eq!(native_gate["receipt_id"], native["receipt_id"], "{gates:#}");
}
