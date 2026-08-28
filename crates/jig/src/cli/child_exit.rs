use super::*;

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
    let runtime_probe = Cli::try_parse_from([
        "jig",
        "__runtime-compatible",
        "--profile",
        "runtime",
        "/tmp/repo",
    ])
    .unwrap();
    assert!(!should_report_json_command_errors(
        true,
        &runtime_probe.command
    ));
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
    assert!(args_target_mcp(
        [
            "--__launcher-contract-version",
            "6",
            "--__launcher-profile",
            "repo",
            "--__launcher-repo-root",
            "/tmp/ExampleProject",
            "mcp",
            "--json",
        ]
        .map(OsString::from)
    ));
    assert!(!args_target_mcp(
        ["--__launcher-profile", "mcp", "prompt", "get"].map(OsString::from)
    ));
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
