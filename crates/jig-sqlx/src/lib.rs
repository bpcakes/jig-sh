use jig_contract::{
    ActionEffect, ActionIntent, AdapterActionDescriptor, AdapterRunnerDescriptor, FeatureContext,
    FeatureDescriptor, NativeToolDescriptor, NativeToolKind, RepositoryAdapterDescriptor, tool,
};

const MIGRATION_ADD_COMMAND: &str = "migration_add_command";
const SCHEMA_CHECK_COMMAND: &str = "schema_check_command";
const SCHEMA_DUMP_COMMAND: &str = "schema_dump_command";
const SQLX_CHECK_COMMAND: &str = "sqlx_check_command";
const COMMAND_KEYS: &[&str] = &[
    MIGRATION_ADD_COMMAND,
    SCHEMA_CHECK_COMMAND,
    SCHEMA_DUMP_COMMAND,
    SQLX_CHECK_COMMAND,
];
const NATIVE_TOOLS: &[NativeToolDescriptor] = &[NativeToolDescriptor::new(
    tool::SCHEMA_CHECK,
    false,
    NativeToolKind::SchemaCheck,
)];
const CHECK_EFFECTS: &[ActionEffect] = &[ActionEffect::ReadOnly, ActionEffect::Process];
const ADAPTER_ACTIONS: &[AdapterActionDescriptor] = &[
    AdapterActionDescriptor::new(
        "sqlx",
        "Check SQLx query metadata and database integration.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(SQLX_CHECK_COMMAND),
        &[
            "Cargo.toml",
            "Cargo.lock",
            ".sqlx/**/*.json",
            "**/*.sql",
            "**/*.rs",
        ],
        Some(tool::SQLX_CHECK),
    ),
    AdapterActionDescriptor::new(
        "schema",
        "Check committed schema output for drift.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Native(tool::SCHEMA_CHECK),
        &["**/*.sql", "**/*.rs"],
        Some(tool::SCHEMA_CHECK),
    ),
    AdapterActionDescriptor::new(
        "schema-dump",
        "Refresh committed schema output.",
        ActionIntent::Generate,
        &[ActionEffect::Worktree, ActionEffect::Process],
        AdapterRunnerDescriptor::Command(SCHEMA_DUMP_COMMAND),
        &["**/*.sql", "**/*.rs"],
        Some(tool::SCHEMA_DUMP),
    ),
    AdapterActionDescriptor::new(
        "migration-add",
        "Add a timestamped SQLx migration.",
        ActionIntent::Generate,
        &[ActionEffect::Worktree, ActionEffect::Process],
        AdapterRunnerDescriptor::Native(tool::MIGRATION_ADD),
        &["**/migrations/**/*.sql"],
        Some(tool::MIGRATION_ADD),
    ),
];
const REPOSITORY_ADAPTERS: &[RepositoryAdapterDescriptor] =
    &[RepositoryAdapterDescriptor::new("sqlx", ADAPTER_ACTIONS)];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    NATIVE_TOOLS,
    REPOSITORY_ADAPTERS,
    required_tools,
    unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = Vec::new();
    let sqlx_enabled = ctx.sqlx_enabled();
    let schema_dump_enabled = sqlx_enabled && ctx.schema_dump_enabled();

    if sqlx_enabled || ctx.has_required_command(SQLX_CHECK_COMMAND) {
        required.push(tool::SQLX_CHECK);
    }
    if ctx.has_required_command(MIGRATION_ADD_COMMAND) {
        required.push(tool::MIGRATION_ADD);
    }
    if schema_dump_enabled || ctx.has_required_command(SCHEMA_CHECK_COMMAND) {
        required.push(tool::SCHEMA_CHECK);
    }
    if schema_dump_enabled || ctx.has_required_command(SCHEMA_DUMP_COMMAND) {
        required.push(tool::SCHEMA_DUMP);
    }
    required
}

fn unavailable_tool_message(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    match tool_name {
        tool::SCHEMA_CHECK | tool::SCHEMA_DUMP if !ctx.sqlx_enabled() => Some(format!(
            "{tool_name} is not available because sqlx_enabled = false in .jig.toml. Enable SQLx and schema dumps, then run `jig update --recopy`, or remove this command/gate."
        )),
        tool::SCHEMA_CHECK | tool::SCHEMA_DUMP if !ctx.schema_dump_enabled() => Some(format!(
            "{tool_name} is not available because schema_dump_enabled = false in .jig.toml. Enable schema dumps, then run `jig update --recopy`, or remove this command/gate."
        )),
        tool::SQLX_CHECK if !ctx.sqlx_enabled() => Some(format!(
            "{tool_name} is not available because sqlx_enabled = false in .jig.toml. Enable SQLx, then run `jig update --recopy`, or remove this command/gate."
        )),
        _ => None,
    }
}
