use super::support::check_by_id;
use super::*;
use std::fs;

use jig_context::RepoContext;
use serde_json::json;
use tempfile::tempdir;

use super::support::write_doctor_fixture;
use crate::cli::format_doctor_summary_for_test as format_summary;
use crate::doctor::check::{DoctorCheck, check};
use crate::doctor::vault::{VAULT_INIT_OPERATOR_FIX, vault_check};
use crate::test_env::{CurrentDirGuard, EnvVarGuard, lock_env};

fn operator_vault_check() -> DoctorCheck {
    check("vault", "Vault", false, false, "not initialized", "")
        .with_fix(VAULT_INIT_OPERATOR_FIX)
        .operator_only()
}

fn passing_repo_check() -> DoctorCheck {
    check("config", ".jig.toml", true, true, "valid", "")
}

#[test]
fn operator_only_vault_setup_is_never_the_next_step() {
    let report = output(None, vec![passing_repo_check(), operator_vault_check()]);

    assert_eq!(report["ok"], true);
    for key in [
        "next_issue",
        "next_step",
        "next_required_step",
        "optional_setup",
    ] {
        assert!(report[key].is_null(), "{key}: {report:#}");
    }
    assert_eq!(report["operator_setup"], VAULT_INIT_OPERATOR_FIX);
    assert_eq!(report["checks"][0]["operator_only"], false);
    assert_eq!(report["checks"][1]["operator_only"], true);

    let summary = format_summary(&report);
    assert!(summary.contains("Vault: operator setup (not initialized, optional)"));
    assert!(summary.contains("Next required step: none"));
    assert!(summary.contains("Optional setup: none"));
    assert!(summary.contains(
        "Operator setup: run `scripts/jig vault init` in a terminal; it prompts for a new vault passphrase."
    ));
}

#[test]
fn promotable_optional_setup_wins_over_operator_only_regardless_of_order() {
    let agent_fix = "Run `scripts/jig agent bootstrap`.";
    let agent_skills =
        check("agent_skills", "Agent skills", false, false, "missing", "").with_fix(agent_fix);

    let report = output(
        None,
        vec![passing_repo_check(), operator_vault_check(), agent_skills],
    );

    assert_eq!(report["ok"], true);
    assert_eq!(report["next_issue"]["id"], "agent_skills");
    assert_eq!(report["next_step"], agent_fix);
    assert_eq!(report["optional_setup"], agent_fix);
    assert_eq!(report["operator_setup"], VAULT_INIT_OPERATOR_FIX);
    let summary = format_summary(&report);
    assert!(summary.contains("Optional setup: scripts/jig agent bootstrap"));
    assert!(summary.contains("Operator setup: run `scripts/jig vault init`"));
}

#[test]
fn required_failure_stays_the_next_step_beside_operator_setup() {
    let config_fix = "Fix `.jig.toml`.";
    let config = check("config", ".jig.toml", true, false, "invalid", "").with_fix(config_fix);

    let report = output(None, vec![operator_vault_check(), config]);

    assert_eq!(report["ok"], false);
    assert_eq!(report["next_issue"]["id"], "config");
    assert_eq!(report["next_step"], config_fix);
    assert_eq!(report["next_required_step"], config_fix);
    assert!(report["optional_setup"].is_null());
    assert_eq!(report["operator_setup"], VAULT_INIT_OPERATOR_FIX);
}

#[test]
fn summary_fallback_skips_operator_only_checks() {
    let report = json!({
        "ok": true,
        "checks": [
            {
                "label": "Vault",
                "status": "not initialized",
                "required": false,
                "operator_only": true,
                "ok": false,
                "fix": VAULT_INIT_OPERATOR_FIX,
            },
        ],
    });

    let summary = format_summary(&report);

    assert!(summary.contains("Vault: operator setup (not initialized, optional)"));
    assert!(summary.contains("Optional setup: none"));
    assert!(!summary.contains("Operator setup:"));
}

#[test]
fn uninitialized_vault_check_is_operator_only_in_doctor_runs() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    write_doctor_fixture(&repo);
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-home"));
    let ctx = RepoContext::load_from_root(repo.clone()).unwrap();

    let vault = vault_check(Ok(&ctx));
    assert_eq!(vault.status, "not initialized");
    assert!(vault.operator_only);
    let fix = vault.fix.as_deref().unwrap();
    assert!(fix.starts_with("Operator step: run `scripts/jig vault init` in a terminal"));
    assert!(fix.contains("never choose or handle the passphrase"));

    let _cwd = CurrentDirGuard::set(&repo);
    let report = run_with_cancellation(&|| false).unwrap();

    assert_eq!(report["ok"], true, "{report:#}");
    assert_eq!(check_by_id(&report, "vault")["operator_only"], true);
    assert_eq!(check_by_id(&report, "config")["operator_only"], false);
    // Every other fixture check passes, so only operator setup remains.
    for key in [
        "next_issue",
        "next_step",
        "next_required_step",
        "optional_setup",
    ] {
        assert!(report[key].is_null(), "{key}: {report:#}");
    }
    assert_eq!(report["operator_setup"], VAULT_INIT_OPERATOR_FIX);
}
