use serde::{Deserialize, Serialize};

pub mod repository;
pub mod run;
pub mod status_provider;

pub use repository::{
    ActionEffect, ActionId, ActionIntent, ActionRunner, ActionSpec, ComponentId, ComponentSpec,
    FieldProvenance, ProfileId, ProfileSpec, ResultParser, TargetId,
};
pub use run::{
    ActionArguments, EvidenceReference, Finding, FindingLocation, FindingSeverity, PlannedTarget,
    RunConclusion, RunPlan, RunResult, RunStatus, SelectionReason, SourceIdentity, TargetRunResult,
};

pub mod kind {
    pub const COMMAND: &str = "command";
    pub const NATIVE: &str = "native";
}

pub mod tool {
    pub const BOOTSTRAP: &str = "jig.bootstrap";
    pub const AGENT_DOCTOR: &str = "jig.agent_doctor";
    pub const CLIPPY: &str = "jig.clippy";
    pub const CONTRACT_CHECK: &str = "jig.contract_check";
    pub const DECISIONS_ADD: &str = "jig.decisions_add";
    pub const FMT_CHECK: &str = "jig.fmt_check";
    pub const LINT: &str = "jig.lint";
    pub const MIGRATION_ADD: &str = "jig.migration_add";
    pub const INSPECT: &str = "jig.inspect";
    pub const PLAN_RUN: &str = "jig.plan_run";
    pub const EXECUTE_RUN: &str = "jig.execute_run";
    pub const CANCEL_RUN: &str = "jig.cancel_run";
    pub const PLANS_APPEND: &str = "jig.plans_append";
    pub const PLANS_CLOSE: &str = "jig.plans_close";
    pub const PLANS_OPEN: &str = "jig.plans_open";
    pub const SCHEMA_CHECK: &str = "jig.schema_check";
    pub const SCHEMA_DUMP: &str = "jig.schema_dump";
    pub const SESSION_END: &str = "jig.session_end";
    pub const SESSION_START: &str = "jig.session_start";
    pub const SQLX_CHECK: &str = "jig.sqlx_check";
    pub const SQLC_CHECK: &str = "jig.sqlc_check";
    pub const TEST: &str = "jig.test";
    pub const TEST_LOCKED: &str = "jig.test_locked";
    pub const TYPESCRIPT_BUILD: &str = "jig.typescript_build";
    pub const TYPESCRIPT_COVERAGE: &str = "jig.typescript_coverage";
    pub const TYPESCRIPT_LINT: &str = "jig.typescript_lint";
    pub const TYPESCRIPT_TYPECHECK: &str = "jig.typescript_typecheck";
    pub const WORK_APPEND: &str = "jig.work_append";
    pub const WORK_CHECK: &str = "jig.work_check";
    pub const WORK_DECIDE: &str = "jig.work_decide";
    pub const WORK_FINISH: &str = "jig.work_finish";
    pub const WORK_GATES: &str = "jig.work_gates";
    pub const WORK_EVIDENCE: &str = "jig.work_evidence";
    pub const WORK_GOAL: &str = "jig.work_goal";
    pub const WORK_REFINE: &str = "jig.work_refine";
    pub const WORK_REVIEW: &str = "jig.work_review";
    pub const WORK_RECEIPTS: &str = "jig.work_receipts";
    pub const WORK_START: &str = "jig.work_start";
    pub const WORK_STATUS: &str = "jig.work_status";
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ManifestTool {
    pub name: String,
    pub kind: String,
    pub description: String,
    #[serde(default)]
    pub command: Option<String>,
}

impl ManifestTool {
    pub fn new(
        name: impl Into<String>,
        kind: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            kind: kind.into(),
            description: description.into(),
            command: None,
        }
    }

    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NativeToolKind {
    ContractCheck,
    MigrationAdd,
    SchemaCheck,
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct NativeToolDescriptor {
    pub name: &'static str,
    pub requires_name: bool,
    pub kind: NativeToolKind,
}

/// The checked-in runner shape contributed by a repository adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AdapterRunnerDescriptor {
    Command(&'static str),
    Native(&'static str),
}

/// One conventional action contributed by a repository adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct AdapterActionDescriptor {
    pub id: &'static str,
    pub description: &'static str,
    pub intent: ActionIntent,
    pub effects: &'static [ActionEffect],
    pub runner: AdapterRunnerDescriptor,
    pub inputs: &'static [&'static str],
    pub legacy_alias: Option<&'static str>,
}

impl AdapterActionDescriptor {
    pub const fn new(
        id: &'static str,
        description: &'static str,
        intent: ActionIntent,
        effects: &'static [ActionEffect],
        runner: AdapterRunnerDescriptor,
        inputs: &'static [&'static str],
        legacy_alias: Option<&'static str>,
    ) -> Self {
        Self {
            id,
            description,
            intent,
            effects,
            runner,
            inputs,
            legacy_alias,
        }
    }
}

