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
        if is_safe_fixture_destination(destination, &std::env::temp_dir())
            && !destination.join(".git").exists()
        {
            init_git_repo_for_test(destination);
        }
    }
    super::super::run_adopt(opts)
}

fn is_safe_fixture_destination(destination: &Path, temp_root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(destination) else {
        return false;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }

    let Ok(canonical_destination) = fs::canonicalize(destination) else {
        return false;
    };
    if canonical_destination == destination {
        return true;
    }

    // macOS exposes its temporary root through `/var`, whose canonical path
    // begins with `/private/var`. Permit that trusted prefix alias without
    // accepting symlink components created below the temporary root.
    let Ok(relative_destination) = destination.strip_prefix(temp_root) else {
        return false;
    };
    let Ok(canonical_temp_root) = fs::canonicalize(temp_root) else {
        return false;
    };
    canonical_temp_root.join(relative_destination) == canonical_destination
}

pub(super) fn write_test_crate_guide(repo: &Path) {
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    if !repo.join(".git").exists() {
        init_git_repo_for_test(repo);
    }
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
}

#[cfg(unix)]
#[test]
fn fixture_destination_allows_only_the_temp_root_alias() {
    use std::os::unix::fs::symlink;

    let outer = tempdir().unwrap();
    let real_temp_root = outer.path().join("real-temp");
    let aliased_temp_root = outer.path().join("aliased-temp");
    let destination = aliased_temp_root.join("repo");
    fs::create_dir_all(real_temp_root.join("repo")).unwrap();
    symlink(&real_temp_root, &aliased_temp_root).unwrap();

    assert!(is_safe_fixture_destination(
        &destination,
        &aliased_temp_root
    ));
    assert!(!is_safe_fixture_destination(&destination, outer.path()));
}
