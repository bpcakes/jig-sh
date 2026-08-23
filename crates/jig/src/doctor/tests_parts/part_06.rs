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
        if matches!(case, "config" | "config_include" | "nested_config") {
            let config_dir = if case == "nested_config" {
                temp.path().join("crates/api/.cargo")
            } else {
                temp.path().join(".cargo")
            };
            fs::create_dir_all(&config_dir).unwrap();
            fs::write(
                config_dir.join("config.toml"),
                if case == "config_include" {
                    "include = 'dispatch.toml'\n"
                } else {
                    "[alias]\nsqlx = 'run --package fake'\n"
                },
            )
            .unwrap();
        }
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(tools.path(), None);
        if case == "environment" {
            environment.cargo_alias_sqlx = Some("run --package fake".into());
        } else if case == "relative_cargo_home" {
            environment.cargo_home = Some("relative-cargo-home".into());
        }

        let check = required_tools_check_with_environment(&ctx, &environment);

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
        assert_eq!(cargo_sqlx_program(&check)["present"], true, "{case}");
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
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
