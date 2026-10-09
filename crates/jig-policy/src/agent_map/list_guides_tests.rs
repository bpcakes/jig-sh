use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use super::list_guides;

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

fn write_guides(root: &Path, guides: &[&str]) {
    for guide in guides {
        let path = root.join(guide);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "# Guide\n").unwrap();
    }
}

#[test]
fn git_work_trees_list_tracked_and_unignored_guides_outside_nested_repositories() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q"]);
    fs::write(root.join(".gitignore"), "scratch/\n").unwrap();
    fs::create_dir_all(root.join("vendor")).unwrap();
    git(&root.join("vendor"), &["init", "-q"]);
    write_guides(
        root,
        &[
            "AGENTS.md",
            "crates/api/AGENTS.md",
            "scratch/trial/AGENTS.md",
            "vendor/AGENTS.md",
        ],
    );
    git(root, &["add", "AGENTS.md"]);

    assert_eq!(
        list_guides(root).unwrap(),
        ["AGENTS.md", "crates/api/AGENTS.md"]
    );
}

#[test]
fn plain_directories_list_every_guide_outside_nested_repositories_and_build_output() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("vendor/.git")).unwrap();
    fs::write(root.join("vendor/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
    write_guides(
        root,
        &[
            "AGENTS.md",
            "crates/api/AGENTS.md",
            "vendor/AGENTS.md",
            "target/package/demo/AGENTS.md",
        ],
    );

    assert_eq!(
        list_guides(root).unwrap(),
        ["AGENTS.md", "crates/api/AGENTS.md"]
    );
}

#[test]
fn tracked_guides_stay_excluded_after_their_parent_becomes_a_repository() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q"]);
    write_guides(
        root,
        &["AGENTS.md", "vendor/deep/AGENTS.md", "kept/AGENTS.md"],
    );
    git(root, &["add", "."]);
    fs::write(root.join(".gitignore"), "kept/\n").unwrap();
    git(&root.join("vendor"), &["init", "-q"]);

    assert_eq!(list_guides(root).unwrap(), ["AGENTS.md", "kept/AGENTS.md"]);
}

#[cfg(unix)]
#[test]
fn tracked_guides_stay_excluded_after_an_ancestor_becomes_a_symlink() {
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q"]);
    write_guides(root, &["AGENTS.md", "component/deep/AGENTS.md"]);
    git(root, &["add", "."]);
    fs::remove_dir_all(root.join("component")).unwrap();
    write_guides(outside.path(), &["deep/AGENTS.md"]);
    std::os::unix::fs::symlink(outside.path(), root.join("component")).unwrap();

    assert_eq!(list_guides(root).unwrap(), ["AGENTS.md"]);
}

#[test]
fn git_file_markers_exclude_nested_guides_with_or_without_an_outer_repository() {
    for tracked in [false, true] {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write_guides(root, &["AGENTS.md", "vendor/deep/AGENTS.md"]);
        if tracked {
            git(root, &["init", "-q"]);
            git(root, &["add", "."]);
        }
        fs::write(root.join("vendor/.git"), "gitdir: ../ExampleGitDir\n").unwrap();

        assert_eq!(
            list_guides(root).unwrap(),
            ["AGENTS.md"],
            "tracked={tracked}"
        );
    }
}
