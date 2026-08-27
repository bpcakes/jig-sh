use super::*;

pub(super) fn repo_worktree_fingerprint_inner(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    collection.ensure_active()?;
    #[cfg(test)]
    WORKTREE_FINGERPRINT_COLLECTION_COUNT.set(WORKTREE_FINGERPRINT_COLLECTION_COUNT.get() + 1);
    let status = git_worktree_proof_stdout(
        root,
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
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git status --porcelain",
        worktree_status_output_limit(),
        collection,
    )?;
    collection.ensure_active()?;
    let order_file = NamedTempFile::new().context("Failed to create worktree diff order file")?;
    let mut unstaged_args = canonical_binary_diff_args(order_file.path(), false, None);
    unstaged_args.extend([OsString::from("."), OsString::from(":(exclude).agent/**")]);
    let unstaged = git_worktree_proof_stdout_os(
        root,
        &unstaged_args,
        "git diff --binary",
        worktree_diff_output_limit(),
        collection,
    )?;
    collection.ensure_active()?;
    let mut staged_args = canonical_binary_diff_args(order_file.path(), true, None);
    staged_args.extend([OsString::from("."), OsString::from(":(exclude).agent/**")]);
    let staged = git_worktree_proof_stdout_os(
        root,
        &staged_args,
        "git diff --cached --binary",
        worktree_diff_output_limit(),
        collection,
    )?;
    collection.ensure_active()?;
    let status_entries = parse_porcelain_status_z(&status)?;
    ensure_no_whole_worktree_staged_deletion_replacements(root, &status_entries)?;
    let tracked_status_paths = status_entries
        .iter()
        .filter(|entry| entry.status != "??")
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    ensure_worktree_gitlinks_are_stable(root, &tracked_status_paths, collection)?;
    let untracked = untracked_file_contents(root, &status, collection)?;

    let mut digest = Sha256::new();
    digest.update(WORKTREE_FINGERPRINT_DOMAIN);
    hash_field(&mut digest, &status);
    hash_field(&mut digest, &unstaged);
    hash_field(&mut digest, &staged);
    hash_field(&mut digest, &untracked);

    collection.ensure_active()?;
    Ok(format!("sha256:{:x}", digest.finalize()))
}

pub(super) fn untracked_file_contents(
    root: &Path,
    status_stdout: &[u8],
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    let mut contents = Vec::new();
    let mut remaining_inline_bytes = MAX_TOTAL_INLINE_UNTRACKED_BYTES;
    for entry in parse_porcelain_status_z(status_stdout)? {
        collection.ensure_active()?;
        if entry.status != "??" {
            continue;
        }
        let full_path = root.join(&entry.path);
        let metadata = fs::symlink_metadata(&full_path).with_context(|| {
            format!(
                "Failed to read untracked path metadata {}",
                full_path.display()
            )
        })?;

        let mut payload = Vec::new();
        append_untracked_path_fingerprint(
            &mut payload,
            root,
            &full_path,
            &metadata,
            &mut remaining_inline_bytes,
            collection,
        )?;
        append_length_prefixed(&mut contents, entry.path.as_os_str().as_encoded_bytes());
        append_length_prefixed(&mut contents, &payload);
    }
    collection.ensure_active()?;
    Ok(contents)
}

pub(super) fn append_length_prefixed(output: &mut Vec<u8>, field: &[u8]) {
    output.extend_from_slice(&(field.len() as u64).to_be_bytes());
    output.extend_from_slice(field);
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct PorcelainStatusEntry {
    status: String,
    pub(super) path: PathBuf,
    pub(super) original_path: Option<PathBuf>,
}

pub(super) fn ensure_no_whole_worktree_staged_deletion_replacements(
    root: &Path,
    entries: &[PorcelainStatusEntry],
) -> Result<()> {
    for entry in entries {
        if entry.status.as_bytes().first() != Some(&b'D') {
            continue;
        }
        let full_path = root.join(&entry.path);
        match fs::symlink_metadata(&full_path) {
            Ok(_) => bail!(
                "Cannot attest staged deletion {}: the repository path still exists in the worktree and may be an ignored same-path replacement; remove the replacement, or restore and stage the checked version before recording gate evidence",
                entry.path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect staged deletion replacement {}",
                        full_path.display()
                    )
                });
            }
        }
    }
    Ok(())
}