/// Metadata contributed by a stack adapter without coupling it to runtime execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RepositoryAdapterDescriptor {
    pub id: &'static str,
    pub actions: &'static [AdapterActionDescriptor],
}

impl RepositoryAdapterDescriptor {
    pub const fn new(id: &'static str, actions: &'static [AdapterActionDescriptor]) -> Self {
        Self { id, actions }
    }
}

impl NativeToolDescriptor {
    pub const fn new(name: &'static str, requires_name: bool, kind: NativeToolKind) -> Self {
        Self {
            name,
            requires_name,
            kind,
        }
    }
}

#[non_exhaustive]
pub struct FeatureDescriptor {
    pub command_keys: &'static [&'static str],
    pub native_tools: &'static [NativeToolDescriptor],
    pub repository_adapters: &'static [RepositoryAdapterDescriptor],
    pub required_tools: fn(&dyn FeatureContext) -> Vec<&'static str>,
    pub unavailable_tool_message: fn(&dyn FeatureContext, &str) -> Option<String>,
    pub tool_admission_error: fn(&dyn FeatureContext, &str) -> Option<String>,
}

impl FeatureDescriptor {
    pub const fn new(
        command_keys: &'static [&'static str],
        native_tools: &'static [NativeToolDescriptor],
        repository_adapters: &'static [RepositoryAdapterDescriptor],
        required_tools: fn(&dyn FeatureContext) -> Vec<&'static str>,
        unavailable_tool_message: fn(&dyn FeatureContext, &str) -> Option<String>,
    ) -> Self {
        Self {
            command_keys,
            native_tools,
            repository_adapters,
            required_tools,
            unavailable_tool_message,
            tool_admission_error: no_tool_admission_error,
        }
    }

    pub const fn with_tool_admission_error(
        mut self,
        tool_admission_error: fn(&dyn FeatureContext, &str) -> Option<String>,
    ) -> Self {
        self.tool_admission_error = tool_admission_error;
        self
    }
}

fn no_tool_admission_error(_ctx: &dyn FeatureContext, _tool_name: &str) -> Option<String> {
    None
}

pub trait FeatureContext {
    fn contract_version(&self) -> u32;
    fn required_commands(&self) -> &[String];
    fn sqlx_enabled(&self) -> bool;
    fn schema_dump_enabled(&self) -> bool;
    fn frontend_app_count(&self) -> usize;
    fn go_backend_enabled(&self) -> bool {
        false
    }
    fn go_postgres_enabled(&self) -> bool {
        false
    }
    fn migration_add_enabled(&self) -> bool {
        self.sqlx_enabled()
    }
    fn migration_authoring_enabled(&self) -> bool {
        self.migration_add_enabled() || self.go_postgres_enabled()
    }
    fn has_required_command(&self, command_key: &str) -> bool {
        self.required_commands()
            .iter()
            .any(|command| command == command_key)
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureContext, ManifestTool, kind};

    struct LegacySqlxContext;

    struct GoPostgresContext;

    impl FeatureContext for LegacySqlxContext {
        fn contract_version(&self) -> u32 {
            4
        }

        fn required_commands(&self) -> &[String] {
            &[]
        }

        fn sqlx_enabled(&self) -> bool {
            true
        }

        fn schema_dump_enabled(&self) -> bool {
            false
        }

        fn frontend_app_count(&self) -> usize {
            0
        }
    }

    impl FeatureContext for GoPostgresContext {
        fn contract_version(&self) -> u32 {
            5
        }

        fn required_commands(&self) -> &[String] {
            &[]
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

        fn go_postgres_enabled(&self) -> bool {
            true
        }
    }

    #[test]
    fn migration_authoring_is_derived_from_the_backend_capabilities() {
        assert!(LegacySqlxContext.migration_authoring_enabled());
        assert!(GoPostgresContext.migration_authoring_enabled());
    }

    #[test]
    fn repository_dtos_do_not_change_legacy_manifest_tool_json() {
        let tool = ManifestTool::new("jig.test", kind::COMMAND, "Run tests.")
            .with_command("rust_test_command");

        assert_eq!(
            serde_json::to_value(tool).unwrap(),
            serde_json::json!({
                "name": "jig.test",
                "kind": "command",
                "description": "Run tests.",
                "command": "rust_test_command"
            })
        );
    }
}
