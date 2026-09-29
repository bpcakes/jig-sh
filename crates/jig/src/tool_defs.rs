use anyhow::{Result, anyhow};
use jig_contract::ManifestTool;
pub(crate) use jig_contract::{kind, tool};
use serde_json::{Map, Value, json};

mod repository;

pub(crate) use repository::{
    AgentRepositoryInspectOutput, AgentRepositoryInspectResult, CancelRunArgs, CancelRunOutput,
    ExecuteRunArgs, ExecuteRunOutput, PlanRunArgs, PlanRunOutput, RepositoryInspectArgs,
    RepositoryInspectOutput, RepositoryInspectResult, RepositoryTool, RunInspection,
};

pub(crate) mod args {
    pub(crate) const NAME: &str = "name";
    pub(crate) const PLAN_ID: &str = "plan_id";
}

pub(crate) mod cli_command {
    pub(crate) const ADOPT: &str = "adopt";
    pub(crate) const AGENT: &str = "agent";
    pub(crate) const AGENT_MAP: &str = "agent-map";
    pub(crate) const AGENT_MAP_GENERATE: &str = "generate";
    pub(crate) const AGENT_BOOTSTRAP: &str = "bootstrap";
    pub(crate) const AGENT_DOCTOR: &str = "doctor";
    // Top-level `jig bootstrap` and nested `jig agent bootstrap` intentionally
    // share the same parser label in different Clap command scopes.
    pub(crate) const BOOTSTRAP: &str = "bootstrap";
    pub(crate) const RUN: &str = "run";
    pub(crate) const CHECK: &str = "check";
    pub(crate) const CHECK_AGENT_MAP: &str = "agent-map";
    pub(crate) const CHECK_AGENT_GUIDES: &str = "agent-guides";
    pub(crate) const CHECK_CLIPPY: &str = "clippy";
    pub(crate) const CHECK_CONTRACT: &str = "contract";
    pub(crate) const CHECK_FMT: &str = "fmt";
    pub(crate) const CHECK_LINT: &str = "lint";
    pub(crate) const CHECK_MIGRATION_IMMUTABILITY: &str = "migration-immutability";
    pub(crate) const CHECK_SCHEMA: &str = "schema";
    pub(crate) const CHECK_SQLX: &str = "sqlx";
    pub(crate) const CHECK_SQLC: &str = "sqlc";
    pub(crate) const CHECK_SQLX_UNCHECKED_NON_TEST: &str = "sqlx-unchecked-non-test";
    pub(crate) const CHECK_TEST: &str = "test";
    pub(crate) const CHECK_TEST_LOCKED: &str = "test-locked";
    pub(crate) const CHECK_TYPESCRIPT_BUILD: &str = "typescript-build";
    pub(crate) const CHECK_TYPESCRIPT_COVERAGE: &str = "typescript-coverage";
    pub(crate) const CHECK_TYPESCRIPT_LINT: &str = "typescript-lint";
    pub(crate) const CHECK_TYPESCRIPT_TYPECHECK: &str = "typescript-typecheck";
    pub(crate) const CLAUDE: &str = "claude";
    pub(crate) const CODEX: &str = "codex";
    pub(crate) const CODEX_HOMES: &str = "homes";
    pub(crate) const CODEX_LAUNCH: &str = "launch";
    pub(crate) const CODEX_RESUME: &str = "resume";
    pub(crate) const DEV: &str = "dev";
    pub(crate) const DEV_STATUS: &str = "status";
    pub(crate) const DEV_RECOVER: &str = "recover";
    pub(crate) const DEV_STOP: &str = "stop";
    pub(crate) const DOCTOR: &str = "doctor";
    pub(crate) const FILE_BUDGET: &str = "file-budget";
    pub(crate) const GENERATE_SQLX_UNCHECKED_QUERIES_TODO: &str =
        "generate-sqlx-unchecked-queries-todo";
    pub(crate) const INFO: &str = "info";
    pub(crate) const INIT: &str = "init";
    pub(crate) const LOOP: &str = "loop";
    pub(crate) const LOOP_ACKNOWLEDGE_OCCURRENCE: &str = "acknowledge-occurrence";
    pub(crate) const LOOP_CLEAR_ATTEMPT: &str = "clear-attempt";
    pub(crate) const LOOP_DISPATCH: &str = "dispatch";
    pub(crate) const LOOP_RUN: &str = "run";
    pub(crate) const LOOP_SHOW: &str = "show";
    pub(crate) const LOOP_STATUS: &str = "status";
    pub(crate) const LOOP_TICK: &str = "tick";
    pub(crate) const MCP: &str = "mcp";
    pub(crate) const MIGRATION: &str = "migration";
    pub(crate) const MIGRATION_ADD_NESTED: &str = "add";
    pub(crate) const MIGRATION_ADD: &str = "migration-add";
    pub(crate) const PRESETS: &str = "presets";
    pub(crate) const PROXY: &str = "proxy";
    pub(crate) const PROXY_ALIAS: &str = "alias";
    pub(crate) const PROXY_CERT: &str = "cert";
    pub(crate) const PROXY_CERT_GENERATE: &str = "generate";
    pub(crate) const PROXY_CERT_STATUS: &str = "status";
    pub(crate) const PROXY_CERT_TRUST: &str = "trust";
    pub(crate) const PROXY_CERT_UNTRUST: &str = "untrust";
    pub(crate) const PROXY_LIST: &str = "list";
    pub(crate) const PROXY_PRUNE: &str = "prune";
    pub(crate) const PROXY_RUN: &str = "run";
    pub(crate) const PROXY_SERVICE: &str = "service";
    pub(crate) const PROXY_SERVICE_INSTALL: &str = "install";
    pub(crate) const PROXY_SERVICE_STATUS: &str = "status";
    pub(crate) const PROXY_SERVICE_UNINSTALL: &str = "uninstall";
    pub(crate) const PROXY_START: &str = "start";
    pub(crate) const PROXY_STOP: &str = "stop";
    pub(crate) const SCHEMA_DUMP: &str = "schema-dump";
    pub(crate) const SETUP: &str = "setup";
    pub(crate) const STATE: &str = "state";
    pub(crate) const STATE_ARCHIVE: &str = "archive";
    pub(crate) const STATE_DIAGNOSE: &str = "diagnose";
    pub(crate) const STATE_EXPORT: &str = "export";
    pub(crate) const STATE_RECEIPTS: &str = "receipts";
    pub(crate) const STATE_RESTORE: &str = "restore";
    pub(crate) const STATE_SUMMARY: &str = "summary";
    pub(crate) const STATUS: &str = "status";
    pub(crate) const SQLX: &str = "sqlx";
    pub(crate) const SQLX_MIGRATION: &str = "migration";
    pub(crate) const SQLX_MIGRATION_ADD: &str = "add";
    pub(crate) const SQLX_SCHEMA: &str = "schema";
    pub(crate) const SQLX_SCHEMA_DUMP: &str = "dump";
    pub(crate) const UI: &str = "ui";
    pub(crate) const UPDATE: &str = "update";
    pub(crate) const VAULT: &str = "vault";
    pub(crate) const VAULT_AUDIT: &str = "audit";
    pub(crate) const VAULT_AUDIT_VERIFY: &str = "verify";
    pub(crate) const VAULT_BACKUP: &str = "backup";
    pub(crate) const VAULT_BACKUP_CREATE: &str = "create";
    pub(crate) const VAULT_BACKUP_RESTORE: &str = "restore";
    pub(crate) const VAULT_FIELD: &str = "field";
    pub(crate) const VAULT_FIELD_LIST: &str = "list";
    pub(crate) const VAULT_FIELD_REMOVE: &str = "remove";
    pub(crate) const VAULT_FIELD_SET: &str = "set";
    pub(crate) const VAULT_EXEC: &str = "exec";
    pub(crate) const VAULT_IMPORT: &str = "import";
    pub(crate) const VAULT_IMPORT_ONEPASSWORD: &str = "onepassword";
    pub(crate) const VAULT_INIT: &str = "init";
    pub(crate) const VAULT_INJECT: &str = "inject";
    pub(crate) const VAULT_MIGRATE: &str = "migrate";
    pub(crate) const VAULT_PASSPHRASE: &str = "passphrase";
    pub(crate) const VAULT_PASSPHRASE_CHANGE: &str = "change";
    pub(crate) const VAULT_READ: &str = "read";
    pub(crate) const VAULT_RUN: &str = "run";
    pub(crate) const VAULT_SECRET: &str = "secret";
    pub(crate) const VAULT_SECRET_LIST: &str = "list";
    pub(crate) const VAULT_SECRET_REMOVE: &str = "remove";
    pub(crate) const VAULT_SECRET_SET: &str = "set";
    pub(crate) const VAULT_STATUS: &str = "status";
    pub(crate) const VAULT_TUI: &str = "tui";
    pub(crate) const WORK: &str = "work";
}

