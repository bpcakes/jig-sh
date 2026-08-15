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

use super::{EMBEDDED_TEMPLATE_SOURCE, HarnessFootprint};

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

pub(super) fn retire_supported_launcher_repair_seeded_caches_best_effort(
    destination: &Path,
) -> usize {
    (crate::context::MIN_SUPPORTED_CONTRACT_VERSION..=crate::context::CURRENT_CONTRACT_VERSION)
        .map(|contract_version| {
            retire_launcher_repair_seeded_caches_best_effort(destination, contract_version)
        })
        .sum()
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeSeedPurpose {
    LauncherRepair,
    EmbeddedTemplate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FullRefreshRuntimePolicy {
    NoManagedRuntime,
    ConfiguredSource,
    EmbeddedTemplate,
}

impl FullRefreshRuntimePolicy {
    pub(super) fn for_render(harness_footprint: HarnessFootprint, template_source: &str) -> Self {
        if harness_footprint == HarnessFootprint::Minimal {
            Self::NoManagedRuntime
        } else if template_source == EMBEDDED_TEMPLATE_SOURCE {
            Self::EmbeddedTemplate
        } else {
            Self::ConfiguredSource
        }
    }
}

pub(super) fn finish_full_refresh(
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

pub(super) fn seed_launcher_repair_runtime(
    destination: &Path,
    contract_version: u32,
) -> Result<LauncherRepairCachePublication> {
    seed_runtime_from_current_executable(
        destination,
        contract_version,
        RuntimeSeedPurpose::LauncherRepair,
    )
}

pub(super) fn seed_embedded_template_runtime(
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
    #[cfg(windows)]
    let (installer, executable, install_root) = (
        crate::shell::windows_bash_compatible_path(&installer)
            .context("Failed to prepare the runtime seeder path for Git Bash")?,
        crate::shell::windows_bash_compatible_path(executable)
            .context("Failed to prepare the running Jig path for Git Bash")?,
        crate::shell::windows_bash_compatible_path(install_root)
            .context("Failed to prepare the runtime seed staging path for Git Bash")?,
    );
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
    super::scrub_git_repository_environment_except(&mut command, &[]);
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

#[cfg(not(test))]
struct RepairToolEnvironment {
    bash: PathBuf,
    helper_path: std::ffi::OsString,
}

pub(super) fn sanitize_launcher_repair_environment(command: &mut std::process::Command) {
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

#[cfg(all(not(test), windows))]
fn repair_tool_environment(destination: &Path) -> Result<RepairToolEnvironment> {
    let standard_roots = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from);
    let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let search_path = env::var_os("PATH");
    let candidates = windows_git_bash_candidates(
        standard_roots,
        local_app_data.as_deref(),
        search_path.as_deref(),
    );
    let installation = select_windows_git_bash_candidate(destination, candidates).context(
        "Launcher-only repair requires a validated Git for Windows installation outside the repository",
    )?;
    let directories =
        windows_repair_tool_directories(destination, &installation, search_path.as_deref())?;
    let helper_path = env::join_paths(directories)
        .context("Failed to construct the launcher-repair Windows helper-command PATH")?;
    Ok(RepairToolEnvironment {
        bash: installation.bash,
        helper_path,
    })
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

#[cfg(any(test, windows))]
#[derive(Debug, Eq, PartialEq)]
struct WindowsGitBashInstallation {
    bash: PathBuf,
    root: PathBuf,
}

#[cfg(any(test, windows))]
fn windows_git_bash_candidates(
    standard_roots: impl IntoIterator<Item = PathBuf>,
    local_app_data: Option<&Path>,
    search_path: Option<&std::ffi::OsStr>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for root in standard_roots {
        candidates.push(root.join("Git/bin/bash.exe"));
        candidates.push(root.join("Git/usr/bin/bash.exe"));
    }
    if let Some(root) = local_app_data {
        candidates.push(root.join("Programs/Git/bin/bash.exe"));
        candidates.push(root.join("Programs/Git/usr/bin/bash.exe"));
    }
    if let Some(path) = search_path {
        candidates.extend(env::split_paths(path).map(|directory| directory.join("bash.exe")));
    }
    candidates
}

#[cfg(any(test, windows))]
fn select_windows_git_bash_candidate(
    destination: &Path,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Option<WindowsGitBashInstallation> {
    let destination = fs::canonicalize(destination).ok()?;
    candidates.into_iter().find_map(|candidate| {
        let installation = windows_git_bash_installation(&candidate)?;
        (!installation.bash.starts_with(&destination)
            && !installation.root.starts_with(&destination)
            && !destination.starts_with(&installation.root))
        .then_some(installation)
    })
}

#[cfg(any(test, windows))]
fn windows_git_bash_installation(candidate: &Path) -> Option<WindowsGitBashInstallation> {
    let bash = fs::canonicalize(candidate).ok()?;
    let is_bash_exe = bash
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("bash.exe"));
    if !is_bash_exe || !native_pe_executable(&bash) {
        return None;
    }

    let bin = bash.parent()?;
    if !path_file_name_eq_ignore_ascii_case(bin, "bin") {
        return None;
    }
    let bin_parent = bin.parent()?;
    let root = if path_file_name_eq_ignore_ascii_case(bin_parent, "usr") {
        bin_parent.parent()?
    } else {
        bin_parent
    };
    let root = fs::canonicalize(root).ok()?;
    let git = root.join("cmd/git.exe");
    if !native_pe_executable(&git) || !root.join("usr/bin").is_dir() {
        return None;
    }
    Some(WindowsGitBashInstallation { bash, root })
}

#[cfg(any(test, windows))]
fn path_file_name_eq_ignore_ascii_case(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

#[cfg(any(test, windows))]
fn native_pe_executable(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let Ok(metadata) = file.metadata() else {
        return false;
    };
    if !metadata.is_file() || metadata.len() < 68 {
        return false;
    }
    let mut dos_header = [0_u8; 64];
    if file.read_exact(&mut dos_header).is_err() || &dos_header[..2] != b"MZ" {
        return false;
    }
    let pe_offset = u32::from_le_bytes(dos_header[0x3c..0x40].try_into().unwrap());
    if pe_offset < 64 || u64::from(pe_offset) + 4 > metadata.len() {
        return false;
    }
    let mut signature = [0_u8; 4];
    file.seek(SeekFrom::Start(u64::from(pe_offset))).is_ok()
        && file.read_exact(&mut signature).is_ok()
        && signature == *b"PE\0\0"
}

#[cfg(any(test, windows))]
fn windows_repair_tool_directories(
    destination: &Path,
    installation: &WindowsGitBashInstallation,
    search_path: Option<&std::ffi::OsStr>,
) -> Result<Vec<PathBuf>> {
    let destination = fs::canonicalize(destination)
        .context("Failed to resolve the repository before selecting Windows repair tools")?;
    let mut restricted = Vec::new();
    for candidate in [
        installation.root.join("bin"),
        installation.root.join("usr/bin"),
        installation.root.join("mingw64/bin"),
    ] {
        let Ok(canonical) = fs::canonicalize(candidate) else {
            continue;
        };
        if !canonical.starts_with(&installation.root)
            || canonical.starts_with(&destination)
            || !fs::metadata(&canonical).is_ok_and(|metadata| metadata.is_dir())
        {
            continue;
        }
        #[cfg(windows)]
        let Ok(canonical) = crate::shell::windows_bash_compatible_path(&canonical) else {
            continue;
        };
        if !restricted.contains(&canonical) {
            restricted.push(canonical);
        }
    }

    let python_directory = select_windows_python_directory(
        &destination,
        restricted
            .iter()
            .cloned()
            .chain(search_path.into_iter().flat_map(env::split_paths)),
    )
    .context(
        "Launcher-only repair requires a native python3.exe outside the repository on the Git or host PATH",
    )?;
    #[cfg(windows)]
    let python_directory = crate::shell::windows_bash_compatible_path(&python_directory)
        .context("Failed to prepare the Windows Python directory for Git Bash")?;
    if !restricted.contains(&python_directory) {
        restricted.push(python_directory);
    }
    Ok(restricted)
}

#[cfg(any(test, windows))]
fn select_windows_python_directory(
    destination: &Path,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    candidates.into_iter().find_map(|candidate| {
        if !candidate.is_absolute() {
            return None;
        }
        let canonical = fs::canonicalize(candidate).ok()?;
        if canonical.starts_with(destination)
            || destination.starts_with(&canonical)
            || !canonical.is_dir()
        {
            return None;
        }
        native_pe_executable(&canonical.join("python3.exe")).then_some(canonical)
    })
}

#[cfg(all(not(test), not(any(unix, windows))))]
fn repair_tool_environment(_destination: &Path) -> Result<RepairToolEnvironment> {
    bail!("Launcher-only repair requires a validated Bash and helper PATH on this platform")
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

#[cfg(test)]
mod platform_policy_tests {
    use super::*;

    fn write_fake_pe(path: &Path) {
        let mut bytes = vec![0_u8; 132];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        fs::write(path, bytes).unwrap();
    }

    fn create_fake_git_install(root: &Path) -> WindowsGitBashInstallation {
        let bash = root.join("bin/bash.exe");
        let git = root.join("cmd/git.exe");
        fs::create_dir_all(bash.parent().unwrap()).unwrap();
        fs::create_dir_all(git.parent().unwrap()).unwrap();
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        write_fake_pe(&bash);
        write_fake_pe(&git);
        WindowsGitBashInstallation {
            bash: fs::canonicalize(bash).unwrap(),
            root: fs::canonicalize(root).unwrap(),
        }
    }

    fn expected_repair_directory(path: &Path) -> PathBuf {
        let canonical = fs::canonicalize(path).unwrap();
        #[cfg(windows)]
        return crate::shell::windows_bash_compatible_path(&canonical).unwrap();
        #[cfg(not(windows))]
        canonical
    }

    #[test]
    fn windows_bash_selection_rejects_repository_controlled_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("repo");
        let repository_install = create_fake_git_install(&destination.join("tools/Git"));
        let external_install = create_fake_git_install(&temp.path().join("Git"));

        let selected = select_windows_git_bash_candidate(
            &destination,
            [repository_install.bash, external_install.bash.clone()],
        )
        .unwrap();

        assert_eq!(selected, external_install);
    }

    #[test]
    fn windows_bash_selection_rejects_non_git_bash_layouts() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("repo");
        fs::create_dir(&destination).unwrap();
        let cygwin_bash = temp.path().join("cygwin64/bin/bash.exe");
        fs::create_dir_all(cygwin_bash.parent().unwrap()).unwrap();
        write_fake_pe(&cygwin_bash);
        let git = create_fake_git_install(&temp.path().join("Git"));

        let selected =
            select_windows_git_bash_candidate(&destination, [cygwin_bash, git.bash.clone()])
                .unwrap();

        assert_eq!(selected, git);
    }

    #[test]
    fn windows_bash_candidates_put_standard_git_roots_before_path() {
        let temp = tempfile::tempdir().unwrap();
        let standard = temp.path().join("Program Files");
        let ambient = temp.path().join("ambient");
        let search_path = env::join_paths([ambient.clone()]).unwrap();

        let candidates = windows_git_bash_candidates([standard.clone()], None, Some(&search_path));

        assert_eq!(candidates[0], standard.join("Git/bin/bash.exe"));
        assert_eq!(candidates[1], standard.join("Git/usr/bin/bash.exe"));
        assert_eq!(candidates.last(), Some(&ambient.join("bash.exe")));
    }

    #[test]
    fn windows_helper_path_excludes_repository_and_relative_entries() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("repo");
        let repository_tools = destination.join("tools");
        let git = create_fake_git_install(&temp.path().join("Git"));
        let git_bin = git.root.join("bin");
        let git_usr_bin = git.root.join("usr/bin");
        let python_bin = temp.path().join("Python");
        let unrelated_bin = temp.path().join("unrelated");
        for directory in [&repository_tools, &python_bin, &unrelated_bin] {
            fs::create_dir_all(directory).unwrap();
        }
        write_fake_pe(&python_bin.join("python3.exe"));
        let search_path = env::join_paths([
            repository_tools,
            PathBuf::from("relative-tools"),
            unrelated_bin.clone(),
            python_bin.clone(),
        ])
        .unwrap();

        let directories =
            windows_repair_tool_directories(&destination, &git, Some(&search_path)).unwrap();

        assert!(directories.contains(&expected_repair_directory(&git_bin)));
        assert!(directories.contains(&expected_repair_directory(&git_usr_bin)));
        assert!(directories.contains(&expected_repair_directory(&python_bin)));
        assert!(!directories.contains(&expected_repair_directory(&unrelated_bin)));
        let destination = expected_repair_directory(&destination);
        assert!(
            directories
                .iter()
                .all(|directory| !directory.starts_with(&destination))
        );
    }

    #[test]
    fn windows_helper_path_rejects_a_non_pe_python_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("repo");
        fs::create_dir(&destination).unwrap();
        let git = create_fake_git_install(&temp.path().join("Git"));
        let python_bin = temp.path().join("Python");
        fs::create_dir(&python_bin).unwrap();
        fs::write(python_bin.join("python3.exe"), "not a PE executable").unwrap();
        let search_path = env::join_paths([python_bin]).unwrap();

        let error = windows_repair_tool_directories(&destination, &git, Some(&search_path))
            .unwrap_err()
            .to_string();

        assert!(error.contains("native python3.exe"), "{error}");
    }

    #[test]
    fn full_refresh_runtime_policy_encodes_source_and_footprint_together() {
        assert_eq!(
            FullRefreshRuntimePolicy::for_render(
                HarnessFootprint::Minimal,
                EMBEDDED_TEMPLATE_SOURCE,
            ),
            FullRefreshRuntimePolicy::NoManagedRuntime
        );
        assert_eq!(
            FullRefreshRuntimePolicy::for_render(HarnessFootprint::Full, EMBEDDED_TEMPLATE_SOURCE,),
            FullRefreshRuntimePolicy::EmbeddedTemplate
        );
        assert_eq!(
            FullRefreshRuntimePolicy::for_render(
                HarnessFootprint::Full,
                "https://example.test/jig.git",
            ),
            FullRefreshRuntimePolicy::ConfiguredSource
        );
    }

    #[test]
    fn failed_embedded_refresh_keeps_the_last_launcher_repair_seed() {
        let _env = crate::test_env::lock_env();
        let _seed_failure =
            crate::test_env::EnvVarGuard::set(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV, "1");
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("repo");
        fs::create_dir(&destination).unwrap();
        let cache = runtime_cache_base(&destination).join(runtime_profile_cache_name(
            crate::context::CURRENT_CONTRACT_VERSION,
            RuntimeCacheProfile::Runtime,
        ));
        fs::create_dir_all(&cache).unwrap();
        let stamp = cache.join(".jig-source-stamp");
        fs::write(&stamp, format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\n")).unwrap();

        let warnings = finish_full_refresh(
            &destination,
            FullRefreshRuntimePolicy::EmbeddedTemplate,
            crate::progress::CliProgress::disabled("test"),
            "done",
        );

        assert_eq!(warnings.len(), 1);
        assert!(stamp.exists(), "the last runnable repair seed was retired");
    }

    #[test]
    fn embedded_seed_test_double_matches_the_compiled_profile_set() {
        let _env = crate::test_env::lock_env();
        let _seed_failure =
            crate::test_env::EnvVarGuard::remove(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV);
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("repo");
        fs::create_dir(&destination).unwrap();
        seed_embedded_template_runtime(&destination, crate::context::CURRENT_CONTRACT_VERSION)
            .unwrap()
            .commit();
        let cache_base = runtime_cache_base(&destination);
        let runtime = cache_base.join(runtime_profile_cache_name(
            crate::context::CURRENT_CONTRACT_VERSION,
            RuntimeCacheProfile::Runtime,
        ));
        let default = cache_base.join(runtime_profile_cache_name(
            crate::context::CURRENT_CONTRACT_VERSION,
            RuntimeCacheProfile::Default,
        ));

        assert!(runtime.is_dir());
        assert_eq!(default.is_dir(), cfg!(feature = "dev-proxy"));
    }
}
