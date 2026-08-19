use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};

use crate::context::{
    LAUNCHER_REPAIR_STAGING_PREFIX, RuntimeCacheProfile, runtime_profile_cache_name,
};
use crate::runtime_cache_lock::{RuntimeCacheLockPolicy, RuntimeCacheLocks};

pub(in crate::bootstrap) const STALE_LAUNCHER_REPAIR_STAGING_AGE: Duration =
    Duration::from_secs(24 * 60 * 60);

#[derive(Debug)]
pub(in crate::bootstrap) struct PublishedLauncherRepairCache {
    pub(in crate::bootstrap) destination: PathBuf,
    pub(in crate::bootstrap) backup: Option<PathBuf>,
}

#[derive(Debug)]
pub(in crate::bootstrap) struct LauncherRepairCachePublication {
    staging: Option<tempfile::TempDir>,
    published: Vec<PublishedLauncherRepairCache>,
    // Ownership is the protocol: these locks are released only after cache
    // publication commits or finishes rolling back.
    _locks: RuntimeCacheLocks,
}

impl LauncherRepairCachePublication {
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            staging: None,
            published: Vec::new(),
            _locks: RuntimeCacheLocks::empty(),
        }
    }

    pub(in crate::bootstrap) fn commit(mut self) {
        self.published.clear();
        drop(self.staging.take());
    }

    pub(in crate::bootstrap) fn finish_failed(mut self, primary: anyhow::Error) -> anyhow::Error {
        let Some(staging) = self.staging.take() else {
            return primary;
        };
        match rollback_published_repair_caches(&staging, &mut self.published) {
            Ok(()) => primary,
            Err(rollback) => preserve_launcher_repair_staging(
                staging,
                primary,
                &[format!(
                    "Failed to roll back repair-cache publication after the rendered-script transaction failed: {rollback:#}"
                )],
            ),
        }
    }
}

impl Drop for LauncherRepairCachePublication {
    fn drop(&mut self) {
        let Some(staging) = self.staging.take() else {
            return;
        };
        if let Err(error) = rollback_published_repair_caches(&staging, &mut self.published) {
            let preserved = staging.keep();
            eprintln!(
                "Warning: failed to roll back an uncommitted launcher-repair cache publication: {error:#}. Recovery artifacts were preserved at {}",
                preserved.display()
            );
        }
    }
}

pub(in crate::bootstrap) fn reap_stale_launcher_repair_staging(
    cache_base: &Path,
    now: SystemTime,
) -> Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(cache_base).with_context(|| {
        format!(
            "Failed to inspect launcher-repair cache root {}",
            cache_base.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "Failed to inspect an entry under launcher-repair cache root {}",
                cache_base.display()
            )
        })?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(LAUNCHER_REPAIR_STAGING_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).with_context(|| {
            format!(
                "Failed to inspect launcher-repair staging {}",
                path.display()
            )
        })?;
        if !metadata.file_type().is_dir() {
            continue;
        }
        let modified = metadata.modified().with_context(|| {
            format!(
                "Failed to read launcher-repair staging timestamp for {}",
                path.display()
            )
        })?;
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < STALE_LAUNCHER_REPAIR_STAGING_AGE
            || launcher_repair_staging_contains_recovery_artifacts(&path)?
        {
            continue;
        }
        fs::remove_dir_all(&path).with_context(|| {
            format!(
                "Failed to remove abandoned launcher-repair staging {}",
                path.display()
            )
        })?;
        removed += 1;
    }
    Ok(removed)
}

