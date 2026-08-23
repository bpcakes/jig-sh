//! Runtime-facing command DTOs.
//!
//! CLI parsing stays in `cli`, and runtime execution stays in `runtime`.
//! This module owns the neutral request shapes passed between them. Types that
//! also back MCP tool arguments derive `Deserialize` here so both CLI and MCP
//! paths reach runtime through the same request vocabulary. Command families
//! live in sibling modules; this file is the public hub for runtime DTO imports.

mod agent;
mod check;
mod loops;
mod prompt;
mod proxy;
mod sqlx;
mod state;
mod vault;
mod work;

pub(crate) use agent::{AgentBootstrapRequest, AgentCommand};
pub(crate) use check::{
    AgentMapCommand, AgentMapRequest, CheckCommand, MigrationImmutabilityRequest,
    RustFileLocRequest, SqlxTodoRequest,
};
pub(crate) use loops::{
    LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopCommand, LoopDispatchRequest,
    LoopRunRequest, LoopStatusRequest, LoopTickRequest,
};
pub(crate) use prompt::{
    PROMPT_BODY_KEY, PromptAddRequest, PromptCommand, PromptEditRequest, PromptExportRequest,
    PromptImportRequest, PromptListRequest, PromptNameRequest, PromptRenderRequest,
    PromptSearchRequest,
};
pub(crate) use proxy::{
    DevCommand, DevRequest, DevStatusRequest, DevStopRequest, ProxyAliasRequest, ProxyCertCommand,
    ProxyCertGenerateRequest, ProxyCertRuntimeRequest, ProxyCertTrustRequest,
    ProxyCertUntrustRequest, ProxyCommand, ProxyListRequest, ProxyPruneRequest, ProxyRunRequest,
    ProxyRuntimeOptions, ProxyServiceCommand, ProxyServiceInstallRequest,
    ProxyServiceRuntimeRequest, ProxyStartRequest, ProxyStopRequest,
};
pub(crate) use sqlx::{MigrationAddRequest, SqlxCommand};
pub(crate) use state::{
    StateArchiveRequest, StateCommand, StateCompactSessionsRequest, StateDiagnoseRequest,
    StateExportReceiptsRequest, StateRestoreRequest,
};
pub(crate) use vault::{
    VaultAuditCommand, VaultAuditVerifyRequest, VaultBackupCommand, VaultBackupCreateRequest,
    VaultBackupRestoreRequest, VaultCommand, VaultExecAssignment, VaultExecEnvironment,
    VaultExecRequest, VaultExecValue, VaultFieldCommand, VaultFieldListRequest,
    VaultFieldRemoveRequest, VaultFieldSetRequest, VaultImportAssignment, VaultImportCommand,
    VaultImportEnvironment, VaultImportOnePasswordRequest, VaultImportValueSource,
    VaultInitRequest, VaultInjectRequest, VaultMigrateRequest, VaultPassphraseChangeRequest,
    VaultPassphraseCommand, VaultReadRequest, VaultRepoScope, VaultRunRequest, VaultRuntimeOptions,
    VaultScopeSelection, VaultSecretCommand, VaultSecretListRequest, VaultSecretRemoveRequest,
    VaultSecretSetRequest, VaultSecretValueSource, VaultStatusRequest, VaultTuiRequest,
    is_valid_vault_scope_id,
};
pub(crate) use work::{
    DEFAULT_REFINE_MAX_ITERATIONS, WorkAppendRequest, WorkCheckRequest, WorkCommand,
    WorkDecisionRequest, WorkEvidenceRequest, WorkFinishRequest, WorkGatesRequest, WorkGoalRequest,
    WorkReceiptsRequest, WorkRefineRequest, WorkReviewRequest, WorkStartRequest,
};

