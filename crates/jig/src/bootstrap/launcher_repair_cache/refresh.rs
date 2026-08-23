use std::{fs, io, path::Path};

use anyhow::{Context, Result};

use crate::context::{RuntimeCacheProfile, runtime_cache_base, runtime_profile_cache_name};
use crate::runtime_cache_lock::{RuntimeCacheLockPolicy, RuntimeCacheLocks};

use super::super::{EMBEDDED_TEMPLATE_SOURCE, HarnessFootprint};
use super::seed::seed_embedded_template_runtime;

pub(crate) const LAUNCHER_REPAIR_SEED_STAMP_HEADER: &str = "jig-seeded-runtime-v1";
pub(in crate::bootstrap) const LAUNCHER_REPAIR_RETIREMENT_RETRY_GUIDANCE: &str = "Launcher-repair cache retirement can be retried by rerunning adopt or update after resolving the cache-lock error";

#[derive(Default)]
pub(in crate::bootstrap) struct LauncherRepairSeedRetirement {
    pub(in crate::bootstrap) retired: usize,
    pub(in crate::bootstrap) errors: Vec<anyhow::Error>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::bootstrap) enum FullRefreshRuntimePolicy {
    NoManagedRuntime,
    ConfiguredSource,
    EmbeddedTemplate,
}

impl FullRefreshRuntimePolicy {
    pub(in crate::bootstrap) fn for_render(
        harness_footprint: HarnessFootprint,
        template_source: &str,
    ) -> Self {
        if harness_footprint == HarnessFootprint::Minimal {
            Self::NoManagedRuntime
        } else if template_source == EMBEDDED_TEMPLATE_SOURCE {
            Self::EmbeddedTemplate
        } else {
            Self::ConfiguredSource
        }
    }
}

pub(in crate::bootstrap) fn retire_launcher_repair_seeded_caches(
    destination: &Path,
    contract_version: u32,
) -> LauncherRepairSeedRetirement {
    let cache_base = runtime_cache_base(destination);
    let mut cache_paths = Vec::new();
    let mut outcome = LauncherRepairSeedRetirement::default();
    for profile in [RuntimeCacheProfile::Default, RuntimeCacheProfile::Runtime] {
        let cache = cache_base.join(runtime_profile_cache_name(contract_version, profile));
        match cache_has_launcher_repair_seed(&cache) {
            Ok(true) => cache_paths.push(cache),
            Ok(false) => {}
            Err(error) => outcome.errors.push(error),
        }
    }
    if cache_paths.is_empty() {
        return outcome;
    }

    let _locks =
        match RuntimeCacheLocks::acquire(&cache_paths, RuntimeCacheLockPolicy::retirement())
            .context(LAUNCHER_REPAIR_RETIREMENT_RETRY_GUIDANCE)
        {
            Ok(locks) => locks,
            Err(error) => {
                outcome.errors.push(error);
                return outcome;
            }
        };
    for cache in cache_paths {
        match cache_has_launcher_repair_seed(&cache) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                outcome.errors.push(error);
                continue;
            }
        }
        // Keep the provenance stamp until last: it is the retry marker that
        // lets a later update recognize an incompletely retired seed.
        let metadata = cache.join(".jig-source-metadata-stamp");
        match fs::remove_file(&metadata) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                outcome
                    .errors
                    .push(anyhow::Error::new(error).context(format!(
                        "Failed to retire launcher-repair metadata {}",
                        metadata.display()
                    )));
                continue;
            }
        }
        let stamp = cache.join(".jig-source-stamp");
        match fs::remove_file(&stamp).with_context(|| {
            format!(
                "Failed to retire launcher-repair provenance {}",
                stamp.display()
            )
        }) {
            Ok(()) => outcome.retired += 1,
            Err(error) => outcome.errors.push(error),
        }
    }
    outcome
}
pub(in crate::bootstrap) fn retire_launcher_repair_seeded_caches_best_effort(
    destination: &Path,
    contract_version: u32,
) -> usize {
    let outcome = retire_launcher_repair_seeded_caches(destination, contract_version);
    for error in outcome.errors {
        eprintln!("{}", launcher_repair_retirement_warning(&error));
    }
    outcome.retired
}

pub(in crate::bootstrap) fn retire_supported_launcher_repair_seeded_caches_best_effort(
    destination: &Path,
) -> usize {
    (crate::context::MIN_SUPPORTED_CONTRACT_VERSION..=crate::context::CURRENT_CONTRACT_VERSION)
        .map(|contract_version| {
            retire_launcher_repair_seeded_caches_best_effort(destination, contract_version)
        })
        .sum()
}

pub(in crate::bootstrap) fn launcher_repair_retirement_warning(error: &anyhow::Error) -> String {
    format!(
        "Warning: harness changes were committed, but launcher-repair cache retirement could not complete: {error:#}"
    )
}

fn cache_has_launcher_repair_seed(cache: &Path) -> Result<bool> {
    let stamp = cache.join(".jig-source-stamp");
    let contents = match fs::read(&stamp) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read runtime provenance {}", stamp.display()));
        }
    };
    Ok(contents
        .split(|byte| *byte == b'\n')
        .next()
        .is_some_and(|header| header == LAUNCHER_REPAIR_SEED_STAMP_HEADER.as_bytes()))
}

pub(in crate::bootstrap) fn finish_full_refresh(
    destination: &Path,
    runtime_policy: FullRefreshRuntimePolicy,
    progress: crate::progress::CliProgress,
    completion_message: &str,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let runtime_replacement_is_durable = match runtime_policy {
        FullRefreshRuntimePolicy::NoManagedRuntime | FullRefreshRuntimePolicy::ConfiguredSource => {
            true
        }
        FullRefreshRuntimePolicy::EmbeddedTemplate => {
            progress.step("runtime cache", "publish embedded-template runtime");
            match seed_embedded_template_runtime(
                destination,
                crate::context::CURRENT_CONTRACT_VERSION,
            ) {
                Ok(publication) => {
                    publication.commit();
                    true
                }
                Err(error) => {
                    let warning = format!(
                        "runtime cache refresh did not complete after the harness files were committed: {error:#}. The harness files remain applied; rerun adopt or update after resolving the cache prerequisite"
                    );
                    eprintln!("Warning: {warning}");
                    warnings.push(warning);
                    false
                }
            }
        }
    };

    // A launcher-repair seed is the last known runnable fallback. Retire it
    // only after an embedded replacement is durable, or when the refreshed
    // harness intentionally has no embedded runtime to replace.
    if runtime_replacement_is_durable {
        let retired = retire_supported_launcher_repair_seeded_caches_best_effort(destination);
        if retired > 0 {
            progress.info(
                "runtime cache",
                format!("retired {retired} launcher-repair seed(s)"),
            );
        }
    }
    progress.done(completion_message);
    warnings
}
