fn pr_worktree_is_registered(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    if !inspect_managed_directory(ctx.root(), worktree, "PR repair worktree")? {
        return Ok(false);
    }
    let listing = git_output(
        ctx,
        ctx.root(),
        ["worktree", "list", "--porcelain", "-z"],
        observer,
    )
    .map_err(pr_step_error)?;
    if !listing.status.success() {
        bail!(
            "Failed to list registered Git worktrees: {}",
            String::from_utf8_lossy(&listing.stderr).trim()
        );
    }
    let expected = fs::canonicalize(worktree).with_context(|| {
        format!(
            "Failed to resolve candidate PR repair worktree {}",
            worktree.display()
        )
    })?;
    let registered = worktree_paths_from_porcelain(&listing.stdout)
        .into_iter()
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .any(|candidate| candidate == expected);
    if !registered {
        return Ok(false);
    }
    validate_linked_worktree_gitfile(ctx, worktree, observer)
}

fn worktree_paths_from_porcelain(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .map(path_from_git_bytes)
        .collect()
}

#[cfg(unix)]
fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;

    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn validate_linked_worktree_gitfile(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    let worktree_dir = Dir::open_ambient_dir(worktree, ambient_authority()).with_context(|| {
        format!("Failed to open PR repair worktree {}", worktree.display())
    })?;
    let Some(gitdir_pointer) = read_nofollow_regular_file(&worktree_dir, ".git")? else {
        return Ok(false);
    };
    let Some(gitdir_path) = parse_gitdir_pointer(&gitdir_pointer, worktree) else {
        return Ok(false);
    };
    let gitdir = match fs::canonicalize(&gitdir_path) {
        Ok(path) => path,
        Err(_) => return Ok(false),
    };
    let common = git_stdout(
        ctx,
        ctx.root(),
        ["rev-parse", "--git-common-dir"],
        observer,
    )
    .map_err(pr_step_error)?;
    let common = PathBuf::from(common);
    let common = if common.is_absolute() {
        common
    } else {
        ctx.root().join(common)
    };
    let common = fs::canonicalize(common).context("Failed to resolve the common Git directory")?;
    if gitdir.parent() != Some(common.join("worktrees").as_path()) {
        return Ok(false);
    }
    let gitdir_directory = Dir::open_ambient_dir(&gitdir, ambient_authority()).with_context(|| {
        format!(
            "Failed to open linked-worktree Git directory {}",
            gitdir.display()
        )
    })?;
    let Some(back_pointer) = read_nofollow_regular_file(&gitdir_directory, "gitdir")? else {
        return Ok(false);
    };
    let back_pointer = path_from_git_bytes(trim_ascii_line(&back_pointer));
    let back_pointer = if back_pointer.is_absolute() {
        back_pointer
    } else {
        gitdir.join(back_pointer)
    };
    let expected_gitfile = fs::canonicalize(worktree.join(".git"))?;
    Ok(fs::canonicalize(back_pointer).ok().as_ref() == Some(&expected_gitfile))
}

fn read_nofollow_regular_file(directory: &Dir, name: &str) -> Result<Option<Vec<u8>>> {
    const MAX_GIT_POINTER_BYTES: u64 = 16 * 1024;

    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to open {name} without following links"));
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_GIT_POINTER_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

fn parse_gitdir_pointer(bytes: &[u8], worktree: &Path) -> Option<PathBuf> {
    let line = trim_ascii_line(bytes);
    if line.contains(&b'\n') || line.contains(&b'\r') {
        return None;
    }
    let path = path_from_git_bytes(line.strip_prefix(b"gitdir: ")?);
    Some(if path.is_absolute() {
        path
    } else {
        worktree.join(path)
    })
}

fn trim_ascii_line(mut bytes: &[u8]) -> &[u8] {
    while bytes
        .last()
        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
    {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
