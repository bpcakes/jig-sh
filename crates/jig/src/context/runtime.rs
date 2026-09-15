use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;
#[cfg(not(test))]
use std::sync::OnceLock;

use anyhow::{Context, Result};
use std::fs;

use super::{
    CURRENT_CONTRACT_VERSION, MAX_SUPPORTED_CONTRACT_VERSION, RepoContext,
    find_repo_root_from_or_env,
};

#[derive(serde::Deserialize)]
pub(super) struct ContractVersionProbe {
    pub(super) contract_version: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct RepoConfigProbe {
    pub(crate) repo_name: String,
    pub(crate) jig_version: Option<String>,
}

#[cfg(not(test))]
static PREVALIDATED_LAUNCHER_CONTEXT: OnceLock<RepoContext> = OnceLock::new();
#[cfg(test)]
// Unit tests run several synthetic CLI invocations in one process, so they use
// a resettable global equivalent of the production OnceLock. Keeping this
// process-global preserves the production invariant that worker threads see the
// launcher-validated context; environment-mutating tests serialize access.
static PREVALIDATED_LAUNCHER_CONTEXT: Mutex<Option<RepoContext>> = Mutex::new(None);

pub(crate) const CURRENT_SESSION_FILE: &str = "jig-current-session.txt";
pub(crate) const JIG_REPO_ROOT_ENV: &str = "JIG_REPO_ROOT";
pub(crate) const MIN_SUPPORTED_CONTRACT_VERSION: u32 = 2;
pub(crate) const LAST_LEGACY_CONTRACT_VERSION: u32 = 8;
// Epochs 9 and 10 remain reserved for historical, unreleased receipt formats.
// T1 readers and writers reserve 11 for tracker journals without making it the
// default rendered repository contract before the linked workflow is complete.
pub(crate) const TRACKER_JOURNAL_CONTRACT_VERSION: u32 = 11;
pub(crate) const GIT_RUNTIME_CACHE_BASE: &str = ".git/jig-tools";
pub(crate) const FALLBACK_RUNTIME_CACHE_BASE: &str = ".agent/.cache/jig";
pub(crate) const RUNTIME_CACHE_PROFILE_SUFFIX: &str = "-runtime";
pub(crate) const LAUNCHER_REPAIR_STAGING_PREFIX: &str = ".jig-launcher-repair-";

pub(crate) fn runtime_cache_base(root: &Path) -> PathBuf {
    if root.join(".git").is_dir() {
        root.join(GIT_RUNTIME_CACHE_BASE)
    } else {
        root.join(FALLBACK_RUNTIME_CACHE_BASE)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeCacheProfile {
    Default,
    Runtime,
}

impl RuntimeCacheProfile {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Runtime => "runtime",
        }
    }
}

pub(crate) fn runtime_profile_cache_name(
    contract_version: u32,
    profile: RuntimeCacheProfile,
) -> String {
    match profile {
        RuntimeCacheProfile::Default => format!("contract-{contract_version}"),
        RuntimeCacheProfile::Runtime => {
            format!("contract-{contract_version}{RUNTIME_CACHE_PROFILE_SUFFIX}")
        }
    }
}

pub(crate) fn runtime_profile_cache_path(
    root: &Path,
    contract_version: u32,
    profile: RuntimeCacheProfile,
) -> PathBuf {
    runtime_cache_base(root).join(runtime_profile_cache_name(contract_version, profile))
}

pub(crate) const fn is_supported_contract_version(version: u32) -> bool {
    (version >= MIN_SUPPORTED_CONTRACT_VERSION && version <= LAST_LEGACY_CONTRACT_VERSION)
        || supports_tracker_journals(version)
}

/// A cached launcher advertises only epochs already active in generated
/// repositories. Future epochs may be internally readable before cutover.
pub(crate) const fn is_active_contract_version(version: u32) -> bool {
    is_active_contract_version_at(version, CURRENT_CONTRACT_VERSION)
}

pub(crate) const fn is_active_contract_version_at(version: u32, current_version: u32) -> bool {
    is_supported_contract_version(version) && version <= current_version
}

pub(crate) const fn supports_tracker_journals(contract_version: u32) -> bool {
    contract_version == TRACKER_JOURNAL_CONTRACT_VERSION
}

pub(crate) fn active_contract_versions() -> impl Iterator<Item = u32> {
    active_contract_versions_at(CURRENT_CONTRACT_VERSION)
}

fn active_contract_versions_at(current_version: u32) -> impl Iterator<Item = u32> {
    (MIN_SUPPORTED_CONTRACT_VERSION..=MAX_SUPPORTED_CONTRACT_VERSION)
        .filter(move |version| is_active_contract_version_at(*version, current_version))
}

#[cfg(test)]
pub(crate) fn supported_contract_versions_label() -> String {
    contract_versions_label(
        (MIN_SUPPORTED_CONTRACT_VERSION..=MAX_SUPPORTED_CONTRACT_VERSION)
            .filter(|version| is_supported_contract_version(*version)),
    )
}

pub(crate) fn active_contract_versions_label() -> String {
    active_contract_versions_label_at(CURRENT_CONTRACT_VERSION)
}

pub(crate) fn active_contract_versions_label_at(current_version: u32) -> String {
    contract_versions_label(active_contract_versions_at(current_version))
}

fn contract_versions_label(versions: impl IntoIterator<Item = u32>) -> String {
    let mut versions = versions.into_iter();
    let Some(mut range_start) = versions.next() else {
        return "none".into();
    };
    let mut range_end = range_start;
    let mut ranges = Vec::new();
    for version in versions {
        if version == range_end.saturating_add(1) {
            range_end = version;
        } else {
            ranges.push(contract_version_range_label(range_start, range_end));
            range_start = version;
            range_end = version;
        }
    }
    ranges.push(contract_version_range_label(range_start, range_end));
    match ranges.len() {
        0 => unreachable!("a first contract version was present"),
        1 => ranges.remove(0),
        2 => format!("{} and {}", ranges[0], ranges[1]),
        _ => {
            let last = ranges.pop().expect("at least one range");
            format!("{}, and {last}", ranges.join(", "))
        }
    }
}

fn contract_version_range_label(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start} through {end}")
    }
}

