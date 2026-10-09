use super::support::check_by_id;
use super::*;
use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::support::write_doctor_fixture;
use crate::cli::format_doctor_summary_for_test as format_summary;
use crate::test_env::{CurrentDirGuard, EnvVarGuard, lock_env};

#[test]
fn doctor_reports_when_configured_root_differs_from_invocation_repo() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let selected = temp.path().join("selected");
    let invocation = temp.path().join("invocation");
    fs::create_dir_all(&selected).unwrap();
    fs::create_dir_all(&invocation).unwrap();
    write_doctor_fixture(&selected);
    write_doctor_fixture(&invocation);
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &selected);

    let notice = doctor_root_override_notice(&invocation, &fs::canonicalize(&selected).unwrap())
        .expect("differing configured and invocation roots must be reported");

    assert!(notice.contains("doctor is using JIG_REPO_ROOT="));
    assert!(notice.contains(&fs::canonicalize(&selected).unwrap().display().to_string()));
    assert!(notice.contains(&fs::canonicalize(&invocation).unwrap().display().to_string()));
    assert!(notice.contains("unset JIG_REPO_ROOT"));
}

#[test]
fn summary_surfaces_optional_missing_agent_skills() {
    let output = json!({
        "ok": true,
        "repo": {
            "root": "/tmp/demo",
        },
        "checks": [
            {
                "label": "Agent skills",
                "status": "missing",
                "required": false,
                "ok": false,
            },
        ],
        "next_step": "Run `scripts/jig agent bootstrap`.",
    });

    let summary = format_summary(&output);

    assert!(summary.contains("Jig doctor: ready"));
    assert!(summary.contains("Agent skills: optional setup (missing, optional)"));
    assert!(summary.contains("Next required step: none"));
    assert!(summary.contains("Optional setup: scripts/jig agent bootstrap"));
}

#[test]
fn summary_surfaces_required_tool_missing_detail() {
    let output = json!({
        "ok": false,
        "repo": {
            "root": "/tmp/demo",
        },
        "checks": [
            {
                "label": "Required tools",
                "status": "missing",
                "required": true,
                "ok": false,
                "detail": "Missing command executable(s): schema_dump_command: scripts/dump-schema.sh",
            },
        ],
        "next_step": "Install the missing executable.",
    });

    let summary = format_summary(&output);

    assert!(summary.contains("Required tools: needs setup (missing, required)"));
    assert!(summary.contains(
        "Detail: Missing command executable(s): schema_dump_command: scripts/dump-schema.sh"
    ));
    assert!(summary.contains("Next required step: Install the missing executable."));
    assert!(summary.contains("Optional setup: none"));
}

#[test]
fn doctor_reports_unified_readiness_checks() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let _cwd = CurrentDirGuard::set(temp.path());

    let output = run_with_cancellation(&|| false).unwrap();

    assert_eq!(output["command"], "doctor");
    assert_eq!(output["repo"]["name"], "demo");
    assert_eq!(output["checks"].as_array().unwrap().len(), 8);
    assert!(check_by_id(&output, "runtime")["ok"].as_bool().unwrap());
    assert!(check_by_id(&output, "config")["ok"].as_bool().unwrap());
    assert!(
        check_by_id(&output, "contract")["ok"].as_bool().unwrap(),
        "{output:#}"
    );
    assert!(
        check_by_id(&output, "required_tools")["ok"]
            .as_bool()
            .unwrap()
    );
    assert!(
        check_by_id(&output, "agent_skills")["ok"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(check_by_id(&output, "agent_skills")["required"], false);
    assert_eq!(check_by_id(&output, "proxy")["status"], "not configured");
    assert!(check_by_id(&output, "proxy")["ok"].as_bool().unwrap());
    assert_eq!(check_by_id(&output, "vault")["required"], false);
}

#[test]
fn doctor_reports_all_checks_when_config_is_invalid() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    fs::write(temp.path().join(".jig.toml"), "repo_name = \n").unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        "#!/bin/sh\n# Runtime selection uses __runtime-compatible.\n",
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());

    let output = run().unwrap();

    assert_eq!(output["command"], "doctor");
    assert_eq!(output["checks"].as_array().unwrap().len(), 8);
    assert_eq!(check_by_id(&output, "config")["status"], "invalid");
    assert_eq!(check_by_id(&output, "contract")["status"], "blocked");
    assert_eq!(check_by_id(&output, "required_tools")["status"], "blocked");
    assert_eq!(check_by_id(&output, "agent_skills")["status"], "blocked");
    assert_eq!(check_by_id(&output, "proxy")["status"], "blocked");
    assert_eq!(check_by_id(&output, "vault")["status"], "blocked");
    for id in ["contract", "required_tools", "agent_skills", "proxy"] {
        assert!(
            check_by_id(&output, id)["detail"]
                .as_str()
                .unwrap()
                .contains(".jig.toml")
        );
    }
    assert!(
        check_by_id(&output, "vault")["detail"]
            .as_str()
            .unwrap()
            .contains("repo context")
    );
    assert!(output["next_step"].as_str().unwrap().contains(".jig.toml"));
    assert!(
        output["next_required_step"]
            .as_str()
            .unwrap()
            .contains(".jig.toml")
    );
    assert!(output["optional_setup"].is_null());
    let summary = format_summary(&output);
    assert!(summary.contains("Next required step: Fix `.jig.toml`"));
    assert!(summary.contains("Optional setup: none"));
}

#[test]
fn doctor_uses_configured_repo_root_before_current_directory() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let other = temp.path().join("other");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();
    write_doctor_fixture(&repo);
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &repo);
    let _cwd = CurrentDirGuard::set(&other);

    let output = run().unwrap();

    assert_eq!(
        output["repo"]["root"],
        fs::canonicalize(&repo).unwrap().display().to_string()
    );
    assert_eq!(output["repo"]["name"], "demo");
}

#[test]
fn doctor_reports_invalid_configured_repo_root() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let missing_config = temp.path().join("missing-config");
    fs::create_dir_all(&missing_config).unwrap();
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing_config);

    let output = run().unwrap();

    assert_eq!(output["ok"], false);
    assert_eq!(check_by_id(&output, "repo")["status"], "missing");
    assert!(
        check_by_id(&output, "repo")["detail"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT does not contain .jig.toml")
    );
    let fix = check_by_id(&output, "repo")["fix"].as_str().unwrap();
    for expected in [
        "init <path> --preset rust-react --db none --frontend web --no-input --no-vault",
        "init <path> --preset harness-only --repo-name <name> --sqlx-enabled false --no-input --no-vault",
        "init <path> --preset go-react --db none --frontend web --go-module example.com/<name> --no-input --no-vault",
        "init <path> --preset rust-library --no-input --no-vault",
        "init <path> --preset rust-cli --no-input --no-vault",
    ] {
        assert!(fix.contains(expected), "missing {expected:?} from {fix}");
    }
    assert!(fix.contains("interactively to choose the same five shapes"));
}
