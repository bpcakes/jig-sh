use super::*;
use crate::info::tests::write_info_fixture;
use crate::test_env::TestRepoBuilder;
use serde_json::json;
#[cfg(feature = "dev-proxy")]
use std::fs;
use tempfile::tempdir;

fn discoverable_command_names() -> Vec<&'static str> {
    crate::root_commands::ALL
        .iter()
        .map(|command| command.name)
        .collect()
}

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
    assert_eq!(output["schema_version"], 3);
    assert_eq!(output["repo"]["context_status"], "valid");
    let commands = output["commands"].as_array().unwrap();
    let names = commands
        .iter()
        .map(|command| command["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, discoverable_command_names());
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
    assert!(summary.contains("sqlx       ready"));
}

#[test]
fn schema_v3_reason_codes_are_stable() {
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
            "migration_backend_not_configured",
            "migration_directory_not_configured",
            "repo_context_unavailable",
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
    assert_command_status(&output, "sqlx", "not_configured", "sqlx_disabled");
    assert!(
        command_by_name(&output, "sqlx")["next_step"]
            .as_str()
            .unwrap()
            .contains("--sqlx-enabled true --rust-migration-dir migrations --force --write")
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
    assert_eq!(names, discoverable_command_names());
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
        "migration",
        "needs_setup",
        "migration_add_tool_missing",
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
        "migration",
        "needs_setup",
        "migration_add_tool_invalid",
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
            "migration",
            "needs_setup",
            "migration_directory_not_configured",
        );
        assert!(
            command_by_name(&output, "migration")["next_step"]
                .as_str()
                .unwrap()
                .contains("Set migration_dir in .jig.toml")
        );
    }
}

#[test]
fn go_postgres_exposes_generic_migration_authoring_without_sqlx() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
backend_language = "go"
go_database = "postgres"
migration_dir = "internal/database/migrations"

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .tool(json!({
            "name": tool::MIGRATION_ADD,
            "kind": "native",
            "description": "Add a migration."
        }))
        .write();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let output = ready_local_inventory(&ctx);

    assert_eq!(command_by_name(&output, "migration")["status"], "ready");
    assert_command_status(&output, "sqlx", "not_configured", "sqlx_disabled");
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
    assert_eq!(names, discoverable_command_names());

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

mod launcher_selection;
use launcher_selection::*;
