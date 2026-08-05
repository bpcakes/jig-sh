use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::tool;

use super::VaultCapability;

mod reason;
use reason::ReasonCode;

pub(super) const COMMAND: &str = "info commands";
const SCHEMA_VERSION: u64 = 1;
const REPO_CONTEXT_NEXT_STEP: &str = "Run `jig doctor`; for an unadopted repository, preview with `jig adopt .` and apply with `jig adopt . --write`.";
pub(super) const INVALID_OVERRIDE_NEXT_STEP: &str =
    "Unset `JIG_REPO_ROOT` or point it to a valid adopted Jig repository, then rerun the command.";

pub(super) enum ContextFallback {
    Tolerant {
        context_status: RepoContextStatus,
        dev: Value,
        vault: VaultCapability,
        jig: String,
        dev_proxy_available: bool,
    },
    Invalid {
        invalid_override: bool,
    },
}

#[derive(Clone, Copy)]
pub(super) enum RepoContextStatus {
    Valid,
    Absent,
    Invalid,
    Recovered,
}

impl RepoContextStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Absent => "absent",
            Self::Invalid => "invalid",
            Self::Recovered => "recovered",
        }
    }
}

#[cfg(test)]
pub(crate) const DISCOVERABLE_COMMAND_NAMES: &[&str] = &[
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
    "migration-add",
    "schema-dump",
    "vault",
    "proxy",
    "prompt",
    "agent",
    "codex",
    "agent-map",
    "state",
    "mcp",
];

pub(super) fn format_summary(value: &Value) -> String {
    let context_status = value["repo"]["context_status"]
        .as_str()
        .unwrap_or("invalid");
    let repo_name = value["repo"]["name"]
        .as_str()
        .unwrap_or(match context_status {
            "absent" => "no adopted repository",
            "recovered" => "recovered repository context",
            "invalid" => "invalid repository context",
            _ => "adopted repository",
        });
    let Some(commands) = value["commands"].as_array() else {
        return format!("Jig command availability: {repo_name}\n  No command data available.");
    };
    let name_width = commands
        .iter()
        .filter_map(|command| command["name"].as_str().map(str::len))
        .max()
        .unwrap_or_default();
    let status_width = commands
        .iter()
        .filter_map(|command| command["status"].as_str())
        .map(command_status_label)
        .map(str::len)
        .max()
        .unwrap_or_default();
    let mut lines = vec![format!("Jig command availability: {repo_name}")];
    if let Some(context_error) = value["repo"]["context_error"].as_str() {
        lines.push(format!("Context ({context_status}): {context_error}"));
    }
    let mut current_category = None;

    for command in commands {
        let category = command["category"].as_str().unwrap_or("other");
        if current_category != Some(category) {
            lines.push(String::new());
            lines.push(format!("{}:", command_category_label(category)));
            current_category = Some(category);
        }

        let name = command["name"].as_str().unwrap_or("<unknown>");
        let status = command["status"].as_str().unwrap_or("unavailable");
        let reason = command["reason"].as_str().unwrap_or("");
        let status = command_status_label(status);
        if reason.is_empty() {
            lines.push(format!("  {name:<name_width$}  {status}"));
        } else {
            lines.push(format!(
                "  {name:<name_width$}  {status:<status_width$}  {reason}"
            ));
        }
        if let Some(next_step) = command["next_step"].as_str() {
            lines.push(format!("  {:name_width$}  Next: {next_step}", ""));
        }
    }

    lines.push(String::new());
    lines.push(
        "Status describes each root command's primary workflow; setup and diagnostic subcommands or flags may still work. Command-specific preflight still runs on invocation."
            .into(),
    );

    lines.join("\n")
}

