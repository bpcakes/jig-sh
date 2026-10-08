fn git_init_branch_flag_unsupported(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("unknown switch `b")
        || stderr.contains("unknown option `b")
        || stderr.contains("unknown option `initial-branch")
        || stderr.contains("unknown option `initial branch")
}

fn git_command_failed_message(path: &Path, output: &std::process::Output) -> String {
    format!(
        "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn set_git_head_branch(
    work_tree: &Path,
    git_dir: &Path,
    git_program: &str,
    default_branch: &str,
) -> Result<()> {
    let output = staged_repository_command(git_program, work_tree, git_dir)
        .args([
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ])
        .output()
        .with_context(|| format!("Failed to start {git_program}"))?;
    require_success(&output, |output| {
        format!(
            "git symbolic-ref HEAD refs/heads/{default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
