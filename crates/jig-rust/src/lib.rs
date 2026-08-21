use jig_contract::{
    ActionEffect, ActionIntent, AdapterActionDescriptor, AdapterRunnerDescriptor, FeatureContext,
    FeatureDescriptor, RepositoryAdapterDescriptor, tool,
};

const CLIPPY_COMMAND: &str = "rust_clippy_command";
const FMT_CHECK_COMMAND: &str = "rust_fmt_check_command";
const TEST_COMMAND: &str = "rust_test_command";
const TEST_LOCKED_COMMAND: &str = "rust_test_locked_command";
const COMMAND_KEYS: &[&str] = &[
    CLIPPY_COMMAND,
    FMT_CHECK_COMMAND,
    TEST_COMMAND,
    TEST_LOCKED_COMMAND,
];
const COMMAND_TOOLS: &[(&str, &str)] = &[
    (FMT_CHECK_COMMAND, tool::FMT_CHECK),
    (CLIPPY_COMMAND, tool::CLIPPY),
    (TEST_COMMAND, tool::TEST),
    (TEST_LOCKED_COMMAND, tool::TEST_LOCKED),
];
const CHECK_EFFECTS: &[ActionEffect] = &[ActionEffect::ReadOnly, ActionEffect::Process];
const ADAPTER_ACTIONS: &[AdapterActionDescriptor] = &[
    AdapterActionDescriptor::new(
        "fmt",
        "Check Rust formatting.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(FMT_CHECK_COMMAND),
        &["Cargo.toml", "Cargo.lock", "**/*.rs"],
        Some(tool::FMT_CHECK),
    ),
    AdapterActionDescriptor::new(
        "clippy",
        "Run Rust Clippy with repository policy.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(CLIPPY_COMMAND),
        &["Cargo.toml", "Cargo.lock", "**/*.rs"],
        Some(tool::CLIPPY),
    ),
    AdapterActionDescriptor::new(
        "test",
        "Run Rust tests.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(TEST_COMMAND),
        &["Cargo.toml", "Cargo.lock", "**/*.rs"],
        Some(tool::TEST),
    ),
    AdapterActionDescriptor::new(
        "test-locked",
        "Run Rust tests using the locked dependency graph.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(TEST_LOCKED_COMMAND),
        &["Cargo.toml", "Cargo.lock", "**/*.rs"],
        Some(tool::TEST_LOCKED),
    ),
];
const REPOSITORY_ADAPTERS: &[RepositoryAdapterDescriptor] =
    &[RepositoryAdapterDescriptor::new("rust", ADAPTER_ACTIONS)];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    &[],
    REPOSITORY_ADAPTERS,
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
