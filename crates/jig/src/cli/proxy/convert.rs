use crate::command;

use super::{
    DevLaunchOpts, DevOpts, DevRecoverOpts, DevStatusOpts, DevStopOpts, DevSubcommand,
    ProxyAliasOpts, ProxyCertCommand, ProxyCertGenerateOpts, ProxyCertRuntimeOpts,
    ProxyCertTrustOpts, ProxyCertUntrustOpts, ProxyCommand, ProxyListOpts, ProxyPruneOpts,
    ProxyRunOpts, ProxyRuntimeOpts, ProxyServiceCommand, ProxyServiceInstallOpts,
    ProxyServiceRuntimeOpts, ProxyStartOpts, ProxyStopOpts,
};

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
mod tests {
    use super::*;

    #[test]
    fn dev_conversion_preserves_default_launch_and_replace() {
        let request: command::DevCommand = DevOpts {
            command: None,
            launch: DevLaunchOpts {
                jig_worker_fd: None,
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
                ..DevStatusOpts::default()
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
                ..DevStopOpts::default()
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
}
