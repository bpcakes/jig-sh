use anyhow::{Result, anyhow};
use jig_contract::ManifestTool;
pub(crate) use jig_contract::{kind, tool};
use serde_json::{Map, Value, json};

mod repository;

pub(crate) use repository::{
    CancelRunArgs, CancelRunOutput, ExecuteRunArgs, ExecuteRunOutput, PlanRunArgs, PlanRunOutput,
    RepositoryInspectArgs, RepositoryInspectOutput, RepositoryInspectResult, RepositoryTool,
    RunInspection,
};

pub(crate) const DEFAULT_RECEIPTS_LIMIT: usize = 20;
pub(crate) const LOOP_CLEAR_ATTEMPT_TOOL: &str = "jig.loop_clear_attempt";
pub(crate) const LOOP_TICK_TOOL: &str = "jig.loop_tick";
pub(crate) const WORKER_RUN_TOOL: &str = "jig.worker_run";

pub(crate) mod args {
    pub(crate) const ALTERNATIVES: &str = "alternatives";
    pub(crate) const BODY: &str = "body";
    pub(crate) const BODY_FILE: &str = "body_file";
    pub(crate) const FAILED_ONLY: &str = "failed_only";
    pub(crate) const LIMIT: &str = "limit";
    pub(crate) const NAME: &str = "name";
    pub(crate) const NOTES: &str = "notes";
    pub(crate) const OPERATION: &str = "operation";
    pub(crate) const OUTCOME: &str = "outcome";
    pub(crate) const PLAN_ID: &str = "plan_id";
    pub(crate) const RATIONALE: &str = "rationale";
    pub(crate) const RESOLUTION: &str = "resolution";
    pub(crate) const SELECTED_OPTION: &str = "selected_option";
    pub(crate) const SESSION_ID: &str = "session_id";
    pub(crate) const SUCCESS: &str = "success";
    pub(crate) const GATES: &str = "gates";
    pub(crate) const MAX_ITERATIONS: &str = "max_iterations";
    pub(crate) const TITLE: &str = "title";
    pub(crate) const TOOL_NAME: &str = "tool_name";
    pub(crate) const TOOLS: &str = "tools";
    pub(crate) const CHECKPOINTS: &str = "checkpoints";
    pub(crate) const CONSTRAINTS: &str = "constraints";
    pub(crate) const OBJECTIVE: &str = "objective";
    pub(crate) const VALIDATIONS: &str = "validations";
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
    pub(crate) const CHECK: &str = "check";
    pub(crate) const CHECK_AGENT_MAP: &str = "agent-map";
    pub(crate) const CHECK_AGENT_GUIDES: &str = "agent-guides";
    pub(crate) const CHECK_CLIPPY: &str = "clippy";
    pub(crate) const CHECK_CONTRACT: &str = "contract";
    pub(crate) const CHECK_FMT: &str = "fmt";
    pub(crate) const CHECK_LINT: &str = "lint";
    pub(crate) const CHECK_MIGRATION_IMMUTABILITY: &str = "migration-immutability";
    pub(crate) const CHECK_NO_MOD_RS: &str = "no-mod-rs";
    pub(crate) const CHECK_RUST_FILE_LOC: &str = "rust-file-loc";
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
    pub(crate) const CODEX: &str = "codex";
    pub(crate) const CODEX_HOMES: &str = "homes";
    pub(crate) const CODEX_LAUNCH: &str = "launch";
    pub(crate) const CODEX_RESUME: &str = "resume";
    pub(crate) const DEV: &str = "dev";
    pub(crate) const DEV_STATUS: &str = "status";
    pub(crate) const DEV_STOP: &str = "stop";
    pub(crate) const DOCTOR: &str = "doctor";
    pub(crate) const GENERATE_SQLX_UNCHECKED_QUERIES_TODO: &str =
        "generate-sqlx-unchecked-queries-todo";
    pub(crate) const INFO: &str = "info";
    pub(crate) const INIT: &str = "init";
    pub(crate) const LOOP: &str = "loop";
    pub(crate) const LOOP_CLEAR_ATTEMPT: &str = "clear-attempt";
    pub(crate) const LOOP_RUN: &str = "run";
    pub(crate) const LOOP_STATUS: &str = "status";
    pub(crate) const LOOP_TICK: &str = "tick";
    pub(crate) const MCP: &str = "mcp";
    pub(crate) const MIGRATION: &str = "migration";
    pub(crate) const MIGRATION_ADD_NESTED: &str = "add";
    pub(crate) const MIGRATION_ADD: &str = "migration-add";
    pub(crate) const PRESETS: &str = "presets";
    pub(crate) const PROMPT: &str = "prompt";
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
    pub(crate) const STATE_COMPACT: &str = "compact";
    pub(crate) const STATE_DIAGNOSE: &str = "diagnose";
    pub(crate) const STATE_EXPORT: &str = "export";
    pub(crate) const STATE_RECEIPTS: &str = "receipts";
    pub(crate) const STATE_RESTORE: &str = "restore";
    pub(crate) const STATE_SESSIONS: &str = "sessions";
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
    pub(crate) const WORK_APPEND: &str = "append";
    pub(crate) const WORK_CHECK: &str = "check";
    pub(crate) const WORK_DECIDE: &str = "decide";
    pub(crate) const WORK_EVIDENCE: &str = "evidence";
    pub(crate) const WORK_FINISH: &str = "finish";
    pub(crate) const WORK_GATES: &str = "gates";
    pub(crate) const WORK_GOAL: &str = "goal";
    pub(crate) const WORK_REFINE: &str = "refine";
    pub(crate) const WORK_REVIEW: &str = "review";
    pub(crate) const WORK_RECEIPTS: &str = "receipts";
    pub(crate) const WORK_START: &str = "start";
    pub(crate) const WORK_STATUS: &str = "status";
}