pub(super) fn info_with_capabilities(
    ctx: &RepoContext,
    vault: VaultCapability,
    agent: &Value,
) -> Value {
    let jig = command_prefix(ctx);
    let mut commands = vec![
        ready_command("init", "get_started"),
        ready_command("presets", "get_started"),
        ready_command("adopt", "get_started"),
        ready_command("update", "get_started"),
        manifest_command(
            ctx,
            "bootstrap",
            "get_started",
            tool::BOOTSTRAP,
            (
                ReasonCode::BootstrapToolMissing,
                "The bootstrap tool is missing from the generated contract.",
            ),
            (
                ReasonCode::BootstrapToolInvalid,
                "The bootstrap tool declaration or configured command is invalid.",
            ),
        ),
        manifest_command(
            ctx,
            "setup",
            "get_started",
            tool::BOOTSTRAP,
            (
                ReasonCode::BootstrapToolMissing,
                "Setup cannot prepare project dependencies because the bootstrap tool is missing from the generated contract.",
            ),
            (
                ReasonCode::BootstrapToolInvalid,
                "Setup cannot prepare project dependencies because the bootstrap tool declaration or configured command is invalid.",
            ),
        ),
        ready_command("doctor", "get_started"),
        ready_command("info", "get_started"),
    ];

    commands.push(dev_command(ctx));
    commands.extend([
        ready_command("check", "develop"),
        ready_command("status", "develop"),
        ready_command("ui", "develop"),
        ready_command("work", "structured_work"),
    ]);
    // `noop-status` is built in and remains available when no custom workflow
    // is configured or every configured custom workflow is disabled.
    commands.push(ready_command("loop", "structured_work"));

    commands.push(migration_add_command(ctx));
    commands.push(schema_dump_command(ctx));
    commands.push(vault_command(vault, &jig));
    commands.push(proxy_command(Some(ctx)));
    commands.push(ready_command("prompt", "agent_automation"));
    commands.push(agent_command(agent, &jig));
    commands.extend([
        ready_command("codex", "agent_automation"),
        ready_command("agent-map", "agent_automation"),
        ready_command("state", "agent_automation"),
    ]);
    // `.mcp.json` registers clients; the root stdio server does not read it.
    commands.push(ready_command("mcp", "agent_automation"));

    json!({
        "ok": true,
        "command": COMMAND,
        "schema_version": SCHEMA_VERSION,
        "repo": {
            "context_status": RepoContextStatus::Valid.as_str(),
            "name": ctx.repo_name(),
            "root": ctx.root().display().to_string(),
            "context_error": null,
        },
        "commands": commands,
    })
}

