use jig_contract::{FeatureContext, FeatureDescriptor, tool};

const FMT_CHECK_COMMAND: &str = "go_fmt_check_command";
const LINT_COMMAND: &str = "go_lint_command";
const SQLC_CHECK_COMMAND: &str = "sqlc_check_command";
const TEST_COMMAND: &str = "go_test_command";
const TEST_LOCKED_COMMAND: &str = "go_test_locked_command";
const COMMAND_KEYS: &[&str] = &[
    FMT_CHECK_COMMAND,
    LINT_COMMAND,
    SQLC_CHECK_COMMAND,
    TEST_COMMAND,
    TEST_LOCKED_COMMAND,
];
const COMMAND_TOOLS: &[(&str, &str)] = &[
    (FMT_CHECK_COMMAND, tool::FMT_CHECK),
    (LINT_COMMAND, tool::LINT),
    (SQLC_CHECK_COMMAND, tool::SQLC_CHECK),
    (TEST_COMMAND, tool::TEST),
    (TEST_LOCKED_COMMAND, tool::TEST_LOCKED),
];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    &[],
    required_tools,
    no_unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    COMMAND_TOOLS
        .iter()
        .filter_map(|(command_key, tool_name)| {
            ctx.has_required_command(command_key).then_some(*tool_name)
        })
        .collect()
}

fn no_unavailable_tool_message(_ctx: &dyn FeatureContext, _tool_name: &str) -> Option<String> {
    None
}
