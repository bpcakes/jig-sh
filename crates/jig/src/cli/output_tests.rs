use serde_json::json;

use super::*;

#[test]
fn setup_summary_reports_orchestration_and_next_step() {
    let summary = format_setup_summary(&json!({
        "ok": true,
        "doctor_before": { "ok": false },
        "bootstrap": { "ok": true },
        "agent": {
            "registrations": [{ "ok": true }],
            "after": { "ok": true, "next_steps": [] }
        },
        "contract": { "ok": true },
        "doctor_after": {
            "ok": true,
            "checks": [{
                "id": "proxy",
                "data": { "configured": true }
            }]
        }
    }));

    assert!(summary.contains("Jig setup: ready"));
    assert!(summary.contains("Doctor preflight: setup required"));
    assert!(summary.contains("Marketplace registrations: 1 completed"));
    assert!(summary.contains("Contract: passed"));
    assert!(summary.contains("Next step: jig status"));
    assert!(!summary.contains("scripts/jig dev"));
}

#[test]
fn setup_summary_does_not_recommend_dev_without_configured_apps() {
    let summary = format_setup_summary(&json!({
        "ok": true,
        "doctor_before": { "ok": true },
        "bootstrap": { "ok": true },
        "agent": {
            "registrations": [],
            "after": { "ok": true, "next_steps": [] }
        },
        "contract": { "ok": true },
        "doctor_after": {
            "ok": true,
            "checks": [{
                "id": "proxy",
                "data": { "configured": false }
            }]
        }
    }));

    assert!(summary.contains("Jig setup: ready"));
    assert!(summary.contains("Next step: jig status"));
    assert!(!summary.contains("Next step: scripts/jig"));
    assert!(!summary.contains("scripts/jig dev"));
}

#[test]
fn setup_summary_uses_safe_next_step_when_dev_app_signal_is_missing() {
    let summary = format_setup_summary(&json!({
        "ok": true,
        "doctor_before": { "ok": true },
        "bootstrap": { "ok": true },
        "agent": {
            "registrations": [],
            "after": { "ok": true, "next_steps": [] }
        },
        "contract": { "ok": true },
        "doctor_after": { "ok": true }
    }));

    assert!(summary.contains("Next step: jig status"));
    assert!(!summary.contains("scripts/jig dev"));
}

#[test]
fn proxy_summary_reports_termination_as_stopped() {
    let summary = format_proxy_summary(&json!({
        "ok": false,
        "interrupted": true,
        "exit_status": 143,
        "exit_signal": 15,
        "termination_signal": "SIGTERM",
        "app": "web",
        "hostname": "web.demo.localhost",
        "port": null
    }));

    assert!(summary.contains("Proxy: stopped (SIGTERM)"));
    assert!(summary.contains("App: web"));
    assert!(!summary.contains("failed"));
}

#[test]
fn vault_run_summary_reports_status_and_redacted_output() {
    let summary = format_vault_run_summary(&json!({
        "result": {
            "exit_status": 2,
            "exit_signal": null,
            "stdout": "redacted stdout",
            "stderr": "redacted stderr"
        }
    }));

    assert!(summary.contains("Vault run: exit 2"));
    assert!(summary.contains("stdout: redacted stdout"));
    assert!(summary.contains("stderr: redacted stderr"));
}

#[test]
fn vault_run_summary_calls_out_truncated_output() {
    let summary = format_vault_run_summary(&json!({
        "result": {
            "exit_status": 1,
            "exit_signal": null,
            "stdout": "x ".repeat(260),
            "stderr": ""
        }
    }));

    assert!(summary.contains("stdout: "));
    assert!(summary.contains("Output truncated; rerun with --json for full output."));
}

#[test]
fn vault_run_summary_preserves_short_multiline_output() {
    let summary = format_vault_run_summary(&json!({
        "result": {
            "exit_status": 1,
            "exit_signal": null,
            "stdout": "",
            "stderr": "first line\nsecond line"
        }
    }));

    assert!(summary.contains("stderr: first line\nsecond line"));
}

