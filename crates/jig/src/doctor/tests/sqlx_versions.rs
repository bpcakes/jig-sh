use crate::doctor::sqlx_driver_probe::SqlxProbeStyle;
#[cfg(unix)]
use std::fs;

#[cfg(unix)]
use jig_context::RepoContext;
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
use super::support::{
    doctor_environment, write_sqlx_doctor_fixture_with_command, write_test_executable,
    write_workspace_version_manifest,
};
#[cfg(unix)]
use crate::doctor::environment::DoctorProcessControl;
#[cfg(unix)]
use crate::doctor::sqlx_cli::sqlx_cli_version_check;

#[cfg(unix)]
#[test]
fn sqlx_cli_versions_follow_the_invocation_style() {
    for (command, program, arguments, product, version) in [
        (
            "cargo sqlx prepare --check",
            "cargo-sqlx",
            "sqlx --version",
            "sqlx-cli-sqlx",
            "0.8.6",
        ),
        (
            "sqlx prepare --check",
            "sqlx",
            "--version",
            "sqlx-cli",
            "0.9.0",
        ),
        (
            "cargo sqlx prepare --check",
            "cargo-sqlx",
            "sqlx --version",
            "sqlx-cli-sqlx",
            "0.9.0",
        ),
        (
            "cargo-sqlx sqlx prepare --check",
            "cargo-sqlx",
            "sqlx --version",
            "sqlx-cli-sqlx",
            "0.9.3",
        ),
        (
            "cargo sqlx prepare --check",
            "cargo-sqlx",
            "sqlx --version",
            "sqlx-cli",
            "0.8.6",
        ),
    ] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("ExampleProject");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_sqlx_doctor_fixture_with_command(&root, command);
        write_workspace_version_manifest(&root, "1.94", Some(&version[..3]));
        write_test_executable(
            &bin.join(program),
            &format!(
                "#!/bin/sh\n[ \"$*\" = '{arguments}' ] || exit 1\nprintf '{product} {version}\\n'\n"
            ),
        );
        let ctx = RepoContext::load_from_root(root).unwrap();
        let check = sqlx_cli_version_check(
            &ctx,
            &doctor_environment(&bin, None),
            DoctorProcessControl::allowed_without_signal_session(),
        )
        .unwrap();
        assert!(check.ok, "{command}: {check:?}");
        assert_eq!(check.status, "compatible");
        assert_eq!(check.data["actual"], version);
    }
}

#[cfg(unix)]
#[test]
fn sqlx_cli_versions_reject_invalid_output_and_incompatible_lines() {
    for (style, product) in [
        (SqlxProbeStyle::Direct, "sqlx-cli"),
        (SqlxProbeStyle::CargoSubcommand, "sqlx-cli-sqlx"),
    ] {
        for output in [
            "unrelated 0.9.0".to_string(),
            format!("{product} invalid"),
            format!("{product} 0.9"),
            format!("{product} 0.9.0-beta.1"),
            format!("{product} 0.9.0 extra"),
            format!("{product} 0.9.0\nwarning"),
            format!("{product} 0.8.6"),
        ] {
            let temp = tempdir().unwrap();
            let root = temp.path().join("ExampleProject");
            let bin = temp.path().join("bin");
            fs::create_dir_all(&bin).unwrap();
            let (command, program) = match style {
                SqlxProbeStyle::Direct => ("sqlx prepare --check", "sqlx"),
                SqlxProbeStyle::CargoSubcommand => ("cargo sqlx prepare --check", "cargo-sqlx"),
            };
            write_sqlx_doctor_fixture_with_command(&root, command);
            write_workspace_version_manifest(&root, "1.94", Some("0.9"));
            write_test_executable(
                &bin.join(program),
                &format!("#!/bin/sh\nprintf '{output}\\n'\n"),
            );
            let ctx = RepoContext::load_from_root(root).unwrap();
            let check = sqlx_cli_version_check(
                &ctx,
                &doctor_environment(&bin, None),
                DoctorProcessControl::allowed_without_signal_session(),
            )
            .unwrap();
            assert!(!check.ok, "{command}, {output}: {check:?}");
            assert_eq!(
                check.status,
                if output.ends_with("0.8.6") {
                    "incompatible"
                } else {
                    "unverified"
                }
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn sqlx_cli_version_check_does_not_execute_checkout_programs() {
    for (command, program) in [
        ("sqlx prepare --check", "sqlx"),
        ("cargo sqlx prepare --check", "cargo-sqlx"),
    ] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("ExampleProject");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_sqlx_doctor_fixture_with_command(&root, command);
        write_workspace_version_manifest(&root, "1.94", Some("0.9"));
        let marker = temp.path().join("executed");
        write_test_executable(
            &bin.join(program),
            &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        );
        let ctx = RepoContext::load_from_root(root).unwrap();
        let check = sqlx_cli_version_check(
            &ctx,
            &doctor_environment(&bin, None),
            DoctorProcessControl::allowed_without_signal_session(),
        )
        .unwrap();
        assert!(!check.ok);
        assert_eq!(check.status, "unverified");
        assert!(!marker.exists());
    }
}

#[cfg(unix)]
#[test]
fn sqlx_cli_version_check_requires_the_dependency_minor_line() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_sqlx_doctor_fixture_with_command(&root, "sqlx prepare -D sqlite:doctor.db");
    write_workspace_version_manifest(&root, "1.94", Some("0.9"));
    write_test_executable(&bin.join("sqlx"), "#!/bin/sh\nprintf 'sqlx-cli 0.8.6\\n'\n");
    let ctx = RepoContext::load_from_root(root).unwrap();

    let check = sqlx_cli_version_check(
        &ctx,
        &doctor_environment(&bin, None),
        DoctorProcessControl::allowed_without_signal_session(),
    )
    .unwrap();

    assert!(!check.ok);
    assert_eq!(check.status, "incompatible");
    assert_eq!(check.data["required"], "0.9");
    assert_eq!(check.data["actual"], "0.8.6");
    assert!(check.fix.as_deref().unwrap().contains("--version ^0.9"));
    assert!(check.fix.as_deref().unwrap().contains("features sqlite"));
}

#[cfg(unix)]
#[test]
fn sqlx_cli_version_check_accepts_matching_patch_versions_and_older_dependency_lines() {
    for (dependency, cli) in [("0.9", "0.9.3"), ("0.8", "0.8.6")] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_sqlx_doctor_fixture_with_command(&root, "sqlx prepare -D sqlite:doctor.db");
        write_workspace_version_manifest(&root, "1.94", Some(dependency));
        write_test_executable(
            &bin.join("sqlx"),
            &format!("#!/bin/sh\nprintf 'sqlx-cli {cli}\\n'\n"),
        );
        let ctx = RepoContext::load_from_root(root).unwrap();

        let check = sqlx_cli_version_check(
            &ctx,
            &doctor_environment(&bin, None),
            DoctorProcessControl::allowed_without_signal_session(),
        )
        .unwrap();

        assert!(check.ok, "{dependency} should accept {cli}: {check:?}");
        assert_eq!(check.status, "compatible");
        assert_eq!(check.data["actual"], cli);
        assert!(check.fix.is_none());
    }
}
