use super::*;

fn worktree_fixture(root: &Path, policy: &str, api_source: Option<&str>) -> (RepoContext, String) {
    let ctx = fixture(root, false, true);
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(10);
    for (index, action) in config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        action["inputs_policy"] = toml::Value::String(policy.into());
        if let Some(source) = if index == 0 {
            api_source
        } else {
            Some("worktree")
        } {
            action
                .as_table_mut()
                .unwrap()
                .insert("source_state".into(), toml::Value::String(source.into()));
        }
    }
    for (index, action) in manifest["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        action["inputs_policy"] = json!(policy);
        if let Some(source) = if index == 0 {
            api_source
        } else {
            Some("worktree")
        } {
            action["source_state"] = json!(source);
        }
    }
    config["commands"]["api_test_command"] =
        toml::Value::String("printf 'api\n' >> .scratch/invocations".into());
    config["commands"]["web_test_command"] =
        toml::Value::String("printf 'web\n' >> .scratch/invocations".into());
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(root.join(".gitignore"), ".scratch/\n").unwrap();
    fs::create_dir(root.join(".scratch")).unwrap();
    run_git(root, &["add", "."]);
    run_git(
        root,
        &["commit", "-qm", "Example worktree check configuration"],
    );
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example source placement".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    (ctx, plan["plan_id"].as_str().unwrap().to_owned())
}

fn check(ctx: &RepoContext, plan_id: &str) -> Value {
    crate::runtime::call_tool(
        ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id":plan_id}),
    )
    .unwrap_or_else(|error| panic!("{error:#}\n{:#}", inspect(ctx, plan_id)))
}

fn inspect(ctx: &RepoContext, plan_id: &str) -> Value {
    crate::runtime::call_tool(
        ctx,
        crate::tool_defs::tool::WORK_GATES,
        json!({"plan_id":plan_id,"freshness_timeout_ms":30000}),
    )
    .unwrap()
}

fn receipt_ids(checked: &Value) -> BTreeMap<String, Value> {
    checked["target_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| {
            (
                target["target"]["component"].as_str().unwrap().to_owned(),
                target["receipt_id"].clone(),
            )
        })
        .collect()
}

fn invocations(ctx: &RepoContext) -> String {
    fs::read_to_string(ctx.root().join(".scratch/invocations")).unwrap()
}

#[test]
fn worktree_checks_reuse_original_passes_after_staging_and_commit_for_both_input_policies() {
    for policy in ["exhaustive", "whole_repository"] {
        let temp = tempdir().unwrap();
        let (ctx, plan_id) = worktree_fixture(temp.path(), policy, Some("worktree"));
        fs::write(
            ctx.root().join("api/example.go"),
            "package example\n// checked edit\n",
        )
        .unwrap();
        let first = check(&ctx, &plan_id);
        assert_eq!(first["ok"], true, "{policy}: {first:#}");
        let originals = receipt_ids(&first);
        assert_eq!(originals.len(), 2);
        let executions = invocations(&ctx);
        assert_eq!(executions.lines().count(), 2);
        run_git(ctx.root(), &["add", "api/example.go"]);
        let staged = inspect(&ctx, &plan_id);
        assert_eq!(staged["overall"], "passed", "{policy}: {staged:#}");
        assert_eq!(staged["recovery"]["execute"], json!([]));
        let reused = check(&ctx, &plan_id);
        assert_eq!(reused["ok"], true, "{policy}: {reused:#}");
        assert!(reused["run"].is_null(), "{policy}: {reused:#}");
        assert_eq!(receipt_ids(&reused), originals);
        assert_eq!(invocations(&ctx), executions);
        run_git(ctx.root(), &["commit", "-qm", "Example checked edit"]);
        let committed = inspect(&ctx, &plan_id);
        assert_eq!(committed["overall"], "passed", "{policy}: {committed:#}");
        let reused = check(&ctx, &plan_id);
        assert_eq!(reused["ok"], true, "{policy}: {reused:#}");
        assert!(reused["run"].is_null(), "{policy}: {reused:#}");
        assert_eq!(receipt_ids(&reused), originals);
        assert_eq!(invocations(&ctx), executions);
        fs::write(
            ctx.root().join("api/example.go"),
            "package example\n// different contents\n",
        )
        .unwrap();
        let changed = inspect(&ctx, &plan_id);
        assert_eq!(changed["overall"], "blocked", "{policy}: {changed:#}");
        let scheduled = changed["recovery"]["execute"].as_array().unwrap();
        assert_eq!(
            scheduled.len(),
            if policy == "exhaustive" { 1 } else { 2 },
            "{changed:#}"
        );
        assert_eq!(
            invocations(&ctx),
            executions,
            "inspection launched a process"
        );
    }
}

