use jig_contract::ManifestTool;
pub use jig_contract::{kind, tool};

pub mod args {
    pub const NAME: &str = "name";
}

/// Nested subcommand names. Top-level command names live in
/// [`crate::root_commands`].
pub mod cli_command {
    pub const AGENT_MAP_GENERATE: &str = "generate";
    pub const AGENT_BOOTSTRAP: &str = "bootstrap";
    pub const AGENT_DOCTOR: &str = "doctor";
    pub const CHECK_AGENT_MAP: &str = "agent-map";
    pub const CHECK_AGENT_GUIDES: &str = "agent-guides";
    pub const CHECK_CLIPPY: &str = "clippy";
    pub const CHECK_CONTRACT: &str = "contract";
    pub const CHECK_FMT: &str = "fmt";
    pub const CHECK_LINT: &str = "lint";
    pub const CHECK_MIGRATION_IMMUTABILITY: &str = "migration-immutability";
    pub const CHECK_SCHEMA: &str = "schema";
    pub const CHECK_SQLX: &str = "sqlx";
    pub const CHECK_SQLC: &str = "sqlc";
    pub const CHECK_SQLX_UNCHECKED_NON_TEST: &str = "sqlx-unchecked-non-test";
    pub const CHECK_TEST: &str = "test";
    pub const CHECK_TEST_LOCKED: &str = "test-locked";
    pub const CHECK_TYPESCRIPT_BUILD: &str = "typescript-build";
    pub const CHECK_TYPESCRIPT_COVERAGE: &str = "typescript-coverage";
    pub const CHECK_TYPESCRIPT_LINT: &str = "typescript-lint";
    pub const CHECK_TYPESCRIPT_TYPECHECK: &str = "typescript-typecheck";
    pub const CODEX_HOMES: &str = "homes";
    pub const CODEX_LAUNCH: &str = "launch";
    pub const CODEX_RESUME: &str = "resume";
    pub const DEV_STATUS: &str = "status";
    pub const DEV_RECOVER: &str = "recover";
    pub const DEV_STOP: &str = "stop";
    pub const LOOP_ACKNOWLEDGE_OCCURRENCE: &str = "acknowledge-occurrence";
    pub const LOOP_CLEAR_ATTEMPT: &str = "clear-attempt";
    pub const LOOP_DISPATCH: &str = "dispatch";
    pub const LOOP_RUN: &str = "run";
    pub const LOOP_SHOW: &str = "show";
    pub const LOOP_STATUS: &str = "status";
    pub const LOOP_TICK: &str = "tick";
    pub const MIGRATION_ADD_NESTED: &str = "add";
    pub const PROXY_ALIAS: &str = "alias";
    pub const PROXY_CERT: &str = "cert";
    pub const PROXY_CERT_GENERATE: &str = "generate";
    pub const PROXY_CERT_STATUS: &str = "status";
    pub const PROXY_CERT_TRUST: &str = "trust";
    pub const PROXY_CERT_UNTRUST: &str = "untrust";
    pub const PROXY_LIST: &str = "list";
    pub const PROXY_PRUNE: &str = "prune";
    pub const PROXY_RUN: &str = "run";
    pub const PROXY_SERVICE: &str = "service";
    pub const PROXY_SERVICE_INSTALL: &str = "install";
    pub const PROXY_SERVICE_STATUS: &str = "status";
    pub const PROXY_SERVICE_UNINSTALL: &str = "uninstall";
    pub const PROXY_START: &str = "start";
    pub const PROXY_STOP: &str = "stop";
    pub const STATE_ARCHIVE: &str = "archive";
    pub const STATE_DIAGNOSE: &str = "diagnose";
    pub const STATE_RESTORE: &str = "restore";
    pub const STATE_SUMMARY: &str = "summary";
    pub const SQLX_MIGRATION: &str = "migration";
    pub const SQLX_MIGRATION_ADD: &str = "add";
    pub const SQLX_SCHEMA: &str = "schema";
    pub const SQLX_SCHEMA_DUMP: &str = "dump";
    pub const VAULT_AUDIT: &str = "audit";
    pub const VAULT_AUDIT_VERIFY: &str = "verify";
    pub const VAULT_BACKUP: &str = "backup";
    pub const VAULT_BACKUP_CREATE: &str = "create";
    pub const VAULT_BACKUP_RESTORE: &str = "restore";
    pub const VAULT_FIELD: &str = "field";
    pub const VAULT_FIELD_LIST: &str = "list";
    pub const VAULT_FIELD_REMOVE: &str = "remove";
    pub const VAULT_FIELD_SET: &str = "set";
    pub const VAULT_EXEC: &str = "exec";
    pub const VAULT_IMPORT: &str = "import";
    pub const VAULT_IMPORT_ONEPASSWORD: &str = "onepassword";
    pub const VAULT_INIT: &str = "init";
    pub const VAULT_INJECT: &str = "inject";
    pub const VAULT_MIGRATE: &str = "migrate";
    pub const VAULT_PASSPHRASE: &str = "passphrase";
    pub const VAULT_PASSPHRASE_CHANGE: &str = "change";
    pub const VAULT_READ: &str = "read";
    pub const VAULT_RUN: &str = "run";
    pub const VAULT_SECRET: &str = "secret";
    pub const VAULT_SECRET_LIST: &str = "list";
    pub const VAULT_SECRET_REMOVE: &str = "remove";
    pub const VAULT_SECRET_SET: &str = "set";
    pub const VAULT_STATUS: &str = "status";
    pub const VAULT_TUI: &str = "tui";
}

pub fn is_command_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::COMMAND
}

pub fn is_native_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::NATIVE
}

pub fn is_execution_tool(tool: &ManifestTool) -> bool {
    is_command_tool(tool) || is_native_tool(tool)
}

pub fn execution_tool_requires_name(tool: &ManifestTool) -> bool {
    jig_features::native_tool_requires_name(&tool.name)
}

pub fn execution_tool_requires_name_for_native_operation(
    tool: &ManifestTool,
    native_operation: Option<&str>,
) -> bool {
    native_operation.map_or_else(
        || execution_tool_requires_name(tool),
        jig_features::native_tool_requires_name,
    )
}
