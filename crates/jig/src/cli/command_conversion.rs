use anyhow::{Result, bail};
use clap::ValueEnum;

use crate::command;

use super::{
    AgentBootstrapOpts, AgentCommand, AgentMapCommand, AgentMapOpts, CheckCommand,
    CheckComparisonOpts, CheckMigrationImmutabilityOpts, CheckOpts, CheckTargetOpts,
    CliExactTreeProvenance, DevLaunchOpts, DevOpts, DevRecoverOpts, DevStatusOpts, DevStopOpts,
    DevSubcommand, GenerateSqlxUncheckedQueriesTodoOpts, LoopAcknowledgeOccurrenceOpts,
    LoopClearAttemptOpts, LoopCommand, LoopDispatchOpts, LoopRunOpts, LoopStatusOpts, LoopTickOpts,
    ProxyAliasOpts, ProxyCertCommand, ProxyCertGenerateOpts, ProxyCertRuntimeOpts,
    ProxyCertTrustOpts, ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts, ProxyPruneOpts,
    ProxyRunOpts, ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts,
    ProxyServiceRuntimeOpts, ProxyStartOpts, ProxyStopOpts, StateArchiveOpts, StateCommand,
    StateRestoreOpts, ToolOpts,
};

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
            })),
            Some(CheckCommand::Selectors(selectors)) => {
                Ok(Self::Repository(command::RepositoryCheckRequest {
                    selectors,
                    profile,
                    affected_base: affected,
                    comparison,
                    explain,
                    fail_fast,
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
                }))
            }
            // Preserve the named command DTO until runtime has loaded the
            // repository contract. `dispatch_named_check` executes the legacy
            // manifest tool only for v2-v5; v6 resolves this name as a
            // repository selector so every component action is included.
            Some(command) => Ok(direct_check_command(command)),
        }
    }
}

include!("command_conversion/external_check.rs");

fn direct_check_command(command: CheckCommand) -> command::CheckCommand {
    match command {
        CheckCommand::Fmt(_) => command::CheckCommand::Fmt,
        CheckCommand::Lint(_) => command::CheckCommand::Lint,
        CheckCommand::Clippy(_) => command::CheckCommand::Clippy,
        CheckCommand::Test(_) => command::CheckCommand::Test,
        CheckCommand::TestLocked(_) => command::CheckCommand::TestLocked,
        CheckCommand::TypeScriptLint(_) => command::CheckCommand::TypeScriptLint,
        CheckCommand::TypeScriptTypecheck(_) => command::CheckCommand::TypeScriptTypecheck,
        CheckCommand::TypeScriptBuild(_) => command::CheckCommand::TypeScriptBuild,
        CheckCommand::TypeScriptCoverage(_) => command::CheckCommand::TypeScriptCoverage,
        CheckCommand::Sqlx(_) => command::CheckCommand::Sqlx,
        CheckCommand::Sqlc(_) => command::CheckCommand::Sqlc,
        CheckCommand::Schema(_) => command::CheckCommand::Schema,
        CheckCommand::Contract(_) => command::CheckCommand::Contract,
        CheckCommand::AgentMap(opts) => command::CheckCommand::AgentMap(opts.into()),
        CheckCommand::AgentGuides => command::CheckCommand::AgentGuides,
        CheckCommand::MigrationImmutability(opts) => {
            command::CheckCommand::MigrationImmutability(opts.into())
        }
        CheckCommand::SqlxUncheckedNonTest => command::CheckCommand::SqlxUncheckedNonTest,
        CheckCommand::Selectors(_) => {
            unreachable!("external selectors are handled before direct commands")
        }
    }
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

impl From<StateCommand> for command::StateCommand {
    fn from(command: StateCommand) -> Self {
        match command {
            StateCommand::Summary => Self::Summary,
            StateCommand::Diagnose => Self::Diagnose,
            StateCommand::Restore(opts) => Self::Restore(opts.into()),
            StateCommand::Archive(opts) => Self::Archive(opts.into()),
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
            Some(DevSubcommand::Recover(opts)) => Self::Recover(opts.into()),
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
            all: opts.all,
            session: opts.session,
        }
    }
}

impl From<DevRecoverOpts> for command::DevRecoverRequest {
    fn from(opts: DevRecoverOpts) -> Self {
        Self {
            state_dir: opts.state_dir,
            session: opts.session,
        }
    }
}

impl From<DevStopOpts> for command::DevStopRequest {
    fn from(opts: DevStopOpts) -> Self {
        Self {
            state_dir: opts.state_dir,
            session: opts.session,
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
            certificate_dns_name: opts.certificate_dns_name,
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

mod loops;
mod vault;
