// agentic-loc-exception: JSON CLI integration coverage shares process-level fixture setup.

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

#[test]
fn named_v6_check_uses_aggregate_output_and_exits_unsuccessfully() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());

    let output = jig()
        .current_dir(repo.path())
        .args(["check", "test"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Jig check: failed"), "{stdout}");
    assert!(stdout.contains("api:test: failed (exit 7)"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn repository_check_prints_lease_contention_before_the_lease_is_released() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());
    fs::create_dir_all(repo.path().join(".agent/.cache")).unwrap();
    let lease = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(repo.path().join(".agent/.cache/repository-execution.lock"))
        .unwrap();
    lease.lock_exclusive().unwrap();
    let stderr_path = repo.path().join("lease-wait.stderr");
    let stderr = File::create(&stderr_path).unwrap();
    let mut child = jig()
        .current_dir(repo.path())
        .args(["check", "api:test", "--no-receipt"])
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let observed = loop {
        let stderr = fs::read_to_string(&stderr_path).unwrap();
        if stderr.contains("Waiting for another repository execution") {
            break true;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("repository check exited with {status} before reporting lease contention");
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !observed {
        let _ = child.kill();
        let _ = child.wait();
        panic!("lease contention remained buffered while the command was waiting");
    }

    drop(lease);
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(1));
    let stderr = fs::read_to_string(stderr_path).unwrap();
    assert_eq!(
        stderr
            .matches("Waiting for another repository execution")
            .count(),
        1,
        "a final progress flush must not redeliver the wait notice: {stderr}"
    );
}

#[test]
fn external_check_selectors_accept_global_json_and_help_after_the_selector() {
    let repo = tempdir().unwrap();
    write_v6_failing_test_repo(repo.path());

    let json_output = jig()
        .current_dir(repo.path())
        .args(["check", "api:test", "--json"])
        .output()
        .unwrap();
    assert_eq!(json_output.status.code(), Some(1));
    let payload: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(payload["run"]["conclusion"], "failure");

    let help = jig()
        .current_dir(repo.path())
        .args(["check", "api:test", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("Run configured project checks"), "{help}");
    assert!(!help.contains("unknown check option"), "{help}");
}

#[test]
fn json_mode_wraps_usage_and_pre_output_command_errors() {
    let usage = jig().args(["work", "check", "--json"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stderr.is_empty());
    let usage: Value = serde_json::from_slice(&usage.stdout).unwrap();
    assert_eq!(usage["ok"], false);
    assert_eq!(usage["error"]["kind"], "usage");
    assert_eq!(usage["exit_status"], 2);

    let repo = tempdir().unwrap();
    let command = jig()
        .current_dir(repo.path())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert_eq!(command.status.code(), Some(1));
    assert!(command.stderr.is_empty());
    let command: Value = serde_json::from_slice(&command.stdout).unwrap();
    assert_eq!(command["ok"], false);
    assert_eq!(command["error"]["kind"], "command_failed");
    assert_eq!(command["exit_status"], 1);
}

#[test]
fn launcher_handoff_root_is_authoritative_over_cwd_and_environment() {
    let ambient = tempdir().unwrap();
    let authoritative = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(authoritative.join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    let contract_version = manifest["contract_version"].as_u64().unwrap().to_string();
    let answers: toml::Value =
        toml::from_str(&std::fs::read_to_string(authoritative.join(".jig.toml")).unwrap()).unwrap();
    let repo_name = answers["repo_name"].as_str().unwrap();

    let output = jig()
        .current_dir(ambient.path())
        .env("JIG_REPO_ROOT", ambient.path())
        .arg("--__launcher-contract-version")
        .arg(contract_version)
        .args(["--__launcher-profile", "runtime"])
        .arg("--__launcher-repo-root")
        .arg(&authoritative)
        .args(["info", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignored JIG_REPO_ROOT")
            && stderr.contains("generated launcher root")
            && stderr.contains("is authoritative"),
        "expected authoritative-root warning, got:\n{stderr}"
    );
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["repo"]["name"], repo_name);
}

#[test]
fn json_mode_classifies_output_mode_conflicts_as_usage_errors() {
    for args in [
        vec!["--json", "status", "--tui"],
        vec!["status", "--tui", "--json"],
        vec![
            "--json",
            "work",
            "start",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "--json",
            "start",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "start",
            "--json",
            "--title",
            "test",
            "--print-plan-id",
        ],
        vec![
            "work",
            "start",
            "--title",
            "test",
            "--print-plan-id",
            "--json",
        ],
    ] {
        let output = jig().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["ok"], false);
        assert_eq!(output["error"]["kind"], "usage");
        assert_eq!(output["exit_status"], 2);
    }
}

#[test]
fn mcp_parse_errors_keep_stdout_reserved_for_protocol_frames() {
    let output = jig().args(["mcp", "--json", "--bogus"]).output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--bogus'"));
}

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
    assert!(human.contains("migration  not configured"));
    assert!(human.contains("sqlx       not configured"));
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
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--local", "--no-hardlinks"])
        .arg(&workspace)
        .arg(&template)
        .status()
        .unwrap();
    assert!(clone.success());
    let template_patch = Command::new("git")
        .current_dir(&workspace)
        .args(["diff", "--binary", "HEAD", "--", "templates"])
        .output()
        .unwrap();
    assert!(template_patch.status.success());
    if !template_patch.stdout.is_empty() {
        let mut apply = Command::new("git")
            .current_dir(&template)
            .args(["apply", "--binary", "-"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        apply
            .stdin
            .take()
            .unwrap()
            .write_all(&template_patch.stdout)
            .unwrap();
        assert!(apply.wait().unwrap().success());
        for args in [
            &["config", "user.email", "reviewer@example.invalid"][..],
            &["config", "user.name", "ExampleReviewer"],
            &["add", "templates"],
            &[
                "commit",
                "--quiet",
                "-m",
                "Synthetic current template snapshot",
            ],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(&template)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    let canonical_template = template.canonicalize().unwrap();
    let commit = Command::new("git")
        .current_dir(&template)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(commit.status.success());
    let commit = String::from_utf8(commit.stdout).unwrap();
    let commit = commit.trim();
    let portable_source = "https://example.invalid/team/custom-jig.git";

    let repo = tempdir().unwrap();
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
    assert!(inventory.status.success());
    let inventory: Value = serde_json::from_slice(&inventory.stdout).unwrap();
    let next_step = command_by_name(&inventory, "sqlx")["next_step"]
        .as_str()
        .unwrap();
    let mut commands = next_step.split('`');
    let preview = commands.nth(1).unwrap();
    let apply = commands.nth(1).unwrap();
    assert!(preview.contains("--minimal"));
    assert!(preview.contains("--template"));
    assert!(preview.contains("--template-mode committed"));
    assert!(preview.contains(&format!("--vcs-ref {commit}")));
    assert!(preview.contains(&format!("--template-source-url {portable_source}")));
    assert!(apply.contains("--force --write"));

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

    assert!(
        preview.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        preview.status,
        String::from_utf8_lossy(&preview.stdout),
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["render_mode"], "preview");
    assert_eq!(preview["harness_footprint"], "minimal");
    assert_eq!(
        preview["template"],
        canonical_template.display().to_string()
    );

    let apply = Command::new("/bin/sh")
        .current_dir(repo.path())
        .env("PATH", &path)
        .env_remove("JIG_REPO_ROOT")
        .arg("-c")
        .arg(format!("{apply} --no-input --no-vault --json"))
        .output()
        .unwrap();
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let apply: Value = serde_json::from_slice(&apply.stdout).unwrap();
    assert_eq!(apply["render_mode"], "copy");
    assert_eq!(apply["harness_footprint"], "minimal");
    assert_eq!(apply["template"], canonical_template.display().to_string());
    let config = fs::read_to_string(repo.path().join(".jig.toml")).unwrap();
    assert!(config.contains("harness_footprint = \"minimal\""));
    assert!(config.contains(&format!("_src_path = {portable_source:?}")));
    assert!(config.contains(&format!(
        "_template_local_path = {:?}",
        canonical_template.display().to_string()
    )));
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
    assert!(
        json_output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        json_output.status,
        String::from_utf8_lossy(&json_output.stdout),
        String::from_utf8_lossy(&json_output.stderr)
    );
    let json_report: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(json_report["ok"], true);
    assert_eq!(json_report["scaffold"]["preset"], "rust-library");
    assert_eq!(json_report["scaffold"]["db"], "none");
    assert_eq!(json_report["scaffold"]["frontends"], json!([]));
    assert_eq!(
        json_report["scaffold"]["files_created"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(
        json_report["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step.as_str() != Some("scripts/jig dev"))
    );
    let notes = json_report["notes"].as_array().unwrap();
    assert!(notes.iter().any(|note| {
        note.as_str()
            .is_some_and(|note| note.contains("Scaffolded project code is project-owned"))
    }));
    assert!(notes.iter().all(|note| {
        !note
            .as_str()
            .is_some_and(|note| note.contains("Scaffolded application code"))
    }));
    let config = fs::read_to_string(json_destination.join(".jig.toml")).unwrap();
    let config = toml::from_str::<toml::Value>(&config).unwrap();
    assert_eq!(config["dev"]["proxy_port"].as_integer(), Some(1355));
    assert_eq!(config["dev"]["https_port"].as_integer(), Some(1443));
    assert_eq!(config["dev"]["https"].as_bool(), Some(false));
    assert_eq!(config["dev"]["http2"].as_bool(), Some(true));
    assert_eq!(config["dev"]["lan"].as_bool(), Some(false));
    assert_eq!(config["dev"]["tld"].as_str(), Some("localhost"));
    assert_eq!(config["dev"]["workspace_discovery"].as_bool(), Some(false));

    for check in ["contract", "agent-map", "agent-guides"] {
        let output = jig()
            .current_dir(&json_destination)
            .args(["check", check])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jig check {check} failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

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
    assert!(
        human_output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        human_output.status,
        String::from_utf8_lossy(&human_output.stdout),
        String::from_utf8_lossy(&human_output.stderr)
    );
    let human = String::from_utf8(human_output.stdout).unwrap();
    assert!(human.contains("scaffold: rust-library for examplelibraryhuman (db: none)"));
    assert!(human.contains("scaffold files: 5 created, 0 modified, 0 unchanged"));
    assert!(human.contains("Scaffolded project code is project-owned"));
    assert!(!human.contains("Scaffolded application code"));
    assert!(!human.contains("frontends:"));
    assert!(!human.contains("scripts/jig dev"));
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