pub(super) fn non_empty_legacy_jig_version<'a>(
    value: Option<&'a str>,
    source: &str,
) -> Result<&'a str> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("legacy jig contract requires a non-empty jig_version in {source}")
        })
}

impl RepoContext {
    pub(crate) fn declared_contract_version_from_root(root: &Path) -> Result<u32> {
        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let probe: ContractVersionProbe = crate::strict_json::from_slice(manifest_text.as_bytes())
            .and_then(serde_json::from_value)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        Ok(probe.contract_version)
    }

    pub(crate) fn load() -> Result<Self> {
        if let Some(ctx) = Self::prevalidated_launcher_context() {
            return Ok(ctx);
        }
        let root = find_repo_root_from_or_env(&std::env::current_dir()?)?;
        Self::load_from_root(root)
    }

    pub(crate) fn remember_prevalidated_launcher_context(self) -> Result<()> {
        #[cfg(not(test))]
        {
            PREVALIDATED_LAUNCHER_CONTEXT
                .set(self)
                .map_err(|_| anyhow::anyhow!("Launcher repository context was already initialized"))
        }
        #[cfg(test)]
        {
            let mut slot = PREVALIDATED_LAUNCHER_CONTEXT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot.is_some() {
                anyhow::bail!("Launcher repository context was already initialized");
            }
            *slot = Some(self);
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn clear_prevalidated_launcher_context() {
        *PREVALIDATED_LAUNCHER_CONTEXT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub(super) fn prevalidated_launcher_context() -> Option<Self> {
        #[cfg(not(test))]
        {
            PREVALIDATED_LAUNCHER_CONTEXT.get().cloned()
        }
        #[cfg(test)]
        {
            PREVALIDATED_LAUNCHER_CONTEXT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .cloned()
        }
    }
}
