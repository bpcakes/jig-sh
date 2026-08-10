use std::{ffi::OsStr, fs, path::Path, time::SystemTime};

use serde_json::json;

use super::{DoctorCheck, LAUNCHER_REPAIR_STAGING_DOCTOR_MIN_AGE, check};
use crate::context::{
    INSTALLER_CACHE_LAYOUT_MARKER, LAUNCHER_REPAIR_STAGING_PREFIX, RuntimeCacheProfile,
    runtime_cache_base, runtime_profile_cache_path,
};

pub(super) fn runtime_check(
    root: &Path,
    contract_version: Option<u32>,
    config_jig_version: Option<&str>,
    config_valid: bool,
) -> DoctorCheck {
    let current_version = env!("CARGO_PKG_VERSION");
    let runtime_executable = std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let script_path = root.join("scripts/jig");
    let installer_path = root.join("scripts/install-jig.sh");
    let launcher = launcher_version(&script_path);
    let installer = installer_version(&installer_path);
    let legacy_version = launcher.version;
    let launcher_contract_version = launcher.contract_version;
    let script_ok = script_path.is_file();
    let installer_present = installer_path.is_file();
    let installer_ok =
        installer_present && installer.read_error.is_none() && installer.contract_probe;
    let contract_matches =
        contract_version.is_some_and(|expected| launcher_contract_version == Some(expected));
    let contract_supported =
        contract_version.is_some_and(crate::context::is_supported_contract_version);
    let launcher_repair_seeded_cache = contract_version
        .is_some_and(|version| launcher_repair_seed_stamp_is_present(root, version));
    let ok = script_ok
        && installer_ok
        && launcher.read_error.is_none()
        && legacy_version.is_none()
        && launcher.contract_probe
        && contract_matches
        && contract_supported;
    let contract_label = contract_version.map_or_else(|| "unknown".into(), |v| v.to_string());
    let detail = if let Some(error) = launcher.read_error.as_deref() {
        format!("running {current_version}, scripts/jig is unreadable ({error})")
    } else if let Some(version) = &legacy_version {
        format!(
            "running {current_version} for contract {contract_label}, but scripts/jig still pins legacy product version {version}"
        )
    } else if !script_ok {
        format!("running {current_version}, but scripts/jig is missing")
    } else if let Some(error) = installer.read_error.as_deref() {
        format!("running {current_version}, scripts/install-jig.sh is unreadable ({error})")
    } else if !installer_present {
        format!("running {current_version}, but scripts/install-jig.sh is missing")
    } else if contract_version.is_none() {
        format!("running {current_version}, but the repository contract version is unreadable")
    } else if !contract_supported {
        format!(
            "running {current_version}, but this Jig runtime does not support repository contract {contract_label}"
        )
    } else if !launcher.contract_probe {
        format!(
            "running {current_version} for contract {contract_label}, but scripts/jig does not use the repository validation handoff"
        )
    } else if !installer.contract_probe {
        format!(
            "running {current_version} for contract {contract_label}, but scripts/install-jig.sh is not a recognizable contract-compatible generated installer"
        )
    } else if !contract_matches {
        match launcher_contract_version {
            Some(actual) => format!(
                "running {current_version} for contract {contract_label}, but scripts/jig embeds contract {actual}"
            ),
            None => format!(
                "running {current_version} for contract {contract_label}, but scripts/jig has no readable CONTRACT_VERSION"
            ),
        }
    } else if launcher_repair_seeded_cache {
        format!(
            "running {current_version}; scripts/jig selects binaries compatible with contract {contract_label}, currently backed by a launcher-repair seeded cache"
        )
    } else {
        format!(
            "running {current_version}; scripts/jig selects binaries compatible with contract {contract_label}"
        )
    };
    let status = if ok {
        "compatible"
    } else if legacy_version.is_some() {
        "migration needed"
    } else if launcher.read_error.is_some() || installer.read_error.is_some() {
        "unreadable"
    } else if !script_ok || !installer_present {
        "missing"
    } else if contract_version.is_none() {
        "unreadable"
    } else if !contract_supported {
        "unsupported"
    } else {
        "outdated"
    };
    let fix = if !ok {
        let executable = runtime_executable
            .as_deref()
            .map(crate::shell::quote)
            .unwrap_or_else(|| "jig".into());
        let repository = crate::shell::quote(&root.to_string_lossy());
        let managed_manifest_exists = root
            .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH)
            .is_file();
        let narrow_repair_recognizable =
            crate::bootstrap::launcher_only_repair_scripts_are_recognizable(root);
        if contract_version
            .is_some_and(|version| !crate::context::is_supported_contract_version(version))
        {
            Some(format!(
                "This Jig runtime does not support the repository's declared contract {contract_label}. Install a newer compatible Jig runtime and rerun its doctor; do not rewrite the repository with this older runtime."
            ))
        } else if !config_valid {
            Some(
                "Repair `.jig.toml`, then rerun `scripts/jig doctor` before attempting launcher repair; launcher-only repair needs readable render answers to preserve the repository's runtime source configuration."
                    .into(),
            )
        } else if contract_version.is_none() {
            if managed_manifest_exists {
                Some(format!(
                    "The repository contract manifest must be repaired before a narrow launcher repair can preserve its epoch. Bypass the repo wrapper with the currently running binary: `{executable} update {repository} --force`, then rerun `scripts/jig doctor`. If that binary is unavailable, run `cargo install jig-sh` first."
                ))
            } else {
                Some(format!(
                    "The repository contract manifest and {} cannot establish a safe repair epoch or ownership. Review the repository's current harness footprint and answer overrides, then run `{executable} adopt {repository} --write --force`; rerun `scripts/jig doctor` afterward. If that binary is unavailable, run `cargo install jig-sh` first.",
                    crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH
                ))
            }
        } else if !managed_manifest_exists
            && (!script_ok || !installer_present || !narrow_repair_recognizable)
        {
            Some(format!(
                "The generated launcher pair and {} cannot establish narrow repair ownership. Review the repository's current harness footprint and answer overrides, then run `{executable} adopt {repository} --write --force`; rerun `scripts/jig doctor` afterward. If that binary is unavailable, run `cargo install jig-sh` first.",
                crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH
            ))
        } else if managed_manifest_exists && legacy_version.is_some() && narrow_repair_recognizable
        {
            Some(format!(
                "Bypass the legacy repo wrapper and migrate the full harness with the currently running binary: `{executable} update {repository} --force`, then rerun `scripts/jig doctor`. If the full update cannot start through the legacy wrapper, use `{executable} update {repository} --launcher-only --force` as the narrow recovery step first."
            ))
        } else {
            let ownership_follow_up = if managed_manifest_exists {
                String::new()
            } else {
                format!(
                    " Because {} is missing, review the current footprint and answer overrides, then run `{executable} adopt {repository} --write --force` before a full update.",
                    crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH
                )
            };
            Some(format!(
                "Bypass the repo wrapper with the currently running binary: `{executable} update {repository} --launcher-only --force`, then rerun `scripts/jig doctor`. If that binary is unavailable, run `cargo install jig-sh` first.{ownership_follow_up}"
            ))
        }
    } else {
        None
    };

    // Keep both names in each pair as stable structured-output compatibility
    // aliases: v4 consumers use runtime_version/legacy_launcher_version while
    // older clients may still read current_version/launcher_version.
    check("runtime", "Runtime compatibility", true, ok, status, detail)
        .with_optional_fix(fix.as_deref())
        .with_data(json!({
                "runtime_version": current_version,
                "current_version": current_version,
                "runtime_executable": runtime_executable,
                "contract_version": contract_version,
                "launcher_contract_version": launcher_contract_version,
                "launcher_path": script_path.display().to_string(),
                "installer_path": installer_path.display().to_string(),
                "installer_present": installer_present,
                "installer_uses_contract_probe": installer.contract_probe,
                "installer_error": installer.read_error,
                "legacy_launcher_version": legacy_version,
                "launcher_version": legacy_version,
                "launcher_uses_contract_probe": launcher.contract_probe,
                "launcher_error": launcher.read_error,
                "launcher_repair_seeded_cache": launcher_repair_seeded_cache,
                "config_valid_for_launcher_repair": config_valid,
                "config_jig_version": config_jig_version,
        }))
}

