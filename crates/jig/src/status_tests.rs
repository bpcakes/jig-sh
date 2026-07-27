use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use jig_contract::status_provider::v1::Input;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;

const PROVIDER_ID: &str = "factorish.test-status";

fn report_value(outcome: &str, revision: Option<&str>) -> Value {
    let inputs = revision
        .map(|revision| {
            vec![json!({
                "name": "target",
                "kind": "git",
                "revision": revision,
            })]
        })
        .unwrap_or_default();
    json!({
        "protocol": "jig.status-provider/v1",
        "provider": {
            "id": PROVIDER_ID,
            "adapter_version": "1.0.0"
        },
        "observed_at_ms": 1_785_142_200_000_u64,
        "outcome": outcome,
        "inputs": inputs,
        "work_packages": [{
            "id": "WP-0001",
            "title": "Test package",
            "specification": {
                "state": "ready",
                "category": "ready"
            },
            "implementation": {
                "state": "in_progress",
                "category": "active"
            },
            "verification": {
                "state": "unverified",
                "category": "pending"
            },
            "acceptance_checks": [{
                "ordinal": 1,
                "state": "covered",
                "category": "complete"
            }],
            "blockers": [{
                "code": "dependency_not_verified",
                "message": "A dependency is not verified"
            }]
        }],
        "diagnostics": []
    })
}

fn provider(argv: Vec<String>) -> StatusProviderConfig {
    StatusProviderConfig {
        id: PROVIDER_ID.into(),
        argv,
        timeout_seconds: 2,
    }
}

#[test]
fn decoder_preserves_additive_fields_and_builds_normalized_summary() {
    let mut value = report_value("complete", None);
    value["future_report_field"] = json!({"kept": true});
    value["provider"]["future_provider_field"] = json!("also kept");
    let decoded = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&value).unwrap(),
    )
    .unwrap();

    assert_eq!(decoded.raw["future_report_field"]["kept"], true);
    assert_eq!(
        decoded.raw["provider"]["future_provider_field"],
        "also kept"
    );
    let summary = serde_json::to_value(ProviderSummary::from_report(&decoded.decoded)).unwrap();
    assert_eq!(summary["work_packages"], 1);
    assert_eq!(summary["work_packages_with_blockers"], 1);
    assert_eq!(summary["blockers"], 1);
    assert_eq!(summary["implementation"]["active"], 1);
    assert_eq!(summary["verification"]["pending"], 1);
    assert_eq!(summary["acceptance"]["complete"], 1);
}

#[test]
fn decoder_rejects_multiple_documents_identity_mismatch_and_semantic_errors() {
    let valid = serde_json::to_string(&report_value("complete", None)).unwrap();
    let multiple = decode_report(
        &provider(vec!["unused".into()]),
        &format!("{valid}\n{valid}"),
    )
    .unwrap_err();
    assert_eq!(multiple.code, "invalid_json");

    let mut mismatch = report_value("complete", None);
    mismatch["provider"]["id"] = json!("factorish.other");
    let mismatch = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&mismatch).unwrap(),
    )
    .unwrap_err();
    assert_eq!(mismatch.code, "provider_id_mismatch");

    let mut invalid = report_value("complete", None);
    invalid["work_packages"][0]["acceptance_checks"][0]["ordinal"] = json!(0);
    let invalid = decode_report(
        &provider(vec!["unused".into()]),
        &serde_json::to_string(&invalid).unwrap(),
    )
    .unwrap_err();
    assert_eq!(invalid.code, "invalid_report");
    assert!(invalid.message.contains("validation error"));
}

#[cfg(unix)]
#[test]
fn runner_accepts_one_valid_document_and_never_trusts_nonzero_stdout() {
    let temp = tempdir().unwrap();
    let report = temp.path().join("report.json");
    fs::write(
        &report,
        serde_json::to_vec(&report_value("complete", None)).unwrap(),
    )
    .unwrap();

    let valid = provider(vec!["cat".into(), report.display().to_string()]);
    let output = run_provider_inner(temp.path(), &valid).unwrap();
    assert_eq!(output.decoded.provider.id, PROVIDER_ID);

    let failed = provider(vec![
        "sh".into(),
        "-c".into(),
        "printf '{\"protocol\":\"jig.status-provider/v1\"}'; printf 'provider broke' >&2; exit 7"
            .into(),
    ]);
    let failure = run_provider_inner(temp.path(), &failed).unwrap_err();
    assert_eq!(failure.code, "exit_nonzero");
    assert_eq!(failure.exit_status, Some(7));
    assert_eq!(failure.stderr.as_deref(), Some("provider broke"));
}

#[cfg(unix)]
#[test]
fn runner_maps_timeout_non_utf8_and_bounded_output_failures() {
    let mut timed_out = provider(vec!["sh".into(), "-c".into(), "sleep 3".into()]);
    timed_out.timeout_seconds = 1;
    let failure = run_provider_inner(tempdir().unwrap().path(), &timed_out).unwrap_err();
    assert_eq!(failure.code, "timed_out");

    let non_utf8 = provider(vec!["sh".into(), "-c".into(), "printf '\\377'".into()]);
    let failure = run_provider_inner(tempdir().unwrap().path(), &non_utf8).unwrap_err();
    assert_eq!(failure.code, "stdout_not_utf8");

    let oversized = provider(vec![
        "sh".into(),
        "-c".into(),
        "printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'".into(),
    ]);
    let failure =
        run_provider_inner_with_limits(tempdir().unwrap().path(), &oversized, 8, 64).unwrap_err();
    assert_eq!(failure.code, "stdout_limit_exceeded");
    assert!(failure.message.contains("8 byte limit"));
}