#[test]
fn agent_doctor_summary_calls_out_source_mismatch() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": false,
        "codex": {
            "required": true,
            "available": true
        },
        "marketplaces": [{
            "id": "jig-skills",
            "source": "bpcakes/jig-skills",
            "configured_source": "https://github.com/example/jig-skills.git",
            "registered": false
        }],
        "next_steps": [
            "Run `scripts/jig agent bootstrap` to register marketplace jig-skills."
        ]
    }));

    assert!(summary.contains("Agent tooling: needs setup"));
    assert!(summary.contains("repo config expects bpcakes/jig-skills"));
    assert!(summary.contains("Codex has https://github.com/example/jig-skills.git"));
    assert!(summary.contains("Next steps:"));
}

#[test]
fn agent_doctor_summary_handles_optional_codex_requirement() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": true,
        "codex": {
            "required": false,
            "available": null
        },
        "marketplaces": [],
        "next_steps": []
    }));

    assert!(summary.contains("Agent tooling: ready"));
    assert!(summary.contains("Codex: not required (probe skipped)"));
    assert!(summary.contains("Marketplaces: none configured"));
    // Regression guard for the previously duplicated requirement/probe label.
    assert!(!summary.contains("not required (not required)"));
    // When Codex is not required, the summary should explain the skipped
    // probe instead of exposing the underlying null availability field.
    assert!(!summary.contains("unknown"));
}

#[test]
fn agent_doctor_summary_handles_ready_marketplace() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": true,
        "codex": {
            "required": true,
            "available": true
        },
        "marketplaces": [{
            "id": "jig-skills",
            "source": "bpcakes/jig-skills",
            "configured_source": "https://github.com/bpcakes/jig-skills.git",
            "registered": true
        }],
        "next_steps": []
    }));

    assert!(summary.contains("Agent tooling: ready"));
    assert!(summary.contains("Codex: required (available)"));
    assert!(summary.contains("jig-skills: registered"));
    assert!(summary.contains("Next steps: none"));
}

#[test]
fn agent_doctor_summary_handles_unknown_required_codex_availability() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": false,
        "codex": {
            "required": true,
            "available": null
        },
        "marketplaces": [],
        "next_steps": []
    }));

    assert!(summary.contains("Codex: required (unknown)"));
}

#[test]
fn work_status_summary_stays_compact() {
    let summary = format_work_status_summary(&json!({
        "repo": {
            "name": "demo",
            "default_branch": "main"
        },
        "current_session_id": null,
        "counts": {
            "open_plans": 1,
            "receipts": 12,
            "failed_receipts": 2,
            "decisions": 3
        },
        "open_plans": [{
            "plan_id": "plan_1",
            "title": "Improve UX"
        }],
        "recent_receipts": [
            {
                "id": "receipt_1",
                "tool_name": "jig.test",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_2",
                "tool_name": "jig.clippy",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_3",
                "tool_name": "jig.fmt_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_4",
                "tool_name": "jig.contract_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_5",
                "tool_name": "jig.bootstrap",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_6",
                "tool_name": "jig.extra",
                "exit_status": 0,
                "diff_summary": "no changes"
            }
        ]
    }));

    assert!(summary.contains("Work status:"));
    assert!(summary.contains("Plans: 1 open"));
    assert!(summary.contains("Receipts: 12 total, 2 failed"));
    assert!(summary.contains("Decisions: 3"));
    assert!(summary.contains("Repo: demo (main)"));
    assert!(summary.contains("Current session: none"));
    assert!(summary.contains("plan_1: Improve UX"));
    assert!(summary.contains("jig.test"));
    assert!(summary.contains("and 1 more recent receipt"));
}

