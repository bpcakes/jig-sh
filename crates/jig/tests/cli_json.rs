// agentic-loc-exception: JSON CLI integration coverage shares process-level fixture setup.
mod support;

use std::fs;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;

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
        "{}",
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

#[test]
fn info_commands_matches_contextless_commands_with_an_invalid_override() {
    for adopted in [false, true] {
        let repo = tempdir().unwrap();
        let vault = tempdir().unwrap();
        if adopted {
            write_info_commands_repo(repo.path());
        }
        let invalid_root = repo.path().join("missing-override");
        let proxy_state = repo.path().join("proxy-state");

        let output = jig()
            .current_dir(repo.path())
            .env("JIG_REPO_ROOT", &invalid_root)
            .env("JIG_VAULT_HOME", vault.path())
            .env("JIG_PROXY_STATE_DIR", &proxy_state)
            .args(["info", "--commands", "--json"])
            .output()
            .unwrap();

        assert!(output.status.success(), "adopted={adopted}");
        assert!(output.stderr.is_empty(), "adopted={adopted}");
        let output: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["repo"]["name"], Value::Null);
        assert_eq!(
            output["repo"]["context_status"],
            if adopted { "recovered" } else { "invalid" }
        );
        assert_eq!(command_status(&output, "info"), "needs_setup");
        assert_eq!(command_status(&output, "prompt"), "ready");
        assert_dev_proxy_status(&output, "proxy", "needs_setup", "repo_context_unavailable");
        if adopted {
            assert_dev_proxy_status(&output, "dev", "not_configured", "dev_apps_not_configured");
        } else {
            assert_dev_proxy_status(&output, "dev", "needs_setup", "repo_context_unavailable");
            #[cfg(feature = "dev-proxy")]
            assert!(
                command_by_name(&output, "dev")["next_step"]
                    .as_str()
                    .unwrap()
                    .contains("JIG_REPO_ROOT")
            );
        }
        assert_eq!(command_status(&output, "vault"), "needs_setup");
        assert_eq!(
            command_by_name(&output, "vault")["reason_code"],
            "vault_not_initialized"
        );
        assert!(
            command_by_name(&output, "info")["next_step"]
                .as_str()
                .unwrap()
                .contains("JIG_REPO_ROOT"),
            "adopted={adopted}: info"
        );
        #[cfg(feature = "dev-proxy")]
        assert!(
            command_by_name(&output, "proxy")["next_step"]
                .as_str()
                .unwrap()
                .contains("JIG_REPO_ROOT"),
            "adopted={adopted}: proxy"
        );

        for args in [
            &["vault", "status", "--json"][..],
            &["prompt", "list", "--json"][..],
        ] {
            let command = jig()
                .current_dir(repo.path())
                .env("JIG_REPO_ROOT", &invalid_root)
                .env("JIG_VAULT_HOME", vault.path())
                .env("JIG_PROXY_STATE_DIR", &proxy_state)
                .args(args)
                .output()
                .unwrap();
            assert!(
                command.status.success(),
                "adopted={adopted}: {}\n{}",
                args.join(" "),
                String::from_utf8_lossy(&command.stderr)
            );
        }

        let proxy_list = jig()
            .current_dir(repo.path())
            .env("JIG_REPO_ROOT", &invalid_root)
            .env("JIG_VAULT_HOME", vault.path())
            .env("JIG_PROXY_STATE_DIR", &proxy_state)
            .args(["proxy", "list", "--json"])
            .output()
            .unwrap();
        assert_eq!(
            proxy_list.status.success(),
            cfg!(feature = "dev-proxy"),
            "adopted={adopted}: proxy list --json\n{}",
            String::from_utf8_lossy(&proxy_list.stderr)
        );

        if adopted && cfg!(feature = "dev-proxy") {
            let dev_status = jig()
                .current_dir(repo.path())
                .env("JIG_REPO_ROOT", &invalid_root)
                .env("JIG_PROXY_STATE_DIR", &proxy_state)
                .args(["dev", "status", "--json"])
                .output()
                .unwrap();
            assert!(
                dev_status.status.success(),
                "{}",
                String::from_utf8_lossy(&dev_status.stderr)
            );
        }
    }
}

#[test]
fn info_commands_prioritizes_invalid_override_recovery_when_the_local_repo_is_also_broken() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    std::fs::write(repo.path().join(".jig.toml"), "not valid toml = [").unwrap();
    let invalid_root = repo.path().join("missing-override");

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_REPO_ROOT", &invalid_root)
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["repo"]["context_status"], "invalid");
    for name in ["info", "check", "agent", "vault", "prompt"] {
        assert!(
            command_by_name(&output, name)["next_step"]
                .as_str()
                .unwrap()
                .contains("JIG_REPO_ROOT"),
            "{name}"
        );
    }
    #[cfg(feature = "dev-proxy")]
    assert!(
        command_by_name(&output, "dev")["next_step"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT"),
        "dev"
    );
}

