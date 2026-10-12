use super::*;

use std::ffi::OsString;
#[cfg(unix)]
use std::fs;

use jig_context::RepoContext;
#[cfg(unix)]
use serde_json::Value;
use tempfile::tempdir;

use super::support::write_doctor_fixture_with_bootstrap_command;
#[cfg(unix)]
use super::support::{
    cargo_sqlx_program, doctor_environment, write_sqlx_doctor_fixture_with_command,
    write_test_executable,
};
use crate::cli::format_doctor_summary_for_test as format_summary;
#[cfg(unix)]
use crate::doctor::check::DoctorCheck;
use crate::doctor::environment::DoctorEnvironment;
#[cfg(unix)]
use crate::doctor::environment::ShellEnvironmentIssue;
use crate::doctor::required_tools::required_tools_check_with_environment;
#[cfg(unix)]
use crate::test_env::{CurrentDirGuard, lock_env};

#[cfg(unix)]
#[test]
fn required_tools_distinguishes_missing_and_incompatible_sqlx_cli() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "cargo-sqlx sqlx prepare");
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=sqlite:private-database-name.db\n",
    )
    .unwrap();

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let missing = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!missing.ok);
    assert_eq!(missing.status, "missing");
    assert!(missing.detail.contains("cargo-sqlx"));

    write_test_executable(
        &bin.join("cargo-sqlx"),
        "#!/bin/sh\nprintf '%s\\n' 'error: error with configuration: no driver found for URL scheme \"sqlite\"'\nexit 1\n",
    );
    let incompatible = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!incompatible.ok);
    assert_eq!(incompatible.status, "incompatible");
    assert!(incompatible.detail.contains("lacks the SQLite driver"));
    assert!(
        incompatible
            .fix
            .as_deref()
            .unwrap()
            .contains("--features sqlite")
    );
    assert_eq!(
        cargo_sqlx_program(&incompatible)["driver_probe"]["status"],
        "missing_driver"
    );
    assert_eq!(
        cargo_sqlx_program(&incompatible)["driver_probe"]["compatible"],
        false
    );

    let serialized = serde_json::to_string(&incompatible).unwrap();
    assert!(!serialized.contains("private-database-name"));
    assert!(!serialized.contains("sqlite:private"));
    let summary = format_summary(&output(None, vec![incompatible]));
    assert!(summary.contains("Required tools: needs setup (incompatible, required)"));
    assert!(summary.contains("--features sqlite"));
    assert!(!summary.contains("private-database-name"));
}
#[cfg(unix)]
fn required_tools_for(command: &str, executables: &[&str]) -> DoctorCheck {
    let repo = tempdir().unwrap();
    write_doctor_fixture_with_bootstrap_command(repo.path(), command);
    let tools = tempdir().unwrap();
    for executable in executables {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
    required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None))
}
#[cfg(unix)]
fn bootstrap_programs(check: &DoctorCheck) -> Vec<Value> {
    check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap()
        .clone()
}
#[cfg(unix)]
fn assert_external_wrapper_contract(wrapper: &str) {
    let command = format!("{wrapper} cargo test");
    let missing_wrapper = required_tools_for(&command, &["cargo"]);
    assert_eq!(missing_wrapper.status, "missing", "{wrapper}");
    assert!(!missing_wrapper.ok, "{wrapper}");
    let programs = bootstrap_programs(&missing_wrapper);
    assert_eq!(programs[0]["program"], wrapper, "{wrapper}");
    assert_eq!(programs[0]["present"], false, "{wrapper}");
    assert_eq!(programs[1]["program"], "cargo", "{wrapper}");
    assert_eq!(programs[1]["present"], true, "{wrapper}");

    let missing_target = required_tools_for(&command, &[wrapper]);
    assert_eq!(missing_target.status, "missing", "{wrapper}");
    let programs = bootstrap_programs(&missing_target);
    assert_eq!(programs[0]["present"], true, "{wrapper}");
    assert_eq!(programs[1]["present"], false, "{wrapper}");

    let all_present = required_tools_for(&command, &[wrapper, "cargo"]);
    assert_eq!(all_present.status, "present", "{wrapper}");
    assert!(all_present.ok, "{wrapper}");
}
#[cfg(unix)]
#[test]
fn required_tools_require_external_wrappers_and_their_targets() {
    for wrapper in ["env", "nohup"] {
        assert_external_wrapper_contract(wrapper);
    }

    for command in ["env --help", "env -0"] {
        let missing_wrapper = required_tools_for(command, &[]);
        assert_eq!(missing_wrapper.status, "missing", "{command:?}");
        let programs = bootstrap_programs(&missing_wrapper);
        assert_eq!(programs.len(), 1, "{command:?}");
        assert_eq!(programs[0]["program"], "env", "{command:?}");
        assert_eq!(programs[0]["present"], false, "{command:?}");
    }

    let dynamic_target = required_tools_for("env \"$TOOL\" test", &[]);
    assert_eq!(dynamic_target.status, "missing");
    let programs = bootstrap_programs(&dynamic_target);
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[0]["present"], false);
    assert_eq!(programs[1]["program"], Value::Null);
    assert_eq!(programs[1]["present"], Value::Null);
    assert!(
        !serde_json::to_string(&dynamic_target)
            .unwrap()
            .contains("TOOL")
    );
}
#[cfg(unix)]
#[test]
fn required_tools_check_nested_external_time_chain_in_order() {
    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    for executable in ["env", "nohup", "time", "cargo"] {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let time = tools.path().join("time");
    write_doctor_fixture_with_bootstrap_command(
        repo.path(),
        &format!("env nohup {} cargo test", time.display()),
    );
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present");
    let programs = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs.len(), 4);
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[1]["program"], "nohup");
    assert_eq!(programs[2]["program"], time.display().to_string());
    assert_eq!(programs[3]["program"], "cargo");
    assert!(programs.iter().all(|program| program["present"] == true));
}
#[cfg(unix)]
#[test]
fn required_tools_marks_ambiguous_wrappers_unverified_without_leaking() {
    for (command, secret) in [
        (
            "env -S 'doctor-split-secret missing-tool --flag'",
            "doctor-split-secret",
        ),
        (
            "env '--split-string=doctor-long-split-secret missing-tool --flag' cargo",
            "doctor-long-split-secret",
        ),
        (
            "exec -z doctor-wrapper-secret cargo test",
            "doctor-wrapper-secret",
        ),
    ] {
        let temp = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(temp.path(), command);
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(tools.path().as_os_str().to_os_string()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        assert!(check.fix.is_none(), "{command:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(tool["present"].is_null(), "{command:?}");
        assert!(
            tool["programs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|program| program["present"].is_null()),
            "{command:?}",
        );
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(secret), "{command:?}");
        assert!(!serialized.contains("No external executable required"));
    }
}
#[test]
fn required_tools_downgrades_dynamic_and_complex_shell_commands() {
    for command in [
        "$DOCTOR_DYNAMIC_TOOL test",
        "eval 'doctor-eval-missing-tool --version'",
        "doctor_fn() { :; }; doctor_fn",
        "cargo \"$(missing-helper)\" test",
        "cargo test >\"$(missing-helper)\"",
        "cat <<EOF\n$(missing-helper)\nEOF",
    ] {
        let temp = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(temp.path(), command);
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(OsString::new()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        assert!(
            check.detail.contains("must be run to verify"),
            "{command:?}"
        );
        assert!(!check.detail.contains("Missing command"), "{command:?}");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains("missing-helper"), "{command:?}");
        assert!(!serialized.contains("DOCTOR_DYNAMIC_TOOL"), "{command:?}");
        assert!(
            !serialized.contains("doctor-eval-missing-tool"),
            "{command:?}"
        );
    }
}
#[cfg(unix)]
#[test]
fn required_tools_preserve_known_presence_but_downgrade_inherited_shell_state() {
    for issue in [
        ShellEnvironmentIssue::BashEnv,
        ShellEnvironmentIssue::PosixEnv,
        ShellEnvironmentIssue::CdPath,
        ShellEnvironmentIssue::ImportedFunction,
    ] {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), "env cargo test");
        let tools = tempdir().unwrap();
        for executable in ["env", "cargo"] {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(tools.path(), None);
        environment.shell_environment_issue = Some(issue);

        let check = required_tools_check_with_environment(&ctx, &environment);

        assert!(check.ok, "{issue:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{issue:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(tool["present"].is_null(), "{issue:?}");
        let programs = tool["programs"].as_array().unwrap();
        assert_eq!(programs[0]["program"], "env", "{issue:?}");
        assert_eq!(programs[0]["present"], true, "{issue:?}");
        assert_eq!(programs[1]["program"], "cargo", "{issue:?}");
        assert_eq!(programs[1]["present"], true, "{issue:?}");
        assert!(programs.last().unwrap()["present"].is_null(), "{issue:?}");
        assert!(
            !serde_json::to_string(&check)
                .unwrap()
                .contains("No external executable required")
        );
    }
}
#[cfg(unix)]
#[test]
fn required_tools_downgrade_prior_dispatch_mutations() {
    for (command, target) in [
        ("hash -p /tmp/shim cargo; cargo test", "cargo"),
        ("enable -f /tmp/plugin custom; custom", "custom"),
        ("trap 'missing-helper' DEBUG; cargo test", "cargo"),
    ] {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), command);
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join(target), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(tool["present"].is_null(), "{command:?}");
        let target = tool["programs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|program| program["program"] == target)
            .unwrap();
        assert!(target["present"].is_null(), "{command:?}");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains("/tmp/shim"), "{command:?}");
        assert!(!serialized.contains("/tmp/plugin"), "{command:?}");
        assert!(!serialized.contains("missing-helper"), "{command:?}");
    }
}
#[cfg(unix)]
#[test]
fn required_tools_resolve_literal_relative_and_empty_path_from_repo_root() {
    let _env = lock_env();
    for (command, relative_executable) in [
        ("PATH=bin cargo test", "bin/cargo"),
        ("PATH= cargo test", "cargo"),
    ] {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), command);
        let invocation = repo.path().join("invocation/subdir");
        fs::create_dir_all(&invocation).unwrap();
        let executable = repo.path().join(relative_executable);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        write_test_executable(&executable, "#!/bin/sh\nexit 0\n");
        let _cwd = CurrentDirGuard::set(&invocation);
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(invocation.as_os_str().to_os_string()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present", "{command:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert_eq!(tool["present"], true, "{command:?}");
        assert_eq!(tool["programs"][0]["present"], true, "{command:?}");
    }
}
#[cfg(unix)]
#[test]
fn required_tools_accept_external_env_non_bash_assignment_names() {
    let repo = tempdir().unwrap();
    write_doctor_fixture_with_bootstrap_command(repo.path(), "env FOO.BAR=x cargo test");
    let tools = tempdir().unwrap();
    for executable in ["env", "cargo"] {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present");
    let programs = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[1]["program"], "cargo");
    assert!(programs.iter().all(|program| program["present"] == true));

    fs::remove_file(tools.path().join("cargo")).unwrap();
    let missing =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));
    assert!(!missing.ok);
    assert_eq!(missing.status, "missing");
    let programs = missing.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[0]["present"], true);
    assert_eq!(programs[1]["program"], "cargo");
    assert_eq!(programs[1]["present"], false);
}
#[cfg(unix)]
#[test]
fn required_tools_avoid_cwd_false_present_and_false_missing_results() {
    for tool_location in ["root", "sub"] {
        let temp = tempdir().unwrap();
        let sub = temp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        write_doctor_fixture_with_bootstrap_command(temp.path(), "env -C sub ./doctor-cwd-tool");
        let tool = if tool_location == "root" {
            temp.path().join("doctor-cwd-tool")
        } else {
            sub.join("doctor-cwd-tool")
        };
        write_test_executable(&tool, "#!/bin/sh\nexit 0\n");
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(tools.path().as_os_str().to_os_string()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{tool_location}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{tool_location}");
        let bootstrap = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(bootstrap["present"].is_null(), "{tool_location}");
        assert_eq!(bootstrap["programs"][0]["program"], "env");
        assert_eq!(bootstrap["programs"][0]["present"], true);
        assert!(bootstrap["programs"][1]["present"].is_null());
    }
}
#[cfg(unix)]
#[test]
fn required_tools_neither_resolves_nor_probes_changed_path_invocations() {
    for repo_tool_is_present in [false, true] {
        let temp = tempdir().unwrap();
        let repo_tools = temp.path().join("repo-secret-bin");
        fs::create_dir(&repo_tools).unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "PATH=repo-secret-bin; sqlx prepare -D sqlite:path-secret.db",
        );

        let ambient = tempdir().unwrap();
        let marker = temp.path().join("path-probe-must-not-run");
        let body = format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display());
        if repo_tool_is_present {
            write_test_executable(&repo_tools.join("sqlx"), &body);
        } else {
            write_test_executable(&ambient.path().join("sqlx"), &body);
        }
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(ambient.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        let sqlx_tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "sqlx_check_command")
            .unwrap();
        assert!(sqlx_tool["present"].is_null());
        assert!(sqlx_tool["programs"][0]["present"].is_null());
        assert_eq!(
            sqlx_tool["programs"][0]["driver_probe"]["status"],
            "unverified"
        );
        assert!(!marker.exists());

        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains("repo-secret-bin"));
        assert!(!serialized.contains("path-secret"));
        assert!(serialized.contains("may change the executable lookup context"));
    }
}
#[cfg(unix)]
#[test]
fn required_tools_localizes_changed_path_and_only_probes_captured_path() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "PATH=repo-tools sqlx prepare -D sqlite:first.db && sqlx prepare -D sqlite:second.db",
    );
    let tools = tempdir().unwrap();
    let marker = temp.path().join("ambient-probe-count");
    let repo_tools = temp.path().join("repo-tools");
    fs::create_dir(&repo_tools).unwrap();
    write_test_executable(
        &repo_tools.join("sqlx"),
        &format!("#!/bin/sh\nprintf r >> '{}'\nexit 0\n", marker.display()),
    );
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!("#!/bin/sh\nprintf x >> '{}'\nexit 0\n", marker.display()),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    let programs = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "sqlx_check_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(programs[0]["present"], true);
    assert_eq!(programs[0]["driver_probe"]["status"], "unverified");
    assert_eq!(programs[1]["present"], true);
    assert_eq!(programs[1]["driver_probe"]["status"], "compatible");
    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
}
#[cfg(unix)]
#[test]
fn required_tools_fails_open_for_nontransparent_wrapper_options() {
    for command in [
        "command -p cargo sqlx prepare -D sqlite:command-p-wrapper-secret.db",
        "exec -a private-argv-zero cargo sqlx prepare -D sqlite:exec-a-wrapper-secret.db",
        "exec -c cargo sqlx prepare",
    ] {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), command);
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &doctor_environment(&bin, Some("sqlite:ambient-wrapper-secret.db")),
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        assert!(check.fix.is_none());
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .filter_map(|program| program["program"].as_str())
                .all(|program| matches!(program, "cargo" | "cargo-sqlx")),
            "{command:?}",
        );
        let serialized = serde_json::to_string(&check).unwrap();
        for secret in [
            "command-p-wrapper-secret",
            "exec-a-wrapper-secret",
            "ambient-wrapper-secret",
            "private-argv-zero",
        ] {
            assert!(!serialized.contains(secret), "{command:?}: leaked {secret}");
        }
    }
}
