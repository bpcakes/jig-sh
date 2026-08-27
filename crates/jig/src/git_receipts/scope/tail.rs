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

pub(super) fn literal_path_chunks<T>(
    paths: &[T],
    encoded_path_bytes: impl Fn(&T) -> usize,
) -> Vec<&[T]> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < paths.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < paths.len() && end - start < MAX_GIT_LITERAL_PATHS_PER_DIFF {
            let pathspec_bytes = b":(top,literal)".len() + encoded_path_bytes(&paths[end]) + 1;
            if end > start && bytes + pathspec_bytes > MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF {
                break;
            }
            bytes += pathspec_bytes;
            end += 1;
        }
        chunks.push(&paths[start..end]);
        start = end;
    }
    chunks
}

pub(super) fn literal_pathspec_chunks<'a>(paths: &'a [&'a String]) -> Vec<&'a [&'a String]> {
    literal_path_chunks(paths, |path| path.len())
}

pub(super) fn ensure_selected_gitlinks_are_stable(
    root: &Path,
    paths: &[&String],
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    for chunk in literal_pathspec_chunks(paths) {
        let mut args = vec![
            "ls-files".to_string(),
            "--stage".to_string(),
            "-z".to_string(),
            "--".to_string(),
        ];
        args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = collection.git_output(root, &arg_refs, "git ls-files gate gitlinks")?;
        for gitlink in parse_gitlinks(&output.stdout)? {
            ensure_gitlink_checkout_is_stable(root, &gitlink, collection)?;
        }
    }
    Ok(())
}

pub(super) fn ensure_worktree_gitlinks_are_stable(
    root: &Path,
    changed_tracked_paths: &[PathBuf],
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    if changed_tracked_paths.is_empty() {
        return Ok(());
    }
    for chunk in literal_os_path_chunks(changed_tracked_paths) {
        let mut args = ["ls-files", "--stage", "-z", "--"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        for path in chunk {
            let mut pathspec = OsString::from(":(top,literal)");
            pathspec.push(path);
            args.push(pathspec);
        }
        let stdout = git_worktree_proof_stdout_os(
            root,
            &args,
            "git ls-files worktree gitlinks",
            worktree_diff_output_limit(),
            collection,
        )?;
        for gitlink in parse_gitlinks(&stdout)? {
            ensure_gitlink_checkout_is_stable(root, &gitlink, collection)?;
        }
    }
    Ok(())
}

pub(super) fn literal_os_path_chunks(paths: &[PathBuf]) -> Vec<&[PathBuf]> {
    literal_path_chunks(paths, |path| path.as_os_str().as_encoded_bytes().len())
}

#[derive(Debug)]
pub(super) struct GitlinkIndexEntry {
    oid: String,
    path: PathBuf,
    stage: String,
}

pub(super) fn parse_gitlinks(stdout: &[u8]) -> Result<Vec<GitlinkIndexEntry>> {
    let mut gitlinks = Vec::new();
    for record in stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| anyhow::anyhow!("Malformed git ls-files --stage entry"))?;
        let metadata = std::str::from_utf8(&record[..separator])
            .context("Git index entry metadata was not UTF-8")?;
        let path_bytes = &record[separator + 1..];
        if path_bytes.is_empty() {
            bail!("Malformed git ls-files --stage entry: empty path");
        }
        let mut fields = metadata.split_ascii_whitespace();
        let mode = fields.next().unwrap_or_default();
        let oid = fields.next().unwrap_or_default();
        let stage = fields.next().unwrap_or_default();
        if fields.next().is_some() || oid.is_empty() || stage.is_empty() {
            bail!("Malformed git ls-files --stage metadata");
        }
        if mode == "160000" {
            #[cfg(unix)]
            let path = path_buf_from_git_bytes(path_bytes);
            #[cfg(not(unix))]
            let path = path_buf_from_git_bytes(path_bytes)?;
            gitlinks.push(GitlinkIndexEntry {
                oid: oid.to_ascii_lowercase(),
                path,
                stage: stage.to_string(),
            });
        }
    }
    Ok(gitlinks)
}

