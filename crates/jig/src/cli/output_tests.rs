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
fn state_summary_focuses_on_persisted_record_counts() {
    let summary = format_state_summary(&json!({
        "repo": { "name": "demo", "default_branch": "main" },
        "counts": {
            "receipts": 20,
            "failed_receipts": 3
        },
        "recent_receipts": [{ "id": "receipt_1", "tool_name": "jig.test" }]
    }));

    assert!(summary.contains("State summary:"));
    assert!(summary.contains("Receipts: 20 (3 failed)"));
    assert!(summary.contains("Repo: demo"));
    assert!(!summary.contains("jig.test"));
}

include!("output_tests_parts.rs");