#[cfg(unix)]
#[test]
fn info_commands_emits_no_progress_when_the_codex_probe_is_skipped() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let (stderr, terminal_output) = terminal_stderr();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands"])
        .stderr(stderr)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(read_terminal(terminal_output), b"");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Jig command availability: phase-two")
    );
}

#[cfg(unix)]
#[test]
fn info_commands_keeps_json_quiet_while_probing_configured_codex_marketplaces() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    let codex_home = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    configure_codex_marketplace(repo.path());
    let codex = write_codex_stub(repo.path(), 2);
    let (stderr, terminal_output) = terminal_stderr();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", codex_home.path())
        .args(["info", "--commands", "--json"])
        .stderr(stderr)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(read_terminal(terminal_output), b"");
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    let agent = output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == "agent")
        .unwrap();
    assert_eq!(agent["status"], "unavailable");
    assert_eq!(
        agent["reason_code"],
        "codex_marketplace_support_unavailable"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn info_commands_reports_terminal_progress_for_a_configured_codex_probe() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    let codex_home = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    configure_codex_marketplace(repo.path());
    write_registered_codex_marketplace(codex_home.path());
    let codex = write_codex_stub(repo.path(), 0);
    let (stderr, terminal_output) = terminal_stderr();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", codex_home.path())
        .env("NO_COLOR", "1")
        .args(["info", "--commands"])
        .stderr(stderr)
        .output()
        .unwrap();

    assert!(output.status.success());
    let terminal_output = String::from_utf8(read_terminal(terminal_output)).unwrap();
    assert!(terminal_output.contains("jig info --commands | inspect local Codex tooling"));
    assert!(terminal_output.contains("probe codex"));
    assert!(terminal_output.contains("command inventory probe complete"));
    assert!(!terminal_output.contains("agent doctor"));
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("Jig command availability: phase-two"));
}

fn write_info_commands_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
default_branch = "main"
repo_name = "phase-two"
jig_version = "0.2.0-beta.1"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": [{
                "name": "jig.bootstrap",
                "kind": "command",
                "description": "Bootstrap the repository.",
                "command": "bootstrap_command"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_test_launcher(root: &Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    for path in [
        root.join("scripts/jig"),
        root.join("scripts/install-jig.sh"),
    ] {
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
        }
    }
}

#[cfg(unix)]
fn configure_codex_marketplace(root: &Path) {
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[agent_tooling.codex]\nmarketplaces = []",
        r#"[[agent_tooling.codex.marketplaces]]
id = "jig-skills"
source = "bpcakes/jig-skills"
plugins = []"#,
    );
    fs::write(config_path, config).unwrap();
}

#[cfg(target_os = "linux")]
fn write_registered_codex_marketplace(codex_home: &Path) {
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"
"#,
    )
    .unwrap();
}

#[cfg(unix)]
fn write_codex_stub(root: &Path, exit_code: u8) -> PathBuf {
    let codex = root.join("codex-stub.sh");
    fs::write(
        &codex,
        format!("#!/bin/sh\nprintf 'captured probe output' >&2\nexit {exit_code}\n"),
    )
    .unwrap();
    let mut permissions = fs::metadata(&codex).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&codex, permissions).unwrap();
    codex
}

fn command_by_name<'a>(output: &'a Value, name: &str) -> &'a Value {
    output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("missing command {name}"))
}

fn command_status<'a>(output: &'a Value, name: &str) -> &'a str {
    command_by_name(output, name)["status"].as_str().unwrap()
}

fn assert_dev_proxy_status(
    output: &Value,
    name: &str,
    feature_status: &str,
    feature_reason_code: &str,
) {
    let (expected_status, expected_reason_code) = if cfg!(feature = "dev-proxy") {
        (feature_status, feature_reason_code)
    } else {
        ("unavailable", "dev_proxy_feature_not_built")
    };
    assert_eq!(command_status(output, name), expected_status, "{name}");
    assert_eq!(
        command_by_name(output, name)["reason_code"],
        expected_reason_code,
        "{name}"
    );
}

#[cfg(unix)]
fn terminal_stderr() -> (Stdio, File) {
    let mut controller = -1;
    let mut terminal = -1;
    // SAFETY: openpty initializes both owned file descriptors on success. Each
    // descriptor is immediately wrapped exactly once and closed by File/Stdio.
    let result = unsafe {
        libc::openpty(
            &mut controller,
            &mut terminal,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        result,
        0,
        "openpty failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: successful openpty returned distinct owned descriptors.
    let controller = unsafe { File::from_raw_fd(controller) };
    // SAFETY: successful openpty returned distinct owned descriptors.
    let terminal = unsafe { File::from_raw_fd(terminal) };
    (Stdio::from(terminal), controller)
}

#[cfg(unix)]
fn read_terminal(mut controller: File) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        match controller.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                break;
            }
            Err(error) => panic!("failed reading terminal stderr: {error}"),
        }
    }
    output
}
