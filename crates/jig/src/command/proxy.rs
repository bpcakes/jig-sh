//! Development proxy command DTOs.

#![cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]

use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum DevCommand {
    Launch(DevRequest),
    Status(DevStatusRequest),
    Stop(DevStopRequest),
}

#[derive(Debug)]
pub(crate) struct DevRequest {
    pub(crate) apps: Vec<String>,
    pub(crate) discover_workspace: bool,
    pub(crate) no_proxy: bool,
    pub(crate) replace: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct DevStatusRequest {
    pub(crate) state_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub(crate) struct DevStopRequest {
    pub(crate) state_dir: Option<PathBuf>,
    pub(crate) forget_ambiguous_orphans: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProxyRuntimeOptions {
    pub(crate) state_dir: Option<PathBuf>,
    pub(crate) http_port: Option<u16>,
    pub(crate) https_port: Option<u16>,
    pub(crate) https: bool,
    pub(crate) no_https: bool,
    pub(crate) http2: bool,
    pub(crate) no_http2: bool,
    pub(crate) lan: bool,
    pub(crate) no_lan: bool,
    pub(crate) tld: Option<String>,
}

#[derive(Debug)]
pub(crate) enum ProxyCommand {
    Start(ProxyStartRequest),
    Stop(ProxyStopRequest),
    List(ProxyListRequest),
    Prune(ProxyPruneRequest),
    Run(ProxyRunRequest),
    Alias(ProxyAliasRequest),
    Cert(ProxyCertCommand),
    Service(ProxyServiceCommand),
}

#[derive(Debug)]
pub(crate) enum ProxyCertCommand {
    Generate(ProxyCertGenerateRequest),
    Status(ProxyCertRuntimeRequest),
    Trust(ProxyCertTrustRequest),
    Untrust(ProxyCertUntrustRequest),
}

#[derive(Debug)]
pub(crate) enum ProxyServiceCommand {
    Install(ProxyServiceInstallRequest),
    Uninstall(ProxyServiceRuntimeRequest),
    Status(ProxyServiceRuntimeRequest),
}

#[derive(Debug)]
pub(crate) struct ProxyStartRequest {
    pub(crate) foreground: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyStopRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyListRequest {
    pub(crate) raw: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyPruneRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct ProxyRunRequest {
    pub(crate) name: String,
    pub(crate) kind: Option<String>,
    pub(crate) dir: Option<PathBuf>,
    pub(crate) port: Option<u16>,
    pub(crate) no_proxy: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
    pub(crate) command: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct ProxyAliasRequest {
    pub(crate) name: String,
    pub(crate) port: u16,
    pub(crate) host: String,
    pub(crate) accept_non_loopback_target: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyCertGenerateRequest {
    pub(crate) force: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyCertRuntimeRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyCertTrustRequest {
    pub(crate) accept_trust_scope: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyCertUntrustRequest {
    pub(crate) accept_trust_scope: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyServiceInstallRequest {
    pub(crate) accept_service_scope: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[derive(Debug, Default)]
pub(crate) struct ProxyServiceRuntimeRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}