#[test]
fn state_summary_focuses_on_persisted_record_counts() {
    let summary = format_state_summary(&json!({
        "repo": { "name": "demo", "default_branch": "main" },
        "current_session_id": "session_1",
        "counts": {
            "sessions": 4,
            "session_events": 7,
            "plans": 5,
            "plan_events": 12,
            "open_plans": 2,
            "receipts": 20,
            "failed_receipts": 3,
            "decisions": 6
        },
        "open_plans": [{ "plan_id": "plan_1", "title": "Not repeated here" }],
        "recent_receipts": [{ "id": "receipt_1", "tool_name": "jig.test" }]
    }));

    assert!(summary.contains("State summary:"));
    assert!(summary.contains("Sessions: 4 (7 events)"));
    assert!(summary.contains("Plans: 5 (2 open, 12 events)"));
    assert!(summary.contains("Receipts: 20 (3 failed)"));
    assert!(summary.contains("Decisions: 6"));
    assert!(summary.contains("Current session: session_1"));
    assert!(!summary.contains("Not repeated here"));
    assert!(!summary.contains("jig.test"));
}

#[test]
fn work_start_plan_id_output_is_shell_friendly() {
    let plan_id = format_work_start_plan_id(&json!({
        "ok": true,
        "plan": {
            "plan_id": "plan_123"
        }
    }))
    .unwrap();

    assert_eq!(plan_id, "plan_123");
}

#[test]
fn work_start_plan_id_output_requires_plan_id() {
    let error = format_work_start_plan_id(&json!({
        "ok": true,
        "plan": {}
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("plan.plan_id"));
}

#[test]
fn work_start_plan_id_output_requires_plan_object() {
    let error = format_work_start_plan_id(&json!({
        "ok": true
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("include plan"));
}

#[test]
fn work_start_plan_id_output_requires_plan_to_be_object() {
    let error = format_work_start_plan_id(&json!({
        "ok": true,
        "plan": null
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("plan was not an object"));
}

#[test]
fn work_start_and_goal_summaries_label_empty_tree_baselines() {
    let plan = json!({
        "plan_id": "plan_initial",
        "body_path": ".agent/plans/initial.md",
        "baseline": {
            "requested_ref": "HEAD",
            "commit_oid": null,
            "empty_tree_oid": "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "error": null
        }
    });
    let start = format_work_start_summary(&json!({
        "plan": plan,
        "session": { "session_id": "session_1" }
    }));
    let goal = format_work_goal_summary(&json!({
        "plan": plan,
        "commands": {}
    }));

    for summary in [start, goal] {
        assert!(
            summary.contains("Baseline: 4b825dc642cb6eb9a060e54bf8d69288fbee4904 (empty tree)")
        );
    }
}

#[test]
fn work_goal_summary_reports_baseline_collection_errors() {
    let summary = format_work_goal_summary(&json!({
        "plan": {
            "plan_id": "plan_error",
            "baseline": {
                "requested_ref": "HEAD",
                "commit_oid": null,
                "empty_tree_oid": null,
                "error": "Git metadata was unavailable"
            }
        },
        "commands": {}
    }));

    assert!(summary.contains("Baseline: unavailable (Git metadata was unavailable)"));
}

#[test]
fn work_status_summary_omits_truncation_hint_at_receipt_limit() {
    let summary = format_work_status_summary(&json!({
        "repo": {
            "name": "demo",
            "default_branch": "main"
        },
        "current_session_id": null,
        "counts": {
            "open_plans": 0,
            "receipts": 5,
            "failed_receipts": 0,
            "decisions": 0
        },
        "open_plans": [],
        "recent_receipts": [
            {
                "id": "receipt_1",
                "tool_name": "jig.test",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_2",
                "tool_name": "jig.clippy",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_3",
                "tool_name": "jig.fmt_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_4",
                "tool_name": "jig.contract_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_5",
                "tool_name": "jig.bootstrap",
                "exit_status": 0,
                "diff_summary": "no changes"
            }
        ]
    }));

    assert!(summary.contains("receipt_5"));
    assert!(!summary.contains("rerun with --json"));
}

#[test]
fn work_status_summary_handles_empty_state() {
    let summary = format_work_status_summary(&json!({
        "repo": {
            "name": "demo",
            "default_branch": "main"
        },
        "current_session_id": null,
        "counts": {
            "open_plans": 0,
            "receipts": 0,
            "failed_receipts": 0,
            "decisions": 0
        },
        "open_plans": [],
        "recent_receipts": []
    }));

    assert!(summary.contains("Plans: 0 open"));
    assert!(summary.contains("Receipts: 0 total, 0 failed"));
    assert!(summary.contains("Decisions: 0"));
    assert!(summary.contains("Current session: none"));
    assert!(summary.contains("Open plans: none"));
    assert!(summary.contains("Recent receipts: none"));
}

#[test]
fn work_check_summary_is_actionable() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(summary.contains("Plan: plan_1"));
    assert!(summary.contains("Batch receipt: receipt_batch"));
    assert!(summary.contains("jig.test: exit 0, receipt receipt_test"));
    assert!(summary.contains("work gates --plan-id plan_1"));
}

#[test]
fn work_check_summary_reports_failed_check_status() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 101,
                "stdout": "",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: failed"));
    assert!(summary.contains("jig.test: exit 101"));
    assert!(summary.contains("inspect failing receipts"));
    assert!(!summary.contains("work gates --plan-id plan_1"));
}

#[test]
fn work_check_summary_reports_failed_check_output() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 101,
                "stdout": "",
                "stderr": "test failure details\n"
            }
        }]
    }));

    assert!(summary.contains("jig.test: exit 101"));
    assert!(summary.contains("output: test failure details"));
}

