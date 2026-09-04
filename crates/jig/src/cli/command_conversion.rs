use anyhow::{Result, bail};
use clap::ValueEnum;

use crate::command;

use super::{
    AgentBootstrapOpts, AgentCommand, AgentMapCommand, AgentMapOpts, CheckCommand,
    CheckComparisonOpts, CheckExactTreeProvenance, CheckMigrationImmutabilityOpts, CheckOpts,
    CheckTargetOpts, DevLaunchOpts, DevOpts, DevStatusOpts, DevStopOpts, DevSubcommand,
    GenerateSqlxUncheckedQueriesTodoOpts, LoopClearAttemptOpts, LoopCommand, LoopRunOpts,
    LoopStatusOpts, LoopTickOpts, ProxyAliasOpts, ProxyCertCommand, ProxyCertGenerateOpts,
    ProxyCertRuntimeOpts, ProxyCertTrustOpts, ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts,
    ProxyPruneOpts, ProxyRunOpts, ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts,
    ProxyServiceRuntimeOpts, ProxyStartOpts, ProxyStopOpts, StateArchiveOpts, StateCommand,
    StateCompactCommand, StateCompactSessionsOpts, StateDiagnoseOpts, StateExportCommand,
    StateExportReceiptsOpts, StateRestoreOpts, ToolOpts, WorkAppendOpts, WorkCheckOpts,
    WorkCommand, WorkDecisionAddOpts, WorkEvidenceOpts, WorkFinishOpts, WorkGatesOpts,
    WorkGoalOpts, WorkReceiptsOpts, WorkRefineOpts, WorkReviewOpts, WorkStartOpts,
};

impl From<ToolOpts> for command::ToolRequest {
    fn from(opts: ToolOpts) -> Self {
        Self::new(opts.plan_id, !opts.no_receipt)
    }
}

impl From<AgentMapCommand> for command::AgentMapCommand {
    fn from(command: AgentMapCommand) -> Self {
        match command {
            AgentMapCommand::Generate(opts) => Self::Generate(opts.into()),
        }
    }
}

impl From<AgentMapOpts> for command::AgentMapRequest {
    fn from(opts: AgentMapOpts) -> Self {
        Self {
            map_path: opts.map_path,
        }
    }
}

impl TryFrom<CheckOpts> for command::CheckCommand {
    type Error = anyhow::Error;

    fn try_from(opts: CheckOpts) -> Result<Self> {
        let CheckOpts {
            mut tool,
            mut profile,
            mut affected,
            mut explain,
            mut fail_fast,
            mut comparison,
            command,
        } = opts;

        let command = match command {
            Some(CheckCommand::Selectors(selectors)) => {
                Some(CheckCommand::Selectors(normalize_external_check_args(
                    selectors,
                    &mut tool,
                    &mut profile,
                    &mut affected,
                    &mut explain,
                    &mut fail_fast,
                    &mut comparison,
                )?))
            }
            command => command,
        };
        let comparison = comparison.request()?;

        match command {
            None => Ok(Self::Repository(command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile,
                affected_base: affected,
                comparison,
                explain,
                fail_fast,
                tool: tool.into(),
            })),
            Some(CheckCommand::Selectors(selectors)) => {
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors,
                    profile,
                    affected_base: affected,
                    comparison,
                    explain,
                    fail_fast,
                    tool: tool.into(),
                }))
            }
            Some(command)
                if profile.is_some()
                    || affected.is_some()
                    || comparison.is_some()
                    || explain
                    || fail_fast
                    || command.has_additional_selectors() =>
            {
                let (selector, child) = repository_selector(command)?;
                let mut selectors = Vec::with_capacity(child.selectors.len() + 1);
                selectors.push(selector.into());
                selectors.extend(child.selectors);
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors,
                    profile,
                    affected_base: affected,
                    comparison,
                    explain,
                    fail_fast,
                    tool: merge_tool_opts(tool, child.tool)?.into(),
                }))
            }
            // Preserve the named command DTO until runtime has loaded the
            // repository contract. `dispatch_named_check` executes the legacy
            // manifest tool only for v2-v5; v6 resolves this name as a
            // repository selector so every component action is included.
            Some(command) => direct_check_command(command, tool),
        }
    }
}

include!("command_conversion/external_check.rs");