#[derive(Debug)]
pub(crate) enum RuntimeCommand {
    Bootstrap(ToolRequest),
    Check(CheckCommand),
    Sqlx(SqlxCommand),
    AgentMap(AgentMapCommand),
    GenerateSqlxUncheckedQueriesTodo(SqlxTodoRequest),
    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    Dev(DevCommand),
    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    Proxy(ProxyCommand),
    Agent(AgentCommand),
    Work(WorkCommand),
    Loop(LoopCommand),
    State(StateCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeSignalPolicy {
    /// The command consumes `ExecutionControl` throughout its potentially long
    /// work and can stop at an operation-owned commit boundary.
    Cooperative,
    /// The command does not consume `ExecutionControl`; leave the platform's
    /// native signal disposition installed instead of swallowing Ctrl-C.
    Native,
}

impl RuntimeCommand {
    pub(crate) const fn signal_policy(&self) -> RuntimeSignalPolicy {
        use RuntimeSignalPolicy::{Cooperative, Native};

        match self {
            Self::Bootstrap(_) | Self::Sqlx(_) | Self::Agent(_) => Cooperative,
            Self::Check(command) => match command {
                CheckCommand::Fmt(_)
                | CheckCommand::Clippy(_)
                | CheckCommand::Test(_)
                | CheckCommand::TestLocked(_)
                | CheckCommand::TypeScriptLint(_)
                | CheckCommand::TypeScriptTypecheck(_)
                | CheckCommand::TypeScriptBuild(_)
                | CheckCommand::TypeScriptCoverage(_)
                | CheckCommand::Sqlx(_)
                | CheckCommand::Schema(_)
                | CheckCommand::Contract(_) => Cooperative,
                CheckCommand::AgentMap(_)
                | CheckCommand::AgentGuides
                | CheckCommand::RustFileLoc(_)
                | CheckCommand::NoModRs
                | CheckCommand::MigrationImmutability(_)
                | CheckCommand::SqlxUncheckedNonTest => Native,
            },
            Self::Work(command) => match command {
                WorkCommand::Check(_)
                | WorkCommand::Gates(_)
                | WorkCommand::Evidence(_)
                | WorkCommand::Review(_)
                | WorkCommand::Refine(_)
                | WorkCommand::Status
                | WorkCommand::Finish(_) => Cooperative,
                WorkCommand::Goal(_)
                | WorkCommand::Start(_)
                | WorkCommand::Append(_)
                | WorkCommand::Decide(_)
                | WorkCommand::Receipts(_) => Native,
            },
            Self::Loop(command) => match command {
                LoopCommand::Tick(_)
                | LoopCommand::Dispatch(_)
                | LoopCommand::Status(_)
                | LoopCommand::Run(_) => Cooperative,
                LoopCommand::ClearAttempt(_) | LoopCommand::AcknowledgeOccurrence(_) => Native,
            },
            Self::State(command) => match command {
                StateCommand::Summary => Cooperative,
                StateCommand::Diagnose(_)
                | StateCommand::CompactSessions(_)
                | StateCommand::Restore(_)
                | StateCommand::ExportReceipts(_)
                | StateCommand::Archive(_) => Native,
            },
            Self::AgentMap(_)
            | Self::GenerateSqlxUncheckedQueriesTodo(_)
            | Self::Dev(_)
            | Self::Proxy(_) => Native,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ToolRequest {
    plan_id: Option<String>,
    record_receipt: bool,
}

impl Default for ToolRequest {
    fn default() -> Self {
        Self {
            plan_id: None,
            record_receipt: true,
        }
    }
}

impl ToolRequest {
    pub(crate) const fn new(plan_id: Option<String>, record_receipt: bool) -> Self {
        Self {
            plan_id,
            record_receipt,
        }
    }

    pub(crate) fn into_parts(self) -> (Option<String>, bool) {
        (self.plan_id, self.record_receipt)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn unsupported_observer_paths_keep_native_signal_handling() {
        let native_commands = [
            RuntimeCommand::Check(CheckCommand::AgentGuides),
            RuntimeCommand::Work(WorkCommand::Receipts(WorkReceiptsRequest {
                session_id: None,
                plan_id: None,
                tool_name: None,
                failed_only: false,
                limit: 10,
            })),
            RuntimeCommand::State(StateCommand::Diagnose(StateDiagnoseRequest { deep: true })),
            RuntimeCommand::State(StateCommand::CompactSessions(StateCompactSessionsRequest {
                dry_run: true,
            })),
            RuntimeCommand::State(StateCommand::Restore(StateRestoreRequest {
                backup: PathBuf::from("backup"),
            })),
            RuntimeCommand::State(StateCommand::ExportReceipts(StateExportReceiptsRequest {
                before: "2026-01-01".into(),
                output: PathBuf::from("receipts.json"),
            })),
            RuntimeCommand::State(StateCommand::Archive(StateArchiveRequest {
                before: "2026-01-01".into(),
                dry_run: true,
            })),
        ];

        for command in native_commands {
            assert_eq!(command.signal_policy(), RuntimeSignalPolicy::Native);
        }
    }

    #[test]
    fn command_backed_and_cancellable_scans_use_cooperative_signals() {
        let cooperative_commands = [
            RuntimeCommand::Check(CheckCommand::Test(ToolRequest::default())),
            RuntimeCommand::Work(WorkCommand::Status),
            RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
            RuntimeCommand::State(StateCommand::Summary),
        ];

        for command in cooperative_commands {
            assert_eq!(command.signal_policy(), RuntimeSignalPolicy::Cooperative);
        }
    }
}
