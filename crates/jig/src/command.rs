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
mod migration;
mod proxy;
mod repository_run;
mod sqlx;
mod state;
mod vault;

pub(crate) use agent::{AgentBootstrapRequest, AgentCommand};
pub(crate) use check::{
    AgentMapCommand, AgentMapRequest, CheckCommand, MigrationImmutabilityRequest,
    RepositoryCheckRequest, SqlxTodoRequest,
};
pub(crate) use loops::{
    LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopCommand, LoopDispatchRequest,
    LoopRunRequest, LoopShowRequest, LoopStatusRequest, LoopTickRequest,
};
pub(crate) use migration::MigrationAddRequest;
pub(crate) use proxy::{
    DevCommand, DevRecoverRequest, DevRequest, DevStatusRequest, DevStopRequest, ProxyAliasRequest,
    ProxyCertCommand, ProxyCertGenerateRequest, ProxyCertRuntimeRequest, ProxyCertTrustRequest,
    ProxyCertUntrustRequest, ProxyCommand, ProxyListRequest, ProxyPruneRequest, ProxyRunRequest,
    ProxyRuntimeOptions, ProxyServiceCommand, ProxyServiceInstallRequest,
    ProxyServiceRuntimeRequest, ProxyStartRequest, ProxyStopRequest,
};
pub(crate) use repository_run::RepositoryRunRequest;
pub(crate) use sqlx::SqlxCommand;
pub(crate) use state::{
    StateArchiveRequest, StateCommand, StateDiagnoseRequest, StateExportReceiptsRequest,
    StateRestoreRequest,
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

#[derive(Debug)]
pub(crate) enum RuntimeCommand {
    Bootstrap,
    Check(CheckCommand),
    Run(RepositoryRunRequest),
    MigrationAdd(MigrationAddRequest),
    Sqlx(SqlxCommand),
    AgentMap(AgentMapCommand),
    GenerateSqlxUncheckedQueriesTodo(SqlxTodoRequest),
    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    Dev(DevCommand),
    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    Proxy(ProxyCommand),
    Agent(AgentCommand),
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
            Self::Run(_)
            | Self::Bootstrap
            | Self::MigrationAdd(_)
            | Self::Sqlx(_)
            | Self::Agent(_) => Cooperative,
            Self::Check(command) => match command {
                CheckCommand::Repository(_)
                | CheckCommand::Fmt
                | CheckCommand::Lint
                | CheckCommand::Clippy
                | CheckCommand::Test
                | CheckCommand::TestLocked
                | CheckCommand::TypeScriptLint
                | CheckCommand::TypeScriptTypecheck
                | CheckCommand::TypeScriptBuild
                | CheckCommand::TypeScriptCoverage
                | CheckCommand::Sqlx
                | CheckCommand::Sqlc
                | CheckCommand::Schema
                | CheckCommand::Contract => Cooperative,
                CheckCommand::AgentMap(_)
                | CheckCommand::AgentGuides
                | CheckCommand::MigrationImmutability(_)
                | CheckCommand::SqlxUncheckedNonTest => Native,
            },
            Self::Loop(command) => match command {
                LoopCommand::Tick(_)
                | LoopCommand::Dispatch(_)
                | LoopCommand::Status(_)
                | LoopCommand::Show(_)
                | LoopCommand::Run(_)
                | LoopCommand::ClearAttempt(_)
                | LoopCommand::AcknowledgeOccurrence(_) => Cooperative,
            },
            Self::State(command) => match command {
                StateCommand::Summary => Cooperative,
                StateCommand::Diagnose(_)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn unsupported_observer_paths_keep_native_signal_handling() {
        let native_commands = [
            RuntimeCommand::Check(CheckCommand::AgentGuides),
            RuntimeCommand::State(StateCommand::Diagnose(StateDiagnoseRequest { deep: true })),
            RuntimeCommand::State(StateCommand::Restore(StateRestoreRequest {
                backup: PathBuf::from("backup"),
            })),
            RuntimeCommand::State(StateCommand::ExportReceipts(StateExportReceiptsRequest {
                before: "2026-01-01".into(),
                output: PathBuf::from("receipts.json"),
            })),
            RuntimeCommand::State(StateCommand::Archive(StateArchiveRequest {
                before: "2026-01-01".into(),
                include_runs: true,
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
            RuntimeCommand::Check(CheckCommand::Test),
            RuntimeCommand::Loop(LoopCommand::Status(LoopStatusRequest { workflow: None })),
            RuntimeCommand::Loop(LoopCommand::ClearAttempt(LoopClearAttemptRequest {
                workflow: "ExampleProject".into(),
                item: "pr-17".into(),
            })),
            RuntimeCommand::Loop(LoopCommand::AcknowledgeOccurrence(
                LoopAcknowledgeOccurrenceRequest {
                    occurrence: "ExampleProject@100".into(),
                },
            )),
            RuntimeCommand::State(StateCommand::Summary),
        ];

        for command in cooperative_commands {
            assert_eq!(command.signal_policy(), RuntimeSignalPolicy::Cooperative);
        }
    }
}
