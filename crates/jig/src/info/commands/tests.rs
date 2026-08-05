use super::*;
use crate::info::tests::write_info_fixture;
use crate::test_env::TestRepoBuilder;
use serde_json::json;
#[cfg(feature = "dev-proxy")]
use std::fs;
use tempfile::tempdir;

#[cfg(feature = "dev-proxy")]
#[test]
fn command_inventory_has_stable_schema_order_and_grouped_human_output() {
    let temp = tempdir().unwrap();
    write_info_fixture(temp.path());
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let agent = json!({ "ok": true });

    let output = info_with_capabilities(
        &ctx,
        VaultCapability {
            available: true,
            initialized: true,
            home: Some("/tmp/vault".into()),
            scope: Some("repo".into()),
            scope_id: Some("scope_1".into()),
            error: None,
        },
        &agent,
    );

    assert_eq!(output["command"], "info commands");
    assert_eq!(output["schema_version"], 1);
    assert_eq!(output["repo"]["context_status"], "valid");
    let commands = output["commands"].as_array().unwrap();
    let names = commands
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, DISCOVERABLE_COMMAND_NAMES);
    assert!(commands.iter().all(|command| command["status"] == "ready"));
    assert!(
        commands
            .iter()
            .all(|command| command["reason_code"].is_null())
    );

    let summary = format_summary(&output);
    for heading in [
        "Get started:",
        "Develop:",
        "Structured work:",
        "Project data:",
        "Local services:",
        "Agent and automation:",
    ] {
        assert_eq!(
            summary.matches(heading).count(),
            1,
            "expected one {heading:?} heading\n\n{summary}"
        );
    }
    assert!(summary.contains("migration-add  ready"));
}

#[test]
fn schema_v1_reason_codes_are_stable() {
    let reason_codes = reason::ALL
        .iter()
        .copied()
        .map(ReasonCode::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        reason_codes,
        [
            "agent_readiness_unknown",
            "bootstrap_tool_invalid",
            "bootstrap_tool_missing",
            "codex_marketplace_support_unavailable",
            "codex_marketplace_unregistered",
            "dev_apps_not_configured",
            "dev_proxy_feature_not_built",
            "migration_add_tool_invalid",
            "migration_add_tool_missing",
            "migration_directory_not_configured",
            "repo_context_unavailable",
            "schema_dump_tool_invalid",
            "schema_dump_tool_missing",
            "schema_dumps_disabled",
            "sqlx_disabled",
            "vault_not_initialized",
            "vault_status_unavailable",
        ]
    );
    assert!(reason_codes.windows(2).all(|pair| pair[0] < pair[1]));
}