fn launcher_repair_staging_contains_recovery_artifacts(path: &Path) -> Result<bool> {
    for entry in fs::read_dir(path).with_context(|| {
        format!(
            "Failed to inspect launcher-repair staging {} for recovery artifacts",
            path.display()
        )
    })? {
        let name = entry
            .with_context(|| {
                format!(
                    "Failed to inspect an entry in launcher-repair staging {}",
                    path.display()
                )
            })?
            .file_name();
        let name = name.to_string_lossy();
        if name.starts_with("backup-") || name.starts_with("displaced-") {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(in crate::bootstrap) fn publish_launcher_repair_caches(
    staging: tempfile::TempDir,
    cache_base: &Path,
    contract_version: u32,
    profiles: &[RuntimeCacheProfile],
) -> Result<LauncherRepairCachePublication> {
    publish_launcher_repair_caches_with_lock_policy(
        staging,
        cache_base,
        contract_version,
        profiles,
        RuntimeCacheLockPolicy::INSTALLER,
    )
}

pub(in crate::bootstrap) fn publish_launcher_repair_caches_with_lock_policy(
    staging: tempfile::TempDir,
    cache_base: &Path,
    contract_version: u32,
    profiles: &[RuntimeCacheProfile],
    lock_policy: RuntimeCacheLockPolicy,
) -> Result<LauncherRepairCachePublication> {
    let destinations = profiles
        .iter()
        .map(|profile| cache_base.join(runtime_profile_cache_name(contract_version, *profile)))
        .collect::<Vec<_>>();
    let locks = RuntimeCacheLocks::acquire(&destinations, lock_policy)?;
    let mut published = Vec::<PublishedLauncherRepairCache>::new();
    for profile in profiles {
        let profile_name = profile.as_str();
        let staged = staging.path().join(profile_name);
        let cache_name = runtime_profile_cache_name(contract_version, *profile);
        let destination = cache_base.join(cache_name);
        let backup = if path_entry_exists(&destination)? {
            let backup = staging.path().join(format!("backup-{profile_name}"));
            if let Err(error) = fs::rename(&destination, &backup) {
                let primary = anyhow::Error::new(error).context(format!(
                    "Failed to preserve existing repair cache {}",
                    destination.display()
                ));
                let rollback = rollback_published_repair_caches(&staging, &mut published)
                    .err()
                    .map(|error| {
                        format!(
                            "Failed to roll back earlier repair-cache publications after preserving {} failed: {error:#}",
                            destination.display()
                        )
                    });
                return Err(match rollback {
                    Some(rollback) => preserve_launcher_repair_staging(
                        staging,
                        primary,
                        std::slice::from_ref(&rollback),
                    ),
                    None => primary,
                });
            }
            Some(backup)
        } else {
            None
        };
        if let Err(error) = fs::rename(&staged, &destination) {
            let primary = anyhow::Error::new(error).context(format!(
                "Failed to publish staged launcher-repair cache {}",
                destination.display()
            ));
            let mut rollback_failures = Vec::new();
            if let Some(backup) = &backup {
                if let Err(error) = fs::rename(backup, &destination) {
                    rollback_failures.push(format!(
                        "Failed to restore repair cache {} after staged publication failed: {error}",
                        destination.display()
                    ));
                }
            }
            if let Err(error) = rollback_published_repair_caches(&staging, &mut published) {
                rollback_failures.push(format!(
                    "Failed to roll back earlier repair-cache publications after publishing {} failed: {error:#}",
                    destination.display()
                ));
            }
            return Err(if rollback_failures.is_empty() {
                primary
            } else {
                preserve_launcher_repair_staging(staging, primary, &rollback_failures)
            });
        }
        published.push(PublishedLauncherRepairCache {
            destination,
            backup,
        });
    }
    Ok(LauncherRepairCachePublication {
        staging: Some(staging),
        published,
        _locks: locks,
    })
}

pub(in crate::bootstrap) fn preserve_launcher_repair_staging(
    staging: tempfile::TempDir,
    primary: anyhow::Error,
    rollback_failures: &[String],
) -> anyhow::Error {
    let preserved = staging.keep();
    anyhow::anyhow!(
        "{primary:#}\nRepair-cache rollback also failed: {}\nRecovery artifacts were preserved at {}",
        rollback_failures.join("; "),
        preserved.display()
    )
}

pub(in crate::bootstrap) fn rollback_published_repair_caches(
    staging: &tempfile::TempDir,
    published: &mut Vec<PublishedLauncherRepairCache>,
) -> Result<()> {
    let mut failures = Vec::new();
    while let Some(cache) = published.pop() {
        let displaced = staging
            .path()
            .join(format!("displaced-{}", published.len()));
        if let Err(error) = fs::rename(&cache.destination, &displaced) {
            failures.push(format!(
                "Failed to withdraw newly published repair cache {}: {error}",
                cache.destination.display()
            ));
            continue;
        }
        if let Some(backup) = cache.backup {
            if let Err(error) = fs::rename(&backup, &cache.destination) {
                failures.push(format!(
                    "Failed to restore previous repair cache {}: {error}",
                    cache.destination.display()
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(failures.join("\n"))
    }
}

fn path_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect repair cache {}", path.display()))
        }
    }
}
