use super::*;
use crate::test_env::{CurrentDirGuard, TestRepoBuilder, lock_env};
use crate::tool_defs::{kind, tool};
use clap::CommandFactory;
use serde_json::json;
use tempfile::tempdir;

const CURRENT_GENERATED_LAUNCHER: &str =
    include_str!("../bootstrap/embedded_template_snapshots/scripts/jig.jinja");

fn generated_launcher_classifies_as_capability_only(args: &[&str]) -> bool {
    fn function_source(name: &str) -> String {
        let start = format!("{name}() {{");
        let body = CURRENT_GENERATED_LAUNCHER
            .split_once(&start)
            .and_then(|(_, remainder)| remainder.split_once("\n}\n"))
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("generated launcher is missing {name}"));
        format!("{start}{body}\n}}\n")
    }

    let script = format!(
        "set -eu\n{}{}jig_capability_only_requested \"$@\"\n",
        function_source("jig_is_global_flag"),
        function_source("jig_capability_only_requested")
    );
    std::process::Command::new("sh")
        .args(["-c", &script, "jig"])
        .args(args)
        .status()
        .unwrap()
        .success()
}

#[test]
fn status_run_reconciles_an_abandoned_worker_before_rendering() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(["rust_test_command"])
        .write();
    let ctx = crate::context::RepoContext::load_from(temp.path()).unwrap();
    let target: jig_contract::TargetId = "repo:test".parse().unwrap();
    let plan = jig_contract::RunPlan::new(
        "run-plan_1",
        "sha256:config",
        jig_contract::SourceIdentity::new(None, "sha256:worktree"),
        vec![jig_contract::PlannedTarget::new(
            target.clone(),
            jig_contract::ActionIntent::Check,
            jig_contract::ActionRunner::command("rust_test_command"),
            "sha256:input",
        )],
        vec![vec![target]],
    );
    let (started, lease) = crate::state::start_run(&ctx, plan, None).unwrap();
    drop(lease);

    let output = status_run_output(&ctx, &started.result.run_id).unwrap();

    assert_eq!(output["command"], "status run");
    assert_eq!(output["result"]["status"], "completed");
    assert_eq!(output["result"]["conclusion"], "blocked");
}

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
fn runtime_compatibility_command_parses_hidden_protocol() {
    let cli = Cli::try_parse_from([
        "jig",
        "__runtime-compatible",
        "--profile",
        "runtime",
        "/tmp/repo",
    ])
    .unwrap();

    match cli.command {
        CommandKind::RuntimeCompatible(opts) => {
            assert_eq!(opts.profile, RuntimeCompatibilityProfile::Runtime);
            assert!(!opts.capability_only);
            assert_eq!(opts.repo_root, PathBuf::from("/tmp/repo"));
        }
        other => panic!("expected runtime compatibility command, got {other:?}"),
    }

    let repository_probe = Cli::try_parse_from([
        "jig",
        "__runtime-compatible",
        "--contract-version",
        "4",
        "--profile",
        "runtime",
        "/tmp/repo",
    ])
    .unwrap();
    match repository_probe.command {
        CommandKind::RuntimeCompatible(opts) => {
            assert!(!opts.capability_only);
            assert_eq!(opts.contract_version, Some(4));
        }
        other => panic!("expected runtime compatibility command, got {other:?}"),
    }
}

