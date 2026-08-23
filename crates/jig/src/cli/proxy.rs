use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const PROXY_RUN_AFTER_HELP: &str = "\
The app command must come after --. Ad-hoc proxy runs bind the app to 127.0.0.1; use [[dev.apps]].host for configured loopback IP targets.

Examples:
  jig proxy run web -- npm run dev
  jig proxy run web -- vite --open
  jig proxy run api --port 3000 -- cargo run
  jig proxy run web --no-proxy -- npm run dev";

pub(super) const DEV_AFTER_HELP: &str = "\
With no subcommand, `jig dev` launches the configured apps as one foreground
development session. `--replace` stops only conflicting registered sessions
owned by this same canonical repository before launching.

Examples:
  jig dev
  jig dev --app web
  jig dev --replace
  jig dev status
  jig dev stop";

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyCommand {
    /// Start the local development proxy.
    #[command(name = tool_defs::cli_command::PROXY_START)]
    Start(ProxyStartOpts),
    /// Stop the local development proxy.
    #[command(name = tool_defs::cli_command::PROXY_STOP)]
    Stop(ProxyStopOpts),
    /// List proxy routes and runtime status.
    #[command(name = tool_defs::cli_command::PROXY_LIST)]
    List(ProxyListOpts),
    /// Remove stale proxy routes.
    #[command(name = tool_defs::cli_command::PROXY_PRUNE)]
    Prune(ProxyPruneOpts),
    /// Run an ad-hoc app command behind the proxy.
    #[command(name = tool_defs::cli_command::PROXY_RUN)]
    Run(ProxyRunOpts),
    /// Route a stable hostname to an already-running local service.
    #[command(name = tool_defs::cli_command::PROXY_ALIAS)]
    Alias(ProxyAliasOpts),
    /// Generate, inspect, trust, or untrust local proxy certificates.
    #[command(name = tool_defs::cli_command::PROXY_CERT, subcommand)]
    Cert(ProxyCertCommand),
    /// Install, uninstall, or inspect a per-user proxy service.
    #[command(name = tool_defs::cli_command::PROXY_SERVICE, subcommand)]
    Service(ProxyServiceCommand),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyCertCommand {
    #[command(name = tool_defs::cli_command::PROXY_CERT_GENERATE)]
    Generate(ProxyCertGenerateOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT_STATUS)]
    Status(ProxyCertRuntimeOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT_TRUST)]
    Trust(ProxyCertTrustOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT_UNTRUST)]
    Untrust(ProxyCertUntrustOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyServiceCommand {
    #[command(name = tool_defs::cli_command::PROXY_SERVICE_INSTALL)]
    Install(ProxyServiceInstallOpts),
    #[command(name = tool_defs::cli_command::PROXY_SERVICE_UNINSTALL)]
    Uninstall(ProxyServiceRuntimeOpts),
    #[command(name = tool_defs::cli_command::PROXY_SERVICE_STATUS)]
    Status(ProxyServiceRuntimeOpts),
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ProxyRuntimeOpts {
    #[arg(
        long,
        help = "Proxy state directory; defaults to JIG_PROXY_STATE_DIR or ~/.jig/proxy"
    )]
    pub(crate) state_dir: Option<PathBuf>,
    #[arg(
        long,
        help = "HTTP listener port for the local proxy; use 0 for an ephemeral runtime port"
    )]
    pub(crate) http_port: Option<u16>,
    #[arg(
        long,
        help = "HTTPS listener port for the local proxy",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub(crate) https_port: Option<u16>,
    #[arg(
        long,
        conflicts_with = "no_https",
        help = "Start or require the HTTPS listener"
    )]
    pub(crate) https: bool,
    #[arg(
        long,
        conflicts_with = "https",
        help = "Disable HTTPS even when [dev].https is true"
    )]
    pub(crate) no_https: bool,
    // Expert diagnostic toggle kept for service parity while HTTP/2 support is
    // still settling; normal users should rely on the [dev] config default.
    #[arg(
        long,
        hide = true,
        conflicts_with = "no_http2",
        help = "Enable HTTP/2 ALPN on the HTTPS listener"
    )]
    pub(crate) http2: bool,
    // Expert diagnostic toggle kept for service parity while HTTP/2 support is
    // still settling; normal users should rely on the [dev] config default.
    #[arg(
        long,
        hide = true,
        conflicts_with = "http2",
        help = "Disable HTTP/2 ALPN on the HTTPS listener"
    )]
    pub(crate) no_http2: bool,
    #[arg(
        long,
        conflicts_with = "no_lan",
        help = "Bind the proxy on 0.0.0.0; LAN clients can reach Jig-supervised loopback apps"
    )]
    pub(crate) lan: bool,
    #[arg(
        long,
        conflicts_with = "lan",
        help = "Disable LAN binding even when [dev].lan is true"
    )]
    pub(crate) no_lan: bool,
    #[arg(long, help = "Private/local TLD for generated route hostnames")]
    pub(crate) tld: Option<String>,
}

