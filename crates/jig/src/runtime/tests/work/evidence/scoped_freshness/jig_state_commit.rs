use super::*;

use super::worktree::{check, inspect, invocations, worktree_fixture};

fn target<'a>(report: &'a Value, component: &str) -> &'a Value {
    report["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == component)
        .unwrap()
}

fn has_reason(target: &Value, code: &str) -> bool {
    target["freshness_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == code)
}

fn fingerprint(ctx: &RepoContext) -> String {
    crate::git_receipts::repository_source_snapshot(ctx.root())
        .unwrap()
        .worktree_fingerprint
}

fn assert_clean(root: &Path) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
}

fn assert_state_only_commit(root: &Path) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let paths = String::from_utf8(output.stdout).unwrap();
    assert!(!paths.is_empty());
    assert!(
        paths.lines().all(|path| path.starts_with(".agent/state/")),
        "{paths}"
    );
}

#[test]
fn jig_only_commit_preserves_source_fingerprint_but_stales_git_checks() {
    for policy in ["exhaustive", "whole_repository"] {
        let temp = tempdir().unwrap();
        let (ctx, plan_id) = worktree_fixture(temp.path(), policy, Some("git"));
        fs::write(ctx.root().join(".gitignore"), ".scratch/\n.agent/.cache/\n").unwrap();
        run_git(ctx.root(), &["add", ".agent", ".gitignore"]);
        run_git(ctx.root(), &["commit", "-qm", "Example plan setup"]);
        let first = check(&ctx, &plan_id);
        assert_eq!(first["ok"], true, "{policy}: {first:#}");
        assert_eq!(inspect(&ctx, &plan_id)["overall"], "passed");
        let before = fingerprint(&ctx);
        let executions = invocations(&ctx);

        run_git(ctx.root(), &["add", ".agent/state"]);
        run_git(ctx.root(), &["commit", "-qm", "Record Jig state only"]);
        assert_state_only_commit(ctx.root());
        assert_clean(ctx.root());
        assert_eq!(fingerprint(&ctx), before, "{policy}");
        let stale = inspect(&ctx, &plan_id);
        assert_eq!(stale["overall"], "blocked", "{policy}: {stale:#}");
        assert!(has_reason(target(&stale, "api"), "git_identity_changed"));
        assert!(!has_reason(target(&stale, "api"), "direct_input_changed"));
        assert_eq!(target(&stale, "web")["status"], "passed");
        assert_eq!(invocations(&ctx), executions);
        let preview = crate::runtime::call_tool(
            &ctx,
            crate::tool_defs::tool::WORK_CHECK,
            json!({"plan_id":plan_id, "phase":"final", "explain":true}),
        )
        .unwrap();
        let api = preview["selected_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["component"] == "api")
            .unwrap();
        assert_eq!(api["disposition"], "selected", "{preview:#}");
        assert_eq!(invocations(&ctx), executions);
        run_git(ctx.root(), &["switch", "-qc", "example-branch"]);
        assert!(has_reason(
            target(&inspect(&ctx, &plan_id), "api"),
            "git_identity_changed"
        ));

        fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
        run_git(ctx.root(), &["add", "api/example.go"]);
        run_git(ctx.root(), &["commit", "-qm", "Change product source"]);
        assert_clean(ctx.root());
        assert_ne!(fingerprint(&ctx), before);
        let changed = inspect(&ctx, &plan_id);
        assert!(has_reason(target(&changed, "api"), "direct_input_changed"));
        assert!(!has_reason(target(&changed, "api"), "git_identity_changed"));
    }
}

#[test]
fn finish_before_state_commit_closes_native_git_gate_without_rerun() {
    let temp = tempdir().unwrap();
    let (ctx, _) = worktree_fixture(temp.path(), "whole_repository", Some("git"));
    fs::write(ctx.root().join(".gitignore"), ".scratch/\n.agent/.cache/\n").unwrap();
    super::native::configure_native(
        &ctx,
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"api/**\"]\nmax_lines=20\n",
    );
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
    run_git(ctx.root(), &["commit", "-qm", "Example native check setup"]);
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example native close".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    run_git(ctx.root(), &["add", ".agent"]);
    run_git(ctx.root(), &["commit", "-qm", "Example native plan setup"]);
    let first = check(&ctx, plan_id);
    assert_eq!(first["ok"], true, "{first:#}");
    let final_check = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id":plan_id, "phase":"final"}),
    )
    .unwrap();
    assert_eq!(final_check["ok"], true, "{final_check:#}");
    assert!(final_check["run"].is_null(), "{final_check:#}");
    assert_eq!(inspect(&ctx, plan_id)["overall"], "passed");
    let before = fingerprint(&ctx);
    let executions = invocations(&ctx);
    let finished = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_FINISH,
        json!({"plan_id":plan_id}),
    )
    .unwrap();
    assert_eq!(finished["ok"], true, "{finished:#}");
    assert_eq!(invocations(&ctx), executions);
    run_git(ctx.root(), &["add", ".agent/state"]);
    run_git(ctx.root(), &["commit", "-qm", "Record closed Jig plan"]);
    assert_state_only_commit(ctx.root());
    assert_clean(ctx.root());
    assert_eq!(fingerprint(&ctx), before);
    let after = inspect(&ctx, plan_id);
    assert_eq!(after["plan_state"], "closed", "{after:#}");
    assert!(has_reason(target(&after, "api"), "git_identity_changed"));
    assert_eq!(target(&after, "web")["status"], "passed");
}