pub(super) fn info_without_context(context_error: &str, fallback: ContextFallback) -> Value {
    let (context_status, dev, vault, prompt, repo_context_next_step, dev_proxy_available) =
        match fallback {
            ContextFallback::Tolerant {
                context_status,
                dev,
                vault,
                jig,
                dev_proxy_available,
            } => (
                context_status,
                dev,
                vault_command(vault, &jig),
                ready_command("prompt", "agent_automation"),
                match context_status {
                    RepoContextStatus::Invalid | RepoContextStatus::Recovered => {
                        INVALID_OVERRIDE_NEXT_STEP
                    }
                    RepoContextStatus::Valid | RepoContextStatus::Absent => REPO_CONTEXT_NEXT_STEP,
                },
                dev_proxy_available,
            ),
            ContextFallback::Invalid { invalid_override } => {
                let next_step = if invalid_override {
                    INVALID_OVERRIDE_NEXT_STEP
                } else {
                    REPO_CONTEXT_NEXT_STEP
                };
                (
                    RepoContextStatus::Invalid,
                    dev_without_context_command_with_next_step(next_step),
                    repo_context_command_with_next_step("vault", "project_data", next_step),
                    repo_context_command_with_next_step("prompt", "agent_automation", next_step),
                    next_step,
                    dev_proxy_available(None),
                )
            }
        };
    let commands = vec![
        ready_command("init", "get_started"),
        ready_command("presets", "get_started"),
        ready_command("adopt", "get_started"),
        repo_context_command_with_next_step("update", "get_started", repo_context_next_step),
        repo_context_command_with_next_step("bootstrap", "get_started", repo_context_next_step),
        repo_context_command_with_next_step("setup", "get_started", repo_context_next_step),
        ready_command("doctor", "get_started"),
        info_without_context_command(repo_context_next_step),
        dev,
        repo_context_command_with_next_step("check", "develop", repo_context_next_step),
        repo_context_command_with_next_step("status", "develop", repo_context_next_step),
        repo_context_command_with_next_step("ui", "develop", repo_context_next_step),
        repo_context_command_with_next_step("work", "structured_work", repo_context_next_step),
        repo_context_command_with_next_step("loop", "structured_work", repo_context_next_step),
        repo_context_command_with_next_step(
            "migration-add",
            "project_data",
            repo_context_next_step,
        ),
        repo_context_command_with_next_step("schema-dump", "project_data", repo_context_next_step),
        vault,
        proxy_without_valid_context_command(repo_context_next_step, dev_proxy_available),
        prompt,
        repo_context_command_with_next_step("agent", "agent_automation", repo_context_next_step),
        ready_command("codex", "agent_automation"),
        repo_context_command_with_next_step(
            "agent-map",
            "agent_automation",
            repo_context_next_step,
        ),
        repo_context_command_with_next_step("state", "agent_automation", repo_context_next_step),
        repo_context_command_with_next_step("mcp", "agent_automation", repo_context_next_step),
    ];

    json!({
        "ok": true,
        "command": COMMAND,
        "schema_version": SCHEMA_VERSION,
        "repo": {
            "context_status": context_status.as_str(),
            "name": null,
            "root": null,
            "context_error": context_error,
        },
        "commands": commands,
    })
}

fn info_without_context_command(next_step: &'static str) -> Value {
    command_value(
        "info",
        "get_started",
        "needs_setup",
        Some(ReasonCode::RepoContextUnavailable),
        Some(
            "The default repository snapshot requires a valid adopted Jig repository; `info --commands` remains available.",
        ),
        Some(next_step),
    )
}

fn repo_context_command_with_next_step(
    name: &'static str,
    category: &'static str,
    next_step: &'static str,
) -> Value {
    command_value(
        name,
        category,
        "needs_setup",
        Some(ReasonCode::RepoContextUnavailable),
        Some("This command's primary workflow requires a valid adopted Jig repository."),
        Some(next_step),
    )
}

fn dev_command(ctx: &RepoContext) -> Value {
    if let Some(command) = dev_feature_unavailable_command(dev_proxy_available(Some(ctx))) {
        return command;
    }
    if !crate::doctor::proxy_configured(ctx) {
        let adopt = adopt_command(ctx);
        let next_step = format!(
            "Configure [[dev.apps]] in .jig.toml, or preview re-adoption with `{adopt}` and, after reviewing the preview, apply it with `{adopt} --force --write`."
        );
        return not_configured_command(
            "dev",
            "develop",
            ReasonCode::DevAppsNotConfigured,
            "No development apps or workspace discovery are configured for the default dev launch.",
            Some(&next_step),
        );
    }
    ready_command("dev", "develop")
}

pub(super) fn dev_capability(ctx: Option<&RepoContext>) -> Value {
    dev_capability_with_next_step(ctx, REPO_CONTEXT_NEXT_STEP)
}

pub(super) fn dev_capability_with_next_step(
    ctx: Option<&RepoContext>,
    next_step: &'static str,
) -> Value {
    ctx.map_or_else(
        || dev_without_context_command_with_next_step(next_step),
        dev_command,
    )
}

fn dev_without_context_command_with_next_step(next_step: &'static str) -> Value {
    dev_feature_unavailable_command(dev_proxy_available(None))
        .unwrap_or_else(|| repo_context_command_with_next_step("dev", "develop", next_step))
}