fn direct_check_command(
    command: CheckCommand,
    parent_tool: ToolOpts,
) -> Result<command::CheckCommand> {
    let command = match command {
        CheckCommand::Fmt(opts) => {
            command::CheckCommand::Fmt(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::Lint(opts) => {
            command::CheckCommand::Lint(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::Clippy(opts) => {
            command::CheckCommand::Clippy(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::Test(opts) => {
            command::CheckCommand::Test(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::TestLocked(opts) => {
            command::CheckCommand::TestLocked(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::TypeScriptLint(opts) => {
            command::CheckCommand::TypeScriptLint(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::TypeScriptTypecheck(opts) => command::CheckCommand::TypeScriptTypecheck(
            merge_tool_opts(parent_tool, opts.tool)?.into(),
        ),
        CheckCommand::TypeScriptBuild(opts) => {
            command::CheckCommand::TypeScriptBuild(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::TypeScriptCoverage(opts) => command::CheckCommand::TypeScriptCoverage(
            merge_tool_opts(parent_tool, opts.tool)?.into(),
        ),
        CheckCommand::Sqlx(opts) => {
            command::CheckCommand::Sqlx(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::Sqlc(opts) => {
            command::CheckCommand::Sqlc(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::Schema(opts) => {
            command::CheckCommand::Schema(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::Contract(opts) => {
            command::CheckCommand::Contract(merge_tool_opts(parent_tool, opts.tool)?.into())
        }
        CheckCommand::AgentMap(opts) => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::AgentMap(opts.into())
        }
        CheckCommand::AgentGuides => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::AgentGuides
        }
        CheckCommand::MigrationImmutability(opts) => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::MigrationImmutability(opts.into())
        }
        CheckCommand::SqlxUncheckedNonTest => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::SqlxUncheckedNonTest
        }
        CheckCommand::Selectors(_) => {
            unreachable!("external selectors are handled before direct commands")
        }
    };
    Ok(command)
}

fn repository_selector(command: CheckCommand) -> Result<(&'static str, CheckTargetOpts)> {
    match command {
        CheckCommand::Fmt(opts) => Ok(("fmt", opts)),
        CheckCommand::Lint(opts) => Ok(("lint", opts)),
        CheckCommand::Clippy(opts) => Ok(("clippy", opts)),
        CheckCommand::Test(opts) => Ok(("test", opts)),
        CheckCommand::TestLocked(opts) => Ok(("test-locked", opts)),
        CheckCommand::TypeScriptLint(opts) => Ok(("typescript-lint", opts)),
        CheckCommand::TypeScriptTypecheck(opts) => Ok(("typescript-typecheck", opts)),
        CheckCommand::TypeScriptBuild(opts) => Ok(("typescript-build", opts)),
        CheckCommand::TypeScriptCoverage(opts) => Ok(("typescript-coverage", opts)),
        CheckCommand::Sqlx(opts) => Ok(("sqlx", opts)),
        CheckCommand::Sqlc(opts) => Ok(("sqlc", opts)),
        CheckCommand::Schema(opts) => Ok(("schema", opts)),
        CheckCommand::Contract(opts) => Ok(("contract", opts)),
        CheckCommand::AgentMap(_)
        | CheckCommand::AgentGuides
        | CheckCommand::MigrationImmutability(_)
        | CheckCommand::SqlxUncheckedNonTest => {
            bail!(
                "profiles, affected selection, --explain, and --fail-fast apply to repository targets, not Jig-owned policy subcommands"
            )
        }
        CheckCommand::Selectors(_) => unreachable!("external selectors are handled separately"),
    }
}

fn merge_tool_opts(parent: ToolOpts, child: ToolOpts) -> Result<ToolOpts> {
    if parent.plan_id.is_some() && child.plan_id.is_some() {
        bail!("--plan-id may be supplied before or after the check name, not both");
    }
    let plan_id = parent.plan_id.or(child.plan_id);
    let no_receipt = parent.no_receipt || child.no_receipt;
    if plan_id.is_some() && no_receipt {
        bail!("--plan-id cannot be combined with --no-receipt");
    }
    Ok(ToolOpts {
        plan_id,
        no_receipt,
    })
}

fn reject_repository_options(tool: &ToolOpts) -> Result<()> {
    if tool.plan_id.is_some() || tool.no_receipt {
        bail!(
            "--plan-id and --no-receipt apply to repository target checks, not Jig-owned policy subcommands"
        );
    }
    Ok(())
}

impl From<CheckMigrationImmutabilityOpts> for command::MigrationImmutabilityRequest {
    fn from(opts: CheckMigrationImmutabilityOpts) -> Self {
        Self {
            changed_against: opts.changed_against,
        }
    }
}

impl From<GenerateSqlxUncheckedQueriesTodoOpts> for command::SqlxTodoRequest {
    fn from(opts: GenerateSqlxUncheckedQueriesTodoOpts) -> Self {
        Self {
            output: opts.output,
        }
    }
}

impl From<AgentCommand> for command::AgentCommand {
    fn from(command: AgentCommand) -> Self {
        match command {
            AgentCommand::Doctor => Self::Doctor,
            AgentCommand::Bootstrap(opts) => Self::Bootstrap(opts.into()),
        }
    }
}

impl From<AgentBootstrapOpts> for command::AgentBootstrapRequest {
    fn from(opts: AgentBootstrapOpts) -> Self {
        Self {
            marketplace: opts.marketplace,
        }
    }
}

impl From<WorkCommand> for command::WorkCommand {
    fn from(command: WorkCommand) -> Self {
        match command {
            WorkCommand::Goal(opts) => Self::Goal(opts.into()),
            WorkCommand::Start(opts) => Self::Start(opts.into()),
            WorkCommand::Append(opts) => Self::Append(opts.into()),
            WorkCommand::Check(opts) => Self::Check(opts.into()),
            WorkCommand::Gates(opts) => Self::Gates(opts.into()),
            WorkCommand::Evidence(opts) => Self::Evidence(opts.into()),
            WorkCommand::Review(opts) => Self::Review(opts.into()),
            WorkCommand::Refine(opts) => Self::Refine(opts.into()),
            WorkCommand::Decide(opts) => Self::Decide(opts.into()),
            WorkCommand::Receipts(opts) => Self::Receipts(opts.into()),
            WorkCommand::Status => Self::Status,
            WorkCommand::Finish(opts) => Self::Finish(opts.into()),
        }
    }
}

impl From<WorkGoalOpts> for command::WorkGoalRequest {
    fn from(opts: WorkGoalOpts) -> Self {
        Self {
            objective: opts.objective,
            success: opts.success,
            validations: opts.validations,
            constraints: opts.constraints,
            checkpoints: opts.checkpoints,
            title: opts.title,
            notes: opts.notes,
        }
    }
}

impl From<WorkStartOpts> for command::WorkStartRequest {
    fn from(opts: WorkStartOpts) -> Self {
        // `--print-plan-id` changes CLI rendering only; runtime still opens
        // the same plan and returns the same structured payload.
        Self {
            title: opts.title,
            body: opts.body,
            body_file: opts.body_file,
            base: opts.base,
        }
    }
}

impl From<WorkAppendOpts> for command::WorkAppendRequest {
    fn from(opts: WorkAppendOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
            body: opts.body,
            body_file: opts.body_file,
        }
    }
}

impl From<WorkCheckOpts> for command::WorkCheckRequest {
    fn from(opts: WorkCheckOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
            gates: opts.gates,
            tools: opts.tools,
        }
    }
}

impl From<WorkGatesOpts> for command::WorkGatesRequest {
    fn from(opts: WorkGatesOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
        }
    }
}

impl From<WorkEvidenceOpts> for command::WorkEvidenceRequest {
    fn from(opts: WorkEvidenceOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
        }
    }
}

impl From<WorkReviewOpts> for command::WorkReviewRequest {
    fn from(opts: WorkReviewOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
            gates: opts.gates,
        }
    }
}

impl From<WorkRefineOpts> for command::WorkRefineRequest {
    fn from(opts: WorkRefineOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
            gates: opts.gates,
            max_iterations: opts.max_iterations,
        }
    }
}

impl From<WorkReceiptsOpts> for command::WorkReceiptsRequest {
    fn from(opts: WorkReceiptsOpts) -> Self {
        Self {
            session_id: opts.session_id,
            plan_id: opts.plan_id,
            tool_name: opts.tool_name,
            failed_only: opts.failed_only,
            limit: opts.limit,
        }
    }
}

impl From<WorkFinishOpts> for command::WorkFinishRequest {
    fn from(opts: WorkFinishOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
            resolution: opts.resolution,
            outcome: opts.outcome,
        }
    }
}

impl From<WorkDecisionAddOpts> for command::WorkDecisionRequest {
    fn from(opts: WorkDecisionAddOpts) -> Self {
        Self {
            title: opts.title,
            selected_option: opts.selected_option,
            rationale: opts.rationale,
            alternatives: opts.alternatives,
            plan_id: opts.plan_id,
        }
    }
}

impl From<LoopCommand> for command::LoopCommand {
    fn from(command: LoopCommand) -> Self {
        match command {
            LoopCommand::Tick(opts) => Self::Tick(opts.into()),
            LoopCommand::Status(opts) => Self::Status(opts.into()),
            LoopCommand::Run(opts) => Self::Run(opts.into()),
            LoopCommand::ClearAttempt(opts) => Self::ClearAttempt(opts.into()),
        }
    }
}

impl From<LoopTickOpts> for command::LoopTickRequest {
    fn from(opts: LoopTickOpts) -> Self {
        Self {
            workflow: Some(opts.workflow),
            lease_ttl_seconds: opts.tuning.lease_ttl_seconds,
            max_attempts: opts.tuning.max_attempts,
            backoff_seconds: opts.tuning.backoff_seconds,
        }
    }
}

impl From<LoopStatusOpts> for command::LoopStatusRequest {
    fn from(opts: LoopStatusOpts) -> Self {
        Self {
            workflow: opts.workflow,
        }
    }
}

impl From<LoopRunOpts> for command::LoopRunRequest {
    fn from(opts: LoopRunOpts) -> Self {
        Self {
            workflow: Some(opts.workflow),
            until: opts.until,
            max_ticks: opts.max_ticks,
            lease_ttl_seconds: opts.tuning.lease_ttl_seconds,
            max_attempts: opts.tuning.max_attempts,
            backoff_seconds: opts.tuning.backoff_seconds,
        }
    }
}

impl From<LoopClearAttemptOpts> for command::LoopClearAttemptRequest {
    fn from(opts: LoopClearAttemptOpts) -> Self {
        Self {
            workflow: opts.workflow,
            item: opts.item,
        }
    }
}

impl From<StateCommand> for command::StateCommand {
    fn from(command: StateCommand) -> Self {
        match command {
            StateCommand::Summary => Self::Summary,
            StateCommand::Diagnose(opts) => Self::Diagnose(opts.into()),
            StateCommand::Compact { command } => match command {
                StateCompactCommand::Sessions(opts) => Self::CompactSessions(opts.into()),
            },
            StateCommand::Restore(opts) => Self::Restore(opts.into()),
            StateCommand::Export { command } => match command {
                StateExportCommand::Receipts(opts) => Self::ExportReceipts(opts.into()),
            },
            StateCommand::Archive(opts) => Self::Archive(opts.into()),
        }
    }
}

impl From<StateDiagnoseOpts> for command::StateDiagnoseRequest {
    fn from(opts: StateDiagnoseOpts) -> Self {
        Self { deep: opts.deep }
    }
}

impl From<StateCompactSessionsOpts> for command::StateCompactSessionsRequest {
    fn from(opts: StateCompactSessionsOpts) -> Self {
        Self {
            dry_run: opts.dry_run,
        }
    }
}

impl From<StateRestoreOpts> for command::StateRestoreRequest {
    fn from(opts: StateRestoreOpts) -> Self {
        Self {
            backup: opts.backup,
        }
    }
}

impl From<StateExportReceiptsOpts> for command::StateExportReceiptsRequest {
    fn from(opts: StateExportReceiptsOpts) -> Self {
        Self {
            before: opts.before,
            output: opts.output,
        }
    }
}

impl From<StateArchiveOpts> for command::StateArchiveRequest {
    fn from(opts: StateArchiveOpts) -> Self {
        Self {
            before: opts.before,
            include_runs: opts.include_runs,
            dry_run: opts.dry_run,
        }
    }
}

impl From<DevOpts> for command::DevCommand {
    fn from(opts: DevOpts) -> Self {
        match opts.command {
            None => Self::Launch(opts.launch.into()),
            Some(DevSubcommand::Status(opts)) => Self::Status(opts.into()),
            Some(DevSubcommand::Stop(opts)) => Self::Stop(opts.into()),
        }
    }
}

impl From<DevLaunchOpts> for command::DevRequest {
    fn from(opts: DevLaunchOpts) -> Self {
        Self {
            apps: opts.apps,
            discover_workspace: opts.discover_workspace,
            no_proxy: opts.no_proxy,
            replace: opts.replace,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<DevStatusOpts> for command::DevStatusRequest {
    fn from(opts: DevStatusOpts) -> Self {
        Self {
            state_dir: opts.state_dir,
        }
    }
}

impl From<DevStopOpts> for command::DevStopRequest {
    fn from(opts: DevStopOpts) -> Self {
        Self {
            state_dir: opts.state_dir,
            forget_ambiguous_orphans: opts.forget_ambiguous_orphans,
        }
    }
}

impl From<ProxyRuntimeOpts> for command::ProxyRuntimeOptions {
    fn from(opts: ProxyRuntimeOpts) -> Self {
        Self {
            state_dir: opts.state_dir,
            http_port: opts.http_port,
            https_port: opts.https_port,
            https: opts.https,
            no_https: opts.no_https,
            http2: opts.http2,
            no_http2: opts.no_http2,
            lan: opts.lan,
            no_lan: opts.no_lan,
            tld: opts.tld,
        }
    }
}

impl From<ProxyCommand> for command::ProxyCommand {
    fn from(command: ProxyCommand) -> Self {
        match command {
            ProxyCommand::Start(opts) => Self::Start(opts.into()),
            ProxyCommand::Stop(opts) => Self::Stop(opts.into()),
            ProxyCommand::List(opts) => Self::List(opts.into()),
            ProxyCommand::Prune(opts) => Self::Prune(opts.into()),
            ProxyCommand::Run(opts) => Self::Run(opts.into()),
            ProxyCommand::Alias(opts) => Self::Alias(opts.into()),
            ProxyCommand::Cert(command) => Self::Cert(command.into()),
            ProxyCommand::Service(command) => Self::Service(command.into()),
        }
    }
}

impl From<ProxyCertCommand> for command::ProxyCertCommand {
    fn from(command: ProxyCertCommand) -> Self {
        match command {
            ProxyCertCommand::Generate(opts) => Self::Generate(opts.into()),
            ProxyCertCommand::Status(opts) => Self::Status(opts.into()),
            ProxyCertCommand::Trust(opts) => Self::Trust(opts.into()),
            ProxyCertCommand::Untrust(opts) => Self::Untrust(opts.into()),
        }
    }
}

impl From<ProxyServiceCommand> for command::ProxyServiceCommand {
    fn from(command: ProxyServiceCommand) -> Self {
        match command {
            ProxyServiceCommand::Install(opts) => Self::Install(opts.into()),
            ProxyServiceCommand::Uninstall(opts) => Self::Uninstall(opts.into()),
            ProxyServiceCommand::Status(opts) => Self::Status(opts.into()),
        }
    }
}

impl From<ProxyStartOpts> for command::ProxyStartRequest {
    fn from(opts: ProxyStartOpts) -> Self {
        Self {
            foreground: opts.foreground,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyStopOpts> for command::ProxyStopRequest {
    fn from(opts: ProxyStopOpts) -> Self {
        Self {
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyListOpts> for command::ProxyListRequest {
    fn from(opts: ProxyListOpts) -> Self {
        Self {
            raw: opts.raw,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyPruneOpts> for command::ProxyPruneRequest {
    fn from(opts: ProxyPruneOpts) -> Self {
        Self {
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyRunOpts> for command::ProxyRunRequest {
    fn from(opts: ProxyRunOpts) -> Self {
        Self {
            name: opts.name,
            kind: opts.kind,
            dir: opts.dir,
            port: opts.port,
            no_proxy: opts.no_proxy,
            proxy: opts.proxy.into(),
            command: opts.command,
        }
    }
}

impl From<ProxyAliasOpts> for command::ProxyAliasRequest {
    fn from(opts: ProxyAliasOpts) -> Self {
        Self {
            name: opts.name,
            port: opts.port,
            host: opts.host,
            accept_non_loopback_target: opts.accept_non_loopback_target,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyCertGenerateOpts> for command::ProxyCertGenerateRequest {
    fn from(opts: ProxyCertGenerateOpts) -> Self {
        Self {
            force: opts.force,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyCertRuntimeOpts> for command::ProxyCertRuntimeRequest {
    fn from(opts: ProxyCertRuntimeOpts) -> Self {
        Self {
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyCertTrustOpts> for command::ProxyCertTrustRequest {
    fn from(opts: ProxyCertTrustOpts) -> Self {
        Self {
            accept_trust_scope: opts.accept_trust_scope,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyCertUntrustOpts> for command::ProxyCertUntrustRequest {
    fn from(opts: ProxyCertUntrustOpts) -> Self {
        Self {
            accept_trust_scope: opts.accept_trust_scope,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyServiceInstallOpts> for command::ProxyServiceInstallRequest {
    fn from(opts: ProxyServiceInstallOpts) -> Self {
        Self {
            accept_service_scope: opts.accept_service_scope,
            proxy: opts.proxy.into(),
        }
    }
}

impl From<ProxyServiceRuntimeOpts> for command::ProxyServiceRuntimeRequest {
    fn from(opts: ProxyServiceRuntimeOpts) -> Self {
        Self {
            proxy: opts.proxy.into(),
        }
    }
}

#[cfg(test)]
mod tests;

mod vault;
