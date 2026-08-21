use serde::{Deserialize, Serialize};

pub mod status_provider;

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
    pub required_tools: fn(&dyn FeatureContext) -> Vec<&'static str>,
    pub unavailable_tool_message: fn(&dyn FeatureContext, &str) -> Option<String>,
}

impl FeatureDescriptor {
    pub const fn new(
        command_keys: &'static [&'static str],
        native_tools: &'static [NativeToolDescriptor],
        required_tools: fn(&dyn FeatureContext) -> Vec<&'static str>,
        unavailable_tool_message: fn(&dyn FeatureContext, &str) -> Option<String>,
    ) -> Self {
        Self {
            command_keys,
            native_tools,
            required_tools,
            unavailable_tool_message,
        }
    }
}

pub trait FeatureContext {
    fn contract_version(&self) -> u32;
    fn required_commands(&self) -> &[String];
    fn migration_authoring_enabled(&self) -> bool;
    fn sqlx_enabled(&self) -> bool;
    fn schema_dump_enabled(&self) -> bool;
    fn frontend_app_count(&self) -> usize;
    fn go_backend_enabled(&self) -> bool {
        false
    }
    fn go_postgres_enabled(&self) -> bool {
        false
    }
    fn has_required_command(&self, command_key: &str) -> bool {
        self.required_commands()
            .iter()
            .any(|command| command == command_key)
    }
}
