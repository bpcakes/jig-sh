use super::*;

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