pub(super) fn launcher_repair_seed_stamp_is_present(root: &Path, contract_version: u32) -> bool {
    [RuntimeCacheProfile::Default, RuntimeCacheProfile::Runtime]
        .into_iter()
        .map(|profile| {
            runtime_profile_cache_path(root, contract_version, profile).join(".jig-source-stamp")
        })
        .any(|stamp| {
            fs::read_to_string(stamp)
                .ok()
                .and_then(|contents| contents.lines().next().map(str::to_owned))
                .is_some_and(|first_line| {
                    first_line == crate::bootstrap::LAUNCHER_REPAIR_SEED_STAMP_HEADER
                })
        })
}

pub(super) fn launcher_repair_cache_check(root: &Path, contract_version: u32) -> DoctorCheck {
    let executable = std::env::current_exe()
        .ok()
        .as_deref()
        .map(|path| crate::shell::quote(&path.to_string_lossy()))
        .unwrap_or_else(|| "jig".into());
    let repository = crate::shell::quote(&root.to_string_lossy());
    let managed_manifest_exists = root
        .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH)
        .is_file();
    let fix = if managed_manifest_exists {
        format!(
            "Replace the repair seed with a cache built from the configured source: `{executable} update {repository} --force`, then rerun `scripts/jig doctor`."
        )
    } else {
        format!(
            "Establish exact managed-path ownership and replace the repair seed: review the current harness footprint and answer overrides, then run `{executable} adopt {repository} --write --force`; rerun `scripts/jig doctor` afterward."
        )
    };
    check(
        "launcher_repair_cache",
        "Launcher repair cache",
        false,
        false,
        "temporary seed",
        format!(
            "contract {contract_version} is currently runnable through a launcher-repair seed, but a full source-built cache is still needed for fresh-clone or cache-cleared startup"
        ),
    )
    .with_fix(&fix)
    .with_data(json!({
        "contract_version": contract_version,
        "managed_paths_manifest_present": managed_manifest_exists,
        "cache_layout": INSTALLER_CACHE_LAYOUT_MARKER,
    }))
}

