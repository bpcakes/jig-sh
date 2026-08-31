use super::*;

// Durable adoption publication intentionally requires Git metadata. Most
// integration fixtures model ordinary repositories but predate that
// requirement, so establish the fixture worktree before invoking production.
// Unsafe and symlink destinations remain untouched for path-validation tests.
pub(super) fn run_adopt(opts: AdoptOpts) -> Result<Value> {
    if opts.write {
        let invocation_cwd = path::bootstrap_invocation_cwd()?;
        let joined_destination;
        let destination = if opts.path.is_absolute() {
            opts.path.as_path()
        } else {
            joined_destination = invocation_cwd.join(&opts.path);
            joined_destination.as_path()
        };
        if fs::symlink_metadata(destination)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            && fs::canonicalize(destination).is_ok_and(|path| path == destination)
            && !destination.join(".git").exists()
        {
            init_git_repo_for_test(destination);
        }
    }
    super::super::run_adopt(opts)
}

pub(super) fn write_test_crate_guide(repo: &Path) {
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    if !repo.join(".git").exists() {
        init_git_repo_for_test(repo);
    }
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
}
