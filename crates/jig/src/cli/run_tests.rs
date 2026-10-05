use std::ffi::OsString;

use super::argument_parsing::{args_request_json, usage_hint};
#[cfg(feature = "dev-proxy")]
use super::dev_launch::dev_launch_identity_present;
use super::*;
use crate::cli::bootstrap_hints::TEMPLATE_ERROR_HINT;
use crate::cli::output;
use crate::cli::runtime_dispatch::FailurePolicy;
use crate::cli::structured_error::require_foreground_status;
use crate::cli::{DevLaunchOpts, DevOpts, DevStatusOpts, DevStopOpts, DevSubcommand, InfoOpts};
use clap::Parser;

/// Parses `args` as the CLI would and returns the hint for the usage error.
fn usage_hint_for(args: &[&str]) -> Option<String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let error = Cli::try_parse_from(&args).unwrap_err();
    usage_hint(&error, &args)
}

#[test]
fn template_errors_get_hint() {
    let missing_template_value =
        Cli::try_parse_from(["jig", "adopt", ".", "--template"]).unwrap_err();
    assert_eq!(
        missing_template_value.kind(),
        clap::error::ErrorKind::InvalidValue
    );
    assert_eq!(
        usage_hint_for(&["jig", "adopt", ".", "--template"]).as_deref(),
        Some(TEMPLATE_ERROR_HINT)
    );

    assert!(usage_hint_for(&["jig", "proxy", "run", "web", "vite"]).is_none());
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
        let expected = format!("This check command moved. Use:\n  {replacement}");
        assert_eq!(
            usage_hint_for(&["jig", legacy]).as_deref(),
            Some(expected.as_str()),
            "wrong moved-command hint for {legacy}"
        );
        assert_eq!(
            usage_hint_for(&["jig", "--json", legacy]).as_deref(),
            Some(expected.as_str()),
            "a global flag before {legacy} must not hide the hint"
        );
    }
}

#[test]
fn nested_agent_map_check_gets_actionable_hint() {
    let error = Cli::try_parse_from(["jig", "agent-map", "check"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    assert_eq!(
        usage_hint_for(&["jig", "agent-map", "check"]).as_deref(),
        Some("This check command moved. Use:\n  jig check agent-map")
    );

    let unrelated_nested = Cli::try_parse_from(["jig", "agent-map", "test"]).unwrap_err();
    assert_eq!(
        unrelated_nested.kind(),
        clap::error::ErrorKind::InvalidSubcommand
    );
    assert!(usage_hint_for(&["jig", "agent-map", "test"]).is_none());
}

#[test]
fn retired_check_spellings_under_another_command_get_no_hint() {
    // `test` and `clippy` moved from the top level only; the same word
    // rejected below an existing command is an ordinary typo.
    for args in [
        &["jig", "agent", "test"][..],
        &["jig", "state", "clippy"][..],
        &["jig", "agent", "check"][..],
    ] {
        let error = Cli::try_parse_from(args.iter().copied()).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
        assert!(
            usage_hint_for(args).is_none(),
            "unexpected hint for {args:?}"
        );
    }
}

#[test]
fn missing_init_path_gets_actionable_hint() {
    let hint = usage_hint_for(&["jig", "init"]).unwrap();

    assert!(hint.contains("jig init /path/to/new-repo"));
    assert!(hint.contains(
        "--preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault"
    ));
    assert!(hint.contains("--preset rust-react"));
    assert!(hint.contains("jig adopt ."));
    assert!(hint.contains("jig adopt . --write"));
    assert_eq!(
        usage_hint_for(&["jig", "--json", "init", "--preset", "harness-only"]),
        Some(hint)
    );
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
    assert!(usage_hint_for(&["jig", "proxy", "run"]).is_none());
    assert!(usage_hint_for(&["jig", "not-a-command"]).is_none());
}

#[test]
fn ok_false_fails_only_the_commands_whose_result_is_an_outcome() {
    let error = require_json_ok(true, &serde_json::json!({ "ok": false }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("ok=false"));
    require_json_ok(false, &serde_json::json!({ "ok": false })).unwrap();

    let failure = |args: &[&str]| match Cli::try_parse_from(args).unwrap().command {
        CommandKind::Agent(command) => command.into_dispatch().failure,
        CommandKind::Loop(command) => command.into_dispatch().failure,
        CommandKind::State(command) => command.into_dispatch().failure,
        CommandKind::Check(opts) => opts.into_dispatch().unwrap().failure,
        CommandKind::Run(opts) => opts.into_dispatch().unwrap().failure,
        other => panic!("{other:?} does not dispatch through the runtime"),
    };
    for args in [
        &["jig", "agent", "doctor"][..],
        &["jig", "loop", "tick"][..],
        &["jig", "loop", "dispatch"][..],
        &["jig", "loop", "run"][..],
        &["jig", "check", "test"][..],
        &["jig", "run", "api:test"][..],
    ] {
        assert_eq!(failure(args), FailurePolicy::OkFalseFails, "{args:?}");
    }
    // Reports stay inspectable: their JSON may carry `ok: false` without
    // turning the report into a CLI error.
    for args in [
        &["jig", "loop", "status"][..],
        &["jig", "agent", "bootstrap"][..],
        &["jig", "state", "summary"][..],
    ] {
        assert_eq!(failure(args), FailurePolicy::ErrorsOnly, "{args:?}");
    }
}

mod child_exit;
mod runtime_pin;