pub(crate) type JsonObject = Map<String, Value>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryTool {
    AgentDoctor,
}

impl MemoryTool {
    const ALL: &'static [Self] = &[Self::AgentDoctor];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            tool::AGENT_DOCTOR => Some(Self::AgentDoctor),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::AgentDoctor => tool::AGENT_DOCTOR,
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::AgentDoctor => "Report local Codex agent tooling status for this repo.",
        }
    }

    fn input_schema(self) -> Value {
        match self {
            Self::AgentDoctor => empty_input_schema(),
        }
    }
}

#[cfg(test)]
pub(crate) fn tool_descriptors(
    contract_version: u32,
    manifest_tools: &[ManifestTool],
) -> Vec<Value> {
    tool_descriptors_for_surface(
        contract_version,
        manifest_tools,
        crate::surface::ResponseSurface::Standard,
    )
}

pub(crate) fn tool_descriptors_for_surface(
    contract_version: u32,
    manifest_tools: &[ManifestTool],
    surface: crate::surface::ResponseSurface,
) -> Vec<Value> {
    let execution = if contract_version >= 6 {
        RepositoryTool::ALL
            .iter()
            .copied()
            .map(|tool| tool.descriptor_for_surface(surface))
            .collect::<Vec<_>>()
    } else {
        manifest_tools
            .iter()
            .filter(|tool| is_execution_tool(tool))
            .map(manifest_tool_descriptor)
            .collect()
    };
    execution
        .into_iter()
        .chain(MemoryTool::ALL.iter().copied().map(memory_tool_descriptor))
        .collect()
}