pub(crate) type JsonObject = Map<String, Value>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryTool {
    AgentDoctor,
    Goal,
    Start,
    Append,
    Check,
    Gates,
    Evidence,
    Review,
    Refine,
    Decide,
    Receipts,
    Status,
    Finish,
}

impl MemoryTool {
    const ALL: &'static [Self] = &[
        Self::AgentDoctor,
        Self::Goal,
        Self::Start,
        Self::Append,
        Self::Check,
        Self::Gates,
        Self::Evidence,
        Self::Review,
        Self::Refine,
        Self::Decide,
        Self::Receipts,
        Self::Status,
        Self::Finish,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            tool::AGENT_DOCTOR => Some(Self::AgentDoctor),
            tool::WORK_GOAL => Some(Self::Goal),
            tool::WORK_START => Some(Self::Start),
            tool::WORK_APPEND => Some(Self::Append),
            tool::WORK_CHECK => Some(Self::Check),
            tool::WORK_GATES => Some(Self::Gates),
            tool::WORK_EVIDENCE => Some(Self::Evidence),
            tool::WORK_REVIEW => Some(Self::Review),
            tool::WORK_REFINE => Some(Self::Refine),
            tool::WORK_DECIDE => Some(Self::Decide),
            tool::WORK_RECEIPTS => Some(Self::Receipts),
            tool::WORK_STATUS => Some(Self::Status),
            tool::WORK_FINISH => Some(Self::Finish),
            _ => None,
        }
    }

    pub(crate) const fn uses_repository_authority(self) -> bool {
        matches!(
            self,
            Self::AgentDoctor
                | Self::Goal
                | Self::Start
                | Self::Check
                | Self::Gates
                | Self::Evidence
                | Self::Review
                | Self::Refine
                | Self::Status
                | Self::Finish
        )
    }

    const fn name(self) -> &'static str {
        match self {
            Self::AgentDoctor => tool::AGENT_DOCTOR,
            Self::Goal => tool::WORK_GOAL,
            Self::Start => tool::WORK_START,
            Self::Append => tool::WORK_APPEND,
            Self::Check => tool::WORK_CHECK,
            Self::Gates => tool::WORK_GATES,
            Self::Evidence => tool::WORK_EVIDENCE,
            Self::Review => tool::WORK_REVIEW,
            Self::Refine => tool::WORK_REFINE,
            Self::Decide => tool::WORK_DECIDE,
            Self::Receipts => tool::WORK_RECEIPTS,
            Self::Status => tool::WORK_STATUS,
            Self::Finish => tool::WORK_FINISH,
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::AgentDoctor => "Report local Codex agent tooling status for this repo.",
            Self::Goal => {
                "Create a goal-mode work harness with a durable plan and validation contract."
            }
            Self::Start => {
                "Start structured work by opening a session and plan; body and body_file are optional but mutually exclusive."
            }
            Self::Append => {
                "Append nonblank progress to a structured work plan using exactly one of body or body_file."
            }
            Self::Check => "Run configured or selected work checks.",
            Self::Gates => "Report configured work gate status for a plan.",
            Self::Evidence => {
                "Summarize work gate evidence and receipt freshness; ok=true means inspection succeeded, while overall reports passed or blocked gates."
            }
            Self::Review => "Run configured Codex review gates and record structured findings.",
            Self::Refine => {
                "Run review-driven refinement, rerun review gates, then rerun normal check gates."
            }
            Self::Decide => "Record a structured work decision.",
            Self::Receipts => "List structured work receipts.",
            Self::Status => "Summarize structured work state.",
            Self::Finish => "Close a structured work plan and active session.",
        }
    }

    fn input_schema(self) -> Value {
        match self {
            Self::AgentDoctor | Self::Status => empty_input_schema(),
            Self::Goal => object_schema(
                &[
                    (args::OBJECTIVE, string_schema()),
                    (args::SUCCESS, string_schema()),
                    (
                        args::VALIDATIONS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        }),
                    ),
                    (
                        args::CONSTRAINTS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (
                        args::CHECKPOINTS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (args::TITLE, string_schema()),
                    (args::NOTES, string_schema()),
                ],
                &[args::OBJECTIVE, args::SUCCESS, args::VALIDATIONS],
            ),
            Self::Gates | Self::Evidence => object_schema(&[(args::PLAN_ID, string_schema())], &[]),
            Self::Review => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (
                        args::GATES,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                ],
                &[args::PLAN_ID],
            ),
            Self::Refine => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (
                        args::GATES,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (
                        args::MAX_ITERATIONS,
                        json!({ "type": "integer", "minimum": 1 }),
                    ),
                ],
                &[args::PLAN_ID],
            ),
            Self::Start => work_start_input_schema(),
            Self::Append => work_append_input_schema(),
            Self::Check => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (
                        args::TOOLS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                ],
                &[args::PLAN_ID],
            ),
            Self::Decide => object_schema(
                &[
                    (args::TITLE, string_schema()),
                    (args::SELECTED_OPTION, string_schema()),
                    (args::RATIONALE, string_schema()),
                    (
                        args::ALTERNATIVES,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (args::PLAN_ID, string_schema()),
                ],
                &[args::TITLE, args::SELECTED_OPTION, args::RATIONALE],
            ),
            Self::Receipts => object_schema(
                &[
                    (args::SESSION_ID, string_schema()),
                    (args::PLAN_ID, string_schema()),
                    (args::TOOL_NAME, string_schema()),
                    (args::FAILED_ONLY, json!({ "type": "boolean" })),
                    (args::LIMIT, json!({ "type": "integer", "minimum": 1 })),
                ],
                &[],
            ),
            Self::Finish => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (args::RESOLUTION, string_schema()),
                    (args::OUTCOME, string_schema()),
                ],
                &[args::PLAN_ID],
            ),
        }
    }
}

