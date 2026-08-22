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

#[cfg(unix)]
#[test]
fn failed_loop_tick_and_dispatch_exit_nonzero_after_json_output() {
    for args in [
        vec!["loop", "tick", "--workflow", "failing-task", "--json"],
        vec!["loop", "dispatch", "--json"],
    ] {
        let repo = tempdir().unwrap();
        write_failing_loop_repo(repo.path());
        let output = jig()
            .current_dir(repo.path())
            .env("JIG_CODEX_BIN", repo.path().join("missing-codex"))
            .args(args)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false, "{value:#}");
        assert_eq!(value["status"], "failed", "{value:#}");
    }
}

#[test]
fn loop_acknowledge_occurrence_has_human_and_json_contracts() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let runtime_dir = repo.path().join(".agent/runtime/loop");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 2,
            "occurrences": {
                "nightly@100": {
                    "occurrence_id": "nightly@100",
                    "workflow_id": "nightly",
                    "scheduled_at_ms": 100,
                    "owner": "owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "finished_at_ms": 200,
                    "status": "needs_attention"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let human = jig()
        .current_dir(repo.path())
        .args([
            "loop",
            "acknowledge-occurrence",
            "--occurrence",
            "nightly@100",
        ])
        .output()
        .unwrap();
    assert!(human.status.success(), "{human:?}");
    assert!(human.stderr.is_empty(), "{human:?}");
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("Loop acknowledge-occurrence: acknowledged"));
    assert!(human.contains("Occurrence: nightly@100"));

    let json = jig()
        .current_dir(repo.path())
        .args([
            "loop",
            "acknowledge-occurrence",
            "--occurrence",
            "nightly@100",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(json.status.success(), "{json:?}");
    assert!(json.stderr.is_empty(), "{json:?}");
    let json: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["command"], "loop acknowledge-occurrence");
    assert_eq!(json["occurrence_id"], "nightly@100");
    assert_eq!(json["changed"], false);
    assert_eq!(json["occurrence"]["status"], "acknowledged");
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
    assert_eq!(structured["schema_version"], 2);
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
    assert!(next_step.contains(&full.path().join("scripts/jig").display().to_string()));
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
    let canonical_template = template.canonicalize().unwrap().display().to_string();
    assert_eq!(preview["template"], canonical_template);

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
    assert_eq!(apply["template"], canonical_template);
    let config = fs::read_to_string(repo.path().join(".jig.toml")).unwrap();
    assert!(config.contains("harness_footprint = \"minimal\""));
    assert!(config.contains(&format!("_src_path = {portable_source:?}")));
    assert!(config.contains(&format!("_template_local_path = {:?}", canonical_template)));
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