#[cfg(feature = "dev-proxy")]
#[test]
fn dev_is_ready_for_legacy_frontends_and_opt_in_workspace_discovery() {
    for config in [
        r#"
[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 0
"#,
        r#"
[dev]
workspace_discovery = true
"#,
    ] {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(format!(
                r#"
bootstrap_command = "printf bootstrap"
{config}
[agent_tooling.codex]
marketplaces = []
"#
            ))
            .required_commands(["bootstrap_command"])
            .tool(json!({
                "name": tool::BOOTSTRAP,
                "kind": "command",
                "description": "Bootstrap the repository.",
                "command": "bootstrap_command"
            }))
            .write();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = dev_command(&ctx);

        assert_eq!(output["status"], "ready", "{config}");
        assert_eq!(output["reason_code"], Value::Null, "{config}");
    }
}

#[cfg(feature = "dev-proxy")]
#[test]
fn command_inventory_explains_disabled_and_incomplete_capabilities() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .required_commands(["bootstrap_command"])
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": "native",
            "description": "Bootstrap the repository."
        }))
        .write();
    write_full_launcher(temp.path());
    fs::write(temp.path().join(".mcp.json"), "{not json").unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let launcher = temp.path().join("scripts/jig").display().to_string();
    let agent = json!({
        "ok": false,
        "codex": { "available": true, "probe_error": null },
        "next_steps": ["Run `scripts/jig agent bootstrap`."],
    });

    let output = info_with_capabilities(
        &ctx,
        VaultCapability {
            available: true,
            initialized: false,
            home: Some("/tmp/vault".into()),
            scope: Some("repo".into()),
            scope_id: Some("scope_1".into()),
            error: None,
        },
        &agent,
    );

    assert_command_status(&output, "dev", "not_configured", "dev_apps_not_configured");
    assert!(
        command_by_name(&output, "dev")["next_step"]
            .as_str()
            .unwrap()
            .contains("--write")
    );
    // The proxy command family remains usable for ad-hoc runs, aliases,
    // certificates, services, and diagnostics without configured dev apps.
    assert_eq!(command_by_name(&output, "proxy")["status"], "ready");
    assert_command_status(
        &output,
        "bootstrap",
        "needs_setup",
        "bootstrap_tool_invalid",
    );
    assert_eq!(command_by_name(&output, "loop")["status"], "ready");
    assert_eq!(command_by_name(&output, "loop")["reason_code"], Value::Null);
    assert_command_status(&output, "migration-add", "not_configured", "sqlx_disabled");
    assert_command_status(&output, "schema-dump", "not_configured", "sqlx_disabled");
    assert!(
        command_by_name(&output, "migration-add")["next_step"]
            .as_str()
            .unwrap()
            .contains("--sqlx-enabled true --rust-migration-dir migrations --force --write")
    );
    assert!(
        command_by_name(&output, "schema-dump")["next_step"]
            .as_str()
            .unwrap()
            .contains(
                "--sqlx-enabled true --rust-migration-dir migrations --schema-dump-enabled true"
            )
    );
    assert_command_status(&output, "vault", "needs_setup", "vault_not_initialized");
    assert_command_status(
        &output,
        "agent",
        "needs_setup",
        "codex_marketplace_unregistered",
    );
    assert_eq!(command_by_name(&output, "mcp")["status"], "ready");
    assert_eq!(command_by_name(&output, "mcp")["reason_code"], Value::Null);
    assert_eq!(
        command_by_name(&output, "agent")["next_step"],
        format!("Run `{launcher} agent bootstrap`.")
    );
    assert_eq!(
        command_by_name(&output, "vault")["next_step"],
        format!("Run `{launcher} vault init`.")
    );
    assert!(
        command_by_name(&output, "bootstrap")["next_step"]
            .as_str()
            .unwrap()
            .contains(&format!("`{launcher} check contract`"))
    );

    let config_path = temp.path().join(".jig.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    config.push_str(
        r#"
[[loop.workflows]]
id = "disabled"
kind = "noop_status"
enabled = false
"#,
    );
    fs::write(config_path, config).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let output = info_with_capabilities(
        &ctx,
        VaultCapability {
            available: true,
            initialized: false,
            home: None,
            scope: None,
            scope_id: None,
            error: None,
        },
        &agent,
    );
    assert_eq!(command_by_name(&output, "loop")["status"], "ready");
    assert_eq!(command_by_name(&output, "loop")["reason_code"], Value::Null);
}

#[test]
fn command_inventory_without_repo_context_keeps_onboarding_commands_available() {
    let output = info_without_context(
        "Could not find repo root containing .jig.toml",
        ContextFallback::Tolerant {
            context_status: RepoContextStatus::Absent,
            dev: dev_capability(None),
            vault: VaultCapability {
                available: true,
                initialized: false,
                home: Some("/tmp/vault".into()),
                scope: Some("legacy".into()),
                scope_id: None,
                error: None,
            },
            jig: "jig".into(),
            dev_proxy_available: dev_proxy_available(None),
        },
    );

    assert_eq!(output["repo"]["name"], Value::Null);
    assert_eq!(output["repo"]["root"], Value::Null);
    assert_eq!(output["repo"]["context_status"], "absent");
    assert!(output["repo"]["context_error"].is_string());
    let names = output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, DISCOVERABLE_COMMAND_NAMES);
    for name in ["init", "presets", "adopt", "doctor", "prompt", "codex"] {
        assert_eq!(command_by_name(&output, name)["status"], "ready", "{name}");
    }
    for name in [
        "update",
        "bootstrap",
        "setup",
        "info",
        "check",
        "loop",
        "mcp",
    ] {
        assert_command_status(&output, name, "needs_setup", "repo_context_unavailable");
    }
    assert!(
        command_by_name(&output, "info")["reason"]
            .as_str()
            .unwrap()
            .contains("info --commands")
    );
    #[cfg(feature = "dev-proxy")]
    assert_command_status(&output, "dev", "needs_setup", "repo_context_unavailable");
    #[cfg(feature = "dev-proxy")]
    assert_command_status(&output, "proxy", "needs_setup", "repo_context_unavailable");
    #[cfg(not(feature = "dev-proxy"))]
    assert_command_status(
        &output,
        "proxy",
        "unavailable",
        "dev_proxy_feature_not_built",
    );
    assert_command_status(&output, "vault", "needs_setup", "vault_not_initialized");

    let summary = format_summary(&output);
    assert!(summary.contains("Jig command availability: no adopted repository"));
    assert!(!summary.contains("<unknown>"));
    assert!(summary.contains("Context (absent): Could not find repo root containing .jig.toml"));
    for heading in [
        "Get started:",
        "Develop:",
        "Structured work:",
        "Project data:",
        "Local services:",
        "Agent and automation:",
    ] {
        assert_eq!(
            summary.matches(heading).count(),
            1,
            "{heading}\n\n{summary}"
        );
    }
}