fn dev_feature_unavailable_command(available: bool) -> Option<Value> {
    (!available).then(|| {
        command_value(
            "dev",
            "develop",
            "unavailable",
            Some(ReasonCode::DevProxyFeatureNotBuilt),
            Some("This Jig binary was built without development app support."),
            Some("Install a Jig build with the dev-proxy feature."),
        )
    })
}

fn migration_add_command(ctx: &RepoContext) -> Value {
    let adopt = adopt_command(ctx);
    if !ctx.sqlx_enabled() {
        let next_step = format!(
            "Preview with `{adopt} --sqlx-enabled true --rust-migration-dir migrations`, then, after reviewing the preview, apply with `{adopt} --sqlx-enabled true --rust-migration-dir migrations --force --write`."
        );
        return not_configured_command(
            "migration-add",
            "project_data",
            ReasonCode::SqlxDisabled,
            "SQLx is disabled for this repository.",
            Some(&next_step),
        );
    }
    if ctx.rust_migration_dir().trim().is_empty() {
        let next_step = format!(
            "Preview with `{adopt} --rust-migration-dir migrations`, then, after reviewing the preview, apply with `{adopt} --rust-migration-dir migrations --force --write`."
        );
        return command_value(
            "migration-add",
            "project_data",
            "needs_setup",
            Some(ReasonCode::MigrationDirectoryNotConfigured),
            Some("Migration workflows require a non-empty rust_migration_dir."),
            Some(&next_step),
        );
    }
    manifest_command(
        ctx,
        "migration-add",
        "project_data",
        tool::MIGRATION_ADD,
        (
            ReasonCode::MigrationAddToolMissing,
            "The migration tool is missing from the generated contract.",
        ),
        (
            ReasonCode::MigrationAddToolInvalid,
            "The migration tool declaration is invalid.",
        ),
    )
}

fn schema_dump_command(ctx: &RepoContext) -> Value {
    let adopt = adopt_command(ctx);
    if !ctx.sqlx_enabled() {
        let next_step = format!(
            "Preview with `{adopt} --sqlx-enabled true --rust-migration-dir migrations --schema-dump-enabled true`, then, after reviewing the preview, apply with `{adopt} --sqlx-enabled true --rust-migration-dir migrations --schema-dump-enabled true --force --write`."
        );
        return not_configured_command(
            "schema-dump",
            "project_data",
            ReasonCode::SqlxDisabled,
            "SQLx is disabled for this repository.",
            Some(&next_step),
        );
    }
    if ctx.rust_migration_dir().trim().is_empty() {
        let schema_dump_flag = if ctx.schema_dump_enabled() {
            ""
        } else {
            " --schema-dump-enabled true"
        };
        let next_step = format!(
            "Preview with `{adopt} --rust-migration-dir migrations{schema_dump_flag}`, then, after reviewing the preview, apply with `{adopt} --rust-migration-dir migrations{schema_dump_flag} --force --write`."
        );
        return command_value(
            "schema-dump",
            "project_data",
            "needs_setup",
            Some(ReasonCode::MigrationDirectoryNotConfigured),
            Some("SQLx workflows require a non-empty rust_migration_dir."),
            Some(&next_step),
        );
    }
    if !ctx.schema_dump_enabled() {
        let next_step = format!(
            "Preview with `{adopt} --schema-dump-enabled true`, then, after reviewing the preview, apply with `{adopt} --schema-dump-enabled true --force --write`."
        );
        return not_configured_command(
            "schema-dump",
            "project_data",
            ReasonCode::SchemaDumpsDisabled,
            "Schema dumps are disabled for this repository.",
            Some(&next_step),
        );
    }
    manifest_command(
        ctx,
        "schema-dump",
        "project_data",
        tool::SCHEMA_DUMP,
        (
            ReasonCode::SchemaDumpToolMissing,
            "The schema dump tool is missing from the generated contract.",
        ),
        (
            ReasonCode::SchemaDumpToolInvalid,
            "The schema dump tool declaration or configured command is invalid.",
        ),
    )
}

