use jig_contract::{
    ActionEffect, ActionIntent, AdapterActionDescriptor, AdapterRunnerDescriptor, FeatureContext,
    FeatureDescriptor, RepositoryAdapterDescriptor, tool,
};

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
const CHECK_EFFECTS: &[ActionEffect] = &[ActionEffect::ReadOnly, ActionEffect::Process];
// Go repositories can contain several modules or a go.work workspace. Treat
// all module/workspace metadata as build authority instead of repeating a
// root-only approximation on each action.
const GO_CHECK_INPUTS: &[&str] = &[
    "go.mod",
    "go.sum",
    "go.work",
    "go.work.sum",
    "**/go.mod",
    "**/go.sum",
    "**/go.work",
    "**/go.work.sum",
    "**/*.go",
    "vendor/modules.txt",
    "**/vendor/modules.txt",
];
const ADAPTER_ACTIONS: &[AdapterActionDescriptor] = &[
    AdapterActionDescriptor::new(
        "fmt",
        "Check Go formatting.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(FMT_CHECK_COMMAND),
        GO_CHECK_INPUTS,
        Some(tool::FMT_CHECK),
    ),
    AdapterActionDescriptor::new(
        "lint",
        "Run Go static analysis.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(LINT_COMMAND),
        GO_CHECK_INPUTS,
        Some(tool::LINT),
    ),
    AdapterActionDescriptor::new(
        "test",
        "Run Go tests.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(TEST_COMMAND),
        GO_CHECK_INPUTS,
        Some(tool::TEST),
    ),
    AdapterActionDescriptor::new(
        "test-locked",
        "Verify Go modules and run tests without dependency updates.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(TEST_LOCKED_COMMAND),
        GO_CHECK_INPUTS,
        Some(tool::TEST_LOCKED),
    ),
];
const GO_POSTGRES_ACTIONS: &[AdapterActionDescriptor] = &[
    AdapterActionDescriptor::new(
        "sqlc",
        "Check sqlc queries and generated-code drift.",
        ActionIntent::Check,
        CHECK_EFFECTS,
        AdapterRunnerDescriptor::Command(SQLC_CHECK_COMMAND),
        &["sqlc.yaml", "**/*.sql"],
        Some(tool::SQLC_CHECK),
    ),
    AdapterActionDescriptor::new(
        "migration-add",
        "Add a timestamped Goose SQL migration.",
        ActionIntent::Generate,
        &[ActionEffect::Worktree, ActionEffect::Process],
        AdapterRunnerDescriptor::Native(tool::MIGRATION_ADD),
        &["internal/database/migrations/**/*.sql"],
        Some(tool::MIGRATION_ADD),
    ),
];
const REPOSITORY_ADAPTERS: &[RepositoryAdapterDescriptor] = &[
    RepositoryAdapterDescriptor::new("go", ADAPTER_ACTIONS),
    RepositoryAdapterDescriptor::new("go-postgres", GO_POSTGRES_ACTIONS),
];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    &[],
    REPOSITORY_ADAPTERS,
    required_tools,
    unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    COMMAND_TOOLS
        .iter()
        .filter_map(|(command_key, tool_name)| {
            ctx.has_required_command(command_key).then_some(*tool_name)
        })
        .collect()
}

fn unavailable_tool_message(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    match tool_name {
        tool::LINT if !ctx.go_backend_enabled() => Some(format!(
            "{tool_name} is not available because backend_language is not \"go\" in .jig.toml. Use `scripts/jig check clippy` for Rust, select a Go backend, or remove this command/gate."
        )),
        tool::LINT => Some(format!(
            "{tool_name} is not declared for this Go repository in .agent/jig-contract.json. Run `jig update --recopy`, add a project-owned go_lint_command and matching tool declaration, or remove this command/gate."
        )),
        tool::SQLC_CHECK if !ctx.go_backend_enabled() => Some(format!(
            "{tool_name} is not available because sqlc checks belong to a Go/PostgreSQL backend. Set backend_language = \"go\" and go_database = \"postgres\", then run `jig update --recopy`, or remove this command/gate."
        )),
        tool::SQLC_CHECK if !ctx.go_postgres_enabled() => Some(format!(
            "{tool_name} is not available because go_database is not \"postgres\" in .jig.toml. Enable Go/PostgreSQL and run `jig update --recopy`, or remove this command/gate."
        )),
        tool::SQLC_CHECK => Some(format!(
            "{tool_name} is not declared for this Go/PostgreSQL repository in .agent/jig-contract.json. Run `jig update --recopy`, add a project-owned sqlc_check_command and matching tool declaration, or remove this command/gate."
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct StubContext {
        go_backend: bool,
        go_postgres: bool,
        required_commands: Vec<String>,
    }

    impl FeatureContext for StubContext {
        fn contract_version(&self) -> u32 {
            5
        }

        fn required_commands(&self) -> &[String] {
            &self.required_commands
        }

        fn sqlx_enabled(&self) -> bool {
            false
        }

        fn schema_dump_enabled(&self) -> bool {
            false
        }

        fn frontend_app_count(&self) -> usize {
            0
        }

        fn go_backend_enabled(&self) -> bool {
            self.go_backend
        }

        fn go_postgres_enabled(&self) -> bool {
            self.go_postgres
        }
    }

    #[test]
    fn every_go_check_tracks_nested_module_authority() {
        for action in ADAPTER_ACTIONS {
            assert!(action.inputs.contains(&"**/go.mod"));
            assert!(action.inputs.contains(&"**/go.sum"));
            assert!(action.inputs.contains(&"go.work"));
            assert!(action.inputs.contains(&"go.work.sum"));
        }
    }

    #[test]
    fn lint_diagnostic_distinguishes_backend_from_stale_manifest() {
        let rust = StubContext::default();
        let message = unavailable_tool_message(&rust, tool::LINT).unwrap();
        assert!(message.contains("backend_language is not \"go\""));
        assert!(message.contains("check clippy"));

        let go = StubContext {
            go_backend: true,
            ..StubContext::default()
        };
        let message = unavailable_tool_message(&go, tool::LINT).unwrap();
        assert!(message.contains("not declared for this Go repository"));
        assert!(message.contains("go_lint_command"));
    }

    #[test]
    fn sqlc_diagnostic_distinguishes_backend_database_and_stale_manifest() {
        let rust = StubContext::default();
        let message = unavailable_tool_message(&rust, tool::SQLC_CHECK).unwrap();
        assert!(message.contains("Go/PostgreSQL backend"));

        let go = StubContext {
            go_backend: true,
            ..StubContext::default()
        };
        let message = unavailable_tool_message(&go, tool::SQLC_CHECK).unwrap();
        assert!(message.contains("go_database is not \"postgres\""));

        let postgres = StubContext {
            go_backend: true,
            go_postgres: true,
            ..StubContext::default()
        };
        let message = unavailable_tool_message(&postgres, tool::SQLC_CHECK).unwrap();
        assert!(message.contains("not declared for this Go/PostgreSQL repository"));
        assert!(message.contains("sqlc_check_command"));
    }
}