fn manifest_tool_descriptor(tool: &ManifestTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": execution_input_schema(tool)
    })
}

fn memory_tool_descriptor(tool: MemoryTool) -> Value {
    json!({
        "name": tool.name(),
        "description": tool.description(),
        "inputSchema": tool.input_schema()
    })
}

pub(crate) fn is_command_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::COMMAND
}

pub(crate) fn is_native_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::NATIVE
}

pub(crate) fn is_execution_tool(tool: &ManifestTool) -> bool {
    is_command_tool(tool) || is_native_tool(tool)
}

pub(crate) fn is_no_arg_execution_tool(tool: &ManifestTool) -> bool {
    is_execution_tool(tool) && !execution_tool_requires_name(tool)
}

pub(crate) fn execution_tool_args(tool: &ManifestTool, args_obj: &JsonObject) -> Result<Value> {
    if execution_tool_requires_name(tool) {
        let name = required_string_arg(args_obj, args::NAME)?;
        return Ok(object_value([(args::NAME, Value::String(name))]));
    }

    Ok(json!({}))
}

pub(crate) fn execution_tool_requires_name(tool: &ManifestTool) -> bool {
    jig_features::native_tool_requires_name(&tool.name)
}

pub(crate) fn execution_tool_requires_name_for_native_operation(
    tool: &ManifestTool,
    native_operation: Option<&str>,
) -> bool {
    native_operation.map_or_else(
        || execution_tool_requires_name(tool),
        jig_features::native_tool_requires_name,
    )
}

fn execution_input_schema(tool: &ManifestTool) -> Value {
    if execution_tool_requires_name(tool) {
        return object_schema(
            &[
                (args::NAME, string_schema()),
                (args::PLAN_ID, string_schema()),
            ],
            &[args::NAME],
        );
    }

    object_schema(&[(args::PLAN_ID, string_schema())], &[])
}

fn empty_input_schema() -> Value {
    object_schema(&[], &[])
}

fn object_schema(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let mut schema = JsonObject::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert(
        "properties".into(),
        object_value(properties.iter().cloned()),
    );
    if !required.is_empty() {
        schema.insert(
            "required".into(),
            Value::Array(
                required
                    .iter()
                    .map(|required| Value::String((*required).into()))
                    .collect(),
            ),
        );
    }
    schema.insert("additionalProperties".into(), Value::Bool(false));
    Value::Object(schema)
}

fn object_value<'a>(entries: impl IntoIterator<Item = (&'a str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn string_schema() -> Value {
    json!({ "type": "string" })
}

pub(crate) fn required_string_arg(map: &JsonObject, key: &str) -> Result<String> {
    string_arg(map, key).ok_or_else(|| anyhow!("Missing required argument: {key}"))
}

pub(crate) fn string_arg(map: &JsonObject, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use jig_contract::ManifestTool;

    use super::*;

    #[test]
    fn memory_tool_names_are_unique_and_complete() {
        let names = MemoryTool::ALL
            .iter()
            .map(|tool| tool.name())
            .collect::<Vec<_>>();
        let unique = names.iter().copied().collect::<BTreeSet<_>>();

        assert_eq!(names.len(), MemoryTool::ALL.len());
        assert_eq!(unique.len(), names.len());
        assert_eq!(names, [tool::AGENT_DOCTOR]);
    }

    #[test]
    fn no_arg_execution_tool_excludes_argument_taking_native_tools() {
        let command =
            ManifestTool::new("jig.test", kind::COMMAND, "Test.").with_command("rust_test_command");
        let contract = ManifestTool::new(tool::CONTRACT_CHECK, kind::NATIVE, "Contract.");
        let migration = ManifestTool::new(tool::MIGRATION_ADD, kind::NATIVE, "Migration.");
        let unsupported = ManifestTool::new("jig.memory", "memory", "Memory.");

        assert!(is_no_arg_execution_tool(&command));
        assert!(is_no_arg_execution_tool(&contract));
        assert!(!is_no_arg_execution_tool(&migration));
        assert!(!is_no_arg_execution_tool(&unsupported));
    }
}