#[test]
fn work_check_summary_surfaces_skipped_harness_defaults() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "No Cargo.toml found; skipping cargo test.\n",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: passed (all skipped)"));
    assert!(summary.contains("output: No Cargo.toml found; skipping cargo test."));
    assert!(summary.contains("all configured Cargo checks skipped"));
    assert!(summary.contains("set explicit commands"));
}

#[test]
fn work_check_summary_reports_some_skipped_harness_defaults() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [
            {
                "tool": "jig.fmt",
                "receipt_id": "receipt_fmt",
                "result": {
                    "exit_status": 0,
                    "stdout": "No Cargo.toml found; skipping cargo fmt.\n",
                    "stderr": ""
                }
            },
            {
                "tool": "jig.contract_check",
                "receipt_id": "receipt_contract",
                "result": {
                    "exit_status": 0,
                    "stdout": "",
                    "stderr": ""
                }
            }
        ]
    }));

    assert!(summary.contains("Work check: passed (some skipped)"));
    assert!(!summary.contains("all configured Cargo checks skipped"));
}

#[test]
fn work_check_summary_ignores_unrelated_skipping_output() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "skipping optional project step\n",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("jig.test: exit 0, receipt receipt_test"));
    assert!(!summary.contains("output:"));
}

#[test]
fn work_check_summary_does_not_treat_stderr_skip_text_as_harness_skip() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "",
                "stderr": "No Cargo.toml found; skipping cargo test.\n"
            }
        }]
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(!summary.contains("passed (all skipped)"));
    assert!(!summary.contains("all configured Cargo checks skipped"));
}

