use std::path::PathBuf;

use clap::{CommandFactory, Parser};
use jig_commands::tool_defs::{kind, tool};
use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::cli::check::CHECK_SUBCOMMAND_NAMES;
use crate::cli::{LAUNCHER_CHECK_SUBCOMMANDS, LAUNCHER_GLOBAL_FLAGS};
use crate::test_env::{CurrentDirGuard, TestRepoBuilder, lock_env};

const CURRENT_GENERATED_LAUNCHER: &str =
    include_str!("../../../bootstrap/embedded_template_snapshots/scripts/jig.jinja");

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
    let capability_subcommands = root_commands::launcher_subcommands(LauncherScope::CapabilityOnly);
    let repository_subcommands = root_commands::launcher_subcommands(LauncherScope::Repository);
    let registered_subcommands = capability_subcommands
        .iter()
        .chain(&repository_subcommands)
        .copied()
        .chain([root_commands::RUNTIME_COMPATIBLE.name])
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        top_level_subcommands, registered_subcommands,
        "every Clap top-level command must be declared in the root command registry"
    );
    let capability_marker = CURRENT_GENERATED_LAUNCHER
        .lines()
        .find_map(|line| line.strip_prefix("# jig-capability-only-subcommands:"))
        .expect("generated launcher must declare its capability-only subcommands");
    assert_eq!(
        capability_marker,
        capability_subcommands.join(","),
        "refresh the launcher command lists when recovery commands change"
    );
    let repository_scope_marker = CURRENT_GENERATED_LAUNCHER
        .lines()
        .find_map(|line| line.strip_prefix("# jig-repository-scope-subcommands:"))
        .expect("generated launcher must declare its repository-scoped subcommands");
    assert_eq!(
        repository_scope_marker,
        repository_subcommands.join(","),
        "refresh the launcher command lists when commands change"
    );
    let capability_invocations = [
        vec!["jig", "adopt", "."],
        vec!["jig", "claude", "homes"],
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
    assert_eq!(capability_commands, capability_subcommands.join(","));
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
        &["check", "contract", "--no-receipt"][..],
        &["check", "unknown-selector"][..],
    ] {
        assert!(
            !generated_launcher_classifies_as_capability_only(args),
            "expected {args:?} to remain repository-scoped"
        );
    }
}

#[test]
fn contract_comparison_options_keep_launcher_and_runtime_repository_scoped() {
    let bare = Cli::try_parse_from(["jig", "check", "contract"]).unwrap();
    assert!(launcher_capability_only_command(&bare.command));
    assert!(generated_launcher_classifies_as_capability_only(&[
        "check", "contract"
    ]));

    for comparison in [
        &["--comparison-base", "master"][..],
        &["--comparison-staged"][..],
        &["--comparison-strict-inventory"][..],
        &[
            "--comparison-exact-tree",
            "abcd",
            "--comparison-provenance",
            "explicit",
        ][..],
    ] {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        write_compatible_runtime_repo(temp.path(), 4);
        let mut args = vec!["check", "contract"];
        args.extend_from_slice(comparison);
        assert!(
            !generated_launcher_classifies_as_capability_only(&args),
            "generated launcher must keep {args:?} repository-scoped"
        );
        let cli = Cli::try_parse_from(
            [
                "jig",
                "--__launcher-contract-version",
                "4",
                "--__launcher-profile",
                "runtime",
                "--__launcher-repo-root",
                temp.path().to_str().unwrap(),
            ]
            .into_iter()
            .chain(args),
        )
        .unwrap();
        assert!(
            !launcher_capability_only_command(&cli.command),
            "runtime must keep {comparison:?} repository-scoped"
        );
        validate_launcher_repository_scope(&cli).unwrap();
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

    run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: false,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap();

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
        std::env::var_os(jig_context::JIG_REPO_ROOT_ENV).as_deref(),
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

    for supported in [4, 8, 9] {
        run_runtime_compatible(RuntimeCompatibleOpts {
            profile: RuntimeCompatibilityProfile::Runtime,
            capability_only: true,
            contract_version: Some(supported),
            repo_root: temp.path().to_path_buf(),
        })
        .unwrap();
    }

    for unsupported in [10, 11, 12, 999] {
        let error = run_runtime_compatible(RuntimeCompatibleOpts {
            profile: RuntimeCompatibilityProfile::Runtime,
            capability_only: true,
            contract_version: Some(unsupported),
            repo_root: temp.path().to_path_buf(),
        })
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(&format!("Inactive Jig contract version {unsupported}")),
            "{error}"
        );
        assert!(
            error.contains("supports active versions 2 through 9"),
            "{error}"
        );
    }
}

#[test]
fn capability_probe_without_explicit_version_rejects_unsupported_epoch() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(11)
        .write();

    let error = run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: true,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Unsupported jig contract version: 11"),
        "{error}"
    );
}

#[test]
fn repository_probe_without_explicit_version_rejects_unsupported_epoch() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(11)
        .config(
            r#"[repository]
default_check_profile = "verify"
components = []
actions = []
profiles = []"#,
        )
        .write();
    let contract_path = temp.path().join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&contract_path).unwrap()).unwrap();
    contract["default_check_profile"] = json!("verify");
    std::fs::write(contract_path, serde_json::to_vec_pretty(&contract).unwrap()).unwrap();

    let error = run_runtime_compatible(RuntimeCompatibleOpts {
        profile: RuntimeCompatibilityProfile::Runtime,
        capability_only: false,
        contract_version: None,
        repo_root: temp.path().to_path_buf(),
    })
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Unsupported jig contract version: 11"),
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
