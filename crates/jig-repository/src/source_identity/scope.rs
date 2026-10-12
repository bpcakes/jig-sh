use super::*;

impl PlanChangeSnapshot {
    pub fn all_changed_paths(&self) -> Vec<String> {
        self.changed_paths
            .iter()
            .chain(&self.untracked_paths)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

pub(super) fn plan_change_snapshot_inner(
    root: &Path,
    baseline_oid: &str,
    collection: GitCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    collection.ensure_active()?;
    let baseline_oid = resolve_git_commit_inner(root, baseline_oid, collection)
        .with_context(|| format!("Failed to resolve plan baseline commit {baseline_oid}"))?;
    plan_change_snapshot_from_resolved_oid(root, baseline_oid, collection)
}

pub(super) fn plan_change_snapshot_from_empty_tree_inner(
    root: &Path,
    expected_oid: &str,
    collection: GitCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    collection.ensure_active()?;
    let actual_oid = resolve_empty_tree_oid_inner(root, collection)?;
    if actual_oid != expected_oid {
        bail!(
            "Stored empty-tree baseline {expected_oid} does not match repository hash format {actual_oid}"
        );
    }
    plan_change_snapshot_from_resolved_oid(root, actual_oid, collection)
}

pub(super) fn plan_change_snapshot_from_resolved_oid(
    root: &Path,
    baseline_oid: String,
    collection: GitCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    #[cfg(test)]
    PLAN_CHANGE_COLLECTION_COUNT.set(PLAN_CHANGE_COLLECTION_COUNT.get() + 1);
    let (changed_paths, untracked_paths) =
        changed_paths_since_baseline(root, &baseline_oid, collection)?;
    Ok(PlanChangeSnapshot {
        changed_paths,
        untracked_paths,
    })
}

pub(super) fn changed_paths_since_baseline(
    root: &Path,
    baseline_oid: &str,
    collection: GitCollection<'_>,
) -> Result<(Vec<String>, Vec<String>)> {
    collection.ensure_active()?;
    let mut discovered_entries = 0;
    let tracked = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --name-status baseline",
    )?;
    let mut changed = Vec::new();
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &tracked,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "baseline-to-worktree diff",
        )?,
        &mut discovered_entries,
        "baseline-to-worktree diff",
    )?;
    let staged = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --cached --name-status baseline",
    )?;
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &staged,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "baseline-to-index diff",
        )?,
        &mut discovered_entries,
        "baseline-to-index diff",
    )?;
    let manifest_tracked = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".jig.toml",
            ".agent/jig-contract.json",
        ],
        "git diff --name-status contract manifest",
    )?;
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &manifest_tracked,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "contract-manifest worktree diff",
        )?,
        &mut discovered_entries,
        "contract-manifest worktree diff",
    )?;
    let manifest_staged = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".jig.toml",
            ".agent/jig-contract.json",
        ],
        "git diff --cached --name-status contract manifest",
    )?;
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &manifest_staged,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "contract-manifest index diff",
        )?,
        &mut discovered_entries,
        "contract-manifest index diff",
    )?;
    collection.ensure_active()?;
    let untracked_output = collection.git_changed_path_stdout(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git ls-files untracked",
    )?;
    let mut untracked = Vec::new();
    extend_discovered_paths(
        &mut untracked,
        parse_nul_utf8_paths_with_limit(
            &untracked_output,
            "git ls-files",
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "untracked files",
        )?,
        &mut discovered_entries,
        "untracked files",
    )?;
    let manifest_untracked = collection.git_changed_path_stdout(
        root,
        &[
            "ls-files",
            "--others",
            "-z",
            "--",
            ".jig.toml",
            ".agent/jig-contract.json",
        ],
        "git ls-files untracked contract manifest",
    )?;
    extend_discovered_paths(
        &mut untracked,
        parse_nul_utf8_paths_with_limit(
            &manifest_untracked,
            "git ls-files contract manifest",
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "untracked contract manifests",
        )?,
        &mut discovered_entries,
        "untracked contract manifests",
    )?;
    untracked.sort();
    untracked.dedup();
    changed.extend(untracked.iter().cloned());
    changed.sort();
    changed.dedup();
    Ok((changed, untracked))
}

