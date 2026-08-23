use anyhow::{Result, bail};

use crate::command;

use super::{
    AgentBootstrapOpts, AgentCommand, AgentMapCommand, AgentMapOpts, CheckCommand,
    CheckMigrationImmutabilityOpts, CheckOpts, CheckRustFileLocOpts, DevLaunchOpts, DevOpts,
    DevStatusOpts, DevStopOpts, DevSubcommand, GenerateSqlxUncheckedQueriesTodoOpts,
    LoopClearAttemptOpts, LoopCommand, LoopRunOpts, LoopStatusOpts, LoopTickOpts, ProxyAliasOpts,
    ProxyCertCommand, ProxyCertGenerateOpts, ProxyCertRuntimeOpts, ProxyCertTrustOpts,
    ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts, ProxyPruneOpts, ProxyRunOpts,
    ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts, ProxyServiceRuntimeOpts,
    ProxyStartOpts, ProxyStopOpts, StateArchiveOpts, StateCommand, StateCompactCommand,
    StateCompactSessionsOpts, StateDiagnoseOpts, StateExportCommand, StateExportReceiptsOpts,
    StateRestoreOpts, ToolOpts, WorkAppendOpts, WorkCheckOpts, WorkCommand, WorkDecisionAddOpts,
    WorkEvidenceOpts, WorkFinishOpts, WorkGatesOpts, WorkGoalOpts, WorkReceiptsOpts,
    WorkRefineOpts, WorkReviewOpts, WorkStartOpts,
};

mod vault;

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
            command,
        } = opts;

        match command {
            None => Ok(Self::Repository(command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile,
                affected_base: affected,
                explain,
                fail_fast,
                tool: tool.into(),
            })),
            Some(CheckCommand::Selectors(selectors)) => {
                let selectors = normalize_external_check_args(
                    selectors,
                    &mut tool,
                    &mut profile,
                    &mut affected,
                    &mut explain,
                    &mut fail_fast,
                )?;
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors,
                    profile,
                    affected_base: affected,
                    explain,
                    fail_fast,
                    tool: tool.into(),
                }))
            }
            Some(command) if profile.is_some() || affected.is_some() || explain || fail_fast => {
                let (selector, child_tool) = repository_selector(command)?;
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors: vec![selector.into()],
                    profile,
                    affected_base: affected,
                    explain,
                    fail_fast,
                    tool: merge_tool_opts(tool, child_tool)?.into(),
                }))
            }
            Some(command) => direct_check_command(command, tool),
        }
    }
}

fn normalize_external_check_args(
    raw: Vec<String>,
    tool: &mut ToolOpts,
    profile: &mut Option<String>,
    affected: &mut Option<String>,
    explain: &mut bool,
    fail_fast: &mut bool,
) -> Result<Vec<String>> {
    let mut selectors = Vec::new();
    let mut args = raw.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-receipt" => tool.no_receipt = true,
            "--explain" => *explain = true,
            "--fail-fast" => *fail_fast = true,
            "--plan-id" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--plan-id requires a value"))?;
                set_external_value(&mut tool.plan_id, value, "--plan-id")?;
            }
            "--profile" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--profile requires a value"))?;
                set_external_value(profile, value, "--profile")?;
            }
            "--affected" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--affected requires a value"))?;
                set_external_value(affected, value, "--affected")?;
            }
            _ if arg.starts_with("--plan-id=") => set_external_value(
                &mut tool.plan_id,
                arg["--plan-id=".len()..].to_owned(),
                "--plan-id",
            )?,
            _ if arg.starts_with("--profile=") => {
                set_external_value(profile, arg["--profile=".len()..].to_owned(), "--profile")?
            }
            _ if arg.starts_with("--affected=") => set_external_value(
                affected,
                arg["--affected=".len()..].to_owned(),
                "--affected",
            )?,
            _ if arg.starts_with('-') => anyhow::bail!("unknown check option '{arg}'"),
            _ => selectors.push(arg),
        }
    }
    if tool.no_receipt && tool.plan_id.is_some() {
        anyhow::bail!("--no-receipt cannot be combined with --plan-id");
    }
    Ok(selectors)
}

