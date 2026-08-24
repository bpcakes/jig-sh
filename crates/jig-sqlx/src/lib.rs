use jig_contract::{FeatureContext, FeatureDescriptor, NativeToolDescriptor, NativeToolKind, tool};

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
const NATIVE_TOOLS: &[NativeToolDescriptor] = &[
    NativeToolDescriptor::new(tool::MIGRATION_ADD, true, NativeToolKind::MigrationAdd),
    NativeToolDescriptor::new(tool::SCHEMA_CHECK, false, NativeToolKind::SchemaCheck),
];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    NATIVE_TOOLS,
    required_tools,
    unavailable_tool_message,
)
.with_tool_admission_error(tool_admission_error);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = Vec::new();
    let sqlx_enabled = ctx.sqlx_enabled();
    let schema_dump_enabled = sqlx_enabled && ctx.schema_dump_enabled();

    if sqlx_enabled || ctx.has_required_command(SQLX_CHECK_COMMAND) {
        required.push(tool::SQLX_CHECK);
    }
    if ctx.migration_add_enabled() {
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
        tool::SQLX_CHECK | tool::MIGRATION_ADD if !ctx.sqlx_enabled() => Some(format!(
            "{tool_name} is not available because sqlx_enabled = false in .jig.toml. Enable SQLx, then run `jig update --recopy`, or remove this command/gate."
        )),
        tool::MIGRATION_ADD if !ctx.migration_add_enabled() => {
            Some(migration_add_layout_error(tool_name))
        }
        _ => None,
    }
}

fn tool_admission_error(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    (tool_name == tool::MIGRATION_ADD && ctx.sqlx_enabled() && !ctx.migration_add_enabled())
        .then(|| migration_add_layout_error(tool_name))
}

fn migration_add_layout_error(tool_name: &str) -> String {
    format!(
        "{tool_name} is not available because the configured Rust migration layout does not permit flat migration stubs. Advance the repository with its forward-only versioning workflow, then run `jig update --recopy` to remove any stale migration-add declaration."
    )
}
