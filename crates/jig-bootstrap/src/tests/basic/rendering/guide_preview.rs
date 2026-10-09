use super::*;

#[test]
fn preview_workspace_only_copies_agent_guides() {
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    fs::create_dir_all(source.path().join("crates/api")).unwrap();
    fs::create_dir_all(source.path().join("crates/vendor/.git/modules/demo")).unwrap();
    fs::create_dir_all(source.path().join("target/debug")).unwrap();
    fs::create_dir_all(source.path().join("target/package/demo")).unwrap();
    fs::write(source.path().join("AGENTS.md"), "root").unwrap();
    fs::write(source.path().join("crates/api/AGENTS.md"), "nested").unwrap();
    fs::write(
        source
            .path()
            .join("crates/vendor/.git/modules/demo/AGENTS.md"),
        "submodule metadata",
    )
    .unwrap();
    fs::write(source.path().join("target/debug/build.log"), "noise").unwrap();
    fs::write(
        source.path().join("target/package/demo/AGENTS.md"),
        "artifact",
    )
    .unwrap();

    seed_preview_workspace(source.path(), destination.path()).unwrap();

    assert!(destination.path().join("AGENTS.md").exists());
    assert!(destination.path().join("crates/api/AGENTS.md").exists());
    assert!(
        !destination
            .path()
            .join("crates/vendor/.git/modules/demo/AGENTS.md")
            .exists()
    );
    assert!(!destination.path().join("target/debug/build.log").exists());
    assert!(
        !destination
            .path()
            .join("target/package/demo/AGENTS.md")
            .exists()
    );
}

#[test]
fn preview_workspace_skips_ignored_paths_and_nested_repositories() {
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    for root in [source.path().to_path_buf(), source.path().join("vendor")] {
        fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(source.path().join(".gitignore"), "scratch/\n").unwrap();
    for guide in [
        "AGENTS.md",
        "crates/api/AGENTS.md",
        "scratch/trial/AGENTS.md",
        "vendor/AGENTS.md",
    ] {
        let path = source.path().join(guide);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "guide").unwrap();
    }

    seed_preview_workspace(source.path(), destination.path()).unwrap();

    assert!(destination.path().join("AGENTS.md").exists());
    assert!(destination.path().join("crates/api/AGENTS.md").exists());
    assert!(!destination.path().join("scratch").exists());
    assert!(!destination.path().join("vendor").exists());
}

#[cfg(unix)]
#[test]
fn preview_workspace_rechecks_tracked_guide_directory_boundaries() {
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let git = |root: &Path, args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
    };
    git(source.path(), &["init", "-q"]);
    for guide in [
        "AGENTS.md",
        "kept/AGENTS.md",
        "component/deep/AGENTS.md",
        "vendor/deep/AGENTS.md",
    ] {
        let path = source.path().join(guide);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "repository guide\n").unwrap();
    }
    git(source.path(), &["add", "."]);
    fs::write(source.path().join(".gitignore"), "kept/\n").unwrap();
    git(&source.path().join("vendor"), &["init", "-q"]);
    fs::remove_dir_all(source.path().join("component")).unwrap();
    fs::create_dir(outside.path().join("deep")).unwrap();
    fs::write(outside.path().join("deep/AGENTS.md"), "outside guide\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), source.path().join("component")).unwrap();

    seed_preview_workspace(source.path(), destination.path()).unwrap();

    assert_eq!(
        fs::read_to_string(destination.path().join("kept/AGENTS.md")).unwrap(),
        "repository guide\n"
    );
    assert!(destination.path().join("AGENTS.md").is_file());
    assert!(!destination.path().join("component").exists());
    assert!(!destination.path().join("vendor").exists());
}
