use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

#[cfg(test)]
use crate::cli::format_info_summary_for_test as format_summary;
use crate::command::{VaultCommand, VaultStatusRequest};
use crate::context::{DevAppConfig, REPO_CONTEXT_NOT_FOUND, RepoContext, WorkGate};

const COMMAND: &str = "info";
const DEFAULT_MCP_COMMAND: &str = "scripts/jig mcp";

mod commands;

pub(crate) fn run(commands: bool, json_output: bool) -> Result<Value> {
    if commands {
        let ctx = match RepoContext::load_optional_strict() {
            Ok(Some(ctx)) => ctx,
            Ok(None) => {
                let vault = vault_capability(None);
                return Ok(commands::info_without_context(
                    REPO_CONTEXT_NOT_FOUND,
                    commands::ContextFallback::Tolerant {
                        context_status: commands::RepoContextStatus::Absent,
                        dev: commands::dev_capability(None),
                        vault,
                        jig: "jig".into(),
                        dev_proxy_available: commands::dev_proxy_available(None),
                    },
                ));
            }
            Err(error) => {
                let fallback = match RepoContext::load_optional_quiet() {
                    Ok(context) => {
                        let context_status = if context.is_some() {
                            commands::RepoContextStatus::Recovered
                        } else {
                            commands::RepoContextStatus::Invalid
                        };
                        commands::ContextFallback::Tolerant {
                            context_status,
                            dev: commands::dev_capability_with_next_step(
                                context.as_ref(),
                                commands::INVALID_OVERRIDE_NEXT_STEP,
                            ),
                            vault: vault_capability(context.as_ref()),
                            jig: context
                                .as_ref()
                                .map_or_else(|| "jig".into(), commands::command_prefix),
                            dev_proxy_available: commands::dev_proxy_available(context.as_ref()),
                        }
                    }
                    Err(_) => commands::ContextFallback::Invalid {
                        invalid_override: RepoContext::repo_root_override_is_set(),
                    },
                };
                return Ok(commands::info_without_context(
                    &format!("{error:#}"),
                    fallback,
                ));
            }
        };
        let vault = vault_capability(Some(&ctx));
        let agent = crate::runtime::agent_doctor_for_inventory(&ctx, !json_output);
        return Ok(commands::info_with_capabilities(&ctx, vault, &agent));
    }
    let ctx = RepoContext::load()?;
    Ok(repo_info(&ctx))
}

pub(crate) fn format_commands_summary(value: &Value) -> String {
    commands::format_summary(value)
}

fn repo_info(ctx: &RepoContext) -> Value {
    repo_info_with_vault(ctx, vault_capability(Some(ctx)))
}

