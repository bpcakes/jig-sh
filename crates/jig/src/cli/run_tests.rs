use super::*;

#[test]
fn template_errors_get_hint() {
    let missing_template_value =
        Cli::try_parse_from(["jig", "adopt", ".", "--template"]).unwrap_err();
    assert_eq!(
        missing_template_value.kind(),
        clap::error::ErrorKind::InvalidValue
    );
    assert!(should_add_template_hint(&missing_template_value));

    let unrelated = Cli::try_parse_from(["jig", "proxy", "run", "web", "vite"]).unwrap_err();
    assert!(!should_add_template_hint(&unrelated));
}

#[test]
fn legacy_check_commands_get_actionable_hint() {
    for (legacy, replacement) in [
        ("fmt-check", "jig check fmt"),
        ("clippy", "jig check clippy"),
        ("test", "jig check test"),
        ("test-locked", "jig check test-locked"),
        ("sqlx-check", "jig check sqlx"),
        ("schema-check", "jig check schema"),
        ("contract-check", "jig check contract"),
        ("check-agent-guides", "jig check agent-guides"),
        ("check-rust-file-loc", "jig check rust-file-loc"),
        ("check-no-mod-rs", "jig check no-mod-rs"),
        (
            "check-migration-immutability",
            "jig check migration-immutability",
        ),
        (
            "check-sqlx-unchecked-non-test",
            "jig check sqlx-unchecked-non-test",
        ),
    ] {
        let error = Cli::try_parse_from(["jig", legacy]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
        let message = error.to_string();
        assert!(
            message.contains("Usage: jig [OPTIONS] <COMMAND>"),
            "legacy top-level hint depends on Clap usage text for {legacy}: {message}"
        );
        assert!(
            message.contains(&format!("'{legacy}'")),
            "legacy top-level hint depends on Clap quoting the invalid command for {legacy}: {message}"
        );
        assert_eq!(
            moved_check_command_hint(&error),
            Some(format!("This check command moved. Use:\n  {replacement}")),
            "wrong moved-command hint for {legacy}"
        );
    }
}

#[test]
fn nested_agent_map_check_gets_actionable_hint() {
    let error = Cli::try_parse_from(["jig", "agent-map", "check"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    let message = error.to_string();
    assert!(
        message.contains("unrecognized subcommand 'check'"),
        "agent-map hint depends on Clap quoting the nested invalid command: {message}"
    );
    assert!(
        message.contains("Usage: jig agent-map [OPTIONS] <COMMAND>"),
        "agent-map hint depends on Clap nested usage text: {message}"
    );
    assert_eq!(
        moved_check_command_hint(&error),
        Some("This check command moved. Use:\n  jig check agent-map".to_string())
    );

    let unrelated_nested = Cli::try_parse_from(["jig", "agent-map", "test"]).unwrap_err();
    assert_eq!(
        unrelated_nested.kind(),
        clap::error::ErrorKind::InvalidSubcommand
    );
    assert!(moved_check_command_hint(&unrelated_nested).is_none());
}

#[test]
fn missing_init_path_gets_actionable_hint() {
    let error = Cli::try_parse_from(["jig", "init"]).unwrap_err();
    let hint = missing_init_path_hint(&error).unwrap();

    assert!(hint.contains("jig init /path/to/new-repo"));
    assert!(hint.contains(
        "--preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault"
    ));
    assert!(hint.contains("--preset rust-react"));
    assert!(hint.contains("jig adopt ."));
    assert!(hint.contains("jig adopt . --write"));
}

#[test]
fn missing_init_path_hint_examples_parse() {
    Cli::try_parse_from([
        "jig",
        "init",
        "/path/to/new-repo",
        "--preset",
        "harness-only",
        "--repo-name",
        "new-repo",
        "--sqlx-enabled",
        "false",
        "--no-input",
        "--no-vault",
    ])
    .unwrap();
    Cli::try_parse_from([
        "jig",
        "init",
        "/path/to/new-repo",
        "--preset",
        "rust-react",
        "--db",
        "postgres",
        "--frontends",
        "web,landing,admin",
    ])
    .unwrap();
    Cli::try_parse_from(["jig", "adopt", "."]).unwrap();
    Cli::try_parse_from(["jig", "adopt", ".", "--write"]).unwrap();
}

#[test]
fn unrelated_parse_errors_do_not_get_missing_init_path_hint() {
    let missing_proxy_args = Cli::try_parse_from(["jig", "proxy", "run"]).unwrap_err();
    assert_eq!(
        missing_proxy_args.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(missing_init_path_hint(&missing_proxy_args).is_none());

    let invalid_subcommand = Cli::try_parse_from(["jig", "not-a-command"]).unwrap_err();
    assert!(missing_init_path_hint(&invalid_subcommand).is_none());
}

#[test]
fn json_ok_false_and_reported_command_failures_are_cli_failures() {
    let error = require_json_ok(true, &serde_json::json!({ "ok": false }))
        .unwrap_err()
        .to_string();

    assert!(error.contains("ok=false"));
    require_json_ok(false, &serde_json::json!({ "ok": false })).unwrap();
    assert!(test_command_reports_failure_with_ok(&CommandKind::Dev(
        DevOpts {
            command: None,
            launch: DevLaunchOpts::default(),
        }
    )));
    assert!(test_command_reports_failure_with_ok(&CommandKind::Doctor));
    assert!(test_command_reports_failure_with_ok(&CommandKind::Agent(
        AgentCommand::Doctor
    )));
    assert!(test_command_reports_failure_with_ok(&CommandKind::Vault(
        VaultCommand::Run(VaultRunOpts {
            env: vec!["TOKEN=api_token".into()],
            files: Vec::new(),
            vault: VaultRuntimeOpts::default(),
            command: vec!["true".into()],
        })
    )));
    assert!(!test_command_reports_failure_with_ok(&CommandKind::Vault(
        VaultCommand::Status(VaultStatusOpts::default())
    )));
    for args in [
        ["jig", "loop", "tick"],
        ["jig", "loop", "dispatch"],
        ["jig", "loop", "run"],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(test_command_reports_failure_with_ok(&cli.command));
    }
    let status = Cli::try_parse_from(["jig", "loop", "status"]).unwrap();
    assert!(!test_command_reports_failure_with_ok(&status.command));
}

#[test]
fn transparent_vault_child_exit_is_silent_and_preserves_status() {
    let error = crate::cli::structured_error::vault_exec_child_exit(37);
    assert!(is_structured_json_failure(&error));
    assert_eq!(structured_error_exit_code(&error), Some(37));
}

#[test]
fn json_error_payload_and_reported_error_preserve_machine_failure_contract() {
    let payload = json_error_payload("command_failed", "configuration is invalid", 7);
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["error"]["kind"], "command_failed");
    assert_eq!(payload["error"]["message"], "configuration is invalid");
    assert_eq!(payload["exit_status"], 7);

    let error = json_reported_error(7);
    assert!(is_structured_json_failure(&error));
    assert_eq!(structured_error_exit_code(&error), Some(7));
}

#[test]
fn json_error_reporting_preserves_protocol_and_post_output_boundaries() {
    assert!(should_report_json_command_errors(
        true,
        &CommandKind::Info(InfoOpts::default())
    ));
    assert!(!should_report_json_command_errors(true, &CommandKind::Mcp));
    assert!(!should_report_json_command_errors(
        false,
        &CommandKind::Info(InfoOpts::default())
    ));

    let post_output =
        finish_after_json_output(Err(anyhow::anyhow!("server failed after startup")), true)
            .unwrap_err();
    assert!(is_json_output_already_emitted(&post_output));
    let propagated = report_json_command_error(Err(post_output)).unwrap_err();
    assert!(is_json_output_already_emitted(&propagated));
    assert_eq!(format!("{propagated:#}"), "server failed after startup");

    let structured = require_json_ok(true, &serde_json::json!({ "ok": false })).unwrap_err();
    let structured = finish_after_json_output(Err(structured), true).unwrap_err();
    assert!(is_structured_json_failure(&structured));
    assert!(!is_json_output_already_emitted(&structured));
}

#[test]
fn json_request_detection_ignores_child_arguments_after_separator() {
    assert!(args_request_json(
        ["work", "status", "--json"].map(OsString::from)
    ));
    assert!(args_request_json(
        ["--json", "work", "status"].map(OsString::from)
    ));
    assert!(!args_request_json(
        ["vault", "run", "--", "tool", "--json"].map(OsString::from)
    ));

    assert!(args_target_mcp(
        ["--json", "mcp", "--bogus"].map(OsString::from)
    ));
    assert!(args_target_mcp(["mcp", "--json"].map(OsString::from)));
    assert!(!args_target_mcp(
        ["prompt", "get", "mcp", "--json"].map(OsString::from)
    ));
    assert!(!args_target_mcp(
        ["vault", "run", "--", "mcp", "--json"].map(OsString::from)
    ));
}

#[test]
fn dev_management_actions_do_not_request_launch_process_identity() {
    let launch = DevOpts {
        command: None,
        launch: DevLaunchOpts {
            jig_project: Some("demo@/tmp/demo".into()),
            ..Default::default()
        },
    };
    assert_eq!(dev_launch_identity_present(&launch), Some(true));
    assert!(matches!(dev_human_output(&launch), HumanOutput::Dev));

    let status = DevOpts {
        command: Some(DevSubcommand::Status(DevStatusOpts::default())),
        launch: DevLaunchOpts::default(),
    };
    assert_eq!(dev_launch_identity_present(&status), None);
    assert!(matches!(dev_human_output(&status), HumanOutput::DevStatus));

    let stop = DevOpts {
        command: Some(DevSubcommand::Stop(DevStopOpts::default())),
        launch: DevLaunchOpts::default(),
    };
    assert_eq!(dev_launch_identity_present(&stop), None);
    assert!(matches!(dev_human_output(&stop), HumanOutput::DevStop));
}

#[test]
fn dev_interruption_exit_status_comes_from_the_runtime_result() {
    for exit_status in [129, 130, 143] {
        let error = require_foreground_status(&serde_json::json!({
            "ok": false,
            "interrupted": true,
            "exit_status": exit_status
        }))
        .unwrap_err();

        assert!(is_structured_json_failure(&error));
        assert_eq!(structured_error_exit_code(&error), Some(exit_status));
    }

    require_foreground_status(&serde_json::json!({ "ok": true })).unwrap();
    let ordinary_failure =
        require_foreground_status(&serde_json::json!({ "ok": false })).unwrap_err();
    assert!(is_structured_json_failure(&ordinary_failure));
    assert_eq!(structured_error_exit_code(&ordinary_failure), None);

    for malformed in [
        serde_json::json!({ "ok": false, "interrupted": true }),
        serde_json::json!({ "ok": false, "interrupted": true, "exit_status": 0 }),
        serde_json::json!({ "ok": false, "interrupted": true, "exit_status": 256 }),
    ] {
        let error = require_foreground_status(&malformed).unwrap_err();
        assert!(!is_structured_json_failure(&error));
        assert_eq!(structured_error_exit_code(&error), None);
    }
}

#[test]
fn codex_child_exit_status_is_preserved_by_the_cli() {
    let error: anyhow::Error = crate::codex::CodexChildExitStatus(37).into();

    assert!(is_structured_json_failure(&error));
    assert_eq!(structured_error_exit_code(&error), Some(37));
}
