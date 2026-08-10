use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::{DiscoveredHomes, DiscoveryIssue, DiscoveryIssueKind, configured_codex_home};

pub(super) fn discover_homes() -> Result<DiscoveredHomes> {
    let user_home = user_home()?;
    let current = current_codex_home()?;
    Ok(discover_homes_from(&user_home, &current))
}

pub(super) fn discover_homes_from(user_home: &Path, current: &Path) -> DiscoveredHomes {
    discover_homes_from_with_metadata(user_home, current, |path| fs::metadata(path))
}

pub(super) fn discover_homes_from_with_metadata<F>(
    user_home: &Path,
    current: &Path,
    metadata: F,
) -> DiscoveredHomes
where
    F: Fn(&Path) -> io::Result<fs::Metadata>,
{
    discover_homes_from_with_sources(user_home, current, metadata, |path, inspect_entry| {
        for entry in fs::read_dir(path)? {
            inspect_entry(entry.map(|entry| (entry.file_name(), entry.path())));
        }
        Ok(())
    })
}

pub(super) fn discover_homes_from_with_sources<F, R>(
    user_home: &Path,
    current: &Path,
    metadata: F,
    read_entries: R,
) -> DiscoveredHomes
where
    F: Fn(&Path) -> io::Result<fs::Metadata>,
    R: FnOnce(&Path, &mut dyn FnMut(io::Result<(OsString, PathBuf)>)) -> io::Result<()>,
{
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut representation_lossy = false;
    let mut inspected = HashSet::new();
    let default = user_home.join(".codex");
    inspected.insert(default.clone());
    add_directory_candidate(
        default,
        false,
        &metadata,
        &mut candidates,
        &mut issues,
        &mut representation_lossy,
    );
    let scan_result = {
        let mut inspect_entry = |entry: io::Result<(OsString, PathBuf)>| {
            let (name, path) = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    issues.push(DiscoveryIssue::new(
                        DiscoveryIssueKind::EntryUnreadable,
                        format!("Failed to inspect a Codex home candidate: {error}"),
                    ));
                    return;
                }
            };
            if name.to_string_lossy().starts_with(".codex-") {
                inspected.insert(path.clone());
                add_directory_candidate(
                    path,
                    true,
                    &metadata,
                    &mut candidates,
                    &mut issues,
                    &mut representation_lossy,
                );
            }
        };
        read_entries(user_home, &mut inspect_entry)
    };
    if let Err(error) = scan_result {
        issues.push(DiscoveryIssue::new(
            DiscoveryIssueKind::ScanIncomplete,
            format!(
                "Failed to inspect {} for Codex homes: {error}",
                user_home.display()
            ),
        ));
    }
    if !inspected.contains(current) {
        add_directory_candidate(
            current.to_path_buf(),
            true,
            &metadata,
            &mut candidates,
            &mut issues,
            &mut representation_lossy,
        );
    }

    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(canonical_key(candidate)));
    candidates.sort_by(|left, right| {
        let left_name = home_name(left);
        let right_name = home_name(right);
        (left_name != "codex", left_name).cmp(&(right_name != "codex", right_name))
    });
    DiscoveredHomes {
        paths: candidates,
        issues,
        representation_lossy,
    }
}

pub(super) fn add_directory_candidate<F>(
    candidate: PathBuf,
    report_not_found: bool,
    metadata: &F,
    candidates: &mut Vec<PathBuf>,
    issues: &mut Vec<DiscoveryIssue>,
    representation_lossy: &mut bool,
) where
    F: Fn(&Path) -> io::Result<fs::Metadata>,
{
    *representation_lossy |= candidate.as_os_str().to_str().is_none();
    match metadata(&candidate) {
        Ok(metadata) if metadata.is_dir() => candidates.push(candidate),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound && !report_not_found => {}
        Err(error) => {
            let kind = if error.kind() == io::ErrorKind::NotFound {
                DiscoveryIssueKind::CandidateMissing
            } else {
                DiscoveryIssueKind::CandidateUnreadable
            };
            issues.push(DiscoveryIssue::new(
                kind,
                format!(
                    "Failed to inspect Codex home candidate {}: {error}",
                    candidate.display()
                ),
            ));
        }
    }
}

pub(super) fn current_codex_home() -> Result<PathBuf> {
    let configured = configured_codex_home()
        .ok_or_else(|| anyhow!("Could not determine the Codex home directory"))?;
    absolute_path(configured)
}

pub(super) fn user_home() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("Could not determine the user home directory"))
}

pub(super) fn expand_tilde_path(input: &Path, user_home: &Path) -> Option<PathBuf> {
    let mut components = input.components();
    if components.next() == Some(std::path::Component::Normal(OsStr::new("~"))) {
        return Some(user_home.join(components.as_path()));
    }
    None
}

pub(super) fn has_tilde_prefix(input: &Path) -> bool {
    input.components().next() == Some(std::path::Component::Normal(OsStr::new("~")))
}

pub(super) fn is_bare_home_name(input: &Path) -> bool {
    let mut components = input.components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

pub(super) fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    absolute_path_with_current_dir(path, || {
        env::current_dir().context("Failed to resolve the current directory")
    })
}

pub(super) fn absolute_path_with_current_dir<C>(path: PathBuf, current_dir: C) -> Result<PathBuf>
where
    C: FnOnce() -> Result<PathBuf>,
{
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(current_dir()?.join(path))
    }
}

pub(super) fn canonical_or(path: PathBuf) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("Failed to resolve Codex home {}", path.display()))
}

pub(super) fn canonical_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    canonical_key(left) == canonical_key(right)
}

pub(super) fn home_name(path: &Path) -> String {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    name.strip_prefix('.').unwrap_or(&name).to_string()
}

pub(super) fn home_name_matches(path: &Path, requested: &OsStr) -> bool {
    let name = path.file_name().unwrap_or(path.as_os_str());
    let encoded = name.as_encoded_bytes();
    encoded.strip_prefix(b".").unwrap_or(encoded) == requested.as_encoded_bytes()
}

pub(super) fn conventional_home(user_home: &Path, requested: &OsStr) -> PathBuf {
    if requested == "codex" || requested == "default" {
        return user_home.join(".codex");
    }
    let mut name = OsString::from(".");
    if !requested.as_encoded_bytes().starts_with(b"codex-") {
        name.push("codex-");
    }
    name.push(requested);
    user_home.join(name)
}
