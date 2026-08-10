use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};

use crate::context::{
    LAUNCHER_REPAIR_STAGING_PREFIX, RuntimeCacheProfile, runtime_cache_base,
    runtime_profile_cache_name,
};
use crate::runtime_cache_lock::{RuntimeCacheLockPolicy, RuntimeCacheLocks};

pub(crate) const LAUNCHER_REPAIR_SEED_STAMP_HEADER: &str = "jig-seeded-runtime-v1";
pub(super) const STALE_LAUNCHER_REPAIR_STAGING_AGE: Duration = Duration::from_secs(24 * 60 * 60);
pub(super) const LAUNCHER_REPAIR_RETIREMENT_RETRY_GUIDANCE: &str = "Launcher-repair cache retirement can be retried by rerunning adopt or update after resolving the cache-lock error";
pub(super) const LAUNCHER_REPAIR_ENVIRONMENT_KEYS: &[&str] = &[
    "LD_AUDIT",
    "LD_DEBUG",
    "LD_LIBRARY_PATH",
    "LD_ORIGIN_PATH",
    "LD_PRELOAD",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_IMAGE_SUFFIX",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_ROOT_PATH",
    "JIG_INSTALL_LOCK_TOKEN",
    "PYTHONHOME",
    "PYTHONINSPECT",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PYTHONWARNINGS",
];
#[cfg(test)]
pub(super) const TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV: &str = "JIG_TEST_FAIL_LAUNCHER_REPAIR_SEED";

pub(super) fn retire_launcher_repair_seeded_caches(
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

    let _locks = match RuntimeCacheLocks::acquire(&cache_paths, RuntimeCacheLockPolicy::immediate())
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

#[derive(Default)]
pub(super) struct LauncherRepairSeedRetirement {
    pub(super) retired: usize,
    pub(super) errors: Vec<anyhow::Error>,
}

pub(super) fn retire_launcher_repair_seeded_caches_best_effort(
    destination: &Path,
    contract_version: u32,
) -> usize {
    let outcome = retire_launcher_repair_seeded_caches(destination, contract_version);
    for error in outcome.errors {
        eprintln!("{}", launcher_repair_retirement_warning(&error));
    }
    outcome.retired
}

pub(super) fn launcher_repair_retirement_warning(error: &anyhow::Error) -> String {
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

#[cfg(test)]
pub(super) fn seed_launcher_repair_runtime(
    _destination: &Path,
    _contract_version: u32,
) -> Result<LauncherRepairCachePublication> {
    if env::var_os(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV).is_some() {
        bail!("injected launcher repair seed failure");
    }
    Ok(LauncherRepairCachePublication::empty())
}

#[cfg(not(test))]
pub(super) fn seed_launcher_repair_runtime(
    destination: &Path,
    contract_version: u32,
) -> Result<LauncherRepairCachePublication> {
    let executable = env::current_exe().context("Failed to locate the running Jig binary")?;
    let cache_base = runtime_cache_base(destination);
    fs::create_dir_all(&cache_base).with_context(|| {
        format!(
            "Failed to create repair cache root {}",
            cache_base.display()
        )
    })?;
    if let Err(error) = reap_stale_launcher_repair_staging(&cache_base, SystemTime::now()) {
        eprintln!(
            "Warning: could not remove abandoned launcher-repair staging under {}: {error:#}",
            cache_base.display()
        );
    }
    let staging = tempfile::Builder::new()
        .prefix(LAUNCHER_REPAIR_STAGING_PREFIX)
        .tempdir_in(&cache_base)
        .with_context(|| {
            format!(
                "Failed to create launcher-repair cache staging under {}",
                cache_base.display()
            )
        })?;
    let mut profiles = vec![RuntimeCacheProfile::Runtime];
    let mut default_compatibility = std::process::Command::new(&executable);
    default_compatibility
        .arg("__runtime-compatible")
        .arg("--capability-only")
        .arg("--contract-version")
        .arg(contract_version.to_string())
        .arg("--profile")
        .arg("default")
        .arg(destination)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    sanitize_launcher_repair_environment(&mut default_compatibility);
    let default_compatible = default_compatibility
        .status()
        .is_ok_and(|status| status.success());
    if default_compatible {
        profiles.push(RuntimeCacheProfile::Default);
    }
    for profile in &profiles {
        let install_root = staging.path().join(profile.as_str());
        seed_launcher_repair_profile(
            destination,
            contract_version,
            &executable,
            *profile,
            &install_root,
        )?;
    }
    publish_launcher_repair_caches(staging, &cache_base, contract_version, &profiles)
}

#[cfg(not(test))]
fn seed_launcher_repair_profile(
    destination: &Path,
    contract_version: u32,
    executable: &Path,
    profile: RuntimeCacheProfile,
    install_root: &Path,
) -> Result<()> {
    let installer = destination.join("scripts/install-jig.sh");
    // Launcher repair is a recovery boundary. Execute the freshly rendered
    // installer and all helper commands it resolves from root-owned,
    // non-writable locations. Standard absolute locations are preferred;
    // trusted ambient entries keep Nix-style systems usable.
    let bash = trusted_bash_path()?;
    let trusted_path = trusted_repair_path(&bash)?;
    let trusted_path_display = trusted_path.to_string_lossy().into_owned();
    let mut command = std::process::Command::new(&bash);
    command
        .arg(&installer)
        .arg("--contract-version")
        .arg(contract_version.to_string())
        .arg("--profile")
        .arg(profile.as_str())
        .arg("--seed-dev-bin")
        .arg(install_root)
        .env("JIG_DEV_BIN", executable)
        .env("PATH", trusted_path)
        .current_dir(destination);
    crate::shell::sanitize_bash_environment(&mut command);
    super::scrub_git_repository_environment_except(&mut command, &[]);
    sanitize_launcher_repair_environment(&mut command);
    let output = command.output().with_context(|| {
        format!(
            "Failed to start the repaired launcher runtime seeder for profile {} with {}",
            profile.as_str(),
            installer.display()
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "Failed to seed the repaired launcher runtime for profile {}{}. Launcher repair restricts helper commands to root-owned, non-writable PATH entries; ensure Python 3 and standard POSIX tools are available there (trusted PATH: {trusted_path_display}).",
            profile.as_str(),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(())
}

pub(super) fn sanitize_launcher_repair_environment(command: &mut std::process::Command) {
    for &key in LAUNCHER_REPAIR_ENVIRONMENT_KEYS {
        command.env_remove(key);
    }
}

#[cfg(all(not(test), unix))]
fn trusted_bash_path() -> Result<PathBuf> {
    let mut candidates = vec![PathBuf::from("/bin/bash"), PathBuf::from("/usr/bin/bash")];
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|directory| directory.join("bash")));
    }
    for candidate in candidates {
        let Ok(canonical) = fs::canonicalize(&candidate) else {
            continue;
        };
        let Ok(metadata) = fs::metadata(&canonical) else {
            continue;
        };
        use std::os::unix::fs::PermissionsExt;
        if metadata.is_file()
            && metadata.permissions().mode() & 0o111 != 0
            && is_root_owned_nonwritable_path(&canonical)
        {
            return Ok(canonical);
        }
    }
    bail!(
        "Launcher-only repair requires Bash at /bin/bash, /usr/bin/bash, or an executable root-owned non-writable bash on PATH"
    )
}

#[cfg(all(not(test), unix))]
fn trusted_repair_path(bash: &Path) -> Result<std::ffi::OsString> {
    let mut candidates = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if let Some(parent) = bash.parent() {
        candidates.push(parent.to_path_buf());
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path));
    }

    let mut trusted = Vec::new();
    for candidate in candidates {
        let Ok(canonical) = fs::canonicalize(candidate) else {
            continue;
        };
        let Ok(metadata) = fs::metadata(&canonical) else {
            continue;
        };
        if metadata.is_dir()
            && is_root_owned_nonwritable_path(&canonical)
            && !trusted.contains(&canonical)
        {
            trusted.push(canonical);
        }
    }
    if trusted.is_empty() {
        bail!("Launcher-only repair could not construct a trusted helper-command PATH");
    }
    env::join_paths(trusted).context("Failed to construct the launcher-repair helper-command PATH")
}