#[test]
fn git_default_keeps_staging_and_commit_sensitive_evidence() {
    for source in [None, Some("git")] {
        let temp = tempdir().unwrap();
        let (ctx, plan_id) = worktree_fixture(temp.path(), "exhaustive", source);
        fs::write(
            ctx.root().join("api/example.go"),
            "package example\n// checked edit\n",
        )
        .unwrap();
        let first = check(&ctx, &plan_id);
        assert_eq!(first["ok"], true, "{first:#}");
        let executions = invocations(&ctx);
        run_git(ctx.root(), &["add", "api/example.go"]);
        let staged = inspect(&ctx, &plan_id);
        assert_eq!(staged["overall"], "blocked", "{staged:#}");
        assert_eq!(
            staged["recovery"]["execute"],
            json!(["api:test".parse::<TargetId>().unwrap()])
        );
        let checked = check(&ctx, &plan_id);
        assert_eq!(checked["ok"], true, "{checked:#}");
        assert_ne!(receipt_ids(&checked)["api"], receipt_ids(&first)["api"]);
        assert_eq!(receipt_ids(&checked)["web"], receipt_ids(&first)["web"]);
        assert_eq!(&invocations(&ctx)[executions.len()..], "api\n");
        run_git(ctx.root(), &["commit", "-qm", "Example checked Git input"]);
        let committed = inspect(&ctx, &plan_id);
        assert_eq!(committed["overall"], "blocked", "{committed:#}");
        assert_eq!(
            committed["recovery"]["execute"],
            json!(["api:test".parse::<TargetId>().unwrap()])
        );
    }
}

#[test]
fn native_git_check_preserves_exact_plan_baseline_across_commit() {
    let temp = tempdir().unwrap();
    let (ctx, _) = worktree_fixture(temp.path(), "exhaustive", Some("git"));
    super::native::configure_native(
        &ctx,
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"api/**\"]\nmax_lines=20\n",
    );
    // Native file-budget requires inputs=["**"]; whole policy excludes the
    // ignored invocation log without claiming it as an exhaustive input.
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"][0]["inputs_policy"] =
        toml::Value::String("whole_repository".into());
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let manifest_path = ctx.root().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["actions"][0]["inputs_policy"] = json!("whole_repository");
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    run_git(ctx.root(), &["add", "."]);
    run_git(ctx.root(), &["commit", "-qm", "Example native Git check"]);
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example native baseline".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    let baseline = inspect(&ctx, plan_id)["plan_baseline"].clone();
    fs::write(
        ctx.root().join("api/example.go"),
        "package example\n// checked native edit\n",
    )
    .unwrap();
    let first = check(&ctx, plan_id);
    assert_eq!(first["ok"], true, "{first:#}");
    run_git(ctx.root(), &["add", "api/example.go"]);
    run_git(
        ctx.root(),
        &["commit", "-qm", "Example native checked edit"],
    );
    let committed = inspect(&ctx, plan_id);
    assert_eq!(committed["plan_baseline"], baseline);
    assert_eq!(committed["overall"], "blocked", "{committed:#}");
    assert_eq!(
        committed["recovery"]["execute"],
        json!(["api:test".parse::<TargetId>().unwrap()])
    );
    let refreshed = check(&ctx, plan_id);
    assert_eq!(refreshed["ok"], true, "{refreshed:#}");
    let native = refreshed["plan"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert_eq!(
        native["prepared_native_input"]["request"]["provenance"], "work_plan",
        "{native:#}"
    );
    assert_eq!(
        native["prepared_native_input"]["request"]["requested_oid"],
        baseline["commit_oid"]
    );
    assert_eq!(inspect(&ctx, plan_id)["overall"], "passed");
    assert_ne!(receipt_ids(&refreshed)["api"], receipt_ids(&first)["api"]);
    assert_eq!(receipt_ids(&refreshed)["web"], receipt_ids(&first)["web"]);
}

fn change_api_command(ctx: &RepoContext, command: &str) -> RepoContext {
    let path = ctx.root().join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config["commands"]["api_test_command"] = toml::Value::String(command.into());
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
    RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap()
}

#[test]
fn worktree_actions_cannot_record_reusable_proof_when_execution_stages_or_mutates_source() {
    for mutation in [
        "git add api/example.go",
        "printf '// mutation\\n' >> api/example.go",
    ] {
        let temp = tempdir().unwrap();
        let (ctx, plan_id) = worktree_fixture(temp.path(), "exhaustive", Some("worktree"));
        let ctx = change_api_command(
            &ctx,
            &format!("printf 'api\\n' >> .scratch/invocations; {mutation}"),
        );
        fs::write(
            ctx.root().join("api/example.go"),
            "package example\n// original checked edit\n",
        )
        .unwrap();
        let result = run_repository_target_for_plan(&ctx, "api:test", &plan_id);
        assert_eq!(result["ok"], false, "{mutation}: {result:#}");
        let target = &result["run"]["targets"][0];
        assert_eq!(
            target["target_freshness"]["state"], "incomplete",
            "{result:#}"
        );
        assert_eq!(
            target["target_freshness"]["global_execution_proof"]["state"], "mutated",
            "{result:#}"
        );
        assert!(
            target["target_freshness"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["code"] == "execution_mutated"),
            "{result:#}"
        );
        let response = inspect(&ctx, &plan_id);
        let api = response["gates"][0]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["target"]["component"] == "api")
            .unwrap();
        assert_ne!(api["status"], "passed", "{response:#}");
        assert_eq!(invocations(&ctx), "api\n", "inspection executed a check");
    }
}

