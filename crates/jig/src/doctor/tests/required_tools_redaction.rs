use super::*;

#[cfg(unix)]
use std::{env, fs};

#[cfg(unix)]
use jig_context::RepoContext;
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
use super::support::{
    cargo_sqlx_program, doctor_environment, write_doctor_fixture,
    write_sqlx_doctor_fixture_with_command, write_test_executable,
};
use crate::cli::format_doctor_summary_for_test as format_summary;
use crate::doctor::required_tools::required_tools_check_with_environment;

#[cfg(unix)]
#[test]
fn required_tools_redacts_sqlx_commands_even_when_resolution_is_ambiguous() {
    let temp = tempdir().unwrap();
    let secret = "doctor-inline-password";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "cargo sqlx prepare --database-url='postgres://doctor-user:{secret}@localhost/demo"
        ),
    );

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(check.ok);
    assert_eq!(check.status, "present_unverified");
    assert!(check.fix.is_none());
    assert_eq!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "sqlx_check_command")
            .unwrap()["command"],
        "<redacted: sqlx_check_command>"
    );

    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor-user"));
    assert!(!summary.contains(secret));
    assert!(!summary.contains("postgres://doctor-user"));
    assert!(summary.contains("present_unverified"));
    assert!(summary.contains("scripts/jig check sqlx"));
    assert!(summary.contains("Next required step: none"));
}
#[cfg(unix)]
#[test]
fn required_tools_redact_unquoted_database_url_expansion_values() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "cargo sqlx prepare --database-url=$DATABASE_URL",
    );
    let tools = tempdir().unwrap();
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    let secret = "doctor-unquoted-expansion-secret";
    let database_url = format!("sqlite:first.db -D postgres://doctor:{secret}@localhost/injected");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(tools.path(), Some(&database_url)),
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor"));
}
#[cfg(unix)]
#[test]
fn required_tools_nearest_dotenv_diagnostics_do_not_leak_values_or_home() {
    let temp = tempdir().unwrap();
    let child = temp.path().join("crates/api");
    fs::create_dir_all(&child).unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "cd crates/api && cargo sqlx prepare");
    let parent_secret = "parent-database-secret";
    let child_secret = "nearest-unrelated-secret";
    fs::write(
        temp.path().join(".env"),
        format!("DATABASE_URL=postgres://doctor:{parent_secret}@localhost/demo\n"),
    )
    .unwrap();
    fs::write(child.join(".env"), format!("OTHER_VALUE={child_secret}\n")).unwrap();
    let tools = tempdir().unwrap();
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&tools.path().join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(parent_secret));
        assert!(!rendered.contains(child_secret));
        if let Some(home) = env::var_os("HOME").and_then(|home| home.into_string().ok()) {
            assert!(!rendered.contains(&home));
        }
    }
}
#[cfg(unix)]
#[test]
fn required_tools_redacts_url_tokens_misparsed_as_sqlx_executables() {
    let temp = tempdir().unwrap();
    let secret = "misparsed-inline-password";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "postgres://doctor-user:{secret}@localhost/demo; cargo sqlx prepare --database-url='$DYNAMIC_DATABASE_URL'"
        ),
    );

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    let serialized = serde_json::to_string(&check).unwrap();
    let summary = format_summary(&output(None, vec![check]));
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor-user"));
    assert!(!summary.contains(secret));
    assert!(!summary.contains("postgres://doctor-user"));
    assert!(serialized.contains("<redacted: command executable>"));
}
#[cfg(unix)]
#[test]
fn required_tools_ignores_commented_sqlx_urls_and_wrapper_separator() {
    let temp = tempdir().unwrap();
    let secret = "commented-database-secret";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "command -v cargo >/dev/null && command -- cargo sqlx prepare # -D postgres://doctor-user:{secret}@localhost/demo"
        ),
    );
    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(&bin, Some("sqlite:doctor.db")),
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["driver"],
        "sqlite"
    );
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| program["program"] != "cargo-sqlx")
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| !matches!(program["program"].as_str(), Some("--" | "-v")))
    );
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("postgres://doctor-user"));
    }
}
#[cfg(unix)]
#[test]
fn required_tools_redacts_every_command_body_and_generic_credential_token() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let secret = "generic-required-command-secret";
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "bootstrap_command = \"printf bootstrap\"",
        &format!(
            "bootstrap_command = {:?}",
            format!("postgres://doctor-user:{secret}@localhost/demo --check")
        ),
    );
    fs::write(config_path, config).unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    assert_eq!(
        check.data["tools"][0]["command"],
        "<redacted: bootstrap_command>"
    );
    assert_eq!(check.data["tools"][0]["command_redacted"], true);
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("postgres://doctor-user"));
    }
    assert!(serialized.contains("<redacted: command executable>"));
}