#[test]
fn invalid_repo_context_blocks_commands_that_consult_optional_context() {
    let output = info_without_context(
        "Failed to parse .jig.toml",
        ContextFallback::Invalid {
            invalid_override: false,
        },
    );

    assert_eq!(output["repo"]["context_status"], "invalid");
    for name in ["info", "vault", "prompt"] {
        assert_command_status(&output, name, "needs_setup", "repo_context_unavailable");
    }
    #[cfg(feature = "dev-proxy")]
    for name in ["dev", "proxy"] {
        assert_command_status(&output, name, "needs_setup", "repo_context_unavailable");
    }
    #[cfg(not(feature = "dev-proxy"))]
    for name in ["dev", "proxy"] {
        assert_command_status(&output, name, "unavailable", "dev_proxy_feature_not_built");
    }
}

#[test]
fn invalid_override_remains_the_remediation_when_local_recovery_also_fails() {
    let output = info_without_context(
        "JIG_REPO_ROOT does not contain .jig.toml",
        ContextFallback::Invalid {
            invalid_override: true,
        },
    );

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
            .contains("JIG_REPO_ROOT")
    );
}

#[cfg(feature = "dev-proxy")]
#[test]
fn tolerant_fallback_uses_recovered_context_only_for_tolerant_workflows() {
    let temp = tempdir().unwrap();
    write_info_fixture(temp.path());
    let context = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let output = info_without_context(
        "JIG_REPO_ROOT does not contain .jig.toml",
        ContextFallback::Tolerant {
            context_status: RepoContextStatus::Recovered,
            dev: dev_capability(Some(&context)),
            vault: VaultCapability {
                available: true,
                initialized: true,
                home: Some("/tmp/vault".into()),
                scope: Some("repo".into()),
                scope_id: Some("scope_1".into()),
                error: None,
            },
            jig: command_prefix(&context),
            dev_proxy_available: dev_proxy_available(Some(&context)),
        },
    );

    assert_eq!(output["repo"]["context_status"], "recovered");
    assert_eq!(command_by_name(&output, "dev")["status"], "ready");
    assert_command_status(&output, "proxy", "needs_setup", "repo_context_unavailable");
    assert_eq!(command_by_name(&output, "vault")["status"], "ready");
    assert_eq!(command_by_name(&output, "prompt")["status"], "ready");
    let summary = format_summary(&output);
    assert!(summary.contains("Jig command availability: recovered repository context"));
    assert!(summary.contains("Context (recovered): JIG_REPO_ROOT"));
    assert!(
        command_by_name(&output, "proxy")["next_step"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT")
    );
}

#[cfg(not(feature = "dev-proxy"))]
#[test]
fn context_free_inventory_prioritizes_compiled_dev_availability() {
    let output = info_without_context(
        "Could not find repo root containing .jig.toml",
        ContextFallback::Tolerant {
            context_status: RepoContextStatus::Absent,
            dev: dev_capability(None),
            vault: VaultCapability {
                available: true,
                initialized: true,
                home: None,
                scope: None,
                scope_id: None,
                error: None,
            },
            jig: "jig".into(),
            dev_proxy_available: dev_proxy_available(None),
        },
    );

    assert_command_status(&output, "dev", "unavailable", "dev_proxy_feature_not_built");
    assert_command_status(
        &output,
        "proxy",
        "unavailable",
        "dev_proxy_feature_not_built",
    );
}

#[test]
fn command_inventory_distinguishes_missing_and_invalid_manifest_tools() {
    let missing = tempdir().unwrap();
    TestRepoBuilder::new(missing.path())
        .config(sqlx_command_inventory_config(true))
        .required_commands([
            "bootstrap_command",
            "migration_add_command",
            "schema_dump_command",
        ])
        .write();
    let missing_ctx = RepoContext::load_from_root(missing.path().to_path_buf()).unwrap();
    let missing_output = ready_local_inventory(&missing_ctx);

    assert_command_status(
        &missing_output,
        "bootstrap",
        "needs_setup",
        "bootstrap_tool_missing",
    );
    assert_command_status(
        &missing_output,
        "setup",
        "needs_setup",
        "bootstrap_tool_missing",
    );
    assert_command_status(
        &missing_output,
        "migration-add",
        "needs_setup",
        "migration_add_tool_missing",
    );
    assert_command_status(
        &missing_output,
        "schema-dump",
        "needs_setup",
        "schema_dump_tool_missing",
    );

    let invalid = tempdir().unwrap();
    TestRepoBuilder::new(invalid.path())
        .config(sqlx_command_inventory_config(true))
        .required_commands([
            "bootstrap_command",
            "migration_add_command",
            "schema_dump_command",
        ])
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": "native",
            "description": "Invalid native bootstrap."
        }))
        .tool(json!({
            "name": tool::MIGRATION_ADD,
            "kind": "command",
            "description": "Invalid migration command.",
            "command": "missing_command_key"
        }))
        .tool(json!({
            "name": tool::SCHEMA_DUMP,
            "kind": "native",
            "description": "Invalid native schema dump."
        }))
        .write();
    let invalid_ctx = RepoContext::load_from_root(invalid.path().to_path_buf()).unwrap();
    let invalid_output = ready_local_inventory(&invalid_ctx);

    assert_command_status(
        &invalid_output,
        "bootstrap",
        "needs_setup",
        "bootstrap_tool_invalid",
    );
    assert_command_status(
        &invalid_output,
        "setup",
        "needs_setup",
        "bootstrap_tool_invalid",
    );
    assert_command_status(
        &invalid_output,
        "migration-add",
        "needs_setup",
        "migration_add_tool_invalid",
    );
    assert_command_status(
        &invalid_output,
        "schema-dump",
        "needs_setup",
        "schema_dump_tool_invalid",
    );

    let disabled = tempdir().unwrap();
    TestRepoBuilder::new(disabled.path())
        .config(sqlx_command_inventory_config(false))
        .required_commands(["bootstrap_command"])
        .write();
    let disabled_ctx = RepoContext::load_from_root(disabled.path().to_path_buf()).unwrap();
    let disabled_output = ready_local_inventory(&disabled_ctx);
    assert_command_status(
        &disabled_output,
        "schema-dump",
        "not_configured",
        "schema_dumps_disabled",
    );
    assert!(
        command_by_name(&disabled_output, "schema-dump")["next_step"]
            .as_str()
            .unwrap()
            .contains("--schema-dump-enabled true --force --write")
    );
}