fn set_external_value(target: &mut Option<String>, value: String, option: &str) -> Result<()> {
    if value.is_empty() {
        anyhow::bail!("{option} requires a non-empty value");
    }
    if target.replace(value).is_some() {
        anyhow::bail!("{option} cannot be used more than once");
    }
    Ok(())
}

fn direct_check_command(
    command: CheckCommand,
    parent_tool: ToolOpts,
) -> Result<command::CheckCommand> {
    let command = match command {
        CheckCommand::Fmt(opts) => {
            command::CheckCommand::Fmt(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Lint(opts) => {
            command::CheckCommand::Lint(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Clippy(opts) => {
            command::CheckCommand::Clippy(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Test(opts) => {
            command::CheckCommand::Test(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::TestLocked(opts) => {
            command::CheckCommand::TestLocked(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::TypeScriptLint(opts) => {
            command::CheckCommand::TypeScriptLint(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::TypeScriptTypecheck(opts) => {
            command::CheckCommand::TypeScriptTypecheck(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::TypeScriptBuild(opts) => {
            command::CheckCommand::TypeScriptBuild(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::TypeScriptCoverage(opts) => {
            command::CheckCommand::TypeScriptCoverage(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Sqlx(opts) => {
            command::CheckCommand::Sqlx(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Sqlc(opts) => {
            command::CheckCommand::Sqlc(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Schema(opts) => {
            command::CheckCommand::Schema(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::Contract(opts) => {
            command::CheckCommand::Contract(merge_tool_opts(parent_tool, opts)?.into())
        }
        CheckCommand::AgentMap(opts) => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::AgentMap(opts.into())
        }
        CheckCommand::AgentGuides => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::AgentGuides
        }
        CheckCommand::RustFileLoc(opts) => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::RustFileLoc(opts.into())
        }
        CheckCommand::NoModRs => {
            reject_repository_options(&parent_tool)?;
            command::CheckCommand::NoModRs
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

fn repository_selector(command: CheckCommand) -> Result<(&'static str, ToolOpts)> {
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
        | CheckCommand::RustFileLoc(_)
        | CheckCommand::NoModRs
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

impl From<CheckRustFileLocOpts> for command::RustFileLocRequest {
    fn from(opts: CheckRustFileLocOpts) -> Self {
        Self {
            staged: opts.staged,
            changed_against: opts.changed_against,
            all: opts.all,
        }
    }
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
mod tests {
    use super::*;

    #[test]
    fn external_check_selectors_accept_execution_flags_after_targets() {
        let request = command::CheckCommand::try_from(CheckOpts {
            tool: ToolOpts::default(),
            profile: None,
            affected: None,
            explain: false,
            fail_fast: false,
            command: Some(CheckCommand::Selectors(vec![
                "api:test".into(),
                "web:lint".into(),
                "--no-receipt".into(),
                "--fail-fast".into(),
            ])),
        })
        .unwrap();

        let command::CheckCommand::Repository(request) = request else {
            panic!("expected repository check request");
        };
        assert_eq!(request.selectors, ["api:test", "web:lint"]);
        assert!(request.fail_fast);
        assert_eq!(request.tool.into_parts(), (None, false));
    }

    #[test]
    fn dev_conversion_preserves_default_launch_and_replace() {
        let request: command::DevCommand = DevOpts {
            command: None,
            launch: DevLaunchOpts {
                jig_project: Some("demo@/tmp/demo".into()),
                apps: vec!["web".into(), "api".into()],
                discover_workspace: true,
                no_proxy: false,
                replace: true,
                proxy: ProxyRuntimeOpts {
                    state_dir: Some("/tmp/proxy".into()),
                    https: true,
                    ..Default::default()
                },
            },
        }
        .into();

        match request {
            command::DevCommand::Launch(request) => {
                assert_eq!(request.apps, vec!["web", "api"]);
                assert!(request.discover_workspace);
                assert!(!request.no_proxy);
                assert!(request.replace);
                assert_eq!(request.proxy.state_dir, Some("/tmp/proxy".into()));
                assert!(request.proxy.https);
            }
            other => panic!("expected dev launch request, got {other:?}"),
        }
    }

    #[test]
    fn dev_conversion_preserves_management_action_state_dirs() {
        let status: command::DevCommand = DevOpts {
            command: Some(DevSubcommand::Status(DevStatusOpts {
                state_dir: Some("/tmp/status".into()),
            })),
            launch: DevLaunchOpts::default(),
        }
        .into();
        match status {
            command::DevCommand::Status(request) => {
                assert_eq!(request.state_dir, Some("/tmp/status".into()));
            }
            other => panic!("expected dev status request, got {other:?}"),
        }

        let stop: command::DevCommand = DevOpts {
            command: Some(DevSubcommand::Stop(DevStopOpts {
                state_dir: Some("/tmp/stop".into()),
                forget_ambiguous_orphans: true,
            })),
            launch: DevLaunchOpts::default(),
        }
        .into();
        match stop {
            command::DevCommand::Stop(request) => {
                assert_eq!(request.state_dir, Some("/tmp/stop".into()));
                assert!(request.forget_ambiguous_orphans);
            }
            other => panic!("expected dev stop request, got {other:?}"),
        }
    }

    #[test]
    fn work_receipts_conversion_preserves_filters() {
        let request: command::WorkReceiptsRequest = WorkReceiptsOpts {
            session_id: Some("session_1".to_string()),
            plan_id: Some("plan_1".to_string()),
            tool_name: Some(crate::tool_defs::tool::TEST.to_string()),
            failed_only: true,
            limit: 7,
        }
        .into();

        assert_eq!(request.session_id.as_deref(), Some("session_1"));
        assert_eq!(request.plan_id.as_deref(), Some("plan_1"));
        assert_eq!(
            request.tool_name.as_deref(),
            Some(crate::tool_defs::tool::TEST)
        );
        assert!(request.failed_only);
        assert_eq!(request.limit, 7);
    }

    #[test]
    fn work_evidence_conversion_preserves_plan_id() {
        let request: command::WorkEvidenceRequest = WorkEvidenceOpts {
            plan_id: Some("plan_1".to_string()),
        }
        .into();

        assert_eq!(request.plan_id.as_deref(), Some("plan_1"));
    }

    #[test]
    fn state_archive_conversion_preserves_cutoff_and_dry_run() {
        let request: command::StateCommand = StateCommand::Archive(StateArchiveOpts {
            before: "2026-01-01".into(),
            dry_run: true,
        })
        .into();

        match request {
            command::StateCommand::Archive(request) => {
                assert_eq!(request.before, "2026-01-01");
                assert!(request.dry_run);
            }
            other => panic!("expected state archive request, got {other:?}"),
        }
    }

    #[test]
    fn state_maintenance_conversion_preserves_arguments() {
        let request: command::StateCommand =
            StateCommand::Diagnose(StateDiagnoseOpts { deep: true }).into();
        match request {
            command::StateCommand::Diagnose(request) => assert!(request.deep),
            other => panic!("expected state diagnose request, got {other:?}"),
        }

        let request: command::StateCommand = StateCommand::Compact {
            command: StateCompactCommand::Sessions(StateCompactSessionsOpts { dry_run: true }),
        }
        .into();
        match request {
            command::StateCommand::CompactSessions(request) => {
                assert!(request.dry_run);
            }
            other => {
                panic!("expected state compact sessions request, got {other:?}")
            }
        }

        let backup = std::path::PathBuf::from("backup/manifest.json");
        let request: command::StateCommand = StateCommand::Restore(StateRestoreOpts {
            backup: backup.clone(),
        })
        .into();
        match request {
            command::StateCommand::Restore(request) => {
                assert_eq!(request.backup, backup);
            }
            other => panic!("expected state restore request, got {other:?}"),
        }

        let output = std::path::PathBuf::from("receipts.jsonl.gz");
        let request: command::StateCommand = StateCommand::Export {
            command: StateExportCommand::Receipts(StateExportReceiptsOpts {
                before: "2026-01-01".into(),
                output: output.clone(),
            }),
        }
        .into();
        match request {
            command::StateCommand::ExportReceipts(request) => {
                assert_eq!(request.before, "2026-01-01");
                assert_eq!(request.output, output);
            }
            other => {
                panic!("expected state export receipts request, got {other:?}")
            }
        }
    }
}