#[test]
fn work_check_summary_does_not_count_failed_prefix_output_as_skipped() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 101,
                "stdout": "No Cargo.toml found; skipping cargo test.\n",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: failed"));
    assert!(!summary.contains("passed (all skipped)"));
    assert!(!summary.contains("all configured Cargo checks skipped"));
}

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
fn work_check_summary_reports_classified_nonexecutions_as_success() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [],
        "gate_evidence": [
            {
                "gate_id": "rust-tests",
                "status": "not_applicable",
                "reason": "no changed paths matched crates/**"
            },
            {
                "gate_id": "frontend-web-lint",
                "status": "reused",
                "reason": "one changed path matched apps/web/**",
                "source_tool_receipt_id": "receipt_source"
            }
        ]
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(
        summary
            .contains("0 executed, 1 reused, 1 not applicable, 0 failed, 0 cancelled, 0 unknown")
    );
    assert!(summary.contains("rust-tests: not_applicable"));
    assert!(summary.contains("source receipt receipt_source"));
    assert!(!summary.contains("no checks configured"));
}

#[test]
fn work_check_summary_reports_failed_and_cancelled_gate_evidence() {
    let summary = format_work_check_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [],
        "gate_evidence": [
            {
                "gate_id": "rust-tests",
                "status": "failed",
                "reason": "configured command exited 101"
            },
            {
                "gate_id": "frontend-web-lint",
                "status": "cancelled",
                "reason": "work check was interrupted"
            }
        ]
    }));

    assert!(summary.contains("Work check: failed"));
    assert!(
        summary
            .contains("0 executed, 0 reused, 0 not applicable, 1 failed, 1 cancelled, 0 unknown")
    );
    assert!(summary.contains("rust-tests: failed; configured command exited 101"));
    assert!(summary.contains("frontend-web-lint: cancelled; work check was interrupted"));
    assert!(!summary.contains("no checks configured"));
}

#[test]
fn work_check_summary_reports_unknown_check_status() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "stdout": "",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: unknown"));
    assert!(summary.contains("jig.test: exit ?"));
    assert!(summary.contains("unknown exit status"));
    assert!(!summary.contains("inspect failing receipts"));
}

#[test]
fn work_gates_summary_reports_blockers() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "blocked",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "missing",
            "freshness": "missing"
        }],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work gates: blocked"));
    assert!(summary.contains("tests: missing, freshness missing, required (jig.test)"));
    assert!(summary.contains("Blocked: missing (tests)"));
    assert!(!summary.contains("failed ()"));
    assert!(summary.contains("work check --plan-id plan_1"));
}

#[test]
fn work_gates_summary_reports_unsupported_blockers() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "blocked",
        "gates": [],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": ["schema"]
    }));

    assert!(summary.contains("Blocked: unsupported (schema)"));
}

#[test]
fn work_gates_summary_reports_combined_blockers_in_stable_order() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "blocked",
        "gates": [],
        "missing_required": ["tests"],
        "failed_required": ["lint"],
        "stale_required": ["schema"],
        "unknown_required": ["docs"],
        "unsupported_required": ["deploy"]
    }));

    assert!(summary.contains(
        "Blocked: missing (tests); failed (lint); stale (schema); unknown (docs); unsupported (deploy)"
    ));
}

#[test]
fn work_gates_summary_handles_uncategorized_blocked_status() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "unknown",
        "gates": [],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work gates: unknown"));
    assert!(summary.contains("Status: unknown; no categorized blockers reported"));
}

#[test]
fn work_gates_summary_reports_finish_command_when_passed() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "open",
        "overall": "passed",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "passed",
            "freshness": "fresh"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work gates: passed"));
    assert!(summary.contains("work finish --plan-id plan_1"));
}

#[test]
fn work_gates_summary_does_not_offer_finish_for_closed_plan() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "passed",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "passed",
            "freshness": "fresh"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Next step: none; plan is closed"));
    assert!(!summary.contains("work finish --plan-id plan_1"));
}

#[test]
fn work_gates_summary_does_not_offer_check_for_closed_blocked_plan() {
    let summary = format_work_gates_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "blocked",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "missing",
            "freshness": "missing"
        }],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Next step: start a new work plan for follow-up changes"));
    assert!(!summary.contains("work check --plan-id plan_1"));
}