fn manifest_command(
    ctx: &RepoContext,
    name: &'static str,
    category: &'static str,
    tool_name: &str,
    missing: (ReasonCode, &'static str),
    invalid: (ReasonCode, &'static str),
) -> Value {
    let next_step = format!(
        "Run `{} check contract` and regenerate the Jig harness contract.",
        command_prefix(ctx)
    );
    let Some(tool) = ctx.tool_spec(tool_name) else {
        return command_value(
            name,
            category,
            "needs_setup",
            Some(missing.0),
            Some(missing.1),
            Some(&next_step),
        );
    };
    let valid = match tool.kind.as_str() {
        jig_contract::kind::NATIVE => jig_features::is_supported_native_tool(tool_name),
        jig_contract::kind::COMMAND => tool
            .command
            .as_deref()
            .is_some_and(|command_key| ctx.command_for_key(command_key).is_ok()),
        _ => false,
    };
    if !valid {
        return command_value(
            name,
            category,
            "needs_setup",
            Some(invalid.0),
            Some(invalid.1),
            Some(&next_step),
        );
    }
    ready_command(name, category)
}

fn vault_command(vault: VaultCapability, jig: &str) -> Value {
    if !vault.available {
        let next_step = format!("Run `{jig} vault status` for details.");
        return command_value(
            "vault",
            "project_data",
            "unavailable",
            Some(ReasonCode::VaultStatusUnavailable),
            Some("Vault availability could not be determined."),
            Some(&next_step),
        );
    }
    if !vault.initialized {
        let next_step = format!("Run `{jig} vault init`.");
        return command_value(
            "vault",
            "project_data",
            "needs_setup",
            Some(ReasonCode::VaultNotInitialized),
            Some("The configured vault has not been initialized."),
            Some(&next_step),
        );
    }
    ready_command("vault", "project_data")
}

fn proxy_command(ctx: Option<&RepoContext>) -> Value {
    if dev_proxy_available(ctx) {
        // The proxy family includes configuration-free commands such as
        // `proxy run`, `proxy alias`, certificate management, and diagnostics.
        // Doctor separately reports whether this repo has configured dev apps.
        ready_command("proxy", "local_services")
    } else {
        command_value(
            "proxy",
            "local_services",
            "unavailable",
            Some(ReasonCode::DevProxyFeatureNotBuilt),
            Some("This Jig binary was built without local proxy support."),
            Some("Install a Jig build with the dev-proxy feature."),
        )
    }
}

fn proxy_without_valid_context_command(next_step: &'static str, available: bool) -> Value {
    if available {
        repo_context_command_with_next_step("proxy", "local_services", next_step)
    } else {
        proxy_command(None)
    }
}

fn agent_command(agent: &Value, jig: &str) -> Value {
    if agent["ok"].as_bool().unwrap_or(false) {
        return ready_command("agent", "agent_automation");
    }

    let (status, reason_code, reason) = if !agent["codex"]["probe_error"].is_null() {
        (
            "unavailable",
            ReasonCode::AgentReadinessUnknown,
            "Codex marketplace readiness could not be determined.",
        )
    } else if agent["codex"]["available"].as_bool() == Some(false) {
        (
            "unavailable",
            ReasonCode::CodexMarketplaceSupportUnavailable,
            "The configured Codex binary does not support plugin marketplaces.",
        )
    } else {
        (
            "needs_setup",
            ReasonCode::CodexMarketplaceUnregistered,
            "One or more required Codex marketplaces are not registered.",
        )
    };
    let next_step = agent["next_steps"]
        .as_array()
        .and_then(|steps| steps.first())
        .and_then(Value::as_str)
        .unwrap_or("Run `jig agent doctor` for details.")
        .replace("`scripts/jig", &format!("`{jig}"));
    command_value(
        "agent",
        "agent_automation",
        status,
        Some(reason_code),
        Some(reason),
        Some(&next_step),
    )
}

fn ready_command(name: &'static str, category: &'static str) -> Value {
    command_value(name, category, "ready", None, None, None)
}

fn not_configured_command(
    name: &'static str,
    category: &'static str,
    reason_code: ReasonCode,
    reason: &'static str,
    next_step: Option<&str>,
) -> Value {
    command_value(
        name,
        category,
        "not_configured",
        Some(reason_code),
        Some(reason),
        next_step,
    )
}

fn command_value(
    name: &str,
    category: &str,
    status: &str,
    reason_code: Option<ReasonCode>,
    reason: Option<&str>,
    next_step: Option<&str>,
) -> Value {
    let reason_code = reason_code.map(ReasonCode::as_str);
    json!({
        "name": name,
        "category": category,
        "status": status,
        "reason_code": reason_code,
        "reason": reason,
        "next_step": next_step,
    })
}

fn command_category_label(category: &str) -> &'static str {
    match category {
        "get_started" => "Get started",
        "develop" => "Develop",
        "structured_work" => "Structured work",
        "project_data" => "Project data",
        "local_services" => "Local services",
        "agent_automation" => "Agent and automation",
        _ => "Other",
    }
}

fn command_status_label(status: &str) -> &'static str {
    match status {
        "ready" => "ready",
        "not_configured" => "not configured",
        "needs_setup" => "needs setup",
        "unavailable" => "unavailable",
        _ => "unavailable",
    }
}