pub(super) fn parse_porcelain_status_z(stdout: &[u8]) -> Result<Vec<PorcelainStatusEntry>> {
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    let mut entries = Vec::new();
    while let Some(field) = fields.next() {
        if field.is_empty() {
            if fields.peek().is_none() {
                break;
            }
            bail!("Malformed git status --porcelain -z output: empty path field");
        }
        if field.len() < 4 || field[2] != b' ' {
            bail!(
                "Malformed git status --porcelain -z record: {}",
                String::from_utf8_lossy(field)
            );
        }
        let status = String::from_utf8_lossy(&field[..2]).to_string();
        #[cfg(unix)]
        let path = path_buf_from_git_bytes(&field[3..]);
        #[cfg(not(unix))]
        let path = path_buf_from_git_bytes(&field[3..])?;

        let original_path = if status.as_bytes().contains(&b'R')
            || status.as_bytes().contains(&b'C')
        {
            let original = fields.next().context(
                "Malformed git status --porcelain -z output: rename/copy record missing original path",
            )?;
            if original.is_empty() {
                bail!("Malformed git status --porcelain -z output: empty original path");
            }
            #[cfg(unix)]
            {
                Some(path_buf_from_git_bytes(original))
            }
            #[cfg(not(unix))]
            {
                Some(path_buf_from_git_bytes(original)?)
            }
        } else {
            None
        };

        entries.push(PorcelainStatusEntry {
            status,
            path,
            original_path,
        });
        let limit = worktree_status_entry_limit();
        if entries.len() > limit {
            bail!(
                "git status --porcelain exceeded the worktree proof entry limit of {limit}; split or reduce the worktree change set before collecting evidence"
            );
        }
    }
    Ok(entries)
}

#[cfg(unix)]
pub(super) fn path_buf_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
pub(super) fn path_buf_from_git_bytes(bytes: &[u8]) -> Result<PathBuf> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .context("Git status path is not UTF-8")
}

pub(super) fn append_untracked_path_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    metadata: &fs::Metadata,
    remaining_inline_bytes: &mut u64,
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    collection.ensure_active()?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        contents.extend_from_slice(b"symlink\0");
        let target = fs::read_link(full_path)
            .with_context(|| format!("Failed to read symlink target {}", full_path.display()))?;
        contents.extend_from_slice(target.as_os_str().as_encoded_bytes());
        return Ok(());
    }

    if metadata.is_dir() {
        bail!(
            "Cannot attest untracked directory {}; it may be an embedded repository",
            full_path.display()
        );
    }

    if metadata.is_file() {
        append_untracked_file_fingerprint(
            contents,
            root,
            full_path,
            metadata,
            remaining_inline_bytes,
            collection,
        )?;
        return Ok(());
    }

    contents.extend_from_slice(b"other\0");
    append_metadata_fallback(contents, metadata);
    Ok(())
}

pub(super) fn append_untracked_file_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    metadata: &fs::Metadata,
    remaining_inline_bytes: &mut u64,
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    collection.ensure_active()?;
    contents.extend_from_slice(b"mode\0");
    #[cfg(unix)]
    contents.extend_from_slice(&(metadata.permissions().mode() & 0o7777).to_be_bytes());
    #[cfg(not(unix))]
    contents.push(u8::from(metadata.permissions().readonly()));
    if metadata.len() > MAX_INLINE_UNTRACKED_BYTES || metadata.len() > *remaining_inline_bytes {
        append_hashed_file_fingerprint(contents, root, full_path, collection)?;
        return Ok(());
    }

    let mut file = fs::File::open(full_path)
        .with_context(|| format!("Failed to open untracked file {}", full_path.display()))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_INLINE_UNTRACKED_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Failed to read untracked file {}", full_path.display()))?;
    collection.ensure_active()?;

    if bytes.len() as u64 > MAX_INLINE_UNTRACKED_BYTES {
        append_hashed_file_fingerprint(contents, root, full_path, collection)?;
        return Ok(());
    }

    contents.extend_from_slice(b"file\0");
    contents.extend_from_slice(&bytes);
    *remaining_inline_bytes = remaining_inline_bytes.saturating_sub(bytes.len() as u64);
    Ok(())
}

pub(super) fn append_hashed_file_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    contents.extend_from_slice(b"file-hash\0");
    contents.extend_from_slice(collection.git_hash_file(root, full_path)?.as_bytes());
    Ok(())
}

pub(super) fn append_metadata_fallback(contents: &mut Vec<u8>, metadata: &fs::Metadata) {
    contents.extend_from_slice(format!("len={}\0", metadata.len()).as_bytes());
    contents.extend_from_slice(
        format!("modified={}\0", system_time_key(metadata.modified().ok())).as_bytes(),
    );
}

pub(super) fn system_time_key(time: Option<SystemTime>) -> u128 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}