#[test]
fn work_evidence_summary_reports_latest_gate_freshness_and_paths() {
    let summary = format_work_evidence_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "open",
        "overall": "blocked",
        "latest_passing_gates": [{
            "tool": "jig.test",
            "gate_id": "tests",
            "receipt_id": "receipt_tool",
            "freshness_receipt_id": "receipt_batch",
            "matches_current_worktree": false,
            "freshness": "stale",
            "freshness_reason": "receipt was recorded for a different worktree fingerprint",
            "diff_summary": "2 files, +4 -1",
            "changed_paths": ["src/lib.rs", "Cargo.toml"]
        }],
        "gates": [{
            "id": "tests",
            "status": "stale",
            "freshness_reason": "receipt was recorded for a different worktree fingerprint"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": ["tests"],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work evidence: blocked"));
    assert!(summary.contains("Latest gate evidence per gate:"));
    assert!(
        summary.contains(
            "jig.test: tests, receipt receipt_batch, matches current worktree no (stale)"
        )
    );
    assert!(summary.contains("receipt was recorded for a different worktree fingerprint"));
    assert!(summary.contains("changed paths covered: src/lib.rs, Cargo.toml"));
    assert!(summary.contains("Next step: scripts/jig work check --plan-id plan_1"));
}

#[test]
fn work_evidence_summary_reports_closed_plan_as_done() {
    let summary = format_work_evidence_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "passed",
        "latest_passing_gates": [],
        "gates": [{
            "id": "tests",
            "status": "passed"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Unresolved gates: none"));
    assert!(summary.contains("Next step: none; plan is closed"));
    assert!(!summary.contains("work finish --plan-id plan_1"));
}

#[test]
fn work_evidence_summary_does_not_offer_check_for_closed_blocked_plan() {
    let summary = format_work_evidence_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "blocked",
        "latest_passing_gates": [],
        "gates": [{
            "id": "tests",
            "status": "missing"
        }],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Next step: start a new work plan for follow-up changes"));
    assert!(!summary.contains("work check --plan-id plan_1"));
}

#[test]
fn work_review_summary_reports_truncated_counts() {
    let summary = format_work_review_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "status": "failed",
        "reviews": [{
            "gate_id": "rust-error-handling",
            "status": "failed",
            "skill": "jig-rust:rust-error-handling-review",
            "finding_count": 105,
            "actionable_count": 105,
            "retained_finding_count": 100,
            "retained_actionable_count": 100,
            "findings_truncated": true,
            "actionable_findings_truncated": true
        }]
    }));

    assert!(summary.contains("105/105 actionable, showing 100/100"));
    assert!(summary.contains("Next step: scripts/jig work refine --plan-id plan_1"));
}

#[test]
fn work_receipts_summary_is_compact() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.fmt_check",
            "exit_status": 1,
            "diff_summary": "2 files, +4 -1",
            "plan_id": "plan_1",
            "session_id": null,
            "stdout_preview": "Diff in src/lib.rs:\n- old\n+ new",
            "stderr_preview": ""
        }]
    }));

    assert!(summary.contains("Work receipts:"));
    assert!(summary.contains("Showing: 1"));
    assert!(summary.contains("jig.fmt_check (receipt_1): exit 1, 2 files, +4 -1"));
    assert!(summary.contains("plan: plan_1; session: none"));
    assert!(summary.contains("output: Diff in src/lib.rs:\n- old\n+ new"));
}

#[test]
fn work_receipts_summary_prefers_stderr_and_truncates() {
    let long_stderr = "error ".repeat(80);
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.clippy",
            "exit_status": 101,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": "session_1",
            "stdout_preview": "stdout should not win",
            "stderr_preview": long_stderr
        }]
    }));

    assert!(summary.contains("plan: none; session: session_1"));
    assert!(summary.contains("output: error error"));
    assert!(summary.contains("..."));
    assert!(!summary.contains("stdout should not win"));
}

