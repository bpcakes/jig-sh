#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use jig_context::RepoContext;
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
use super::support::{
    cargo_sqlx_program, doctor_environment, write_sqlx_doctor_fixture_with_command,
    write_test_executable,
};
use crate::doctor::cargo_sqlx::cargo_sqlx_command_changes_dispatch_environment;
#[cfg(unix)]
use crate::doctor::check::DoctorCheck;
use crate::doctor::required_tools::required_tools_check_with_environment;

#[test]
fn cargo_dispatch_detects_command_local_alias_and_home_environment_changes() {
    for command in [
        "CARGO_ALIAS_SQLX='run --package fake' cargo sqlx prepare",
        "CARGO_HOME=/tmp/cargo-home cargo sqlx prepare",
        "CARGO_HOME[0]=/tmp/cargo-home cargo sqlx prepare",
        "env HOME=/tmp/home cargo sqlx prepare",
        "env -i cargo sqlx prepare",
        "export CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
        "declare -x CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
        "builtin declare -x CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
        "declare -x CARGO_ALIAS_SQLX='run --package fake'; cargo sqlx prepare",
        "read CARGO_HOME <<< /tmp/cargo-home; cargo sqlx prepare",
        "printf -v CARGO_HOME %s /tmp/cargo-home; cargo sqlx prepare",
        "printf -v 'CARGO_HOME[0]' %s /tmp/cargo-home; cargo sqlx prepare",
        "mapfile -t CARGO_HOME </dev/null; cargo sqlx prepare",
        "getopts p CARGO_HOME; cargo sqlx prepare",
        "let CARGO_HOME=0; cargo sqlx prepare",
        "declare -n cargo_home_ref=CARGO_HOME; cargo sqlx prepare",
        "HOME=/tmp/home; cargo sqlx prepare",
        "exec -c cargo sqlx prepare",
    ] {
        assert!(
            cargo_sqlx_command_changes_dispatch_environment(command),
            "{command:?}",
        );
    }

    for command in [
        "OTHER_VALUE=/tmp cargo sqlx prepare",
        "env -u OTHER_VALUE cargo sqlx prepare",
        "printf '%s' HOME=/tmp/home; cargo sqlx prepare",
        "env export CARGO_HOME=/tmp/home; cargo sqlx prepare",
    ] {
        assert!(
            !cargo_sqlx_command_changes_dispatch_environment(command),
            "{command:?}",
        );
    }
}
#[cfg(unix)]
fn ambiguous_cargo_sqlx_check(case: &str) -> DoctorCheck {
    let temp = tempdir().unwrap();
    let command = match case {
        "command_environment" => {
            "CARGO_ALIAS_SQLX='run --package fake' cargo sqlx prepare -D sqlite:doctor.db"
        }
        "inline" => {
            "cargo --config alias.sqlx='run --package fake' sqlx prepare -D sqlite:doctor.db"
        }
        "inline_include" => {
            "cargo --config include='dispatch.toml' sqlx prepare -D sqlite:doctor.db"
        }
        "nested_config" => "cd crates/api && cargo sqlx prepare -D sqlite:doctor.db",
        _ => "cargo sqlx prepare -D sqlite:doctor.db",
    };
    write_sqlx_doctor_fixture_with_command(temp.path(), command);
    write_ambiguous_cargo_config(temp.path(), case);
    let tools = tempdir().unwrap();
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let mut environment = doctor_environment(tools.path(), None);
    if case == "environment" {
        environment.cargo_alias_sqlx = Some("run --package fake".into());
    } else if case == "relative_cargo_home" {
        environment.cargo_home = Some("relative-cargo-home".into());
    }
    required_tools_check_with_environment(&ctx, &environment)
}
#[cfg(unix)]
fn write_ambiguous_cargo_config(root: &Path, case: &str) {
    if !matches!(case, "config" | "config_include" | "nested_config") {
        return;
    }
    let config_dir = if case == "nested_config" {
        root.join("crates/api/.cargo")
    } else {
        root.join(".cargo")
    };
    fs::create_dir_all(&config_dir).unwrap();
    let contents = if case == "config_include" {
        "include = 'dispatch.toml'\n"
    } else {
        "[alias]\nsqlx = 'run --package fake'\n"
    };
    fs::write(config_dir.join("config.toml"), contents).unwrap();
}
#[cfg(unix)]
fn assert_ambiguous_cargo_sqlx(case: &str, check: &DoctorCheck) {
    assert!(check.ok, "{case}: {}", check.detail);
    assert_eq!(check.status, "present_unverified", "{case}");
    assert!(check.detail.contains("cargo sqlx dispatch"), "{case}");
    if matches!(
        case,
        "inline" | "inline_include" | "config" | "config_include" | "nested_config"
    ) {
        assert!(check.detail.contains("config"), "{case}: {}", check.detail);
    }
    if matches!(case, "environment" | "command_environment") {
        let detail = check.detail.to_ascii_lowercase();
        assert!(
            detail.contains("alias") || detail.contains("home"),
            "{case}: {}",
            check.detail,
        );
    }
    if case == "relative_cargo_home" {
        assert!(check.detail.contains("config"), "{case}: {}", check.detail);
    }
    assert_eq!(cargo_sqlx_program(check)["present"], true, "{case}");
    assert_eq!(
        cargo_sqlx_program(check)["driver_probe"]["status"],
        "unverified",
        "{case}",
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| program["program"] != "cargo-sqlx"),
        "{case}",
    );
}
#[cfg(unix)]
#[test]
fn required_tools_does_not_trust_ambiguous_cargo_sqlx_dispatch() {
    for case in [
        "environment",
        "command_environment",
        "inline",
        "inline_include",
        "config",
        "config_include",
        "nested_config",
        "relative_cargo_home",
    ] {
        let check = ambiguous_cargo_sqlx_check(case);
        assert_ambiguous_cargo_sqlx(case, &check);
    }
}
#[cfg(unix)]
#[test]
fn unresolved_cargo_does_not_probe_an_external_subcommand() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "cargo sqlx prepare -D sqlite:doctor.db");
    let tools = tempdir().unwrap();
    let probe_marker = temp.path().join("probe-marker");
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf probed > '{}'\nexit 0\n",
            probe_marker.display()
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    assert!(!probe_marker.exists());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    assert!(check.detail.contains("external cargo path does not prove"));
}
#[cfg(unix)]
#[test]
fn required_tools_reports_explicit_cargo_wrapper_without_subcommand_probe() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("scripts")).unwrap();
    let cargo = temp.path().join("scripts/cargo");
    write_test_executable(&cargo, "#!/bin/sh\nexit 0\n");
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "scripts/cargo sqlx prepare -D sqlite:doctor.db",
    );
    let tools = tempdir().unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(check.detail.contains("external cargo path does not prove"));
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| program["program"] != "cargo-sqlx")
    );
}
#[cfg(unix)]
#[test]
fn cargo_alias_leaves_cargo_unverified_while_direct_clis_probe() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "cargo sqlx prepare -D sqlite:alias.db && sqlx prepare -D sqlite:direct.db && cargo-sqlx sqlx prepare -D sqlite:shim.db",
    );
    let tools = tempdir().unwrap();
    let probe_log = temp.path().join("probe-log");
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!("#!/bin/sh\nprintf d >> '{}'\nexit 0\n", probe_log.display()),
    );
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!("#!/bin/sh\nprintf c >> '{}'\nexit 0\n", probe_log.display()),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let mut environment = doctor_environment(tools.path(), None);
    environment.cargo_alias_sqlx = Some("run --package fake".into());

    let check = required_tools_check_with_environment(&ctx, &environment);

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert_eq!(fs::read_to_string(probe_log).unwrap(), "dc");
    let probes = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .filter_map(|program| program.get("driver_probe"))
        .collect::<Vec<_>>();
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "unverified")
            .count(),
        1
    );
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "compatible")
            .count(),
        2
    );
}
#[cfg(unix)]
#[test]
fn required_tools_never_probes_cargo_subcommand_dispatch() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "DATABASE_URL=sqlite:first.db cargo sqlx prepare && cargo sqlx migrate info --database-url=sqlite:second.db",
    );

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    let probe_count = temp.path().join("probe-count");
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &bin.join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf x >> '{}'\nexit 0\n",
            probe_count.display()
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(check.ok);
    assert_eq!(check.status, "present_unverified");
    assert!(!probe_count.exists());
    assert_eq!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .filter(|program| program.get("driver_probe").is_some())
            .count(),
        2
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .filter_map(|program| program.get("driver_probe"))
            .all(|probe| probe["status"] == "unverified")
    );
}
