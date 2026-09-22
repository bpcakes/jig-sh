use super::*;

#[test]
fn contextless_proxy_commands_are_limited_to_host_cleanup_and_status() {
    assert!(commands::can_run_without_context(&ProxyCommand::Stop(
        ProxyStopRequest::default()
    )));
    assert!(commands::can_run_without_context(&ProxyCommand::Service(
        ProxyServiceCommand::Status(ProxyServiceRuntimeRequest::default())
    )));
    assert!(commands::can_run_without_context(&ProxyCommand::Start(
        ProxyStartRequest {
            foreground: true,
            certificate_dns_name: Vec::new(),
            proxy: ProxyRuntimeOptions::default(),
        }
    )));
    assert!(!commands::can_run_without_context(&ProxyCommand::Start(
        ProxyStartRequest {
            foreground: false,
            certificate_dns_name: Vec::new(),
            proxy: ProxyRuntimeOptions::default(),
        }
    )));
    assert!(!commands::can_run_without_context(&ProxyCommand::Cert(
        ProxyCertCommand::Generate(ProxyCertGenerateRequest::default())
    )));
}

#[test]
fn contextless_proxy_allowlist_is_exhaustive() {
    let commands = proxy_command_cases();
    let allowed = commands
        .iter()
        .filter_map(|command| {
            commands::can_run_without_context(command).then_some(proxy_command_case_name(command))
        })
        .collect::<Vec<_>>();

    assert_eq!(
        allowed,
        vec![
            "start:foreground",
            "stop",
            "list",
            "prune",
            "cert:status",
            "cert:trust",
            "cert:untrust",
            "service:uninstall",
            "service:status",
        ]
    );
}

fn proxy_command_cases() -> Vec<ProxyCommand> {
    vec![
        ProxyCommand::Start(ProxyStartRequest {
            foreground: true,
            certificate_dns_name: Vec::new(),
            proxy: ProxyRuntimeOptions::default(),
        }),
        ProxyCommand::Start(ProxyStartRequest {
            foreground: false,
            certificate_dns_name: Vec::new(),
            proxy: ProxyRuntimeOptions::default(),
        }),
        ProxyCommand::Stop(ProxyStopRequest::default()),
        ProxyCommand::List(ProxyListRequest::default()),
        ProxyCommand::Prune(ProxyPruneRequest::default()),
        ProxyCommand::Run(ProxyRunRequest {
            name: "web".into(),
            kind: None,
            dir: None,
            port: Some(3000),
            no_proxy: false,
            proxy: ProxyRuntimeOptions::default(),
            command: vec!["npm".into(), "run".into(), "dev".into()],
        }),
        ProxyCommand::Alias(ProxyAliasRequest {
            name: "web".into(),
            port: 3000,
            host: "127.0.0.1".into(),
            accept_non_loopback_target: false,
            proxy: ProxyRuntimeOptions::default(),
        }),
        ProxyCommand::Cert(ProxyCertCommand::Generate(
            ProxyCertGenerateRequest::default(),
        )),
        ProxyCommand::Cert(ProxyCertCommand::Status(ProxyCertRuntimeRequest::default())),
        ProxyCommand::Cert(ProxyCertCommand::Trust(ProxyCertTrustRequest {
            accept_trust_scope: true,
            proxy: ProxyRuntimeOptions::default(),
        })),
        ProxyCommand::Cert(ProxyCertCommand::Untrust(ProxyCertUntrustRequest {
            accept_trust_scope: true,
            proxy: ProxyRuntimeOptions::default(),
        })),
        ProxyCommand::Service(ProxyServiceCommand::Install(ProxyServiceInstallRequest {
            accept_service_scope: true,
            proxy: ProxyRuntimeOptions::default(),
        })),
        ProxyCommand::Service(ProxyServiceCommand::Uninstall(
            ProxyServiceRuntimeRequest::default(),
        )),
        ProxyCommand::Service(ProxyServiceCommand::Status(
            ProxyServiceRuntimeRequest::default(),
        )),
    ]
}

fn proxy_command_case_name(command: &ProxyCommand) -> &'static str {
    match command {
        ProxyCommand::Start(opts) if opts.foreground => "start:foreground",
        ProxyCommand::Start(_) => "start:background",
        ProxyCommand::Stop(_) => "stop",
        ProxyCommand::List(_) => "list",
        ProxyCommand::Prune(_) => "prune",
        ProxyCommand::Run(_) => "run",
        ProxyCommand::Alias(_) => "alias",
        ProxyCommand::Cert(ProxyCertCommand::Generate(_)) => "cert:generate",
        ProxyCommand::Cert(ProxyCertCommand::Status(_)) => "cert:status",
        ProxyCommand::Cert(ProxyCertCommand::Trust(_)) => "cert:trust",
        ProxyCommand::Cert(ProxyCertCommand::Untrust(_)) => "cert:untrust",
        ProxyCommand::Service(ProxyServiceCommand::Install(_)) => "service:install",
        ProxyCommand::Service(ProxyServiceCommand::Uninstall(_)) => "service:uninstall",
        ProxyCommand::Service(ProxyServiceCommand::Status(_)) => "service:status",
    }
}

#[test]
fn contextless_proxy_settings_use_runtime_flags() {
    let temp = tempdir().unwrap();
    let settings = settings_without_context(&ProxyRuntimeOptions {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: Some(1555),
        https_port: Some(1556),
        https: true,
        no_https: false,
        http2: false,
        no_http2: true,
        lan: true,
        no_lan: false,
        tld: Some("Test".into()),
    })
    .unwrap();

    assert_eq!(settings.state_dir, Some(temp.path().to_path_buf()));
    assert_eq!(settings.http_port, 1555);
    assert_eq!(settings.https_port, Some(1556));
    assert!(settings.https);
    assert!(!settings.http2);
    assert!(settings.lan);
    assert_eq!(settings.tld, "test");
    assert!(settings.additional_dns_names.is_empty());
}
