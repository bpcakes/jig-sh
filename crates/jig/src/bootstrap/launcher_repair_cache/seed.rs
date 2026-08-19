#[cfg(not(test))]
use std::time::SystemTime;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

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

#[cfg(any(test, windows))]
#[derive(Debug, Eq, PartialEq)]
struct WindowsGitBashInstallation {
    bash: PathBuf,
    root: PathBuf,
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

#[cfg(test)]
mod platform_policy_tests;