pub(super) fn extend_discovered_paths(
    destination: &mut Vec<String>,
    paths: Vec<String>,
    discovered_entries: &mut usize,
    label: &str,
) -> Result<()> {
    extend_discovered_paths_with_limit(
        destination,
        paths,
        discovered_entries,
        label,
        MAX_CHANGED_PATH_DISCOVERY_ENTRIES,
    )
}

pub(super) fn extend_discovered_paths_with_limit(
    destination: &mut Vec<String>,
    paths: Vec<String>,
    discovered_entries: &mut usize,
    label: &str,
    limit: usize,
) -> Result<()> {
    let next = discovered_entries
        .checked_add(paths.len())
        .ok_or_else(|| anyhow::anyhow!("Changed-path discovery count overflowed"))?;
    if next > limit {
        bail!(
            "Changed-path discovery exceeded the limit of {limit} path entries while reading {label}; split or reduce the worktree change set before collecting gate evidence"
        );
    }
    *discovered_entries = next;
    destination.extend(paths);
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct NameStatusPath {
    status: String,
    path: String,
}

pub(super) fn parse_name_status_z(
    stdout: &[u8],
    entry_limit: usize,
    label: &str,
) -> Result<Vec<String>> {
    Ok(parse_name_status_paths_z(stdout, entry_limit, label)?
        .into_iter()
        .map(|entry| entry.path)
        .collect())
}

pub(super) fn parse_name_status_paths_z(
    stdout: &[u8],
    entry_limit: usize,
    label: &str,
) -> Result<Vec<NameStatusPath>> {
    require_nul_terminated(stdout, "git diff --name-status -z")?;
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    let mut paths = Vec::new();
    while let Some(status) = fields.next() {
        if status.is_empty() {
            if fields.peek().is_some() {
                bail!("Malformed git diff --name-status -z output: empty status field");
            }
            break;
        }
        let status = std::str::from_utf8(status).context("Git diff status was not UTF-8")?;
        let path_count = usize::from(status.starts_with('R') || status.starts_with('C')) + 1;
        for _ in 0..path_count {
            let path = fields
                .next()
                .filter(|field| !field.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Malformed git diff --name-status -z output"))?;
            if paths.len() == entry_limit {
                bail!(
                    "Changed-path discovery exceeded the remaining limit of {entry_limit} path entries while parsing {label}; split or reduce the worktree change set before collecting gate evidence"
                );
            }
            paths.push(NameStatusPath {
                status: status.to_string(),
                path: std::str::from_utf8(path)
                    .context("Changed repository path was not UTF-8")?
                    .to_string(),
            });
        }
    }
    Ok(paths)
}

pub(super) fn parse_nul_utf8_paths(stdout: &[u8], label: &str) -> Result<Vec<String>> {
    parse_nul_utf8_paths_with_limit(stdout, label, usize::MAX, label)
}

pub(super) fn parse_nul_utf8_paths_with_limit(
    stdout: &[u8],
    label: &str,
    entry_limit: usize,
    discovery_label: &str,
) -> Result<Vec<String>> {
    require_nul_terminated(stdout, label)?;
    let mut paths = Vec::new();
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    while let Some(path) = fields.next() {
        if path.is_empty() {
            if fields.peek().is_some() {
                bail!("Malformed {label} -z output: empty path field");
            }
            break;
        }
        if paths.len() == entry_limit {
            bail!(
                "Changed-path discovery exceeded the remaining limit of {entry_limit} path entries while parsing {discovery_label}; split or reduce the worktree change set before collecting gate evidence"
            );
        }
        paths.push(
            std::str::from_utf8(path)
                .with_context(|| format!("{label} path was not UTF-8"))?
                .to_string(),
        );
    }
    Ok(paths)
}

pub(super) fn require_nul_terminated(stdout: &[u8], label: &str) -> Result<()> {
    if !stdout.is_empty() && !stdout.ends_with(&[0]) {
        bail!("Malformed {label} output: missing NUL terminator");
    }
    Ok(())
}

pub(super) fn ensure_staged_deletion_has_no_worktree_replacement(
    root: &Path,
    path: &str,
) -> Result<()> {
    let full_path = root.join(path);
    match fs::symlink_metadata(&full_path) {
        Ok(_) => bail!(
            "Cannot attest staged deletion {path}: the repository path still exists in the worktree and may be an ignored same-path replacement; remove the replacement, or restore and stage the checked version before recording gate evidence"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to inspect staged deletion replacement {}",
                full_path.display()
            )
        }),
    }
}