#[cfg(all(not(test), not(unix)))]
fn trusted_repair_path(_bash: &Path) -> Result<std::ffi::OsString> {
    bail!("Launcher-only repair requires a trusted helper-command PATH on this platform")
}

#[cfg(unix)]
pub(super) fn is_root_owned_nonwritable_path(path: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    path.ancestors().enumerate().all(|(index, ancestor)| {
        fs::metadata(ancestor).is_ok_and(|metadata| {
            root_owned_nonwritable_component(
                metadata.uid(),
                metadata.permissions().mode(),
                index == 0,
            )
        })
    })
}

#[cfg(unix)]
pub(super) const fn root_owned_nonwritable_component(uid: u32, mode: u32, is_leaf: bool) -> bool {
    uid == 0 && (mode & 0o022 == 0 || (!is_leaf && mode & 0o1000 != 0))
}

#[cfg(all(not(test), not(unix)))]
fn trusted_bash_path() -> Result<PathBuf> {
    bail!("Launcher-only repair requires a trusted Bash executable")
}

pub(super) fn reap_stale_launcher_repair_staging(
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

#[derive(Debug)]
pub(super) struct PublishedLauncherRepairCache {
    pub(super) destination: PathBuf,
    pub(super) backup: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) struct LauncherRepairCachePublication {
    staging: Option<tempfile::TempDir>,
    published: Vec<PublishedLauncherRepairCache>,
    // Ownership is the protocol: these locks are released only after cache
    // publication commits or finishes rolling back.
    _locks: RuntimeCacheLocks,
}

impl LauncherRepairCachePublication {
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            staging: None,
            published: Vec::new(),
            _locks: RuntimeCacheLocks::empty(),
        }
    }

    pub(super) fn commit(mut self) {
        self.published.clear();
        drop(self.staging.take());
    }

    pub(super) fn finish_failed(mut self, primary: anyhow::Error) -> anyhow::Error {
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

pub(super) fn publish_launcher_repair_caches(
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

pub(super) fn publish_launcher_repair_caches_with_lock_policy(
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

pub(super) fn preserve_launcher_repair_staging(
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

pub(super) fn rollback_published_repair_caches(
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
