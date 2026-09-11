use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use jig_contract::{ComponentId, ComponentSpec};
use serde_json::{Value, json};

use crate::bootstrap::file_budget_lifecycle::{
    generated_file_budget_action, validate_receipt_proof_with_context,
};
use crate::context::RepoContext;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lifecycle_freshness_keeps_global_native_proof_and_rejects_unusable_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    crate::test_env::TestRepoBuilder::new(root)
        .repo_name("ExampleProject")
        .contract_version(8)
        .required_commands(std::iter::empty::<String>())
        .write();
    let components = vec![ComponentSpec::new(ComponentId::parse("repo").unwrap(), ".")];
    let actions = vec![generated_file_budget_action().unwrap()];
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config.as_table_mut().unwrap().insert(
        "repository".into(),
        toml::Value::try_from(json!({"components": components, "actions": actions, "profiles": [{"id":"verify", "targets":[actions[0].target]}], "default_check_profile":"verify"})).unwrap(),
    );
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["components"] = json!(components);
    manifest["actions"] = json!(actions);
    manifest["profiles"] = json!([{"id":"verify", "targets":[actions[0].target]}]);
    manifest["default_check_profile"] = json!("verify");
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::create_dir_all(root.join(".jig")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/example.rs"), "fn example() {}\n").unwrap();
    fs::write(
        root.join(".jig/file-budget.toml"),
        "version=1\n[[rules]]\nid=\"source\"\ninclude=[\"src/**\"]\nmax_lines=100\n",
    )
    .unwrap();
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.name", "Example Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "Example lifecycle fixture"]);
    let ctx = RepoContext::load_from_root(root.to_path_buf()).unwrap();
    let opts = crate::cli::CheckOpts {
        tool: crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        },
        profile: None,
        affected: None,
        explain: false,
        fail_fast: false,
        comparison: crate::cli::CheckComparisonOpts::default(),
        command: Some(crate::cli::CheckCommand::Selectors(vec![
            "repo:file-budget".into(),
        ])),
    };
    let result = crate::runtime::dispatch(
        &ctx,
        crate::command::RuntimeCommand::Check(opts.try_into().unwrap()),
    )
    .unwrap();
    assert_eq!(result["ok"], true, "{result:#}");
    assert_eq!(
        result["run"]["targets"][0]["target_freshness"]["state"], "complete",
        "{result:#}"
    );
    let proof = validate_receipt_proof_with_context(&ctx).unwrap();
    assert_eq!(
        proof.effective_time,
        Some(jig_contract::freshness::EffectiveTimeValidityV1::default())
    );
    fs::write(root.join("unrelated.txt"), "Example unrelated source\n").unwrap();
    let error = validate_receipt_proof_with_context(&ctx).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("stale for current repository source"),
        "{error:#}"
    );
    fs::remove_file(root.join("unrelated.txt")).unwrap();
    let path = ctx.state_file("receipts.jsonl");
    let journal = fs::read_to_string(&path).unwrap();
    let original = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|receipt| receipt["target"].is_object())
        .unwrap();
    for (index, metadata) in [Value::Null, json!({"schema_version": 99})]
        .into_iter()
        .enumerate()
    {
        let mut receipt = original.clone();
        receipt["id"] = json!(format!("receipt_unusable_{}", index));
        receipt["ended_at_ms"] =
            json!(original["ended_at_ms"].as_u64().unwrap() + index as u64 + 1);
        if metadata.is_null() {
            receipt.as_object_mut().unwrap().remove("target_freshness");
        } else {
            receipt["target_freshness"] = metadata;
        }
        writeln!(
            fs::OpenOptions::new().append(true).open(&path).unwrap(),
            "{receipt}"
        )
        .unwrap();
        let error = validate_receipt_proof_with_context(&ctx).unwrap_err();
        assert!(
            error.to_string().contains("unusable target freshness"),
            "{error:#}"
        );
    }
}
