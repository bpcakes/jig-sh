use serde_json::json;

use super::*;

#[test]
fn work_check_summary_reports_empty_checks() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": []
    }));

    assert!(summary.contains("Work check: no checks configured"));
    assert!(summary.contains("Checks: 0"));
    assert!(summary.contains("configure work checks"));
    assert!(summary.contains("--tool <tool>"));
}

#[test]
fn work_check_summary_reports_component_target_evidence() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": null,
        "checks": [],
        "run": {
            "conclusion": "success",
            "targets": [{
                "target": {"component": "api", "action": "test"},
                "conclusion": "success",
                "receipt_id": "receipt_api"
            }]
        }
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(summary.contains("Checks: 0"));
    assert!(summary.contains("Targets: 1"));
    assert!(summary.contains("api:test: success, receipt receipt_api"));
    assert!(!summary.contains("no checks configured"));
}

#[test]
fn work_check_summary_formats_executed_profile_payload() {
    use crate::context::RepoContext;
    use crate::runtime::call_tool;
    use crate::test_env::TestRepoBuilder;
    use jig_contract::tool;
    use std::fs;
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repository = json!({
        "default_check_profile": "verify",
        "components": [{"id": "example", "root": "."}],
        "actions": (["test", "lint"].map(|action| json!({
            "target": {"component": "example", "action": action},
            "intent": "check", "effects": ["read_only", "process"],
            "runner": {"kind": "command", "command": "example_check_command"},
            "inputs": ["example.txt"]
        }))),
        "profiles": [{"id": "verify", "targets": [
            {"component": "example", "action": "test"},
            {"component": "example", "action": "lint"}
        ]}]
    });
    TestRepoBuilder::new(root).repo_name("ExampleProject").contract_version(6)
        .config(toml::to_string(&json!({
            "commands": {"example_check_command": "printf 'example passed\\n'"},
            "repository": repository,
            "work": {"gates": [{"id": "verify", "kind": "evidence", "profile": "verify", "conclusion": "success"}]}
        })).unwrap()).write_config();
    let mut manifest = repository;
    manifest["contract_version"] = json!(6);
    manifest["tool_namespace"] = json!("jig");
    manifest["required_commands"] = json!(["example_check_command"]);
    manifest["tools"] = json!([]);
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(root.join("example.txt"), "Example input.\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Example",
            "-c",
            "user.email=example@example.invalid",
            "commit",
            "-qm",
            "Example baseline",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let ctx = RepoContext::load_from(root).unwrap();
    let opened = call_tool(
        &ctx,
        tool::WORK_START,
        json!({"title": "Example verification", "body": "Run fixture targets."}),
    )
    .unwrap();
    let plan_id = &opened["plan"]["plan_id"];
    let result = call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id": plan_id})).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["checks"], json!([]));
    assert_eq!(result["results"].as_array().unwrap().len(), 2);
    let summary = format_work_check_summary(&result);
    assert!(summary.contains("Work check: passed"), "{summary}");
    assert!(summary.contains("Targets: 2"), "{summary}");
    assert!(summary.contains(result["run"]["run_id"].as_str().unwrap()));
    for target in result["results"].as_array().unwrap() {
        let action = target["target"]["action"].as_str().unwrap();
        let receipt = target["response"]["receipt_id"].as_str().unwrap();
        assert!(
            summary.contains(&format!("example:{action}: success, receipt {receipt}")),
            "{summary}"
        );
    }
    assert!(summary.contains("Next step: scripts/jig work gates"));
    // Compatibility payloads expose executed responses at the top level.
    let mut compatibility = result;
    compatibility["run"]
        .as_object_mut()
        .unwrap()
        .remove("targets");
    assert_eq!(format_work_check_summary(&compatibility), summary);
}

#[test]
fn work_check_summary_reports_profile_failures_and_incomplete_results() {
    for conclusion in ["failure", "cancelled", "timed_out", "blocked", "unknown"] {
        let summary = format_work_check_summary(&json!({
            "ok": false, "checks": [], "run": {"conclusion": conclusion},
            "results": []
        }));
        assert!(!summary.contains("Work check: passed"), "{summary}");
        assert!(!summary.contains("no checks configured"), "{summary}");
        if conclusion != "unknown" {
            assert!(summary.contains("Work check: failed"), "{summary}");
        }
    }
    let summary = format_work_check_summary(&json!({
        "ok": true, "checks": [], "run": {"conclusion": "success"},
        "results": [{"target": {"component": "example", "action": "test"}, "response": {"result": {}}}]
    }));
    assert!(summary.contains("Work check: unknown"), "{summary}");
}

#[test]
fn work_check_summary_includes_unstarted_durable_targets() {
    for conclusion in ["cancelled", "blocked"] {
        let summary = format_work_check_summary(&json!({
            "ok": false,
            "plan_id": "plan_example",
            "run": {
                "run_id": "run_example", "status": "completed", "conclusion": conclusion,
                "targets": [
                    {"target": {"component": "example", "action": "lint"},
                     "conclusion": "success", "receipt_id": "receipt_lint"},
                    {"target": {"component": "example", "action": "test"},
                     "conclusion": conclusion, "started_at_ms": null, "receipt_id": null}
                ]
            },
            "results": [{
                "target": {"component": "example", "action": "lint"},
                "response": {"ok": true, "result": {"exit_status": 0}, "receipt_id": "receipt_lint"}
            }]
        }));
        assert!(summary.contains("Work check: failed"), "{summary}");
        assert!(
            summary.contains(&format!("Run: run_example ({conclusion})")),
            "{summary}"
        );
        assert!(summary.contains("Targets: 2"), "{summary}");
        assert!(
            summary.contains("example:lint: success, receipt receipt_lint"),
            "{summary}"
        );
        assert!(
            summary.contains(&format!("example:test: {conclusion}, receipt none")),
            "{summary}"
        );
        assert!(summary.contains("inspect failing receipts"), "{summary}");
    }
}

#[test]
fn work_check_summary_combines_profile_and_legacy_outcomes() {
    let mut result = json!({
        "ok": true,
        "checks": [{"tool": "jig.test", "result": {"exit_status": 0}}],
        "run": {"conclusion": "success"},
        "results": [{"target": {"component": "example", "action": "lint"},
            "response": {"ok": true, "result": {"exit_status": 0}, "receipt_id": "receipt_lint"}}]
    });
    let summary = format_work_check_summary(&result);
    assert!(summary.contains("Work check: passed"), "{summary}");
    assert!(summary.contains("Checks: 1"));
    assert!(summary.contains("Targets: 1"));
    result["checks"][0]["result"]["exit_status"] = json!(1);
    assert!(format_work_check_summary(&result).contains("Work check: failed"));
    result["checks"][0]["result"]["exit_status"] = json!(0);
    result["results"][0]["response"]["ok"] = json!(false);
    assert!(format_work_check_summary(&result).contains("Work check: failed"));
}

#[test]
fn work_evidence_summary_names_profile_evidence_without_unknown_labels() {
    let summary = format_work_evidence_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "open",
        "overall": "passed",
        "latest_passing_gates": [{
            "tool": null,
            "skill": null,
            "profile": "verify",
            "gate_id": "verify",
            "receipt_id": "receipt_web",
            "run_id": "run_1",
            "matches_current_worktree": true,
            "freshness": "fresh",
            "freshness_reason": "all required target receipts match current inputs"
        }],
        "gates": [{
            "id": "verify",
            "kind": "evidence",
            "profile": "verify",
            "status": "passed"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("profile verify: verify, receipt receipt_web"));
    assert!(!summary.contains("<unknown>"));
}

#[test]
fn work_check_summary_exposes_reused_native_provenance_and_aggregate_failure() {
    let mut value = json!({"ok": true, "plan_id": "plan_1", "checks": [],
        "target_validation_receipt_id": "receipt_validation",
        "target_evidence": [{"target": {"component": "api", "action": "test"},
            "status": "passed", "disposition": "reused", "receipt_id": "receipt_original", "run_id": "run_original"}]});
    let summary = format_work_check_summary(&value);
    assert!(summary.contains("Work check: passed"));
    assert!(
        summary.contains("api:test: passed (reused), receipt receipt_original, run run_original")
    );
    assert!(summary.contains("Target validation receipt: receipt_validation"));
    value["ok"] = json!(false);
    assert!(format_work_check_summary(&value).contains("Work check: failed"));
}