#[test]
fn work_receipts_summary_uses_stdout_when_stderr_is_empty() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.test",
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": null,
            "stdout_preview": "test output",
            "stderr_preview": ""
        }]
    }));

    assert!(summary.contains("output: test output"));
}

#[test]
fn work_receipts_summary_lists_multiple_receipts() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [
            {
                "id": "receipt_1",
                "tool_name": "jig.fmt_check",
                "exit_status": 0,
                "diff_summary": "no changes",
                "plan_id": "plan_1",
                "session_id": null,
                "stdout_preview": "",
                "stderr_preview": ""
            },
            {
                "id": "receipt_2",
                "tool_name": "jig.test",
                "exit_status": 101,
                "diff_summary": "1 file, +1 -0",
                "plan_id": "plan_1",
                "session_id": "session_1",
                "stdout_preview": "tests failed",
                "stderr_preview": ""
            }
        ]
    }));

    assert!(summary.contains("Showing: 2"));
    assert!(summary.contains("jig.fmt_check (receipt_1): exit 0, no changes"));
    assert!(summary.contains("jig.test (receipt_2): exit 101, 1 file, +1 -0"));
    assert!(summary.contains("output: tests failed"));
    assert!(!summary.contains("No receipts matched"));
}

#[test]
fn work_receipts_summary_handles_empty_results() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": []
    }));

    assert!(summary.contains("Showing: 0"));
    assert!(summary.contains("No receipts matched"));
}

#[test]
fn work_receipts_summary_omits_output_line_without_preview() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.test",
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": null
        }]
    }));

    assert!(summary.contains("jig.test (receipt_1): exit 0, no changes"));
    assert!(summary.contains("plan: none; session: none"));
    assert!(!summary.contains("output:"));
}

#[test]
fn state_diagnose_shallow_summary_does_not_imply_deep_cleanliness() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": false,
        "totals": { "bytes": 42 },
        "sessions": null,
        "receipts": null
    }));

    assert!(summary.contains("Total bytes: 42"));
    assert!(summary.contains("Session recursion: not analyzed"));
    assert!(summary.contains("Receipt payloads: not analyzed"));
    assert!(!summary.contains("Recursive session records: 0"));
}

#[test]
fn state_diagnose_deep_summary_reports_compaction_opportunity() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": { "bytes": 1_000 },
        "sessions": {
            "recursive_session_records": 7,
            "estimated_reclaimable_bytes": 800
        }
    }));

    assert!(summary.contains("Recursive session records: 7"));
    assert!(summary.contains("Estimated reclaimable bytes: 800"));
}

#[test]
fn state_diagnose_summary_reports_cache_and_actionable_recommendations() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": {
            "checkout_state_bytes": 1_000,
            "maintenance_cache_bytes": 250,
            "local_disk_bytes": 1_250
        },
        "maintenance_cache": {
            "state_backups": { "bytes": 200 },
            "state_archives": { "bytes": 50 }
        },
        "sessions": {
            "recursive_session_records": 0,
            "estimated_reclaimable_bytes": 0
        },
        "recommendations": [{
            "reason": "Receipt state is large.",
            "command": "jig state archive --before <YYYY-MM-DD> --dry-run",
            "alternative_command": "jig state export receipts --before <YYYY-MM-DD> --output receipts.jsonl.gz"
        }]
    }));

    assert!(summary.contains("Total bytes: 1250"));
    assert!(summary.contains("State checkout bytes: 1000"));
    assert!(summary.contains("Maintenance cache bytes: 250"));
    assert!(summary.contains("State recovery backups: 200"));
    assert!(summary.contains("Receipt archives: 50"));
    assert!(summary.contains("Recommendations:"));
    assert!(summary.contains("state archive"));
    assert!(summary.contains("state export receipts"));
}

