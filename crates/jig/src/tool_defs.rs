use jig_contract::ManifestTool;
pub(crate) use jig_contract::{kind, tool};

pub(crate) mod args {
    pub(crate) const NAME: &str = "name";
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
