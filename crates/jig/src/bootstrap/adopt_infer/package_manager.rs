use std::collections::BTreeSet;
use std::path::Path;

use super::scan::{RepoScan, push_scan_warning, relative_path_string};

#[derive(Clone, Debug, Default)]
pub(super) struct PackageManagerInference {
    pub(super) value: Option<String>,
    pub(super) sources: Vec<String>,
}

#[cfg(test)]
pub(super) fn infer_package_manager(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Option<String> {
    infer_package_manager_with_metadata(root, scan, warnings).value
}

pub(super) fn infer_package_manager_with_metadata(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> PackageManagerInference {
    let lockfiles = [
        ("pnpm-lock.yaml", "pnpm"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("npm-shrinkwrap.json", "npm"),
        ("package-lock.json", "npm"),
        ("yarn.lock", "yarn"),
    ];
    let root_matches = lockfiles
        .iter()
        .filter(|(lockfile, _)| root.join(lockfile).is_file())
        .map(|(lockfile, manager)| ((*lockfile).to_string(), *manager))
        .collect::<Vec<_>>();
    if let Some((source, manager)) = root_matches.first() {
        let managers = root_matches
            .iter()
            .map(|(_, manager)| *manager)
            .collect::<BTreeSet<_>>();
        if managers.len() > 1 {
            push_scan_warning(
                warnings,
                root,
                &format!(
                    "multiple root package manager lockfiles detected ({}); using {manager}. Remove stale lockfiles, or pass --web-package-manager when configuring frontend apps.",
                    lockfile_summary(root_matches.iter().map(|(source, _)| source.clone()))
                ),
            );
        }
        return PackageManagerInference {
            value: Some((*manager).into()),
            sources: vec![source.clone()],
        };
    }

    let mut matches = Vec::new();
    for (lockfile, manager) in lockfiles {
        for path in scan.named_files(lockfile) {
            matches.push((path_depth(root, path), path.clone(), manager));
        }
    }
    matches.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let managers = matches
        .iter()
        .map(|(_, _, manager)| *manager)
        .collect::<BTreeSet<_>>();
    if managers.len() > 1 {
        push_scan_warning(
            warnings,
            root,
            &format!(
                "multiple package manager lockfiles detected ({}); using {}. If these are tool caches or vendored examples, add them to .gitignore so adopt inference can skip them.",
                lockfile_summary(matches.iter().map(|(_, path, _)| {
                    relative_path_string(path.strip_prefix(root).unwrap_or(path))
                })),
                matches[0].2
            ),
        );
    }
    matches
        .first()
        .map(|(_, path, manager)| PackageManagerInference {
            value: Some((*manager).into()),
            sources: vec![relative_path_string(
                path.strip_prefix(root).unwrap_or(path),
            )],
        })
        .unwrap_or_default()
}

fn lockfile_summary(sources: impl IntoIterator<Item = String>) -> String {
    let mut sources = sources.into_iter().collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    const MAX_DISPLAYED_LOCKFILES: usize = 5;
    if sources.len() <= MAX_DISPLAYED_LOCKFILES {
        return sources.join(", ");
    }
    let omitted = sources.len() - MAX_DISPLAYED_LOCKFILES;
    format!(
        "{}, and {omitted} more",
        sources
            .into_iter()
            .take(MAX_DISPLAYED_LOCKFILES)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn path_depth(root: &Path, path: &Path) -> usize {
    path.strip_prefix(root)
        .map(|path| path.components().count())
        .unwrap_or(usize::MAX)
}
