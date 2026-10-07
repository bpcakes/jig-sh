//! Internal provider boundary for agent homes and transparent launches.
//!
//! Providers own discovery, credentials, configuration identity, and report schemas.
//! The CLI owns orchestration; terminal code receives only display metadata and updates.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde_json::Value;

pub struct Metadata {
    pub command: &'static str,
    pub homes_command: &'static str,
    pub name: &'static str,
    pub executable: &'static str,
    pub executable_env: &'static str,
    pub usage: bool,
    /// Whether plain home listings inspect accounts (rather than only directories).
    pub inspect_on_list: bool,
    pub subscription_bucket: Option<&'static str>,
}

impl Metadata {
    pub fn executable(&self) -> OsString {
        std::env::var_os(self.executable_env).unwrap_or_else(|| self.executable.into())
    }
}

pub struct Choice<H> {
    pub selection: H,
    pub path: PathBuf,
    pub name: String,
    pub current: bool,
    pub details: Vec<(String, String)>,
}

pub struct Discovery<H> {
    pub choices: Vec<Choice<H>>,
    pub warnings: Vec<String>,
    pub inspection: Option<Box<dyn HomeInspection>>,
}

/// Emits normalized, secret-free reports keyed by original discovery index.
/// Implementations must observe cancellation and retire their children before returning.
pub trait HomeInspection: Send + Sync {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String>;
}

pub struct PreparedLaunch {
    pub command: Command,
    pub report: Value,
    pub error_context: String,
}

pub trait AgentProvider {
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
pub trait SessionProvider: AgentProvider {
    /// Implementations must observe cancellation and retire their children before returning.
    fn resolve_session(
        &self,
        session: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Self::Home>;
}
