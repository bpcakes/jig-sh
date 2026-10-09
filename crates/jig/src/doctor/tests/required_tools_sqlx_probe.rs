#[cfg(unix)]
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use jig_context::RepoContext;
#[cfg(unix)]
use serde_json::json;
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
use super::support::{
    cargo_sqlx_program, doctor_environment, write_sqlx_doctor_fixture_with_command,
    write_test_executable,
};
#[cfg(unix)]
use crate::doctor::environment::inherited_shell_environment_issue;
use crate::doctor::environment::{DoctorEnvironment, ShellEnvironmentIssue};
use crate::doctor::required_tools::required_tools_check_with_environment;
use crate::test_env::{EnvVarGuard, lock_env};

#[cfg(unix)]
#[test]
fn required_tools_does_not_probe_driver_from_assignment_removed_by_wrapper() {
    let temp = tempdir().unwrap();
    let secret = "doctor-removed-assignment-secret";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "DATABASE_URL=postgres://doctor:{secret}@localhost/demo env -u DATABASE_URL cargo-sqlx sqlx prepare"
        ),
    );
    let tools = tempdir().unwrap();
    let marker = tools.path().join("probe-marker");
    write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(tools.path(), Some("sqlite:ambient.db")),
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(!marker.exists());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["driver"],
        json!(null)
    );
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor"));
}
#[cfg(unix)]
#[test]
fn required_tools_preserve_sqlx_probe_through_external_wrapper_chain() {
    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    for executable in ["env", "nohup", "time"] {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let marker = tools.path().join("sqlx-probe-marker");
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
    );
    let time = tools.path().join("time");
    write_sqlx_doctor_fixture_with_command(
        repo.path(),
        &format!(
            "env nohup {} sqlx prepare -D sqlite:wrapper-chain.db",
            time.display()
        ),
    );
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present");
    assert!(marker.exists());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "compatible"
    );
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(&time.display().to_string()));
    assert!(!serialized.contains("wrapper-chain.db"));
}
#[cfg(unix)]
#[test]
fn required_tools_treats_indeterminate_sqlx_probe_as_present_unverified() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "cargo-sqlx sqlx prepare -D sqlite:doctor.db",
    );

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    write_test_executable(
        &bin.join("cargo-sqlx"),
        "#!/bin/sh\nprintf '%s\\n' 'unexpected doctor probe response'\nexit 2\n",
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(&bin, Some("sqlite:doctor.db")),
    );
    assert!(check.ok);
    assert_eq!(check.status, "present_unverified");
    assert!(check.fix.is_none());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    assert!(cargo_sqlx_program(&check)["driver_probe"]["compatible"].is_null());
    assert!(check.detail.contains("scripts/jig check sqlx"));
    assert!(check.detail.contains("in the SQLx CLI"));
    assert!(!check.detail.contains("in cargo-sqlx"));
    assert!(!check.detail.contains("reinstall"));
}
#[cfg(unix)]
#[test]
fn required_tools_does_not_execute_sqlx_probe_with_shell_environment_poisoning() {
    use std::os::unix::ffi::OsStringExt;

    let secret = "shell-environment-poison-secret";
    let issue = |controls: [Option<&OsStr>; 7], variables: Vec<(OsString, OsString)>| {
        inherited_shell_environment_issue(
            [
                (ShellEnvironmentIssue::BashEnv, controls[0]),
                (ShellEnvironmentIssue::PosixEnv, controls[1]),
                (ShellEnvironmentIssue::CdPath, controls[2]),
                (ShellEnvironmentIssue::ShellOptions, controls[3]),
                (ShellEnvironmentIssue::BashOptions, controls[4]),
                (ShellEnvironmentIssue::TracePrompt, controls[5]),
                (ShellEnvironmentIssue::TraceFileDescriptor, controls[6]),
            ],
            variables,
        )
    };
    let scenarios = [
        issue(
            [Some(OsStr::new(secret)), None, None, None, None, None, None],
            Vec::new(),
        ),
        issue(
            [None, Some(OsStr::new(secret)), None, None, None, None, None],
            Vec::new(),
        ),
        issue(
            [None, None, Some(OsStr::new(secret)), None, None, None, None],
            Vec::new(),
        ),
        issue(
            [None, None, None, Some(OsStr::new(secret)), None, None, None],
            Vec::new(),
        ),
        issue(
            [None, None, None, None, Some(OsStr::new(secret)), None, None],
            Vec::new(),
        ),
        issue(
            [None, None, None, None, None, Some(OsStr::new(secret)), None],
            Vec::new(),
        ),
        issue(
            [None, None, None, None, None, None, Some(OsStr::new(secret))],
            Vec::new(),
        ),
        issue(
            [None; 7],
            vec![(
                OsString::from("BASH_FUNC_sqlx%%"),
                OsString::from(format!("() {{ printf {secret}; }}")),
            )],
        ),
        issue(
            [None; 7],
            vec![(
                OsString::from_vec(b"BASH_FUNC_sqlx_\xff%%".to_vec()),
                OsString::from(format!("() {{ printf {secret}; }}")),
            )],
        ),
    ];
    assert_eq!(
        scenarios,
        [
            Some(ShellEnvironmentIssue::BashEnv),
            Some(ShellEnvironmentIssue::PosixEnv),
            Some(ShellEnvironmentIssue::CdPath),
            Some(ShellEnvironmentIssue::ShellOptions),
            Some(ShellEnvironmentIssue::BashOptions),
            Some(ShellEnvironmentIssue::TracePrompt),
            Some(ShellEnvironmentIssue::TraceFileDescriptor),
            Some(ShellEnvironmentIssue::ImportedFunction),
            Some(ShellEnvironmentIssue::ImportedFunction),
        ]
    );

    for (index, issue) in scenarios.into_iter().enumerate() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:doctor.db");
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let marker = temp.path().join(format!("probe-ran-{index}"));
        write_test_executable(
            &bin.join("sqlx"),
            &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(&bin, Some("sqlite:doctor.db"));
        environment.shell_environment_issue = issue;

        let check = required_tools_check_with_environment(&ctx, &environment);

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert!(
            check
                .detail
                .contains("external executable reference(s) inspected")
        );
        assert!(
            !marker.exists(),
            "ambient shell state allowed probe execution"
        );
        let probe = &cargo_sqlx_program(&check)["driver_probe"];
        assert!(probe["driver"].is_null());
        assert!(probe["source"].is_null());
        assert_eq!(probe["status"], "unverified");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(secret));
        assert!(serialized.contains("inherited shell state"));
    }

    assert_eq!(issue([None; 7], Vec::new()), None);
    assert_eq!(issue([Some(OsStr::new("")); 7], Vec::new()), None);
}
#[test]
fn doctor_environment_capture_audits_bash_startup_state_without_retaining_values() {
    let _env = lock_env();
    let _posix_env = EnvVarGuard::remove("ENV");
    let _cdpath = EnvVarGuard::remove("CDPATH");
    let secret = "doctor-bash-env-secret";
    let _bash_env = EnvVarGuard::set("BASH_ENV", secret);

    let environment = DoctorEnvironment::capture();

    assert_eq!(
        environment.shell_environment_issue,
        Some(ShellEnvironmentIssue::BashEnv)
    );
    assert!(!format!("{environment:?}").contains(secret));
}
#[cfg(unix)]
#[test]
fn required_tools_probes_bare_path_forms_but_not_explicit_sqlx_paths() {
    let temp = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    let probe_log = temp.path().join("probe-log");
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &bin.join("sqlx"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nprintf d >> '{}'\nexit 0\n",
            probe_log.display()
        ),
    );
    write_test_executable(
        &bin.join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = sqlx ] || exit 9\n[ \"$2\" = migrate ] || exit 9\nprintf c >> '{}'\nexit 0\n",
            probe_log.display()
        ),
    );
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "CARGO=cargo sqlx prepare -D sqlite:direct.db && {} sqlx prepare -Dsqlite:shim.db && {} sqlx prepare -D=sqlite:cargo.db",
            bin.join("cargo-sqlx").display(),
            bin.join("cargo").display(),
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert_eq!(fs::read_to_string(probe_log).unwrap(), "d");
    let probes = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .filter_map(|program| program.get("driver_probe"))
        .collect::<Vec<_>>();
    assert_eq!(probes.len(), 3);
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "compatible")
            .count(),
        1
    );
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "unverified")
            .count(),
        2
    );
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(&bin.display().to_string()));
    assert!(!serialized.contains("sqlite:direct.db"));
}
#[cfg(unix)]
#[test]
fn required_tools_never_executes_repo_local_or_explicit_sqlx_tools() {
    let temp = tempdir().unwrap();
    let repo_bin = temp.path().join("bin");
    let repo_scripts = temp.path().join("scripts");
    fs::create_dir(&repo_bin).unwrap();
    fs::create_dir(&repo_scripts).unwrap();
    let external = tempdir().unwrap();
    let marker = temp.path().join("probe-must-not-run");
    let body = format!("#!/bin/sh\nprintf ran >> '{}'\nexit 0\n", marker.display());
    write_test_executable(&repo_bin.join("sqlx"), &body);
    write_test_executable(&repo_scripts.join("sqlx"), &body);
    write_test_executable(&external.path().join("sqlx"), &body);
    let relative_external = Path::new("..").join(
        external
            .path()
            .file_name()
            .expect("temporary tool directory has a basename"),
    );
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "sqlx prepare -D sqlite:repo-path.db && scripts/sqlx prepare -D sqlite:repo-explicit.db && {}/sqlx prepare -D sqlite:custom-relative.db && {} prepare -D sqlite:custom-absolute.db",
            relative_external.display(),
            external.path().join("sqlx").display(),
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&repo_bin, None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(!marker.exists());
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(&temp.path().display().to_string()));
    assert!(!serialized.contains(&external.path().display().to_string()));

    let symlink_repo = tempdir().unwrap();
    let symlink_scripts = symlink_repo.path().join("scripts");
    let symlink_marker = symlink_repo.path().join("probe-must-not-run");
    write_sqlx_doctor_fixture_with_command(
        symlink_repo.path(),
        "sqlx prepare -D sqlite:symlink.db",
    );
    let symlink_body = format!(
        "#!/bin/sh\nprintf ran >> '{}'\nexit 0\n",
        symlink_marker.display()
    );
    write_test_executable(&symlink_scripts.join("sqlx"), &symlink_body);
    let symlink_path = tempdir().unwrap();
    std::os::unix::fs::symlink(
        symlink_scripts.join("sqlx"),
        symlink_path.path().join("sqlx"),
    )
    .unwrap();
    let symlink_ctx = RepoContext::load_from_root(symlink_repo.path().to_path_buf()).unwrap();
    let symlink_check = required_tools_check_with_environment(
        &symlink_ctx,
        &doctor_environment(symlink_path.path(), None),
    );
    assert!(symlink_check.ok, "{}", symlink_check.detail);
    assert_eq!(symlink_check.status, "present_unverified");
    assert!(!symlink_marker.exists());

    let linked_directory_repo = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        linked_directory_repo.path(),
        "sqlx prepare -D sqlite:linked-directory.db",
    );
    let real_tools = tempdir().unwrap();
    let linked_directory_marker = linked_directory_repo.path().join("probe-must-not-run");
    write_test_executable(
        &real_tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nprintf ran > '{}'\nexit 0\n",
            linked_directory_marker.display()
        ),
    );
    let path_container = tempdir().unwrap();
    let linked_tools = path_container.path().join("linked-tools");
    std::os::unix::fs::symlink(real_tools.path(), &linked_tools).unwrap();
    let linked_directory_ctx =
        RepoContext::load_from_root(linked_directory_repo.path().to_path_buf()).unwrap();
    let linked_directory_check = required_tools_check_with_environment(
        &linked_directory_ctx,
        &DoctorEnvironment {
            search_path: Some(linked_tools.into_os_string()),
            ..DoctorEnvironment::default()
        },
    );
    assert!(
        linked_directory_check.ok,
        "{}",
        linked_directory_check.detail
    );
    assert_eq!(linked_directory_check.status, "present_unverified");
    assert!(!linked_directory_marker.exists());
}
#[cfg(unix)]
#[test]
fn required_tools_marks_no_url_and_custom_sqlx_wrappers_unverified() {
    let no_url = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(no_url.path(), "cargo sqlx prepare --no-dotenv");
    let no_url_bin = no_url.path().join("bin");
    fs::create_dir(&no_url_bin).unwrap();
    write_test_executable(&no_url_bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&no_url_bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let no_url_ctx = RepoContext::load_from_root(no_url.path().to_path_buf()).unwrap();

    let no_url_check =
        required_tools_check_with_environment(&no_url_ctx, &doctor_environment(&no_url_bin, None));
    assert!(no_url_check.ok, "{}", no_url_check.detail);
    assert_eq!(no_url_check.status, "present_unverified");
    assert!(no_url_check.fix.is_none());
    assert!(no_url_check.detail.contains("scripts/jig check sqlx"));
    assert!(
        !no_url_check
            .detail
            .to_ascii_lowercase()
            .contains("reinstall")
    );

    let wrapper = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(wrapper.path(), "scripts/private-sqlx-wrapper --check");
    write_test_executable(
        &wrapper.path().join("scripts/private-sqlx-wrapper"),
        "#!/bin/sh\nexit 99\n",
    );
    let wrapper_bin = wrapper.path().join("bin");
    fs::create_dir(&wrapper_bin).unwrap();
    let wrapper_ctx = RepoContext::load_from_root(wrapper.path().to_path_buf()).unwrap();

    let wrapper_check = required_tools_check_with_environment(
        &wrapper_ctx,
        &doctor_environment(&wrapper_bin, None),
    );
    assert!(wrapper_check.ok, "{}", wrapper_check.detail);
    assert_eq!(wrapper_check.status, "present_unverified");
    assert!(wrapper_check.fix.is_none());
    let serialized = serde_json::to_string(&wrapper_check).unwrap();
    assert!(!serialized.contains("private-sqlx-wrapper"));
    assert!(serialized.contains("<redacted: command executable>"));
}
