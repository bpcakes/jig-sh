#[cfg(not(test))]
use std::time::SystemTime;
use std::{env, fs, path::Path};

#[cfg(not(test))]
use anyhow::Context;
use anyhow::{Result, bail};
#[cfg(not(test))]
use std::path::PathBuf;

use crate::context::{LAUNCHER_REPAIR_STAGING_PREFIX, RuntimeCacheProfile, runtime_cache_base};

#[cfg(not(test))]
use super::publication::reap_stale_launcher_repair_staging;
use super::publication::{LauncherRepairCachePublication, publish_launcher_repair_caches};

pub(in crate::bootstrap) const LAUNCHER_REPAIR_ENVIRONMENT_KEYS: &[&str] = &[
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
pub(in crate::bootstrap) const TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV: &str =
    "JIG_TEST_FAIL_LAUNCHER_REPAIR_SEED";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeSeedPurpose {
    LauncherRepair,
    EmbeddedTemplate,
}

#[cfg(not(test))]
impl RuntimeSeedPurpose {
    const fn installer_value(self) -> &'static str {
        match self {
            Self::LauncherRepair => "launcher-repair",
            Self::EmbeddedTemplate => "embedded-template",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::LauncherRepair => "repaired launcher runtime",
            Self::EmbeddedTemplate => "embedded-template runtime",
        }
    }
}

#[cfg(not(test))]
struct RepairToolEnvironment {
    bash: PathBuf,
    helper_path: std::ffi::OsString,
}

pub(in crate::bootstrap) fn seed_launcher_repair_runtime(
    destination: &Path,
    contract_version: u32,
) -> Result<LauncherRepairCachePublication> {
    seed_runtime_from_current_executable(
        destination,
        contract_version,
        RuntimeSeedPurpose::LauncherRepair,
    )
}

pub(in crate::bootstrap) fn seed_embedded_template_runtime(
    destination: &Path,
    contract_version: u32,
) -> Result<LauncherRepairCachePublication> {
    seed_runtime_from_current_executable(
        destination,
        contract_version,
        RuntimeSeedPurpose::EmbeddedTemplate,
    )
}

#[cfg(test)]
fn seed_runtime_from_current_executable(
    destination: &Path,
    contract_version: u32,
    purpose: RuntimeSeedPurpose,
) -> Result<LauncherRepairCachePublication> {
    if env::var_os(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV).is_some() {
        bail!("injected launcher repair seed failure");
    }
    if purpose == RuntimeSeedPurpose::LauncherRepair {
        return Ok(LauncherRepairCachePublication::empty());
    }

    let cache_base = runtime_cache_base(destination);
    fs::create_dir_all(&cache_base)?;
    let staging = tempfile::Builder::new()
        .prefix(LAUNCHER_REPAIR_STAGING_PREFIX)
        .tempdir_in(&cache_base)?;
    let mut profiles = vec![RuntimeCacheProfile::Runtime];
    if cfg!(feature = "dev-proxy") {
        profiles.push(RuntimeCacheProfile::Default);
    }
    for profile in &profiles {
        let staged = staging.path().join(profile.as_str());
        fs::create_dir_all(staged.join("bin"))?;
        fs::write(staged.join("bin/jig"), "embedded runtime fixture")?;
        fs::write(
            staged.join(".jig-source-stamp"),
            "jig-embedded-runtime-v1\nsource:fixture\n",
        )?;
    }
    publish_launcher_repair_caches(staging, &cache_base, contract_version, &profiles)
}

#[cfg(not(test))]
fn seed_runtime_from_current_executable(
    destination: &Path,
    contract_version: u32,
    purpose: RuntimeSeedPurpose,
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
            purpose,
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
    purpose: RuntimeSeedPurpose,
) -> Result<()> {
    let installer = destination.join("scripts/install-jig.sh");
    // Runtime seeding is a recovery boundary. Resolve Bash and its helper PATH
    // together so the executable and the tools it can invoke share one
    // platform-specific validation policy.
    let tool_environment = repair_tool_environment(destination)?;
    let helper_path_display = tool_environment.helper_path.to_string_lossy().into_owned();
    let mut command = std::process::Command::new(&tool_environment.bash);
    command
        .arg(&installer)
        .arg("--contract-version")
        .arg(contract_version.to_string())
        .arg("--profile")
        .arg(profile.as_str())
        .arg("--seed-dev-bin")
        .arg("--seed-purpose")
        .arg(purpose.installer_value())
        .arg(install_root)
        .env("JIG_DEV_BIN", executable)
        .env("PATH", tool_environment.helper_path)
        .current_dir(destination);
    crate::shell::sanitize_bash_environment(&mut command);
    super::super::scrub_git_repository_environment_except(&mut command, &[]);
    sanitize_launcher_repair_environment(&mut command);
    let output = command.output().with_context(|| {
        format!(
            "Failed to start the {} seeder for profile {} with {}",
            purpose.description(),
            profile.as_str(),
            installer.display()
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "Failed to seed the {} for profile {}{}. Runtime seeding restricts helper commands to a validated platform PATH; ensure Python 3 and standard POSIX tools are available there (helper PATH: {helper_path_display}).",
            purpose.description(),
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

pub(in crate::bootstrap) fn sanitize_launcher_repair_environment(
    command: &mut std::process::Command,
) {
    for &key in LAUNCHER_REPAIR_ENVIRONMENT_KEYS {
        command.env_remove(key);
    }
}

#[cfg(all(not(test), unix))]
fn repair_tool_environment(_destination: &Path) -> Result<RepairToolEnvironment> {
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
            let helper_path = trusted_unix_repair_path(&canonical)?;
            return Ok(RepairToolEnvironment {
                bash: canonical,
                helper_path,
            });
        }
    }
    bail!(
        "Launcher-only repair requires Bash at /bin/bash, /usr/bin/bash, or an executable root-owned non-writable bash on PATH"
    )
}

#[cfg(all(not(test), unix))]
fn trusted_unix_repair_path(bash: &Path) -> Result<std::ffi::OsString> {
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

#[cfg(unix)]
pub(in crate::bootstrap) fn is_root_owned_nonwritable_path(path: &Path) -> bool {
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
pub(in crate::bootstrap) const fn root_owned_nonwritable_component(
    uid: u32,
    mode: u32,
    is_leaf: bool,
) -> bool {
    uid == 0 && (mode & 0o022 == 0 || (!is_leaf && mode & 0o1000 != 0))
}

#[cfg(all(not(test), not(unix)))]
fn repair_tool_environment(_destination: &Path) -> Result<RepairToolEnvironment> {
    bail!("Launcher-only repair requires a validated Bash and helper PATH on this platform")
}
