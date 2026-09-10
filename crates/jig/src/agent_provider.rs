//! Internal provider boundary for agent homes and transparent launches.
//!
//! Providers own discovery, credentials, configuration identity, and report schemas.
//! The CLI owns orchestration; terminal code receives only display metadata and updates.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde_json::Value;

pub(crate) struct Metadata {
    pub(crate) command: &'static str,
    pub(crate) homes_command: &'static str,
    pub(crate) name: &'static str,
    pub(crate) executable: &'static str,
    pub(crate) executable_env: &'static str,
    pub(crate) usage: bool,
    /// Whether plain home listings inspect accounts (rather than only directories).
    pub(crate) inspect_on_list: bool,
    pub(crate) subscription_bucket: Option<&'static str>,
}

impl Metadata {
    pub(crate) fn executable(&self) -> OsString {
        std::env::var_os(self.executable_env).unwrap_or_else(|| self.executable.into())
    }
}

pub(crate) struct Choice<H> {
    pub(crate) selection: H,
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) current: bool,
    pub(crate) details: Vec<(String, String)>,
}

pub(crate) struct Discovery<H> {
    pub(crate) choices: Vec<Choice<H>>,
    pub(crate) warnings: Vec<String>,
    pub(crate) inspection: Option<Box<dyn HomeInspection>>,
}

/// Emits normalized, secret-free reports keyed by original discovery index.
/// Implementations must observe cancellation and retire their children before returning.
pub(crate) trait HomeInspection: Send + Sync {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String>;
}

pub(crate) struct PreparedLaunch {
    pub(crate) command: Command,
    pub(crate) report: Value,
    pub(crate) error_context: String,
}

pub(crate) trait AgentProvider {
    type Home;
    const METADATA: Metadata;

    fn resolve(&self, input: &Path) -> Result<Self::Home>;
    fn discover(&self) -> Result<Discovery<Self::Home>>;
    /// Recheck a choice after the interactive wait, retaining provider-specific modes.
    fn revalidate(&self, home: &Self::Home) -> Result<Self::Home>;
    /// Prepare exact argv/environment and a display-only report without spawning or reading auth.
    fn prepare(&self, home: &Self::Home, args: &[OsString]) -> Result<PreparedLaunch>;
    /// Providers retain their established report schema and inspection scheduling policy.
    fn homes_report(
        &self,
        usage: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Value>;
}

/// Optional capability: resolving sessions is not required of every agent provider.
pub(crate) trait SessionProvider: AgentProvider {
    fn resolve_session(
        &self,
        session: &str,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Self::Home>;
}
