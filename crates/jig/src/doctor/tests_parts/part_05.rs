
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_executes_the_launcher_through_a_clean_bash_environment() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let poison_marker = temp.path().join("proxy-poison-ran");
    let trace_marker = temp.path().join("proxy-trace-poison-ran");
    fs::write(
        temp.path().join("scripts/proxy-startup-poison.sh"),
        "printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"\nexit 91\n",
    )
    .unwrap();
    fs::write(
            temp.path().join("scripts/jig"),
            r#"#!/bin/bash
if [ "$#" -ne 3 ] || [ "$1" != proxy ] || [ "$2" != list ] || [ "$3" != --json ]; then
  exit 19
fi
if [ ! -f .jig.toml ]; then
  exit 20
fi
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ]; then
  exit 21
fi
if declare -F jig_doctor_proxy_poison >/dev/null; then
  exit 22
fi
case "$-" in *x*|*v*) exit 23 ;; esac
shopt -q extglob && exit 24
case "$PS4" in *JIG_DOCTOR_PROXY_PS4_POISON*) exit 25 ;; esac
[ "$JIG_DOCTOR_PROXY_ORDINARY" = preserved ] || exit 26
printf '%s\n' '{"ok":true,"running":false,"routes":[]}'
"#,
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            temp.path().join("scripts/jig"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let (_, mut command) = proxy_list_command(temp.path()).unwrap();
    command
        .env(
            "BASH_ENV",
            temp.path().join("scripts/proxy-startup-poison.sh"),
        )
        .env("ENV", temp.path().join("scripts/proxy-startup-poison.sh"))
        .env("CDPATH", ".")
        .env(
            "BASH_FUNC_jig_doctor_proxy_poison%%",
            "() { printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"; }",
        )
        .env("SHELLOPTS", "xtrace:verbose")
        .env("BASHOPTS", "extglob")
        .env(
            "PS4",
            "JIG_DOCTOR_PROXY_PS4_POISON$(printf poison > \"$JIG_DOCTOR_PROXY_TRACE_MARKER\")",
        )
        .env("BASH_XTRACEFD", "2")
        .env("JIG_DOCTOR_PROXY_POISON_MARKER", &poison_marker)
        .env("JIG_DOCTOR_PROXY_TRACE_MARKER", &trace_marker)
        .env("JIG_DOCTOR_PROXY_ORDINARY", "preserved");
    crate::shell::sanitize_bash_environment(&mut command);

    let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["running"], false);
    assert_eq!(output["routes"], json!([]));
    assert!(
        !poison_marker.exists(),
        "Bash startup control environment executed during proxy diagnostics"
    );
    assert!(
        !trace_marker.exists(),
        "Bash trace environment executed during proxy diagnostics"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_accepts_valid_json_larger_than_the_diagnostic_default() {
    let temp = tempdir().unwrap();
    let launcher = temp.path().join("proxy-list-valid");
    write_test_executable(
        &launcher,
        "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 1100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
    );
    let mut command = Command::new(launcher);

    let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["running"], false);
    assert_eq!(output["routes"], json!([]));
    assert!(
        output["padding"].as_str().unwrap().len() > ProcessOutputLimits::default().stdout,
        "proxy functional JSON must not share the 16 KiB diagnostic cap"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_reports_injected_stdout_truncation() {
    let temp = tempdir().unwrap();
    let launcher = temp.path().join("proxy-list-truncated");
    write_test_executable(
        &launcher,
        "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
    );
    let mut command = Command::new(launcher);

    let error = proxy_list_output_with_timeout_and_limits_and_cancellation(
        &mut command,
        Duration::from_secs(2),
        ProcessOutputLimits {
            stdout: 128,
            stderr: ProcessOutputLimits::default().stderr,
        },
        || false,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("exceeded the diagnostic capture limit"),
        "{error}"
    );
}

#[cfg(feature = "dev-proxy")]
#[test]
fn proxy_list_capture_limit_exceeds_the_unchanged_routes_file_limit() {
    assert_eq!(jig_dev_proxy::MAX_ROUTES_FILE_BYTES, 4 * 1024 * 1024);
    let limits = proxy_list_output_limits();
    assert!(limits.stdout > jig_dev_proxy::MAX_ROUTES_FILE_BYTES as usize);
    assert_eq!(limits.stderr, ProcessOutputLimits::default().stderr);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_timeout_reaps_its_exact_descendant() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("proxy-list-descendant");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "doctor::tests::proxy_list_output_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
        .env("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER", &marker);

    let error = proxy_list_output_with_timeout(&mut command, Duration::from_millis(100))
        .unwrap_err()
        .to_string();
    let descendant = read_test_process_identity(&marker);

    assert!(error.contains("timed out"), "{error}");
    assert_test_process_stopped(&descendant);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_check_sigint_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_PROXY_SIGINT_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after proxy cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_check_sigint_reaps_its_exact_descendant_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
        ),
    )
    .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::proxy_list_output_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let descendant_marker = temp.path().join("proxy-sigint-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::proxy_check_sigint_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_PROXY_SIGINT_ROOT", temp.path())
        .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
        .env(
            "JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER",
            &descendant_marker,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the live isolated helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("SIGINT helper did not terminate after proxy cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_sqlx_probe_sigint_helper() {
    let Some(marker) = std::env::var_os("JIG_DOCTOR_SQLX_PRODUCTION_MARKER") else {
        return;
    };
    let identity = TestProcessIdentity::capture_current().unwrap();
    publish_test_process_identity(Path::new(&marker), &identity);
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_sqlx_sigint_sequence_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_SQLX_SEQUENCE_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after SQLx cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_during_production_sqlx_prevents_codex_and_proxy_spawns() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "sqlx prepare -D sqlite:production-signal.db",
    );
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

    let probe_marker = temp.path().join("sqlx-production-probe");
    let tools = tempdir().unwrap();
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nJIG_DOCTOR_SQLX_PRODUCTION_MARKER={} exec {} --exact doctor::tests::production_sqlx_probe_sigint_helper --nocapture\n",
            shell_quote_test_path(&probe_marker),
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let codex_marker = temp.path().join("codex-started");
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nprintf c > '{}'\nexit 0\n",
            codex_marker.display()
        ),
    );
    let proxy_marker = temp.path().join("proxy-started");
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            proxy_marker.display()
        ),
    );

    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::doctor_sqlx_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_SQLX_SEQUENCE_ROOT", temp.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env("PATH", fs::canonicalize(tools.path()).unwrap())
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let probe = read_test_process_identity(&probe_marker);
    // SAFETY: this test owns the isolated doctor helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("doctor helper did not terminate after SQLx cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&probe);
    assert!(
        !codex_marker.exists(),
        "Codex started after SQLx cancellation"
    );
    assert!(
        !proxy_marker.exists(),
        "proxy started after SQLx cancellation"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_codex_probe_helper() {
    let Some(marker) = std::env::var_os("JIG_DOCTOR_CODEX_DESCENDANT_MARKER") else {
        return;
    };
    for _ in 0..2_000 {
        println!("codex-probe-secret-that-must-not-leak");
        eprintln!("codex-probe-secret-that-must-not-leak");
    }
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::owned_process_descendant_helper",
            "--nocapture",
        ])
        .env(OWNED_PROCESS_DESCENDANT_MARKER_ENV, &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    std::mem::forget(child);
    let _ = read_test_process_identity(Path::new(&marker));
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_codex_sigint_sequence_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_CODEX_SEQUENCE_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after Codex cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_during_noisy_codex_reaps_descendant_and_prevents_proxy_spawn() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
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
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::production_codex_probe_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let proxy_marker = temp.path().join("proxy-started");
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            proxy_marker.display()
        ),
    );
    let descendant_marker = temp.path().join("codex-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::doctor_codex_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_CODEX_SEQUENCE_ROOT", temp.path())
        .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the isolated doctor helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("doctor helper did not terminate after Codex cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
    assert!(
        !proxy_marker.exists(),
        "proxy started after Codex cancellation"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn standalone_codex_sigint_sequence_helper() {
    let Some(codex) = std::env::var_os("JIG_STANDALONE_CODEX_SIGINT_BIN") else {
        return;
    };
    let result = standalone_codex_support_probe_with_signal_session(
        codex.as_os_str(),
        Duration::from_secs(30),
    );
    panic!("SIGINT was not re-delivered after standalone Codex cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn standalone_codex_sigint_reaps_its_exact_descendant_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::production_codex_probe_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let descendant_marker = temp.path().join("standalone-codex-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::standalone_codex_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_STANDALONE_CODEX_SIGINT_BIN", &codex)
        .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the isolated standalone doctor helper.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("standalone doctor helper did not terminate after Codex cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}

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
#[test]
fn required_tools_require_external_wrappers_and_their_targets() {
    let run = |command: &str, executables: &[&str]| {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), command);
        let tools = tempdir().unwrap();
        for executable in executables {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None))
    };
    let bootstrap_programs = |check: &DoctorCheck| {
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap()["programs"]
            .as_array()
            .unwrap()
            .clone()
    };

    for wrapper in ["env", "nohup"] {
        let command = format!("{wrapper} cargo test");
        let missing_wrapper = run(&command, &["cargo"]);
        assert_eq!(missing_wrapper.status, "missing", "{wrapper}");
        assert!(!missing_wrapper.ok, "{wrapper}");
        let programs = bootstrap_programs(&missing_wrapper);
        assert_eq!(programs[0]["program"], wrapper, "{wrapper}");
        assert_eq!(programs[0]["present"], false, "{wrapper}");
        assert_eq!(programs[1]["program"], "cargo", "{wrapper}");
        assert_eq!(programs[1]["present"], true, "{wrapper}");

        let missing_target = run(&command, &[wrapper]);
        assert_eq!(missing_target.status, "missing", "{wrapper}");
        let programs = bootstrap_programs(&missing_target);
        assert_eq!(programs[0]["present"], true, "{wrapper}");
        assert_eq!(programs[1]["present"], false, "{wrapper}");

        let all_present = run(&command, &[wrapper, "cargo"]);
        assert_eq!(all_present.status, "present", "{wrapper}");
        assert!(all_present.ok, "{wrapper}");
    }

    for command in ["env --help", "env -0"] {
        let missing_wrapper = run(command, &[]);
        assert_eq!(missing_wrapper.status, "missing", "{command:?}");
        let programs = bootstrap_programs(&missing_wrapper);
        assert_eq!(programs.len(), 1, "{command:?}");
        assert_eq!(programs[0]["program"], "env", "{command:?}");
        assert_eq!(programs[0]["present"], false, "{command:?}");
    }

    let dynamic_target = run("env \"$TOOL\" test", &[]);
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
