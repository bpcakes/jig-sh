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
fn doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches() {
    const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BATCH_HELPER";
    if let Some(root) = std::env::var_os(HELPER) {
        let root = PathBuf::from(root);
        let ctx = RepoContext::load_from_root(root.clone()).unwrap();
        let first = doctor_context_checks(&ctx);
        assert!(first.required_tools.ok, "{}", first.required_tools.detail);
        assert_eq!(
            cargo_sqlx_program(&first.required_tools)["driver_probe"]["status"],
            "compatible"
        );
        assert_eq!(first.agent.status, "missing", "{}", first.agent.detail);
        assert_eq!(first.agent.data["codex"]["available"], true);
        assert_eq!(first.proxy.status, "not running", "{}", first.proxy.detail);
        assert_eq!(
            fs::read_to_string(root.join("probe-count")).unwrap(),
            "dckp"
        );

        let second = doctor_context_checks(&ctx);
        assert!(second.required_tools.ok, "{}", second.required_tools.detail);
        assert_eq!(second.required_tools.status, "present");
        assert_eq!(
            cargo_sqlx_program(&second.required_tools)["driver_probe"]["status"],
            "compatible"
        );
        assert_eq!(second.agent.status, "missing", "{}", second.agent.detail);
        assert_eq!(second.agent.data["codex"]["available"], true);
        assert_eq!(
            second.proxy.status, "not running",
            "{}",
            second.proxy.detail
        );
        assert_eq!(
            fs::read_to_string(root.join("probe-count")).unwrap(),
            "dckpdckp"
        );
        return;
    }

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "sqlx prepare -D sqlite:reusable.db && cargo-sqlx sqlx prepare -D sqlite:reusable.db",
    );
    let tools = tempdir().unwrap();
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nprintf d >> '{}'\nexit 0\n",
            temp.path().join("probe-count").display()
        ),
    );
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf c >> '{}'\nexit 0\n",
            temp.path().join("probe-count").display()
        ),
    );
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
        ),
    )
    .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    fs::write(
            temp.path().join(".jig.toml"),
            fs::read_to_string(temp.path().join(".jig.toml"))
                .unwrap()
                .replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                ),
        )
        .unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nprintf k >> '{}'\n[ \"$*\" = \"plugin marketplace add --help\" ]\n",
            temp.path().join("probe-count").display()
        ),
    );
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p >> '{}'\nprintf '%s\\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            temp.path().join("probe-count").display()
        ),
    );
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches",
            "--nocapture",
        ])
        .env(HELPER, temp.path())
        .env("PATH", fs::canonicalize(tools.path()).unwrap())
        .env("JIG_CODEX_BIN", codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "reusable batch helper exited with {status}"
    );
}

#[cfg(unix)]
#[test]
fn signal_retirement_failure_invalidates_every_configured_process_check() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:retirement.db");
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let mut checks = DoctorContextChecks {
        required_tools: check(
            "required_tools",
            "Required tools",
            true,
            true,
            "present",
            "present",
        ),
        rust_runtime: Some(check(
            "rust_runtime",
            "Rust runtime",
            true,
            true,
            "compatible",
            "compatible",
        )),
        go_runtime: None,
        node_runtime: Some(check(
            "node_runtime",
            "Node runtime",
            true,
            true,
            "compatible",
            "compatible",
        )),
        sqlx_cli: Some(check(
            "sqlx_cli",
            "SQLx CLI",
            true,
            true,
            "compatible",
            "compatible",
        )),
        agent: check(
            "agent_skills",
            "Agent skills",
            false,
            true,
            "installed",
            "installed",
        ),
        proxy: check("proxy", "Dev proxy", false, true, "running", "running"),
    };

    mark_doctor_signal_retirement_failure(&ctx, &mut checks);

    assert_eq!(checks.required_tools.status, "present_unverified");
    assert!(
        checks
            .required_tools
            .detail
            .contains("could not retire safely")
    );
    let node_runtime = checks.node_runtime.as_ref().unwrap();
    assert!(!node_runtime.ok);
    assert_eq!(node_runtime.status, "unverified");
    assert!(node_runtime.detail.contains("could not retire safely"));
    for runtime in [
        checks.rust_runtime.as_ref().unwrap(),
        checks.sqlx_cli.as_ref().unwrap(),
    ] {
        assert!(!runtime.ok);
        assert_eq!(runtime.status, "unverified");
        assert!(runtime.detail.contains("could not retire safely"));
    }
    for process_check in [&checks.agent, &checks.proxy] {
        assert!(!process_check.ok);
        assert_eq!(process_check.status, "error");
        assert!(process_check.detail.contains("could not retire safely"));
    }
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
