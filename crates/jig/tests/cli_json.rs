use std::fs;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use fs4::fs_std::FileExt;
use serde_json::{Value, json};
use support::tempdir;

#[path = "cli_json_parts/cognitive_helpers.rs"]
mod cognitive_helpers;
use cognitive_helpers::*;

fn jig() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("NO_COLOR", "1");
    command
}

fn write_v6_failing_test_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join("api")).unwrap();
    fs::write(root.join("api/example.rs"), "pub fn example() {}\n").unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"

[commands]
api_test_command = "printf 'tests failed\n' >&2; exit 7"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "api"

[[repository.actions]]
target = { component = "api", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_test_command" }
inputs = ["api/**"]

[[repository.profiles]]
id = "verify"
targets = [{ component = "api", action = "test" }]
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": ["api_test_command"],
            "tools": [],
            "components": [{"id": "api", "root": "api"}],
            "actions": [{
                "target": {"component": "api", "action": "test"},
                "intent": "check",
                "effects": ["read_only", "process"],
                "runner": {"kind": "command", "command": "api_test_command"},
                "inputs": ["api/**"]
            }],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "api", "action": "test"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
    for args in [
        &["init"][..],
        &["config", "user.email", "fixture@example.com"],
        &["config", "user.name", "Fixture"],
        &["add", "."],
        &["commit", "-m", "fixture"],
    ] {
        assert!(
            Command::new("git")
                .current_dir(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn write_file_budget_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join(".jig")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "one\n").unwrap();
    fs::write(
        root.join(".jig/file-budget.toml"),
        r#"version = 1
[[rules]]
id = "rust"
include = ["src/**"]
max_lines = 1
max_bytes = 1024
"#,
    )
    .unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."

[[repository.actions]]
target = { component = "repo", action = "file-budget" }
intent = "check"
effects = ["read_only"]
runner = { kind = "native", operation = "jig.file_budget" }
inputs = ["**"]

[[repository.profiles]]
id = "verify"
targets = [{ component = "repo", action = "file-budget" }]
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "contract_version": 7,
            "tool_namespace": "jig",
            "required_commands": [],
            "tools": [],
            "components": [{"id": "repo", "root": "."}],
            "actions": [{
                "target": {"component": "repo", "action": "file-budget"},
                "intent": "check",
                "effects": ["read_only"],
                "runner": {"kind": "native", "operation": "jig.file_budget"},
                "inputs": ["**"]
            }],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "repo", "action": "file-budget"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "fixture@example.invalid"],
        &["config", "user.name", "Fixture"],
        &["add", "."],
        &["commit", "-q", "-m", "fixture"],
    ] {
        assert!(
            Command::new("git")
                .current_dir(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[path = "cli_json/checks.rs"]
mod checks;

#[test]
fn prompt_get_honors_json_mode() {
    let home = tempdir().unwrap();
    let repo = tempdir().unwrap();
    let added = jig()
        .current_dir(repo.path())
        .env("JIG_PROMPT_HOME", home.path())
        .args(["prompt", "add", "json-test", "Hello {{ name }}"])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_PROMPT_HOME", home.path())
        .args([
            "prompt",
            "get",
            "json-test",
            "--var",
            "name=world",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "prompt get");
    assert_eq!(output["body"], "Hello world");
}

#[test]
fn info_commands_exposes_versioned_json_and_grouped_human_output() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let state_dir = repo.path().join(".agent/state");
    assert!(!state_dir.exists());

    let structured = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();
    assert!(
        structured.status.success(),
        "{}",
        String::from_utf8_lossy(&structured.stderr)
    );
    assert!(structured.stderr.is_empty());
    let structured: Value = serde_json::from_slice(&structured.stdout).unwrap();
    assert_eq!(structured["command"], "info commands");
    assert_eq!(structured["schema_version"], 3);
    assert_eq!(structured["repo"]["context_status"], "valid");
    let command_names = structured["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        command_names,
        [
            "init",
            "presets",
            "adopt",
            "update",
            "bootstrap",
            "setup",
            "doctor",
            "info",
            "dev",
            "check",
            "file-budget",
            "status",
            "ui",
            "work",
            "loop",
            "migration",
            "sqlx",
            "vault",
            "proxy",
            "prompt",
            "agent",
            "codex",
            "agent-map",
            "state",
            "mcp",
        ]
    );
    let sqlx = structured["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == "sqlx")
        .unwrap();
    assert_eq!(sqlx["status"], "not_configured");
    assert_eq!(sqlx["reason_code"], "sqlx_disabled");

    let human = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands"])
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(human.stderr.is_empty());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("Jig command availability: phase-two"));
    assert!(human.contains("Get started:"));
    assert!(human.contains("Agent and automation:"));
    for name in ["migration", "sqlx"] {
        assert!(human.lines().any(|line| {
            line.trim_start().starts_with(name) && line.contains("not configured")
        }));
    }
    assert!(human.contains("Next:"));
    assert!(!state_dir.exists());
}

#[test]
fn info_commands_exposes_onboarding_inventory_before_adoption() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path().join("uninitialized"))
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["repo"]["name"], Value::Null);
    assert_eq!(output["repo"]["root"], Value::Null);
    assert_eq!(output["repo"]["context_status"], "absent");
    assert_eq!(command_status(&output, "adopt"), "ready");
    assert_eq!(command_status(&output, "doctor"), "ready");
    assert_eq!(command_status(&output, "info"), "needs_setup");
    assert_eq!(command_status(&output, "bootstrap"), "needs_setup");
    assert_dev_proxy_status(&output, "proxy", "needs_setup", "repo_context_unavailable");
    assert_eq!(command_status(&output, "vault"), "needs_setup");
    assert_eq!(
        command_by_name(&output, "vault")["reason_code"],
        "vault_not_initialized"
    );
    assert_eq!(
        command_by_name(&output, "bootstrap")["reason_code"],
        "repo_context_unavailable"
    );
    assert_eq!(fs::read_dir(repo.path()).unwrap().count(), 0);

    let plain_info = jig()
        .current_dir(repo.path())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert!(!plain_info.status.success());

    let proxy_run = jig()
        .current_dir(repo.path())
        .args(["proxy", "run", "review-probe", "--", "true"])
        .output()
        .unwrap();
    assert!(!proxy_run.status.success());
}

#[test]
fn info_commands_sqlx_remediation_commands_preview_successfully() {
    for extra_args in [Vec::<&str>::new(), vec!["--schema-dump-enabled", "true"]] {
        let repo = tempdir().unwrap();
        let mut args = vec![
            "adopt",
            ".",
            "--sqlx-enabled",
            "true",
            "--rust-migration-dir",
            "migrations",
        ];
        args.extend(extra_args);
        args.push("--json");

        let output = jig().current_dir(repo.path()).args(args).output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["render_mode"], "preview");
    }
}