pub(super) fn launcher_repair_staging_check(root: &Path) -> Option<DoctorCheck> {
    launcher_repair_staging_check_at(root, SystemTime::now())
}

pub(super) fn launcher_repair_staging_check_at(
    root: &Path,
    now: SystemTime,
) -> Option<DoctorCheck> {
    let cache_base = runtime_cache_base(root);
    let entries = match fs::read_dir(&cache_base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(check(
                "launcher_repair_staging",
                "Launcher repair staging",
                false,
                false,
                "inspection failed",
                format!("could not inspect {}: {error}", cache_base.display()),
            ));
        }
    };
    let mut leftovers = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(LAUNCHER_REPAIR_STAGING_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_dir() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < LAUNCHER_REPAIR_STAGING_DOCTOR_MIN_AGE {
            continue;
        }
        leftovers.push(path);
    }
    if leftovers.is_empty() {
        return None;
    }
    leftovers.sort();
    let detail = leftovers
        .iter()
        .take(8)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let overflow = leftovers.len().saturating_sub(8);
    let detail = if overflow == 0 {
        detail
    } else {
        format!("{detail}, and {overflow} more")
    };
    Some(check(
        "launcher_repair_staging",
        "Launcher repair staging",
        false,
        false,
        "recovery artifacts",
        format!(
            "found {} launcher-repair staging director{}: {detail}",
            leftovers.len(),
            if leftovers.len() == 1 { "y" } else { "ies" }
        ),
    )
    .with_fix(
        "Inspect backup-* and displaced-* entries for recovery data, then remove each reported staging directory after recovery is complete.",
    )
    .with_data(json!({
        "cache_base": cache_base.display().to_string(),
        "paths": leftovers,
    })))
}

