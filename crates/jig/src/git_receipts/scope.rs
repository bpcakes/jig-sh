use super::*;

pub(super) fn plan_change_snapshot_inner(
    root: &Path,
    baseline_oid: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    collection.ensure_active()?;
    let baseline_oid = resolve_git_commit_inner(root, baseline_oid, collection)
        .with_context(|| format!("Failed to resolve plan baseline commit {baseline_oid}"))?;
    plan_change_snapshot_from_resolved_oid(root, baseline_oid, collection)
}

pub(super) fn plan_change_snapshot_from_empty_tree_inner(
    root: &Path,
    expected_oid: &str,
    collection: GitReceiptCollection<'_>,
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
    collection: GitReceiptCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    #[cfg(test)]
    PLAN_CHANGE_COLLECTION_COUNT.set(PLAN_CHANGE_COLLECTION_COUNT.get() + 1);
    let (changed_paths, untracked_paths) =
        changed_paths_since_baseline(root, &baseline_oid, collection)?;
    Ok(PlanChangeSnapshot {
        baseline_oid,
        changed_paths,
        untracked_paths,
        scope_cache: RefCell::new(BTreeMap::new()),
    })
}

pub(super) fn gate_scope_snapshot_from_plan_change_inner(
    root: &Path,
    plan: &PlanChangeSnapshot,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    gate_signature: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<GateScopeSnapshot> {
    collection.ensure_active()?;
    let key = gate_scope_policy_key(paths, paths_ignore);
    if let Some(cached) = plan.scope_cache.borrow().get(&key).cloned() {
        return cached
            .map(|snapshot| snapshot.for_gate_signature(gate_signature))
            .map_err(anyhow::Error::msg);
    }
    let snapshot = match gate_scope_input_snapshot_from_plan_change_inner(
        root,
        plan,
        key.paths.as_deref(),
        &key.paths_ignore,
        collection,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) if is_git_receipt_collection_cancellation(&error) => return Err(error),
        Err(error) => {
            let message = format!("{error:#}");
            plan.scope_cache
                .borrow_mut()
                .insert(key, Err(message.clone()));
            return Err(anyhow::Error::msg(message));
        }
    };
    plan.scope_cache
        .borrow_mut()
        .insert(key, Ok(snapshot.clone()));
    Ok(snapshot.for_gate_signature(gate_signature))
}

pub(super) fn gate_scope_policy_key(
    paths: Option<&[String]>,
    paths_ignore: &[String],
) -> GateScopePolicyKey {
    fn normalized(patterns: &[String]) -> Vec<String> {
        let mut patterns = patterns.to_vec();
        patterns.sort();
        patterns.dedup();
        patterns
    }

    GateScopePolicyKey {
        paths: paths.map(normalized),
        paths_ignore: normalized(paths_ignore),
    }
}