fn repo_info_with_vault(ctx: &RepoContext, vault: VaultCapability) -> Value {
    let mcp_command = mcp_command(ctx.root());
    let dev_apps = ctx
        .dev_config()
        .apps
        .iter()
        .map(dev_app_value)
        .collect::<Vec<_>>();
    let frontend_apps = ctx
        .frontend_apps()
        .iter()
        .map(|app| {
            json!({
                "name": &app.name,
                "dir": &app.dir,
                "coverage_threshold": app.coverage_threshold,
                "kind": ctx.frontend_app_kind(app),
                "role": ctx.frontend_app_role(app),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "command": COMMAND,
        "repo": {
            "name": ctx.repo_name(),
            "root": ctx.root().display().to_string(),
            "template_source": ctx.source_path(),
            "template_commit": ctx.source_commit(),
            // Compatibility alias for v2/v3 repositories. It is null for v4,
            // where generated configuration no longer pins a product release.
            "jig_version": ctx.legacy_jig_version(),
            "runtime_version": env!("CARGO_PKG_VERSION"),
            "contract_version": ctx.contract_version(),
        },
        "capabilities": {
            "sqlx": ctx.sqlx_enabled(),
            "schema_dumps": ctx.sqlx_enabled() && ctx.schema_dump_enabled(),
            "frontend_apps": !frontend_apps.is_empty(),
            "dev_proxy": crate::doctor::proxy_configured(ctx),
            "vault": vault.available,
            "vault_available": vault.available,
            "vault_initialized": vault.initialized,
            "vault_home": vault.home,
            "vault_scope": vault.scope,
            "vault_scope_id": vault.scope_id,
            "vault_error": vault.error,
        },
        "check_tools": ctx.work_check_tools(),
        "contract_tools": ctx.tool_specs().iter().map(|tool| {
            json!({
                "name": &tool.name,
                "kind": &tool.kind,
                "command": &tool.command,
                "description": &tool.description,
            })
        }).collect::<Vec<_>>(),
        "work_gates": ctx.work_gates().iter().map(work_gate_value).collect::<Vec<_>>(),
        "frontend_apps": frontend_apps,
        "dev": {
            "proxy_port": ctx.dev_config().proxy_port,
            "https_port": ctx.dev_config().https_port,
            "https": ctx.dev_config().https,
            "http2": ctx.dev_config().http2,
            "lan": ctx.dev_config().lan,
            "tld": &ctx.dev_config().tld,
            "workspace_discovery": ctx.dev_config().workspace_discovery,
        },
        "dev_apps": dev_apps,
        "mcp_command": mcp_command.command,
        "mcp_command_source": mcp_command.source,
        "mcp_command_error": mcp_command.error,
    })
}

struct VaultCapability {
    available: bool,
    initialized: bool,
    home: Option<String>,
    scope: Option<String>,
    scope_id: Option<String>,
    error: Option<String>,
}

fn vault_capability(ctx: Option<&RepoContext>) -> VaultCapability {
    let command = VaultCommand::Status(VaultStatusRequest {
        vault: crate::runtime::vault_options_for_context(ctx),
    });
    match crate::runtime::dispatch_vault(command) {
        Ok(output) => VaultCapability {
            available: true,
            initialized: output["exists"].as_bool().unwrap_or(false),
            home: output["vault_home"].as_str().map(str::to_string),
            scope: output["vault_scope"].as_str().map(str::to_string),
            scope_id: output["vault_scope_id"].as_str().map(str::to_string),
            error: None,
        },
        Err(error) => VaultCapability {
            available: false,
            initialized: false,
            home: None,
            scope: None,
            scope_id: None,
            error: Some(format!("{error:#}")),
        },
    }
}

struct McpCommandInfo {
    command: String,
    source: &'static str,
    error: Option<String>,
}

fn mcp_command(root: &Path) -> McpCommandInfo {
    let path = root.join(".mcp.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return default_mcp_command(None),
        Err(error) => {
            return default_mcp_command(Some(format!(
                "Failed to read {}: {error}",
                path.display()
            )));
        }
    };
    let value = match serde_json::from_str::<Value>(&text) {
        Ok(value) => value,
        Err(error) => {
            return default_mcp_command(Some(format!(
                "Failed to parse {}: {error}",
                path.display()
            )));
        }
    };
    let command = match mcp_command_parts(&value, &path) {
        Ok(command) => command,
        Err(error) => return default_mcp_command(Some(error)),
    };

    // MCP `command` is a single program path and `args` are already structured.
    // Render a shell-friendly display line for humans; Jig does not parse this
    // string back into an MCP command.
    McpCommandInfo {
        command: shell_display_command(&command),
        source: ".mcp.json",
        error: None,
    }
}

fn mcp_command_parts(value: &Value, path: &Path) -> Result<Vec<String>, String> {
    let server = &value["mcpServers"]["jig"];
    let command = server["command"].as_str().ok_or_else(|| {
        format!(
            "{} does not define a non-empty mcpServers.jig.command",
            path.display()
        )
    })?;
    if command.is_empty() {
        return Err(format!(
            "{} does not define a non-empty mcpServers.jig.command",
            path.display()
        ));
    }
    let mut parts = vec![normalize_repo_relative_command(command)];
    if let Some(args_value) = server.get("args") {
        let Some(args) = args_value.as_array() else {
            return Err(format!(
                "{} mcpServers.jig.args must be an array of strings",
                path.display()
            ));
        };
        for (index, arg) in args.iter().enumerate() {
            let arg = arg.as_str().ok_or_else(|| {
                format!(
                    "{} mcpServers.jig.args[{index}] must be a string",
                    path.display()
                )
            })?;
            parts.push(arg.to_string());
        }
    }
    Ok(parts)
}

fn shell_display_command(command: &[String]) -> String {
    command
        .iter()
        .map(|part| crate::shell::quote(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_mcp_command(error: Option<String>) -> McpCommandInfo {
    McpCommandInfo {
        command: DEFAULT_MCP_COMMAND.into(),
        source: "default",
        error,
    }
}

fn normalize_repo_relative_command(command: &str) -> String {
    command.strip_prefix("./").unwrap_or(command).to_string()
}

fn dev_app_value(app: &DevAppConfig) -> Value {
    json!({
        "name": &app.name,
        "dir": &app.dir,
        "kind": &app.kind,
        "command": &app.command,
        "argv": &app.argv,
        "port": app.port,
        "host": &app.host,
        "proxy": app.proxy,
    })
}

fn work_gate_value(gate: &WorkGate) -> Value {
    match gate {
        WorkGate::Check(gate) => json!({
            "id": &gate.id,
            "kind": "check",
            "tool": &gate.tool,
            "required": gate.required,
        }),
        WorkGate::CodexReview(gate) => json!({
            "id": &gate.id,
            "kind": "codex_review",
            "skill": &gate.skill,
            "fail_on": gate.threshold,
            "scope": &gate.scope,
            "model": &gate.model,
            "required": gate.required,
        }),
        WorkGate::Unsupported(gate) => json!({
            "id": &gate.id,
            "kind": &gate.kind,
            "required": gate.required,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::TestRepoBuilder;
    use crate::tool_defs::tool;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn reports_repo_contract_capabilities_and_dev_apps() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info_with_vault(
            &ctx,
            VaultCapability {
                available: true,
                initialized: true,
                home: Some("/tmp/vault".into()),
                scope: Some("repo".into()),
                scope_id: Some("scope_1".into()),
                error: None,
            },
        );

        assert_eq!(output["command"], "info");
        assert_eq!(output["repo"]["name"], "demo");
        assert_eq!(output["repo"]["template_source"], "/tmp/template");
        assert_eq!(output["repo"]["template_commit"], "abc123");
        assert_eq!(output["repo"]["runtime_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(output["repo"]["contract_version"], 3);
        assert_eq!(output["repo"]["jig_version"], "0.2.0-beta.1");
        assert_eq!(output["capabilities"]["sqlx"], true);
        assert_eq!(output["capabilities"]["schema_dumps"], true);
        assert_eq!(output["capabilities"]["frontend_apps"], true);
        assert_eq!(output["capabilities"]["dev_proxy"], true);
        assert_eq!(output["capabilities"]["vault"], true);
        assert_eq!(output["capabilities"]["vault_available"], true);
        assert_eq!(output["capabilities"]["vault_initialized"], true);
        assert_eq!(output["capabilities"]["vault_home"], "/tmp/vault");
        assert_eq!(output["capabilities"]["vault_scope"], "repo");
        assert_eq!(output["capabilities"]["vault_scope_id"], "scope_1");
        assert_eq!(output["check_tools"][0], "jig.test");
        assert_eq!(output["work_gates"][0]["id"], "tests");
        assert_eq!(output["dev_apps"][0]["name"], "web");
        assert_eq!(output["frontend_apps"][0]["kind"], "vite");
        assert_eq!(output["frontend_apps"][0]["role"], "spa");
        assert_eq!(output["mcp_command"], "scripts/jig mcp");
        assert_eq!(output["mcp_command_source"], "default");
        assert_eq!(output["mcp_command_error"], Value::Null);

        let summary = format_summary(&output);
        assert!(summary.contains("Jig info: demo"));
        assert!(summary.contains("Template source: /tmp/template @ abc123"));
        assert!(summary.contains(&format!(
            "Runtime: jig {} · contract v3",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(summary.contains(
            "Capabilities: SQLx, schema dumps, frontend apps, dev proxy, vault initialized"
        ));
    }

    #[test]
    fn distinguishes_available_uninitialized_vault() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info_with_vault(
            &ctx,
            VaultCapability {
                available: true,
                initialized: false,
                home: Some("/tmp/vault".into()),
                scope: Some("repo".into()),
                scope_id: Some("scope_1".into()),
                error: None,
            },
        );

        assert_eq!(output["capabilities"]["vault"], true);
        assert_eq!(output["capabilities"]["vault_available"], true);
        assert_eq!(output["capabilities"]["vault_initialized"], false);
        assert_eq!(output["capabilities"]["vault_home"], "/tmp/vault");
        let summary = format_summary(&output);
        assert!(summary.contains("vault available (not initialized)"));
    }

    #[test]
    fn reports_vault_error_when_status_fails() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info_with_vault(
            &ctx,
            VaultCapability {
                available: false,
                initialized: false,
                home: None,
                scope: None,
                scope_id: None,
                error: Some("vault status failed".into()),
            },
        );

        assert_eq!(output["capabilities"]["vault"], false);
        assert_eq!(output["capabilities"]["vault_available"], false);
        assert_eq!(output["capabilities"]["vault_initialized"], false);
        assert_eq!(output["capabilities"]["vault_error"], "vault status failed");
    }

    #[test]
    fn reports_mcp_command_from_mcp_json() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "jig": {
                        "command": "./tools/local jig",
                        "args": ["mcp", "--mode", "agent one"]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info(&ctx);

        assert_eq!(
            output["mcp_command"],
            "'tools/local jig' mcp --mode 'agent one'"
        );
        assert_eq!(output["mcp_command_source"], ".mcp.json");
        assert_eq!(output["mcp_command_error"], Value::Null);
    }

    #[test]
    fn reports_default_mcp_command_for_malformed_mcp_json() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        fs::write(temp.path().join(".mcp.json"), "{not json").unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info(&ctx);

        assert_eq!(output["mcp_command"], DEFAULT_MCP_COMMAND);
        assert_eq!(output["mcp_command_source"], "default");
        assert!(
            output["mcp_command_error"]
                .as_str()
                .unwrap()
                .contains("Failed to parse")
        );

        let summary = format_summary(&output);
        assert!(summary.contains("MCP command fallback: Failed to parse"));
    }

    #[test]
    fn reports_default_mcp_command_for_unreadable_mcp_json() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        fs::create_dir(temp.path().join(".mcp.json")).unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info(&ctx);

        assert_eq!(output["mcp_command"], DEFAULT_MCP_COMMAND);
        assert_eq!(output["mcp_command_source"], "default");
        assert!(
            output["mcp_command_error"]
                .as_str()
                .unwrap()
                .contains("Failed to read")
        );
    }

    #[test]
    fn reports_default_mcp_command_for_empty_mcp_command() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "jig": {
                        "command": "",
                        "args": ["mcp"]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info(&ctx);

        assert_eq!(output["mcp_command"], DEFAULT_MCP_COMMAND);
        assert_eq!(output["mcp_command_source"], "default");
        assert!(
            output["mcp_command_error"]
                .as_str()
                .unwrap()
                .contains("non-empty mcpServers.jig.command")
        );
    }

    #[test]
    fn reports_default_mcp_command_for_non_string_mcp_arg() {
        let temp = tempdir().unwrap();
        write_info_fixture(temp.path());
        fs::write(
            temp.path().join(".mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "jig": {
                        "command": "scripts/jig",
                        "args": [123, "mcp"]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let output = repo_info(&ctx);

        assert_eq!(output["mcp_command"], DEFAULT_MCP_COMMAND);
        assert!(
            output["mcp_command_error"]
                .as_str()
                .unwrap()
                .contains("args[0] must be a string")
        );
    }

    pub(super) fn write_info_fixture(root: &Path) {
        TestRepoBuilder::new(root)
            .config(
                r#"
sqlx_enabled = true
rust_migration_dir = "migrations"
rust_sqlx_metadata_dir = ".sqlx"
schema_dump_enabled = true
schema_dump_command = "printf schema"
bootstrap_command = "printf bootstrap"
rust_test_command = "cargo test"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[dev]
workspace_discovery = false

[[dev.apps]]
name = "web"
dir = "apps/web"
kind = "vite"
argv = ["npm", "run", "dev"]

[[work.gates]]
id = "tests"
kind = "check"
tool = "jig.test"

[agent_tooling.codex]
marketplaces = []
"#,
            )
            .required_commands([
                "bootstrap_command",
                "schema_dump_command",
                "rust_test_command",
            ])
            .tool(json!({
                "name": "jig.test",
                "kind": "command",
                "description": "Run tests.",
                "command": "rust_test_command"
            }))
            .tool(json!({
                "name": tool::BOOTSTRAP,
                "kind": "command",
                "description": "Bootstrap the repository.",
                "command": "bootstrap_command"
            }))
            .tool(json!({
                "name": tool::MIGRATION_ADD,
                "kind": "native",
                "description": "Add a migration."
            }))
            .tool(json!({
                "name": tool::SCHEMA_DUMP,
                "kind": "command",
                "description": "Dump the schema.",
                "command": "schema_dump_command"
            }))
            .write();
    }
}