#[test]
fn launcher_capability_flag_allowlist_matches_clap_globals() {
    let command = Cli::command();
    let globals = command
        .get_arguments()
        .filter(|argument| argument.is_global_set())
        .map(|argument| {
            format!(
                "--{}",
                argument
                    .get_long()
                    .expect("pre-subcommand global options must have a long spelling")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(globals, ["--json"]);
    assert_eq!(
        globals.join(","),
        LAUNCHER_GLOBAL_FLAGS,
        "update the launcher global-flag protocol when Clap globals change"
    );

    let launcher = CURRENT_GENERATED_LAUNCHER;
    let global_flag_function = launcher
        .split_once("jig_is_global_flag() {")
        .and_then(|(_, remainder)| remainder.split_once("\n}\n"))
        .map(|(body, _)| body)
        .expect("launcher is missing jig_is_global_flag");
    for flag in &globals {
        assert!(
            global_flag_function.contains(&format!("{flag})")),
            "launcher global-option recognizer does not allow Clap global flag {flag}"
        );
    }
    for function_name in [
        "jig_info_requested_before_separator",
        "jig_subcommand",
        "jig_capability_only_requested",
    ] {
        let function_start = format!("{function_name}() {{");
        let function_body = launcher
            .split_once(&function_start)
            .and_then(|(_, remainder)| remainder.split_once("\n}\n"))
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("launcher is missing {function_name}"));
        assert!(
            function_body.contains("jig_is_global_flag"),
            "launcher {function_name} does not use the centralized global-option recognizer"
        );
    }

    let top_level_subcommands = command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name())
        .collect::<std::collections::BTreeSet<_>>();
    for subcommand in command.get_subcommands() {
        let expected = LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS
            .split(',')
            .any(|capability| capability == subcommand.get_name());
        assert_eq!(
            launcher_capability_only_top_level_name(subcommand.get_name()),
            expected,
            "Rust launcher handoff policy disagrees for top-level command {}",
            subcommand.get_name()
        );
    }
    for capability_subcommand in LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS.split(',') {
        assert!(
            top_level_subcommands.contains(capability_subcommand),
            "launcher capability-only policy names missing Clap subcommand {capability_subcommand}"
        );
    }
    let capability_marker = CURRENT_GENERATED_LAUNCHER
        .lines()
        .find_map(|line| line.strip_prefix("# jig-capability-only-subcommands:"))
        .expect("generated launcher must declare its capability-only subcommands");
    assert_eq!(
        capability_marker, LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS,
        "update the launcher capability-only marker and parser when recovery commands change"
    );
    let repository_scope_subcommands = LAUNCHER_REPOSITORY_SCOPE_SUBCOMMANDS
        .split(',')
        .collect::<std::collections::BTreeSet<_>>();
    let expected_repository_scope_subcommands = top_level_subcommands
        .difference(
            &LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS
                .split(',')
                .collect::<std::collections::BTreeSet<_>>(),
        )
        .copied()
        .filter(|name| *name != "__runtime-compatible")
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        repository_scope_subcommands, expected_repository_scope_subcommands,
        "every public top-level command must explicitly choose capability-only or repository scope"
    );
    let repository_scope_marker = CURRENT_GENERATED_LAUNCHER
        .lines()
        .find_map(|line| line.strip_prefix("# jig-repository-scope-subcommands:"))
        .expect("generated launcher must declare its repository-scoped subcommands");
    assert_eq!(
        repository_scope_marker, LAUNCHER_REPOSITORY_SCOPE_SUBCOMMANDS,
        "update the launcher repository-scope marker and parser when commands change"
    );
    let capability_invocations = [
        vec!["jig", "adopt", "."],
        vec!["jig", "codex", "homes"],
        vec!["jig", "doctor"],
        vec!["jig", "init", "fixture"],
        vec!["jig", "presets"],
        vec!["jig", "update", "."],
    ];
    let capability_commands = capability_invocations
        .into_iter()
        .map(|args| {
            let name = args[1];
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(
                launcher_capability_only_command(&cli.command),
                "Rust launcher handoff guard does not classify capability-only command {name}"
            );
            name
        })
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(capability_commands, LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS);
    let contract_check = Cli::try_parse_from(["jig", "check", "contract"]).unwrap();
    assert!(launcher_capability_only_command(&contract_check.command));
    let setup = Cli::try_parse_from(["jig", "setup"]).unwrap();
    assert!(
        !launcher_capability_only_command(&setup.command),
        "repository-scoped commands must retain generated-launcher handoff validation"
    );

    let check_subcommands = command
        .find_subcommand("check")
        .expect("Clap must expose the check command")
        .get_subcommands()
        .map(|subcommand| subcommand.get_name())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(check_subcommands, CHECK_SUBCOMMAND_NAMES.join(","));
    assert_eq!(
        check_subcommands, LAUNCHER_CHECK_SUBCOMMANDS,
        "update the launcher check-subcommand marker and strict/capability parser when Clap check commands change"
    );
}

#[test]
fn generated_launcher_keeps_bare_check_and_target_selectors_repository_scoped() {
    for args in [
        &[][..],
        &["check", "contract"][..],
        &["check", "--help"][..],
        &["check", "--version"][..],
        &["check", "--plan-id", "plan_example", "contract"][..],
        &["check", "--plan-id=plan_example", "contract"][..],
        &["check", "contract", "--plan-id=plan_example"][..],
        &["check", "contract", "--no-receipt"][..],
    ] {
        assert!(generated_launcher_classifies_as_capability_only(args));
    }
    for args in [
        &["check"][..],
        &["check", "api:test"][..],
        &["check", "web:*"][..],
        &["check", "--profile", "ci", "--explain"][..],
        &["check", "contract", "--profile", "ci"][..],
        &["check", "--no-receipt"][..],
        &["check", "unknown-selector"][..],
    ] {
        assert!(
            !generated_launcher_classifies_as_capability_only(args),
            "expected {args:?} to remain repository-scoped"
        );
    }
}

fn write_compatible_runtime_repo(root: &std::path::Path, contract_version: u32) {
    TestRepoBuilder::new(root)
        .contract_version(contract_version)
        .config(
            r#"
harness_footprint = "minimal"
bootstrap_command = "true"
"#,
        )
        .required_commands(["bootstrap_command"])
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": kind::COMMAND,
            "description": "Bootstrap.",
            "command": "bootstrap_command"
        }))
        .tool(json!({
            "name": tool::CONTRACT_CHECK,
            "kind": kind::NATIVE,
            "description": "Contract check."
        }))
        .write();
}