#[derive(Args, Debug)]
#[command(
    args_conflicts_with_subcommands = true,
    flatten_help = true,
    after_help = DEV_AFTER_HELP
)]
pub(crate) struct DevOpts {
    #[command(subcommand)]
    pub(crate) command: Option<DevSubcommand>,
    #[command(flatten)]
    pub(crate) launch: DevLaunchOpts,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DevSubcommand {
    /// Show registered development sessions owned by the current repository.
    #[command(name = tool_defs::cli_command::DEV_STATUS)]
    Status(DevStatusOpts),
    /// Stop all registered development sessions owned by the current repository.
    #[command(name = tool_defs::cli_command::DEV_STOP)]
    Stop(DevStopOpts),
}

#[derive(Args, Debug, Default)]
pub(crate) struct DevLaunchOpts {
    #[arg(
        long = "jig-project",
        hide = true,
        value_name = "NAME@ROOT",
        help = "Internal process-list identity added by Jig"
    )]
    pub(crate) jig_project: Option<OsString>,
    #[arg(long = "app", help = "Configured app name to run; may be repeated")]
    pub(crate) apps: Vec<String>,
    #[arg(long, help = "Discover JavaScript workspace apps with dev scripts")]
    pub(crate) discover_workspace: bool,
    #[arg(long, help = "Run apps directly without publishing proxy routes")]
    pub(crate) no_proxy: bool,
    #[arg(
        long,
        help = "Stop conflicting live dev sessions owned by this repository before launching"
    )]
    pub(crate) replace: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct DevStatusOpts {
    #[arg(
        long,
        help = "Proxy state directory; defaults to JIG_PROXY_STATE_DIR or ~/.jig/proxy"
    )]
    pub(crate) state_dir: Option<PathBuf>,
}

#[derive(Args, Debug, Default)]
pub(crate) struct DevStopOpts {
    #[arg(
        long,
        help = "Proxy state directory; defaults to JIG_PROXY_STATE_DIR or ~/.jig/proxy"
    )]
    pub(crate) state_dir: Option<PathBuf>,
    #[arg(
        long,
        help = "Forget dead-supervisor orphan records with unconfirmed preflight cleanup or unprovable spawn history; never signals stored PIDs"
    )]
    pub(crate) forget_ambiguous_orphans: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ProxyStartOpts {
    #[arg(long, help = "Run the proxy in the foreground instead of detaching")]
    pub(crate) foreground: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyStopOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyListOpts {
    #[arg(long, help = "Print raw route and listener details")]
    pub(crate) raw: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyPruneOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug)]
#[command(after_help = PROXY_RUN_AFTER_HELP)]
pub(crate) struct ProxyRunOpts {
    #[arg(help = "Route name to publish for the ad-hoc app")]
    pub(crate) name: String,
    #[arg(
        long,
        help = "App kind used for command setup, such as env-port or vite"
    )]
    pub(crate) kind: Option<String>,
    #[arg(long, help = "Working directory for the app command")]
    pub(crate) dir: Option<PathBuf>,
    #[arg(long, help = "Fixed backend port for the app", value_parser = clap::value_parser!(u16).range(1..))]
    pub(crate) port: Option<u16>,
    #[arg(long, help = "Run directly without publishing a proxy route")]
    pub(crate) no_proxy: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        required = true,
        help = "Command to run after --, for example: vite --open"
    )]
    pub(crate) command: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ProxyAliasOpts {
    #[arg(help = "Route name to publish for the existing service")]
    pub(crate) name: String,
    #[arg(long, help = "Backend TCP port to route to", value_parser = clap::value_parser!(u16).range(1..))]
    pub(crate) port: u16,
    #[arg(
        long,
        default_value = "127.0.0.1",
        value_parser = parse_ip_literal_string,
        help = "Backend host as an IP literal"
    )]
    pub(crate) host: String,
    #[arg(
        long,
        help = "Acknowledge that this local alias can proxy browser requests to a non-loopback target IP"
    )]
    pub(crate) accept_non_loopback_target: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertGenerateOpts {
    #[arg(
        long,
        help = "Regenerate certificate files even when usable files already exist"
    )]
    pub(crate) force: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertRuntimeOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertTrustOpts {
    #[arg(
        long,
        required = true,
        help = "Acknowledge that the Jig local CA is a non-name-constrained root that can sign certificates for any hostname on this machine, and that Linux trust helpers are resolved from fixed system tool directories"
    )]
    pub(crate) accept_trust_scope: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertUntrustOpts {
    #[arg(
        long,
        required = true,
        help = "Acknowledge that Jig will mutate the platform trust store to remove matching Jig local CA certificates"
    )]
    pub(crate) accept_trust_scope: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyServiceInstallOpts {
    #[arg(
        long,
        required = true,
        help = "Acknowledge that Jig will write and load a per-user launchd/systemd service for the local development proxy"
    )]
    pub(crate) accept_service_scope: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyServiceRuntimeOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

fn parse_ip_literal_string(value: &str) -> std::result::Result<String, String> {
    value
        .parse::<std::net::IpAddr>()
        .map(|_| value.to_string())
        .map_err(|error| format!("'{value}' must be an IP literal: {error}"))
}