#[test]
fn migration_workflows_require_a_non_blank_migration_directory() {
    for tool_spec in [
        json!({
            "name": tool::MIGRATION_ADD,
            "kind": "native",
            "description": "Add a migration."
        }),
        json!({
            "name": tool::MIGRATION_ADD,
            "kind": "command",
            "description": "Add a migration.",
            "command": "migration_add_command"
        }),
    ] {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(
                r#"
sqlx_enabled = true
rust_migration_dir = "   "
migration_add_command = "printf migration"

[agent_tooling.codex]
marketplaces = []
"#,
            )
            .tool(tool_spec)
            .write();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = ready_local_inventory(&ctx);

        assert_command_status(
            &output,
            "migration-add",
            "needs_setup",
            "migration_directory_not_configured",
        );
        assert_command_status(
            &output,
            "schema-dump",
            "needs_setup",
            "migration_directory_not_configured",
        );
        for name in ["migration-add", "schema-dump"] {
            assert!(
                command_by_name(&output, name)["next_step"]
                    .as_str()
                    .unwrap()
                    .contains("--rust-migration-dir migrations")
            );
        }
    }
}

#[cfg(not(feature = "dev-proxy"))]
#[test]
fn command_inventory_marks_dev_and_proxy_unavailable_without_feature() {
    let temp = tempdir().unwrap();
    write_info_fixture(temp.path());
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let output = info_with_capabilities(
        &ctx,
        VaultCapability {
            available: true,
            initialized: true,
            home: None,
            scope: None,
            scope_id: None,
            error: None,
        },
        &json!({ "ok": true }),
    );

    let names = output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, DISCOVERABLE_COMMAND_NAMES);

    assert_command_status(&output, "dev", "unavailable", "dev_proxy_feature_not_built");
    assert_command_status(
        &output,
        "proxy",
        "unavailable",
        "dev_proxy_feature_not_built",
    );
}