#[test]
fn newer_failed_worktree_execution_blocks_an_older_pass_until_successful_retry() {
    let temp = tempdir().unwrap();
    let (ctx, plan_id) = worktree_fixture(temp.path(), "exhaustive", Some("worktree"));
    let ctx = change_api_command(
        &ctx,
        "printf 'api\n' >> .scratch/invocations; test ! -f .scratch/fail",
    );
    let first = check(&ctx, &plan_id);
    assert_eq!(first["ok"], true, "{first:#}");
    let originals = receipt_ids(&first);
    fs::write(
        ctx.root().join(".scratch/fail"),
        "Example controlled failure\n",
    )
    .unwrap();
    let failed = run_repository_target_for_plan(&ctx, "api:test", &plan_id);
    assert_eq!(failed["ok"], false, "{failed:#}");
    let failed_receipt = &failed["run"]["targets"][0]["receipt_id"];
    assert_ne!(*failed_receipt, originals["api"]);
    fs::remove_file(ctx.root().join(".scratch/fail")).unwrap();
    let executions = invocations(&ctx);
    assert_eq!(executions.lines().count(), 3);
    let response = inspect(&ctx, &plan_id);
    let api = response["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert_eq!(api["status"], "failed", "{response:#}");
    assert_eq!(&api["receipt_id"], failed_receipt);
    assert_eq!(invocations(&ctx), executions);
    let retry = check(&ctx, &plan_id);
    assert_eq!(retry["ok"], true, "{retry:#}");
    assert_eq!(&invocations(&ctx)[executions.len()..], "api\n");
    assert_ne!(receipt_ids(&retry)["api"], originals["api"]);
    assert_eq!(receipt_ids(&retry)["web"], originals["web"]);
}

#[test]
fn epoch_ten_whole_git_and_worktree_checks_support_an_unborn_repository() {
    let source = tempdir().unwrap();
    let (template, _) = worktree_fixture(source.path(), "whole_repository", None);
    let temp = tempdir().unwrap();
    for directory in [".agent", "api", "web", ".scratch"] {
        fs::create_dir_all(temp.path().join(directory)).unwrap();
    }
    for path in [
        ".jig.toml",
        ".agent/jig-contract.json",
        ".gitignore",
        "api/example.go",
        "web/example.ts",
    ] {
        fs::copy(template.root().join(path), temp.path().join(path)).unwrap();
    }
    run_git(temp.path(), &["init", "-b", "main"]);
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example unborn checks".into(),
            body: None,
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    let result = check(&ctx, plan_id);
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(invocations(&ctx).lines().count(), 2);
    assert_eq!(inspect(&ctx, plan_id)["overall"], "passed");
}

#[test]
fn whole_worktree_reuses_passes_when_explicit_tracker_metadata_changes_during_execution() {
    let temp = tempdir().unwrap();
    let (ctx, plan_id) = worktree_fixture(temp.path(), "whole_repository", Some("worktree"));
    fs::create_dir(ctx.root().join(".beads")).unwrap();
    fs::write(ctx.root().join(".beads/issues.jsonl"), "Example tracker\n").unwrap();
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["work"].as_table_mut().unwrap().insert(
        "receipt_metadata".into(),
        toml::Value::try_from(vec!["beads"]).unwrap(),
    );
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let ctx = change_api_command(
        &ctx,
        "printf 'api\n' >> .scratch/invocations; printf 'Updated example tracker\n' > .beads/issues.jsonl",
    );
    let first = check(&ctx, &plan_id);
    assert_eq!(first["ok"], true, "{first:#}");
    let originals = receipt_ids(&first);
    let executions = invocations(&ctx);
    fs::write(
        ctx.root().join(".beads/issues.jsonl"),
        "Another tracker update\n",
    )
    .unwrap();
    run_git(ctx.root(), &["add", ".beads/issues.jsonl"]);
    run_git(
        ctx.root(),
        &["commit", "-qm", "Example tracker-only update"],
    );
    let reused = check(&ctx, &plan_id);
    assert_eq!(reused["ok"], true, "{reused:#}");
    assert!(reused["run"].is_null(), "{reused:#}");
    assert_eq!(receipt_ids(&reused), originals);
    assert_eq!(invocations(&ctx), executions);
}