#[test]
fn info_commands_remediation_is_anchored_to_the_discovered_repository() {
    let full = tempdir().unwrap();
    write_info_commands_repo(full.path());
    write_test_launcher(full.path());
    let full_root = full.path().canonicalize().unwrap();
    let nested = full.path().join("nested/directory");
    fs::create_dir_all(&nested).unwrap();

    let output = jig()
        .current_dir(&nested)
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(command_status(&output, "proxy"), "ready");
    assert_eq!(command_status(&output, "dev"), "not_configured");
    let next_step = command_by_name(&output, "sqlx")["next_step"]
        .as_str()
        .unwrap();
    assert!(next_step.contains(&full_root.join("scripts/jig").display().to_string()));
    assert!(next_step.contains(&format!("adopt {}", full_root.display())));
    assert!(!next_step.contains("adopt ."));

    let minimal = tempdir().unwrap();
    write_info_commands_repo(minimal.path());
    let config_path = minimal.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(
            "\n[agent_tooling.codex]",
            "\nharness_footprint = \"minimal\"\n\n[agent_tooling.codex]",
        ),
    )
    .unwrap();
    let minimal_root = minimal.path().canonicalize().unwrap();
    let outside = tempdir().unwrap();
    let output = jig()
        .current_dir(outside.path())
        .env("JIG_REPO_ROOT", minimal.path())
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    let next_step = command_by_name(&output, "sqlx")["next_step"]
        .as_str()
        .unwrap();
    assert!(next_step.contains(&format!("`jig adopt {}", minimal_root.display())));
    assert!(!next_step.contains("adopt ."));
}