#[test]
fn state_compact_summary_distinguishes_noop_and_recovery_artifact() {
    let compacted = format_state_compact_summary(&json!({
        "dry_run": false,
        "records_changed": 3,
        "duplicate_records": 1,
        "bytes_before": 1_000,
        "bytes_after": 100,
        "bytes_reclaimable": 900,
        "source_sha256": "source-checksum",
        "backup_path": ".agent/.cache/state-backups/sessions-1"
    }));
    let noop = format_state_compact_summary(&json!({
        "dry_run": false,
        "records_changed": 0,
        "duplicate_records": 0,
        "bytes_before": 100,
        "bytes_after": 100,
        "backup_path": null
    }));

    assert!(compacted.contains("State compact sessions: compacted"));
    assert!(compacted.contains("Recovery backup: .agent/.cache/state-backups/sessions-1"));
    assert!(compacted.contains("Source SHA-256: source-checksum"));
    assert!(compacted.contains("local and ignored"));
    assert!(compacted.contains("does not remove reachable Git blobs"));
    assert!(noop.contains("State compact sessions: no-op"));
    assert!(noop.contains("state was already canonical"));
}

#[test]
fn state_restore_summary_reports_noop_checksum_and_recovery_path() {
    let restored = format_state_restore_summary(&json!({
        "stream": "sessions",
        "changed": true,
        "bytes_restored": 1_000,
        "backup_path": ".agent/.cache/state-backups/sessions-1",
        "sha256_restored": "restored-checksum",
        "recovery_backup_path": ".agent/.cache/state-backups/recovery-1"
    }));
    let noop = format_state_restore_summary(&json!({
        "stream": "sessions",
        "changed": false,
        "bytes_restored": 1_000,
        "recovery_backup_path": null
    }));

    assert!(restored.contains("State restore: restored"));
    assert!(restored.contains("Restored SHA-256: restored-checksum"));
    assert!(restored.contains("Replaced-state recovery backup: .agent/.cache"));
    assert!(restored.contains("local and ignored"));
    assert!(restored.contains("does not rewrite reachable Git blobs"));
    assert!(noop.contains("State restore: no-op"));
    assert!(noop.contains("Replaced-state recovery backup: not needed"));
}

#[test]
fn state_archive_and_export_summaries_report_storage_and_durability() {
    let archived = format_state_archive_summary(&json!({
        "dry_run": false,
        "before": "2026-01-01",
        "archive_path": ".agent/.cache/state-archives/receipts.jsonl.gz",
        "recovery_backup_path": ".agent/.cache/state-backups/receipts-1",
        "receipts_archived": 20,
        "receipts_retained": 5,
        "protected_receipts_retained": 2,
        "uncompressed_bytes": 10_000,
        "compressed_bytes": 1_000,
        "sha256": "gzip-checksum",
        "content_sha256": "content-checksum"
    }));
    let exported = format_state_export_summary(&json!({
        "before": "2026-01-01",
        "output_path": "receipts.jsonl.gz",
        "receipts_exported": 20,
        "uncompressed_bytes": 10_000,
        "compressed_bytes": 1_000,
        "sha256": "gzip-checksum",
        "content_sha256": "content-checksum"
    }));

    assert!(archived.contains("State archive: archived"));
    assert!(archived.contains("Protected receipts retained: 2"));
    assert!(archived.contains("Compressed bytes: 1000"));
    assert!(archived.contains("Gzip SHA-256: gzip-checksum"));
    assert!(archived.contains("Exact pre-archive recovery backup: .agent/.cache"));
    assert!(archived.contains("not an off-machine backup"));
    assert!(archived.contains("does not remove reachable Git blobs"));

    assert!(exported.contains("State export receipts: exported"));
    assert!(exported.contains("Active state: unchanged"));
    assert!(exported.contains("JSONL SHA-256: content-checksum"));
    assert!(exported.contains("durability depends on the selected destination"));
    assert!(exported.contains("does not remove reachable Git blobs"));
}
