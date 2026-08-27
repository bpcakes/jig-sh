fn rust_renames(ctx: &RepoContext, opts: &RustFileLocInput) -> Result<BTreeMap<String, String>> {
    let mut args = vec!["diff", "--name-status", "--diff-filter=R", "-z"];
    if opts.staged {
        args.push("--cached");
    } else if let Some(reference) = &opts.changed_against {
        args.push(reference);
        args.push("HEAD");
    }
    args.push("--");
    let root_args = ctx
        .rust_crate_roots()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    args.extend(root_args);
    let entries = split_nul(&git_output(ctx.root(), &args)?);
    let mut renames = BTreeMap::new();
    let mut index = 0usize;
    while index + 2 < entries.len() {
        let _status = &entries[index];
        let old = entries[index + 1].clone();
        let new = entries[index + 2].clone();
        renames.insert(new, old);
        index += 3;
    }
    Ok(renames)
}

fn previous_line_count(
    root: &Path,
    reference: &str,
    path: &str,
    renamed_from: Option<&String>,
) -> Result<usize> {
    if let Some(contents) = git_blob_optional(root, &format!("{reference}:{path}"))? {
        return Ok(contents.lines().count());
    }
    let Some(old) = renamed_from else {
        return Ok(0);
    };
    Ok(git_blob_optional(root, &format!("{reference}:{old}"))?
        .map(|contents| contents.lines().count())
        .unwrap_or(0))
}

fn git_list_files(root: &Path, roots: &[String]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend(roots.iter().map(String::as_str));
    Ok(split_nul(&git_output(root, &args)?))
}

fn git_blob(root: &Path, spec: &str) -> Result<String> {
    git_text(root, &["show", spec])
}

fn git_blob_optional(root: &Path, spec: &str) -> Result<Option<String>> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["show", spec])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    configure_known_root_git_environment(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

pub(super) fn git_success(root: &Path, args: &[&str]) -> Result<bool> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_known_root_git_environment(&mut command);
    Ok(command.status()?.success())
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_output(root, args)?).into_owned())
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args).stdin(Stdio::null());
    configure_known_root_git_environment(&mut command);
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

pub(super) fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_sep = false;
        } else if !last_was_sep && !slug.is_empty() {
            slug.push('_');
            last_was_sep = true;
        }
    }
    slug.trim_matches('_').to_string()
}

fn utc_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}