pub(super) fn legacy_version_cache_check(root: &Path) -> Option<DoctorCheck> {
    let cache_base = runtime_cache_base(root);
    let entries = match fs::read_dir(&cache_base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(check(
                "legacy_runtime_cache",
                "Legacy runtime cache",
                false,
                false,
                "inspection failed",
                format!("could not inspect {}: {error}", cache_base.display()),
            ));
        }
    };
    let mut leftovers = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| is_legacy_version_cache_name(&entry.file_name()))
        .filter(|entry| {
            entry
                .path()
                .join("bin")
                .join(format!("jig{}", std::env::consts::EXE_SUFFIX))
                .is_file()
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    if leftovers.is_empty() {
        return None;
    }
    leftovers.sort();
    let detail = leftovers
        .iter()
        .take(8)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let overflow = leftovers.len().saturating_sub(8);
    let detail = if overflow == 0 {
        detail
    } else {
        format!("{detail}, and {overflow} more")
    };
    Some(
        check(
            "legacy_runtime_cache",
            "Legacy runtime cache",
            false,
            false,
            "cleanup available",
            format!(
                "found {} product-version-keyed runtime cache director{} left by the pre-contract layout: {detail}",
                leftovers.len(),
                if leftovers.len() == 1 { "y" } else { "ies" }
            ),
        )
        .with_fix(
            "Complete the full harness update, confirm `scripts/jig doctor` reports a compatible contract-keyed runtime, then remove the reported legacy cache directories.",
        )
        .with_data(json!({
            "cache_base": cache_base.display().to_string(),
            "paths": leftovers,
            "cache_layout": INSTALLER_CACHE_LAYOUT_MARKER,
        })),
    )
}

pub(super) fn is_legacy_version_cache_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let version = name.strip_suffix("-runtime").unwrap_or(name);
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut components = core.split('.');
    let valid_component = |component: Option<&str>| {
        component.is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    valid_component(components.next())
        && valid_component(components.next())
        && valid_component(components.next())
        && components.next().is_none()
}

pub(super) fn contract_migration_check(root: &Path, contract_version: u32) -> DoctorCheck {
    let executable = std::env::current_exe()
        .ok()
        .as_deref()
        .map(|path| crate::shell::quote(&path.to_string_lossy()))
        .unwrap_or_else(|| "jig".into());
    let repository = crate::shell::quote(&root.to_string_lossy());
    let managed_manifest_exists = root
        .join(crate::bootstrap::MANAGED_PATHS_MANIFEST_PATH)
        .is_file();
    let fix = if managed_manifest_exists {
        format!(
            "Migrate while this compatible runtime is available: `{executable} update {repository} --force`, then rerun `scripts/jig doctor`."
        )
    } else {
        format!(
            "Establish exact ownership and migrate while this compatible runtime is available: review the current footprint and answer overrides, then run `{executable} adopt {repository} --write --force`; rerun `scripts/jig doctor` afterward."
        )
    };
    check(
        "contract_migration",
        "Contract migration",
        false,
        false,
        "migration available",
        format!(
            "contract {contract_version} remains supported, but its recorded source may predate compatibility-aware runtime installation; current generated repositories use contract {}",
            crate::context::CURRENT_CONTRACT_VERSION
        ),
    )
    .with_fix(&fix)
    .with_data(json!({
        "contract_version": contract_version,
        "current_contract_version": crate::context::CURRENT_CONTRACT_VERSION,
        "managed_paths_manifest_present": managed_manifest_exists,
    }))
}

pub(super) struct LauncherVersion {
    version: Option<String>,
    contract_version: Option<u32>,
    contract_probe: bool,
    read_error: Option<String>,
}

pub(super) fn launcher_version(path: &Path) -> LauncherVersion {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LauncherVersion {
                version: None,
                contract_version: None,
                contract_probe: false,
                read_error: None,
            };
        }
        Err(error) => {
            return LauncherVersion {
                version: None,
                contract_version: None,
                contract_probe: false,
                read_error: Some(error.to_string()),
            };
        }
    };
    let inspection = crate::runtime_artifacts::inspect_launcher(&text);
    LauncherVersion {
        version: inspection.legacy_version().map(str::to_string),
        contract_version: inspection.readable_contract_version(),
        contract_probe: inspection.uses_repository_scope_protocol(),
        read_error: None,
    }
}

pub(super) struct InstallerVersion {
    contract_probe: bool,
    read_error: Option<String>,
}

pub(super) fn installer_version(path: &Path) -> InstallerVersion {
    match fs::read_to_string(path) {
        Ok(text) => InstallerVersion {
            contract_probe: crate::runtime_artifacts::inspect_installer(&text)
                .uses_repository_scope_protocol(),
            read_error: None,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => InstallerVersion {
            contract_probe: false,
            read_error: None,
        },
        Err(error) => InstallerVersion {
            contract_probe: false,
            read_error: Some(error.to_string()),
        },
    }
}