pub(super) fn command_prefix(ctx: &RepoContext) -> String {
    if repo_launcher_available(ctx) {
        crate::shell::quote(&ctx.root().join("scripts/jig").display().to_string())
    } else {
        "jig".into()
    }
}

pub(super) fn dev_proxy_available(ctx: Option<&RepoContext>) -> bool {
    cfg!(feature = "dev-proxy") || ctx.is_some_and(repo_launcher_available)
}

fn repo_launcher_available(ctx: &RepoContext) -> bool {
    !ctx.is_minimal_footprint()
        && is_executable_file(&ctx.root().join("scripts/jig"))
        && is_executable_file(&ctx.root().join("scripts/install-jig.sh"))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn adopt_command(ctx: &RepoContext) -> String {
    let destination = crate::shell::quote(&ctx.root().display().to_string());
    let mut command = format!("{} adopt {destination}", command_prefix(ctx));
    let stored_source = ctx.source_path().trim();
    let local_source = ctx.template_local_path().trim();
    let local_source = (!local_source.is_empty()).then(|| {
        let path = std::path::PathBuf::from(local_source);
        if path.is_absolute() {
            path
        } else {
            ctx.root().join(path)
        }
    });
    let use_local_source = ctx.template_mode().trim() == "committed"
        && local_source.as_ref().is_some_and(|path| path.is_dir());
    let source = if use_local_source {
        local_source.as_ref().unwrap().display().to_string()
    } else {
        stored_source.to_string()
    };
    if !source.is_empty() {
        command.push_str(" --template ");
        command.push_str(&crate::shell::quote(&source));
        let is_remote =
            source.contains("://") || (source.starts_with("git@") && source.contains(':'));
        let is_embedded = source.starts_with("embedded:");
        if ctx.template_mode().trim() == "committed" && !is_remote && !is_embedded {
            command.push_str(" --template-mode committed");
        }
        let commit = ctx.source_commit().trim();
        if !commit.is_empty() && !is_embedded {
            command.push_str(" --vcs-ref ");
            command.push_str(&crate::shell::quote(commit));
        }
        if use_local_source && !stored_source.is_empty() && stored_source != source {
            command.push_str(" --template-source-url ");
            command.push_str(&crate::shell::quote(stored_source));
        }
    }
    if ctx.is_minimal_footprint() {
        command.push_str(" --minimal");
    }
    command
}

#[cfg(test)]
mod tests;