#[test]
fn command_inventory_marks_indeterminate_local_capabilities_unavailable() {
    let temp = tempdir().unwrap();
    write_info_fixture(temp.path());
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let agent = json!({
        "ok": false,
        "codex": { "available": null, "probe_error": "probe failed" },
        "next_steps": [],
    });

    let output = info_with_capabilities(
        &ctx,
        VaultCapability {
            available: false,
            initialized: false,
            home: None,
            scope: None,
            scope_id: None,
            error: Some("vault failed".into()),
        },
        &agent,
    );

    assert_command_status(&output, "vault", "unavailable", "vault_status_unavailable");
    assert_command_status(&output, "agent", "unavailable", "agent_readiness_unknown");
    assert_eq!(
        command_by_name(&output, "agent")["next_step"],
        "Run `jig agent doctor` for details."
    );
}

#[cfg(all(unix, not(feature = "dev-proxy")))]
#[test]
fn stripped_build_requires_the_full_executable_launcher_chain() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    write_info_fixture(temp.path());
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let launcher = temp.path().join("scripts/jig");
    let installer = temp.path().join("scripts/install-jig.sh");
    std::fs::write(&launcher, "#!/bin/sh\n").unwrap();
    let mut permissions = std::fs::metadata(&launcher).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&launcher, permissions).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let missing_installer = ready_local_inventory(&ctx);
    assert_command_status(
        &missing_installer,
        "proxy",
        "unavailable",
        "dev_proxy_feature_not_built",
    );

    std::fs::write(&installer, "#!/bin/sh\n").unwrap();
    let non_executable_installer = ready_local_inventory(&ctx);
    assert_command_status(
        &non_executable_installer,
        "proxy",
        "unavailable",
        "dev_proxy_feature_not_built",
    );

    let mut permissions = std::fs::metadata(&installer).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&installer, permissions).unwrap();
    let complete_launcher = ready_local_inventory(&ctx);
    assert_eq!(
        command_by_name(&complete_launcher, "proxy")["status"],
        "ready"
    );
}

#[test]
fn minimal_footprint_uses_the_installed_jig_even_if_a_launcher_is_present() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
harness_footprint = "minimal"
sqlx_enabled = false

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .write();
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    std::fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let output = ready_local_inventory(&ctx);

    assert!(
        command_by_name(&output, "migration-add")["next_step"]
            .as_str()
            .unwrap()
            .contains("`jig adopt ")
    );
    assert!(
        command_by_name(&output, "migration-add")["next_step"]
            .as_str()
            .unwrap()
            .contains("--minimal")
    );
    assert!(
        command_by_name(&output, "migration-add")["next_step"]
            .as_str()
            .unwrap()
            .contains("--template /tmp/template")
    );
    assert!(
        command_by_name(&output, "migration-add")["next_step"]
            .as_str()
            .unwrap()
            .contains(&temp.path().display().to_string())
    );
    assert!(
        !command_by_name(&output, "migration-add")["next_step"]
            .as_str()
            .unwrap()
            .contains("scripts/jig")
    );
}

fn sqlx_command_inventory_config(schema_dump_enabled: bool) -> String {
    format!(
        r#"
sqlx_enabled = true
rust_migration_dir = "migrations"
schema_dump_enabled = {schema_dump_enabled}
bootstrap_command = "printf bootstrap"
migration_add_command = "printf migration"
schema_dump_command = "printf schema"

[agent_tooling.codex]
marketplaces = []
"#
    )
}

fn ready_local_inventory(ctx: &RepoContext) -> Value {
    info_with_capabilities(
        ctx,
        VaultCapability {
            available: true,
            initialized: true,
            home: None,
            scope: None,
            scope_id: None,
            error: None,
        },
        &json!({ "ok": true }),
    )
}

fn assert_command_status(output: &Value, name: &str, status: &str, reason_code: &str) {
    let command = command_by_name(output, name);
    assert_eq!(command["status"], status, "{name}");
    assert_eq!(command["reason_code"], reason_code, "{name}");
    assert!(
        command["reason"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        command["next_step"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}

fn command_by_name<'a>(output: &'a Value, name: &str) -> &'a Value {
    output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("missing command {name}"))
}

#[cfg(feature = "dev-proxy")]
fn write_full_launcher(root: &std::path::Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    for path in [
        root.join("scripts/jig"),
        root.join("scripts/install-jig.sh"),
    ] {
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
        }
    }
}