pub(super) fn gate_scope_input_snapshot_from_plan_change_inner(
    root: &Path,
    plan: &PlanChangeSnapshot,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    collection: GitReceiptCollection<'_>,
) -> Result<GateScopeInputSnapshot> {
    collection.ensure_active()?;
    #[cfg(test)]
    GATE_SCOPE_INPUT_COLLECTION_COUNT.set(GATE_SCOPE_INPUT_COLLECTION_COUNT.get() + 1);
    let baseline_oid = &plan.baseline_oid;
    let all_changed = &plan.changed_paths;
    let matcher = paths.map(build_gate_glob_set).transpose()?;
    let ignore_matcher = build_gate_glob_set(paths_ignore)?;
    let matching = all_changed
        .iter()
        .filter(|path| {
            is_global_gate_authority(path)
                || (matcher
                    .as_ref()
                    .is_none_or(|matcher| matcher.is_match(path))
                    && !ignore_matcher.is_match(path))
        })
        .cloned()
        .collect::<Vec<_>>();
    let applicability = if paths.is_none() || !matching.is_empty() {
        GateApplicability::Applicable
    } else {
        GateApplicability::NotApplicable
    };
    let reason = match applicability {
        GateApplicability::Applicable if paths.is_none() => {
            "gate has no path filter and is always applicable".to_string()
        }
        GateApplicability::Applicable
            if matching.iter().any(|path| is_global_gate_authority(path)) =>
        {
            format!(
                "{} changed path(s) matched, including a global gate authority",
                matching.len()
            )
        }
        GateApplicability::Applicable => {
            format!(
                "{} changed path(s) matched the gate path policy",
                matching.len()
            )
        }
        GateApplicability::NotApplicable => format!(
            "none of the {} changed path(s) matched the gate path policy",
            all_changed.len()
        ),
    };

    let input_fingerprint = gate_scope_input_fingerprint(
        root,
        baseline_oid,
        &matching,
        &plan.untracked_paths,
        collection,
    )?;
    let all_bounded = bounded_changed_paths(all_changed.clone());
    let matching_bounded = bounded_changed_paths(matching);
    Ok(GateScopeInputSnapshot {
        facts: GateScopeFacts {
            baseline_oid: baseline_oid.clone(),
            applicability,
            reason,
            changed_paths: all_bounded.preview,
            changed_path_count: all_bounded.total,
            changed_paths_truncated: all_bounded.truncated,
            changed_paths_digest: all_bounded.digest,
            matching_paths: matching_bounded.preview,
            matching_path_count: matching_bounded.total,
            matching_paths_truncated: matching_bounded.truncated,
            matching_paths_digest: matching_bounded.digest,
        },
        input_fingerprint,
    })
}

pub(super) fn build_gate_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .with_context(|| format!("Invalid gate path glob '{pattern}'"))?,
        );
    }
    builder.build().context("Failed to compile gate path globs")
}

