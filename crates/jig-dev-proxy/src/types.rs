use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::host::{RouteHostname, TargetHost};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RouteMode {
    Process,
    Alias,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Route {
    pub(crate) hostname: RouteHostname,
    pub(crate) target_host: TargetHost,
    pub(crate) target_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_start_token: Option<String>,
    pub(crate) mode: RouteMode,
    pub(crate) created_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct ProxySettings {
    pub state_dir: Option<PathBuf>,
    pub http_port: u16,
    pub https_port: Option<u16>,
    pub https: bool,
    pub http2: bool,
    pub lan: bool,
    pub tld: String,
    pub additional_dns_names: Vec<String>,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            state_dir: None,
            http_port: 1355,
            https_port: Some(1443),
            https: false,
            http2: true,
            lan: false,
            tld: "localhost".into(),
            additional_dns_names: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AppKind {
    EnvPort,
    Vite,
}

impl AppKind {
    /// Parses a configured development application kind.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not a supported application kind.
    pub fn from_config(value: &str) -> Result<Self> {
        match value {
            "env-port" => Ok(Self::EnvPort),
            "vite" => Ok(Self::Vite),
            _ => bail!("Unsupported dev app kind '{value}'. Expected 'env-port' or 'vite'."),
        }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CommandSpec {
    Argv(Vec<String>),
    Shell(String),
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AppRunSpec {
    pub name: String,
    pub dir: PathBuf,
    pub command: CommandSpec,
    pub kind: AppKind,
    pub hostname: String,
    pub target_host: String,
    pub explicit_port: Option<u16>,
    pub proxy: bool,
}

impl AppRunSpec {
    pub fn new(
        name: impl Into<String>,
        dir: PathBuf,
        command: CommandSpec,
        hostname: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            dir,
            command,
            kind: AppKind::EnvPort,
            hostname: hostname.into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: true,
        }
    }

    pub const fn with_kind(mut self, kind: AppKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_target_host(mut self, target_host: impl Into<String>) -> Self {
        self.target_host = target_host.into();
        self
    }

    pub const fn with_explicit_port(mut self, explicit_port: Option<u16>) -> Self {
        self.explicit_port = explicit_port;
        self
    }

    pub const fn with_proxy(mut self, proxy: bool) -> Self {
        self.proxy = proxy;
        self
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DevRequest {
    pub repo_name: String,
    pub root: PathBuf,
    pub package_manager: String,
    pub settings: ProxySettings,
    pub apps: Vec<AppRunSpec>,
    pub selected_apps: Vec<String>,
    pub discover_workspace: bool,
    pub no_proxy: bool,
    pub replace: bool,
}

impl DevRequest {
    pub fn new(
        repo_name: impl Into<String>,
        root: PathBuf,
        package_manager: impl Into<String>,
        settings: ProxySettings,
    ) -> Self {
        Self {
            repo_name: repo_name.into(),
            root,
            package_manager: package_manager.into(),
            settings,
            apps: Vec::new(),
            selected_apps: Vec::new(),
            discover_workspace: false,
            no_proxy: false,
            replace: false,
        }
    }

    pub fn with_apps(mut self, apps: Vec<AppRunSpec>) -> Self {
        self.apps = apps;
        self
    }

    pub fn with_selected_apps(mut self, selected_apps: Vec<String>) -> Self {
        self.selected_apps = selected_apps;
        self
    }

    pub const fn with_discover_workspace(mut self, discover_workspace: bool) -> Self {
        self.discover_workspace = discover_workspace;
        self
    }

    pub const fn with_no_proxy(mut self, no_proxy: bool) -> Self {
        self.no_proxy = no_proxy;
        self
    }

    pub const fn with_replace(mut self, replace: bool) -> Self {
        self.replace = replace;
        self
    }
}

#[derive(Clone, Debug)]
pub struct DevStatusRequest {
    pub repo_name: String,
    pub root: PathBuf,
    pub state_dir: Option<PathBuf>,
}

impl DevStatusRequest {
    pub fn new(repo_name: impl Into<String>, root: PathBuf, state_dir: Option<PathBuf>) -> Self {
        Self {
            repo_name: repo_name.into(),
            root,
            state_dir,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DevStopRequest {
    pub repo_name: String,
    pub root: PathBuf,
    pub state_dir: Option<PathBuf>,
}

impl DevStopRequest {
    pub fn new(repo_name: impl Into<String>, root: PathBuf, state_dir: Option<PathBuf>) -> Self {
        Self {
            repo_name: repo_name.into(),
            root,
            state_dir,
        }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProxyStartRequest {
    pub settings: ProxySettings,
    pub foreground: bool,
}

impl ProxyStartRequest {
    pub const fn new(settings: ProxySettings, foreground: bool) -> Self {
        Self {
            settings,
            foreground,
        }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProxyStopRequest {
    pub settings: ProxySettings,
}

impl ProxyStopRequest {
    pub const fn new(settings: ProxySettings) -> Self {
        Self { settings }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProxyListRequest {
    pub settings: ProxySettings,
    pub raw: bool,
}

impl ProxyListRequest {
    pub const fn new(settings: ProxySettings, raw: bool) -> Self {
        Self { settings, raw }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProxyPruneRequest {
    pub settings: ProxySettings,
}

impl ProxyPruneRequest {
    pub const fn new(settings: ProxySettings) -> Self {
        Self { settings }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProxyRunRequest {
    pub settings: ProxySettings,
    pub spec: AppRunSpec,
}

impl ProxyRunRequest {
    pub const fn new(settings: ProxySettings, spec: AppRunSpec) -> Self {
        Self { settings, spec }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProxyAliasRequest {
    pub settings: ProxySettings,
    pub repo_name: String,
    pub name: String,
    pub target_host: String,
    pub target_port: u16,
    pub accept_non_loopback_target: bool,
}

impl ProxyAliasRequest {
    pub fn new(
        settings: ProxySettings,
        repo_name: impl Into<String>,
        name: impl Into<String>,
        target_host: impl Into<String>,
        target_port: u16,
    ) -> Self {
        Self {
            settings,
            repo_name: repo_name.into(),
            name: name.into(),
            target_host: target_host.into(),
            target_port,
            accept_non_loopback_target: false,
        }
    }

    pub const fn with_accept_non_loopback_target(
        mut self,
        accept_non_loopback_target: bool,
    ) -> Self {
        self.accept_non_loopback_target = accept_non_loopback_target;
        self
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ProxyCertRequest {
    Generate {
        settings: ProxySettings,
        force: bool,
    },
    Status {
        settings: ProxySettings,
    },
    Trust {
        settings: ProxySettings,
        accept_trust_scope: bool,
    },
    Untrust {
        settings: ProxySettings,
        accept_trust_scope: bool,
    },
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ProxyServiceRequest {
    Install {
        settings: ProxySettings,
        current_exe: PathBuf,
        repo_root: PathBuf,
        accept_service_scope: bool,
    },
    Uninstall {
        settings: ProxySettings,
    },
    Status {
        settings: ProxySettings,
    },
}
