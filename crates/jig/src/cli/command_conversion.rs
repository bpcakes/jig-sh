use crate::command;

use super::{
    AgentBootstrapOpts, AgentCommand, AgentMapCommand, AgentMapOpts, CheckCommand,
    CheckMigrationImmutabilityOpts, CheckRustFileLocOpts, DevLaunchOpts, DevOpts, DevStatusOpts,
    DevStopOpts, DevSubcommand, GenerateSqlxUncheckedQueriesTodoOpts, LoopClearAttemptOpts,
    LoopCommand, LoopDispatchOpts, LoopRunOpts, LoopStatusOpts, LoopTickOpts, ProxyAliasOpts,
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

impl From<CheckCommand> for command::CheckCommand {
    fn from(command: CheckCommand) -> Self {
        match command {
            CheckCommand::Fmt(opts) => Self::Fmt(opts.into()),
            CheckCommand::Clippy(opts) => Self::Clippy(opts.into()),
            CheckCommand::Test(opts) => Self::Test(opts.into()),
            CheckCommand::TestLocked(opts) => Self::TestLocked(opts.into()),
            CheckCommand::TypeScriptLint(opts) => Self::TypeScriptLint(opts.into()),
            CheckCommand::TypeScriptTypecheck(opts) => Self::TypeScriptTypecheck(opts.into()),
            CheckCommand::TypeScriptBuild(opts) => Self::TypeScriptBuild(opts.into()),
            CheckCommand::TypeScriptCoverage(opts) => Self::TypeScriptCoverage(opts.into()),
            CheckCommand::Sqlx(opts) => Self::Sqlx(opts.into()),
            CheckCommand::Schema(opts) => Self::Schema(opts.into()),
            CheckCommand::Contract(opts) => Self::Contract(opts.into()),
            CheckCommand::AgentMap(opts) => Self::AgentMap(opts.into()),
            CheckCommand::AgentGuides => Self::AgentGuides,
            CheckCommand::RustFileLoc(opts) => Self::RustFileLoc(opts.into()),
            CheckCommand::NoModRs => Self::NoModRs,
            CheckCommand::MigrationImmutability(opts) => Self::MigrationImmutability(opts.into()),
            CheckCommand::SqlxUncheckedNonTest => Self::SqlxUncheckedNonTest,
        }
    }
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
            LoopCommand::Dispatch(opts) => Self::Dispatch(opts.into()),
            LoopCommand::Status(opts) => Self::Status(opts.into()),
            LoopCommand::Run(opts) => Self::Run(opts.into()),
            LoopCommand::ClearAttempt(opts) => Self::ClearAttempt(opts.into()),
        }
    }
}

impl From<LoopDispatchOpts> for command::LoopDispatchRequest {
    fn from(_: LoopDispatchOpts) -> Self {
        Self {}
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
            })),
            launch: DevLaunchOpts::default(),
        }
        .into();
        match stop {
            command::DevCommand::Stop(request) => {
                assert_eq!(request.state_dir, Some("/tmp/stop".into()));
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