#[test]
fn runtime_compatibility_checks_contract_and_profile() {
    let temp = tempdir().unwrap();
    write_compatible_runtime_repo(temp.path(), 4);

    for profile in [
        RuntimeCompatibilityProfile::Runtime,
        RuntimeCompatibilityProfile::Mcp,
    ] {
        run_runtime_compatible(RuntimeCompatibleOpts {
            profile,
            capability_only: false,
            contract_version: None,
            repo_root: temp.path().to_path_buf(),
        })
        .unwrap();
    }

    let default_result = run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Default,
        capability_only: false,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    });
    if cfg!(feature = "dev-proxy") {
        default_result.unwrap();
    } else {
        assert!(
            default_result
                .unwrap_err()
                .to_string()
                .contains("without the dev-proxy feature")
        );
    }
}

#[test]
fn generated_launcher_handoff_validates_and_reuses_the_loaded_context() {
    let _env = lock_env();
    for option in [
        "--__launcher-contract-version",
        "--__launcher-profile",
        "--__launcher-repo-root",
    ] {
        assert!(
            CURRENT_GENERATED_LAUNCHER.contains(option),
            "generated launcher must pass hidden root option {option}"
        );
    }
    let temp = tempdir().unwrap();
    write_compatible_runtime_repo(temp.path(), 4);
    let _configured_root =
        crate::test_env::EnvVarGuard::set("JIG_REPO_ROOT", temp.path().join("wrong-repository"));
    let cli = Cli::try_parse_from([
        "jig",
        "--__launcher-contract-version",
        "4",
        "--__launcher-profile",
        "runtime",
        "--__launcher-repo-root",
        temp.path().to_str().unwrap(),
        "info",
    ])
    .unwrap();

    validate_launcher_repository_scope(&cli).unwrap();
    assert_eq!(
        std::env::var_os(crate::context::JIG_REPO_ROOT_ENV).as_deref(),
        Some(std::fs::canonicalize(temp.path()).unwrap().as_os_str()),
        "descendants must inherit the launcher-authoritative repository root"
    );
    std::fs::write(temp.path().join(".jig.toml"), "[malformed").unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());

    let worker_ctx = std::thread::spawn(RepoContext::load)
        .join()
        .expect("launcher-context worker panicked")
        .expect("worker should reuse the launcher-validated context");
    assert_eq!(worker_ctx.contract_version(), 4);
    let ctx = RepoContext::load().expect("launcher-validated context should be reused");
    assert_eq!(ctx.contract_version(), 4);
}

#[test]
fn generated_launcher_handoff_rejects_misclassified_capability_commands() {
    let cli = Cli::try_parse_from([
        "jig",
        "--__launcher-contract-version",
        "4",
        "--__launcher-profile",
        "runtime",
        "--__launcher-repo-root",
        "/tmp/wrong-repository",
        "check",
        "contract",
    ])
    .unwrap();

    let error = validate_launcher_repository_scope(&cli)
        .unwrap_err()
        .to_string();
    assert!(error.contains("launcher and this Jig runtime disagree"));
    assert!(error.contains("jig update <repo> --launcher-only --force"));
}

#[test]
fn generated_launcher_context_can_only_be_initialized_once() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    write_compatible_runtime_repo(temp.path(), 4);
    let cli = Cli::try_parse_from([
        "jig",
        "--__launcher-contract-version",
        "4",
        "--__launcher-profile",
        "runtime",
        "--__launcher-repo-root",
        temp.path().to_str().unwrap(),
        "info",
    ])
    .unwrap();

    validate_launcher_repository_scope(&cli).unwrap();
    let error = validate_launcher_repository_scope(&cli)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already initialized"));
}

#[test]
fn generated_launcher_handoff_rejects_incomplete_protocol() {
    let cli = Cli::try_parse_from(["jig", "--__launcher-contract-version", "4", "info"]).unwrap();

    let error = validate_launcher_repository_scope(&cli)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Incomplete generated-launcher repository validation handoff"));
}

#[test]
fn runtime_compatibility_accepts_legacy_prerelease_product_version() {
    let temp = tempdir().unwrap();
    write_compatible_runtime_repo(temp.path(), 3);

    run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: false,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap();
}

#[test]
fn runtime_compatibility_rejects_invalid_contract_policy() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(4)
        .config("harness_footprint = \"minimal\"\nbootstrap_command = \"true\"")
        .required_commands(["unsupported_command"])
        .tool(json!({
            "name": tool::CONTRACT_CHECK,
            "kind": kind::NATIVE,
            "description": "Contract check."
        }))
        .write();

    let error = run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: false,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported required command"), "{error}");

    run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: true,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap();
}

#[test]
fn capability_probe_can_use_launcher_contract_when_manifest_is_malformed() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".agent")).unwrap();
    std::fs::write(temp.path().join(".agent/jig-contract.json"), "{").unwrap();

    run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: true,
        contract_version: Some(4),
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap();

    let error = run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: true,
        contract_version: Some(999),
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("Unsupported Jig contract version 999"),
        "{error}"
    );
}

#[test]
fn repository_probe_rejects_launcher_manifest_contract_drift() {
    let temp = tempdir().unwrap();
    write_compatible_runtime_repo(temp.path(), 4);

    let error = run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: false,
        contract_version: Some(3),
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Launcher contract version 3 does not match repository contract version 4"),
        "{error}"
    );
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

mod child_exit;