pub(crate) fn tool_descriptors(
    contract_version: u32,
    manifest_tools: &[ManifestTool],
) -> Vec<Value> {
    let execution = if contract_version >= 6 {
        RepositoryTool::ALL
            .iter()
            .copied()
            .map(RepositoryTool::descriptor)
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

fn work_start_input_schema() -> Value {
    let mut schema = object_schema(
        &[
            (args::TITLE, string_schema()),
            (args::BODY, string_schema()),
            (args::BODY_FILE, string_schema()),
        ],
        &[args::TITLE],
    );
    schema["not"] = json!({ "required": [args::BODY, args::BODY_FILE] });
    schema
}

fn work_append_input_schema() -> Value {
    let mut schema = object_schema(
        &[
            (args::PLAN_ID, string_schema()),
            (args::BODY, nonblank_string_schema()),
            (args::BODY_FILE, nonblank_string_schema()),
        ],
        &[args::PLAN_ID],
    );
    schema["oneOf"] = json!([
        { "required": [args::BODY] },
        { "required": [args::BODY_FILE] }
    ]);
    schema
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

fn nonblank_string_schema() -> Value {
    json!({
        "type": "string",
        "pattern": "\\S"
    })
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
        assert!(unique.contains(tool::WORK_EVIDENCE));
        assert!(unique.contains(tool::WORK_REVIEW));
        assert!(unique.contains(tool::WORK_REFINE));
    }

    #[test]
    fn work_append_schema_requires_exactly_one_nonblank_body_source() {
        let schema = MemoryTool::Append.input_schema();

        assert_eq!(schema["required"], json!([args::PLAN_ID]));
        assert_eq!(
            schema["oneOf"],
            json!([
                { "required": [args::BODY] },
                { "required": [args::BODY_FILE] }
            ])
        );
        assert_eq!(schema["properties"][args::BODY]["pattern"], "\\S");
        assert_eq!(schema["properties"][args::BODY_FILE]["pattern"], "\\S");
    }

    #[test]
    fn work_start_schema_rejects_conflicting_optional_body_sources() {
        let schema = MemoryTool::Start.input_schema();

        assert_eq!(schema["required"], json!([args::TITLE]));
        assert_eq!(
            schema["not"],
            json!({ "required": [args::BODY, args::BODY_FILE] })
        );
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
