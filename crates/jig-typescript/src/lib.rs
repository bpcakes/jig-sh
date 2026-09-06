use jig_contract::{
    ActionEffect, ActionIntent, AdapterActionDescriptor, AdapterRunnerDescriptor, FeatureContext,
    FeatureDescriptor, RepositoryAdapterDescriptor, tool,
};

pub mod dev_script;
pub mod workspace;

const LINT_COMMAND: &str = "typescript_lint_command";
const TYPECHECK_COMMAND: &str = "typescript_typecheck_command";
const BUILD_COMMAND: &str = "typescript_build_command";
const COVERAGE_COMMAND: &str = "typescript_coverage_command";

const COMMAND_KEYS: &[&str] = &[
    LINT_COMMAND,
    TYPECHECK_COMMAND,
    BUILD_COMMAND,
    COVERAGE_COMMAND,
];
const CHECK_EFFECTS: &[ActionEffect] = &[ActionEffect::ReadOnly, ActionEffect::Process];
const ADAPTER_ACTIONS: &[AdapterActionDescriptor] = &[
    AdapterActionDescriptor::new(
        "lint",
        "Lint this TypeScript component.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(LINT_COMMAND),
        &["package.json", "**/*"],
        Some(tool::TYPESCRIPT_LINT),
    ),
    AdapterActionDescriptor::new(
        "typecheck",
        "Type-check this TypeScript component.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(TYPECHECK_COMMAND),
        &["package.json", "**/*"],
        Some(tool::TYPESCRIPT_TYPECHECK),
    ),
    AdapterActionDescriptor::new(
        "build",
        "Build this TypeScript component.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(BUILD_COMMAND),
        &["package.json", "**/*"],
        Some(tool::TYPESCRIPT_BUILD),
    ),
    AdapterActionDescriptor::new(
        "test",
        "Run this TypeScript component's coverage tests.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(COVERAGE_COMMAND),
        &["package.json", "**/*"],
        Some(tool::TYPESCRIPT_COVERAGE),
    ),
];
const REPOSITORY_ADAPTERS: &[RepositoryAdapterDescriptor] = &[RepositoryAdapterDescriptor::new(
    "typescript",
    ADAPTER_ACTIONS,
)];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    &[],
    REPOSITORY_ADAPTERS,
    required_tools,
    unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let generated_typescript_gates = ctx.contract_version() >= 3 && ctx.frontend_app_count() > 0;

    [
        (LINT_COMMAND, tool::TYPESCRIPT_LINT),
        (TYPECHECK_COMMAND, tool::TYPESCRIPT_TYPECHECK),
        (BUILD_COMMAND, tool::TYPESCRIPT_BUILD),
        (COVERAGE_COMMAND, tool::TYPESCRIPT_COVERAGE),
    ]
    .into_iter()
    .filter_map(|(command_key, tool_name)| {
        (generated_typescript_gates || ctx.has_required_command(command_key)).then_some(tool_name)
    })
    .collect()
}

fn unavailable_tool_message(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    if !matches!(
        tool_name,
        tool::TYPESCRIPT_LINT
            | tool::TYPESCRIPT_TYPECHECK
            | tool::TYPESCRIPT_BUILD
            | tool::TYPESCRIPT_COVERAGE
    ) {
        return None;
    }

    if ctx.frontend_app_count() == 0 {
        Some(format!(
            "{tool_name} is not available because no [[frontend_apps]] are configured in .jig.toml. Add frontend apps then run `jig update --recopy`, add project-owned [commands] and tool definitions, or remove this command/gate."
        ))
    } else {
        Some(format!(
            "{tool_name} is not declared in .agent/jig-contract.json. Run `jig update --recopy`, add the matching tools[] entry for an existing project-owned [commands] entry, or remove this command/gate."
        ))
    }
}
