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