#[cfg(unix)]
#[test]
fn aggregate_joins_provider_git_work_gate_and_loop_state_without_writes() {
    let outer = tempdir().unwrap();
    let root = outer.path().join("repo");
    let report_path = root.join("provider-report.json");
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "status-fixture"
default_branch = "main"
jig_version = "0.2.0-beta.1"
sqlx_enabled = false

[[status.providers]]
id = "{PROVIDER_ID}"
argv = ["cat", "provider-report.json"]
timeout_seconds = 2
"#
        ),
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "provider-report.json\n").unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": []
        }))
        .unwrap(),
    )
    .unwrap();
    init_git_repo(&root);
    let original_revision = git_text_for_test(&root, &["rev-parse", "HEAD"]);
    fs::write(
        &report_path,
        serde_json::to_vec(&report_value("complete", Some(&original_revision))).unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(&root).unwrap();
    let current = snapshot(&ctx).unwrap();
    assert_eq!(current["outcome"], "complete");
    assert_eq!(current["repository"]["dirty"], false);
    assert_eq!(current["work"]["state"]["counts"]["open_plans"], 0);
    assert_eq!(current["work"]["gates"], json!([]));
    assert!(current["loops"]["leases"].as_array().unwrap().is_empty());
    assert_eq!(current["providers"][0]["status"], "complete");
    assert_eq!(
        current["providers"][0]["input_freshness"][0]["status"],
        "current"
    );
    // A domain blocker is a trustworthy fact, not a collection failure.
    assert_eq!(current["providers"][0]["summary"]["blockers"], 1);
    assert!(!root.join(".agent/state").exists());

    fs::write(root.join("untracked.txt"), "local change").unwrap();
    let dirty = snapshot(&ctx).unwrap();
    assert_eq!(dirty["outcome"], "complete");
    assert_eq!(dirty["repository"]["dirty"], true);
    assert_eq!(
        dirty["providers"][0]["input_freshness"][0]["status"],
        "dirty"
    );

    git(&root, &["add", "untracked.txt"]);
    git(&root, &["commit", "-m", "advance target"]);
    let stale = snapshot(&ctx).unwrap();
    assert_eq!(stale["repository"]["dirty"], false);
    assert_eq!(
        stale["providers"][0]["input_freshness"][0]["status"],
        "stale"
    );
    assert_eq!(stale["outcome"], "complete");

    let partial_report = report_value(
        "partial",
        Some(&git_text_for_test(&root, &["rev-parse", "HEAD"])),
    );
    fs::write(&report_path, serde_json::to_vec(&partial_report).unwrap()).unwrap();
    let partial = snapshot(&ctx).unwrap();
    assert_eq!(partial["providers"][0]["status"], "partial");
    assert_eq!(partial["outcome"], "partial");

    fs::write(&report_path, "not JSON").unwrap();
    let failed = snapshot(&ctx).unwrap();
    assert_eq!(failed["providers"][0]["status"], "failed");
    assert_eq!(failed["providers"][0]["report"], Value::Null);
    assert_eq!(failed["providers"][0]["error"]["code"], "invalid_json");
    assert_eq!(failed["outcome"], "partial");
    assert!(!root.join(".agent/state").exists());
}

#[test]
fn non_git_inputs_are_explicitly_not_applicable() {
    let input = Input::new("catalog", "document_catalog");
    let freshness = input_freshness(Path::new("."), &input, &mut BTreeMap::new());
    let value = serde_json::to_value(freshness).unwrap();

    assert_eq!(value["status"], "not_applicable");
    assert_eq!(value["observed_revision"], Value::Null);
}

#[cfg(unix)]
#[test]
fn nested_git_inputs_use_the_reported_repository_relative_checkout() {
    let root = tempdir().unwrap();
    let legacy = root.path().join("legacy/hocr");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("README.md"), "legacy fixture").unwrap();
    init_git_repo(&legacy);
    let revision = git_text_for_test(&legacy, &["rev-parse", "HEAD"]);
    let mut input = Input::new("legacy", "git");
    input.path = Some("legacy/hocr".into());
    input.revision = Some(revision.clone());

    let current = input_freshness(root.path(), &input, &mut BTreeMap::new());
    assert_eq!(current.status, "current");
    assert_eq!(
        current.observed_revision.as_deref(),
        Some(revision.as_str())
    );

    fs::write(legacy.join("untracked.txt"), "change").unwrap();
    let dirty = input_freshness(root.path(), &input, &mut BTreeMap::new());
    assert_eq!(dirty.status, "dirty");
}

#[cfg(unix)]
fn init_git_repo(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "fixture@example.com"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial fixture"]);
}

#[cfg(unix)]
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn git_text_for_test(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