#[cfg(unix)]
#[test]
fn info_commands_sqlx_remediation_preserves_minimal_custom_template_identity() {
    let template_parent = tempdir().unwrap();
    let template = template_parent.path().join("custom-template");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    prepare_current_template(&template, &workspace);
    let canonical_template = template.canonicalize().unwrap();
    let commit = template_commit(&template);
    let portable_source = "https://example.invalid/team/custom-jig.git";

    let repo = tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(repo.path())
            .args(["init", "-q", "-b", "main"])
            .status()
            .unwrap()
            .success()
    );
    write_info_commands_repo(repo.path());
    fs::write(
        repo.path().join(".jig.toml"),
        format!(
            r#"_src_path = {portable_source:?}
_commit = {commit:?}
_template_mode = "committed"
_template_local_path = {:?}
default_branch = "main"
repo_name = "phase-two"
jig_version = "0.2.0-beta.1"
harness_footprint = "minimal"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
            template.display().to_string()
        ),
    )
    .unwrap();

    let inventory = jig()
        .current_dir(repo.path())
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();
    let (preview, apply) = remediation_commands(&inventory, &commit, portable_source);

    let jig_bin = Path::new(env!("CARGO_BIN_EXE_jig"));
    let binary_dir = jig_bin.parent().unwrap();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(binary_dir.to_path_buf()).chain(std::env::split_paths(&path)),
    )
    .unwrap();
    let preview = Command::new("/bin/sh")
        .current_dir(repo.path())
        .env("PATH", &path)
        .env_remove("JIG_REPO_ROOT")
        .arg("-c")
        .arg(format!("{preview} --json"))
        .output()
        .unwrap();

    assert_bootstrap_report(&preview, "preview", &canonical_template);

    let apply = Command::new("/bin/sh")
        .current_dir(repo.path())
        .env("PATH", &path)
        .env_remove("JIG_REPO_ROOT")
        .arg("-c")
        .arg(format!("{apply} --no-input --no-vault --json"))
        .output()
        .unwrap();
    assert_bootstrap_report(&apply, "copy", &canonical_template);
    let config = fs::read_to_string(repo.path().join(".jig.toml")).unwrap();
    assert_minimal_template_identity(&config, portable_source, &canonical_template);
}

#[test]
fn rust_library_init_has_exact_json_and_human_process_summaries() {
    let template_parent = tempdir().unwrap();
    let template = template_parent.path().join("ExampleProject-template");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--local", "--no-hardlinks"])
        .arg(&workspace)
        .arg(&template)
        .status()
        .unwrap();
    assert!(clone.success());

    let destinations = tempdir().unwrap();
    let json_destination = destinations.path().join("ExampleLibraryJson");
    let json_output = jig()
        .args([
            "--json",
            "init",
            json_destination.to_str().unwrap(),
            "--preset",
            "rust-library",
            "--template",
            template.to_str().unwrap(),
            "--template-mode",
            "committed",
            "--no-input",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert_rust_library_json(&json_output);
    assert_rust_library_dev_defaults(&json_destination);
    assert_rust_library_checks(&json_destination);

    let human_destination = destinations.path().join("ExampleLibraryHuman");
    let human_output = jig()
        .args([
            "init",
            human_destination.to_str().unwrap(),
            "--preset",
            "rust-library",
            "--template",
            template.to_str().unwrap(),
            "--template-mode",
            "committed",
            "--no-input",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert_rust_library_human(&human_output);
}

#[test]
fn forbidden_rust_library_answers_fail_before_template_vault_and_publication() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(&answers, "unexpected_shape_authority = true\n").unwrap();
    let destination = temp.path().join("ExampleLibrary");

    let output = jig()
        .env_remove("JIG_VAULT_PASSPHRASE")
        .args([
            "--json",
            "init",
            destination.to_str().unwrap(),
            "--preset",
            "rust-library",
            "--answers-file",
            answers.to_str().unwrap(),
            "--template",
            "/missing/ExampleProject-template",
            "--no-input",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("rust-library"), "{message}");
    assert!(message.contains("unexpected_shape_authority"), "{message}");
    assert!(!message.contains("JIG_VAULT_PASSPHRASE"), "{message}");
    assert!(
        !message.contains("Failed to inspect template source"),
        "{message}"
    );
    assert!(!destination.exists());
}

include!("cli_json_parts/rust_cli.rs");
include!("cli_json_parts/rust_only_acceptance.rs");

#[test]
fn info_commands_distinguishes_a_broken_repo_from_no_repo() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    fs::write(repo.path().join(".jig.toml"), "repo_name = [broken").unwrap();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["repo"]["context_status"], "invalid");
    assert!(output["repo"]["context_error"].is_string());
    for name in ["info", "vault", "prompt"] {
        assert_eq!(command_status(&output, name), "needs_setup", "{name}");
        assert_eq!(
            command_by_name(&output, name)["reason_code"],
            "repo_context_unavailable",
            "{name}"
        );
    }
    assert_dev_proxy_status(&output, "proxy", "needs_setup", "repo_context_unavailable");

    for args in [
        &["info", "--json"][..],
        &["vault", "status", "--json"][..],
        &["proxy", "list", "--json"][..],
        &["prompt", "list", "--json"][..],
    ] {
        let command = jig()
            .current_dir(repo.path())
            .env("JIG_VAULT_HOME", vault.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!command.status.success(), "{}", args.join(" "));
    }
}

#[path = "cli_json_parts/info_commands_edge_cases.rs"]
mod info_commands_edge_cases;
mod support;
use info_commands_edge_cases::*;

include!("cli_json_parts/loop_commands.rs");