pub(super) fn ensure_gitlink_checkout_is_stable(
    root: &Path,
    gitlink: &GitlinkIndexEntry,
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    if gitlink.stage != "0" {
        bail!(
            "Cannot attest conflicted submodule gitlink {}",
            gitlink.path.display()
        );
    }
    let checkout = root.join(&gitlink.path);
    if !checkout.exists() {
        return Ok(());
    }
    if !checkout.is_dir() {
        return Ok(());
    }
    if !checkout.join(".git").exists() {
        if fs::read_dir(&checkout)
            .with_context(|| format!("Failed to inspect submodule path {}", checkout.display()))?
            .next()
            .is_none()
        {
            return Ok(());
        }
        bail!(
            "Cannot attest gitlink {} because its checkout is not an initialized submodule",
            gitlink.path.display()
        );
    }
    let head = collection.git_output(
        &checkout,
        &["rev-parse", "--verify", "HEAD"],
        "git rev-parse submodule HEAD",
    )?;
    let head = parse_git_object_oid(&head.stdout, "submodule HEAD")?;
    if head != gitlink.oid {
        bail!(
            "Cannot attest gitlink {}: checkout HEAD {head} differs from index {}",
            gitlink.path.display(),
            gitlink.oid
        );
    }
    let dirty = git_status_is_dirty(
        &checkout,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
        "git status submodule",
        collection,
    )?;
    if dirty {
        bail!(
            "Cannot attest gitlink {} because its checkout contains changes",
            gitlink.path.display()
        );
    }
    Ok(())
}

pub(super) fn git_status_is_dirty(
    root: &Path,
    args: &[&str],
    label: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<bool> {
    collection.ensure_active()?;
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_read_only_git_environment(&mut command);
    let mut observer = GitReceiptProcessObserver { collection };
    let output = match run_owned_process_tree_with_output_policy_and_observer(
        &mut command,
        Duration::MAX,
        ProcessOutputLimits {
            stdout: 1,
            stderr: MAX_GIT_ERROR_PREVIEW_BYTES as usize,
        },
        ProcessOutputOverflowPolicy::Error,
        &mut observer,
    ) {
        Ok(output) => output,
        Err(error) if error.is_cancellation() => {
            return Err(GitReceiptCollectionCancelled.into());
        }
        Err(OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stdout)) => {
            return Ok(true);
        }
        Err(OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stderr)) => {
            bail!(
                "{label} exceeded the Git diagnostic output limit of {MAX_GIT_ERROR_PREVIEW_BYTES} bytes"
            );
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!("Failed to run {label} in {}", root.display())));
        }
    };
    let stdout = output
        .stdout
        .context("supervised Git dirtiness probe did not capture stdout")?;
    let stderr = output
        .stderr
        .context("supervised Git dirtiness probe did not capture stderr")?;
    if !stdout.complete || !stderr.complete {
        bail!(
            "Failed to capture complete output from {label} in {}",
            root.display()
        );
    }
    let output = Output {
        status: output.status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    };
    require_success(&output, |output| {
        format!(
            "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    collection.ensure_active()?;
    Ok(!output.stdout.is_empty())
}

pub(super) fn is_global_gate_authority(path: &str) -> bool {
    GLOBAL_GATE_AUTHORITY_PATHS.contains(&path)
}

pub(super) fn gate_scope_fingerprint(
    baseline_oid: &str,
    gate_signature: &str,
    input_fingerprint: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(GATE_SCOPE_FINGERPRINT_DOMAIN);
    hash_field(&mut digest, baseline_oid.as_bytes());
    hash_field(&mut digest, gate_signature.as_bytes());
    hash_field(&mut digest, input_fingerprint.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

pub(super) fn hash_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

pub(super) fn canonical_binary_diff_args(
    order_file: &Path,
    cached: bool,
    baseline_oid: Option<&str>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("-c"),
        OsString::from("core.fileMode=true"),
        OsString::from("-c"),
        OsString::from("diff.ignoreSubmodules=none"),
        OsString::from("-c"),
        OsString::from("diff.algorithm=myers"),
        OsString::from("-c"),
        OsString::from("diff.indentHeuristic=false"),
        OsString::from("-c"),
        OsString::from("diff.renames=false"),
        OsString::from("-c"),
        OsString::from("diff.context=3"),
        OsString::from("-c"),
        OsString::from("diff.interHunkContext=0"),
        OsString::from("-c"),
        OsString::from("diff.relative=false"),
        OsString::from("diff"),
    ];
    if cached {
        args.push(OsString::from("--cached"));
    }
    args.extend(
        [
            "--binary",
            "--full-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--no-indent-heuristic",
            "--diff-algorithm=myers",
            "--unified=3",
            "--inter-hunk-context=0",
            "--no-relative",
            "--ignore-submodules=none",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ]
        .into_iter()
        .map(OsString::from),
    );
    let mut order_arg = OsString::from("-O");
    order_arg.push(order_file.as_os_str());
    args.push(order_arg);
    if let Some(baseline_oid) = baseline_oid {
        args.push(OsString::from(baseline_oid));
    }
    args.push(OsString::from("--"));
    args
}
