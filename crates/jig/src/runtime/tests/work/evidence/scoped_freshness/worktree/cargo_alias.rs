use super::*;
use crate::repository::freshness::adoption::{Request, preview};
use std::process::Command;

#[test]
fn cargo_fmt_alias_requires_fresh_execution_after_staging() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    worktree_fixture(root, "whole_repository", None);
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"format-check\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        r#"fn main() {
    use std::io::Write;
    let mut log = std::fs::OpenOptions::new().create(true).append(true)
        .open(".scratch/alias-invocations").unwrap();
    writeln!(log, "fmt").unwrap();
    let status = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"]).status().unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
"#,
    )
    .unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[alias]\nfmt = [\"run\", \"--offline\", \"--quiet\", \"--target-dir\", \".scratch/alias-target\", \"--bin\", \"format-check\", \"--\"]\n",
    )
    .unwrap();
    // Prepare all build outputs and the lockfile before receipt collection.
    let built = Command::new("cargo")
        .args([
            "build",
            "--offline",
            "--quiet",
            "--target-dir",
            ".scratch/alias-target",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["api_test_command"] = "cargo fmt --all -- --check".into();
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "Example Cargo alias"]);
    let ctx = RepoContext::load_from_root(root.to_path_buf()).unwrap();
    let preview = preview(
        &ctx,
        &Request {
            targets: vec!["api:test".parse().unwrap()],
            patch: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        preview["targets"][0]["reason"],
        "formatter_requires_assertion"
    );
    assert_eq!(preview["targets"][0]["proposed"]["source_state"], "git");
    assert_eq!(preview["patch"], "");
    let plan = crate::state::plans_open(
        &ctx,
        crate::state::PlanOpenRequest {
            title: "Example alias freshness".into(),
            body: None,
            body_file: None,
            base: Some("HEAD".into()),
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    fs::write(root.join("api/example.go"), "package example\n// edited\n").unwrap();
    let first = check(&ctx, plan_id);
    assert_eq!(first["ok"], true, "{first:#}");
    let first_receipts = receipt_ids(&first);
    assert_eq!(
        fs::read_to_string(root.join(".scratch/alias-invocations")).unwrap(),
        "fmt\n"
    );

    run_git(root, &["add", "api/example.go"]);
    let staged = inspect(&ctx, plan_id);
    assert_eq!(staged["overall"], "blocked", "{staged:#}");
    assert_eq!(
        staged["recovery"]["execute"],
        json!(["api:test".parse::<TargetId>().unwrap()])
    );
    let failure = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_CHECK,
        json!({"plan_id": plan_id}),
    )
    .unwrap_err();
    assert!(
        failure.to_string().contains("api:test (failure)"),
        "{failure:#}"
    );
    let rerun = inspect(&ctx, plan_id);
    assert_eq!(rerun["gates"][0]["status"], "failed", "{rerun:#}");
    let failed = rerun["gates"][0]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["component"] == "api")
        .unwrap();
    assert_eq!(failed["exit_status"], 1);
    assert_ne!(failed["receipt_id"], first_receipts["api"]);
    assert_eq!(
        fs::read_to_string(root.join(".scratch/alias-invocations")).unwrap(),
        "fmt\nfmt\n"
    );
}