pub(super) fn changed_paths_since_baseline(
    root: &Path,
    baseline_oid: &str,
    collection: GitReceiptCollection<'_>,
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

pub(super) fn gate_scope_input_fingerprint(
    root: &Path,
    baseline_oid: &str,
    matching_paths: &[String],
    untracked: &[String],
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    collection.ensure_active()?;
    let mut digest = Sha256::new();
    digest.update(GATE_SCOPE_INPUT_FINGERPRINT_DOMAIN);
    hash_field(&mut digest, baseline_oid.as_bytes());
    let all_matching = matching_paths.iter().collect::<Vec<_>>();
    ensure_no_partially_staged_paths(root, baseline_oid, &all_matching, collection)?;
    let tracked_matching = matching_paths
        .iter()
        .filter(|path| untracked.binary_search(path).is_err())
        .collect::<Vec<_>>();
    ensure_selected_gitlinks_are_stable(root, &tracked_matching, collection)?;
    hash_field(&mut digest, b"tracked-paths");
    hash_field(&mut digest, &(tracked_matching.len() as u64).to_be_bytes());
    for path in &tracked_matching {
        hash_field(&mut digest, path.as_bytes());
    }
    let order_file = NamedTempFile::new().context("Failed to create Git diff order file")?;
    for (chunk_index, chunk) in literal_pathspec_chunks(&tracked_matching)
        .into_iter()
        .enumerate()
    {
        collection.ensure_active()?;
        hash_field(&mut digest, b"tracked-diff-chunk");
        hash_field(&mut digest, &(chunk_index as u64).to_be_bytes());
        let mut args = canonical_binary_diff_args(order_file.path(), false, Some(baseline_oid));
        args.extend(
            chunk
                .iter()
                .map(|path| OsString::from(format!(":(top,literal){path}"))),
        );
        let mut index_args =
            canonical_binary_diff_args(order_file.path(), true, Some(baseline_oid));
        index_args.extend(
            chunk
                .iter()
                .map(|path| OsString::from(format!(":(top,literal){path}"))),
        );
        let index_diff = git_bounded_proof_stdout_os(
            root,
            &index_args,
            "git diff --cached gate scope",
            gate_scope_diff_output_limit(),
            "gate-scope proof",
            collection,
        )?;
        hash_field(&mut digest, b"baseline-to-index");
        hash_field(&mut digest, &index_diff);
        let diff = git_bounded_proof_stdout_os(
            root,
            &args,
            "git diff gate scope",
            gate_scope_diff_output_limit(),
            "gate-scope proof",
            collection,
        )?;
        hash_field(&mut digest, b"baseline-to-worktree");
        hash_field(&mut digest, &diff);
    }
    let mut remaining_inline_bytes = MAX_TOTAL_INLINE_UNTRACKED_BYTES;
    for path in untracked
        .iter()
        .filter(|path| matching_paths.binary_search(path).is_ok())
    {
        collection.ensure_active()?;
        hash_field(&mut digest, path.as_bytes());
        let full_path = root.join(path);
        let metadata = fs::symlink_metadata(&full_path).with_context(|| {
            format!("Failed to inspect gate-scope path {}", full_path.display())
        })?;
        let mut encoded = Vec::new();
        append_untracked_path_fingerprint(
            &mut encoded,
            root,
            &full_path,
            &metadata,
            &mut remaining_inline_bytes,
            collection,
        )?;
        hash_field(&mut digest, &encoded);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

pub(super) fn ensure_no_partially_staged_paths(
    root: &Path,
    baseline_oid: &str,
    paths: &[&String],
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    let mut staged_since_baseline = BTreeSet::new();
    let mut index_to_worktree = BTreeSet::new();
    for chunk in literal_pathspec_chunks(paths) {
        collection.ensure_active()?;
        let mut staged_args = vec![
            "-c".to_string(),
            "core.fileMode=true".to_string(),
            "-c".to_string(),
            "diff.ignoreSubmodules=none".to_string(),
            "diff".to_string(),
            "--cached".to_string(),
            "--name-status".to_string(),
            "-z".to_string(),
            "--no-renames".to_string(),
            "--no-ext-diff".to_string(),
            "--ignore-submodules=none".to_string(),
            baseline_oid.to_string(),
            "--".to_string(),
        ];
        staged_args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        let staged_refs = staged_args.iter().map(String::as_str).collect::<Vec<_>>();
        let staged = collection.git_output(
            root,
            &staged_refs,
            "git diff --cached --name-status gate scope",
        )?;
        let staged_entries = parse_name_status_paths_z(
            &staged.stdout,
            usize::MAX,
            "gate-scope baseline-to-index diff",
        )?;
        for entry in staged_entries {
            if entry.status.starts_with('D') {
                ensure_staged_deletion_has_no_worktree_replacement(root, &entry.path)?;
            }
            staged_since_baseline.insert(entry.path);
        }

        let mut unstaged_args = vec![
            "-c".to_string(),
            "core.fileMode=true".to_string(),
            "-c".to_string(),
            "diff.ignoreSubmodules=none".to_string(),
            "diff".to_string(),
            "--name-only".to_string(),
            "-z".to_string(),
            "--no-renames".to_string(),
            "--no-ext-diff".to_string(),
            "--ignore-submodules=none".to_string(),
            "--".to_string(),
        ];
        unstaged_args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        let unstaged_refs = unstaged_args.iter().map(String::as_str).collect::<Vec<_>>();
        let unstaged = collection.git_output(
            root,
            &unstaged_refs,
            "git diff --name-only index to worktree gate scope",
        )?;
        index_to_worktree.extend(parse_nul_utf8_paths(
            &unstaged.stdout,
            "git diff --name-only index to worktree",
        )?);
    }

    if let Some(path) = staged_since_baseline
        .intersection(&index_to_worktree)
        .next()
    {
        bail!(
            "Cannot attest partially staged gate input {path}: the index differs from the plan baseline and the worktree differs from the index; stage the checked version or unstage the index version before recording gate evidence"
        );
    }
    Ok(())
}

include!("scope/tail.rs");
