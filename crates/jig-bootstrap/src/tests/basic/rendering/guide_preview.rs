use super::*;
use crate::git::git_command;

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

#[test]
fn preview_workspace_ignores_ambient_git_repository_selectors() {
    let _guard = lock_env();
    let source = tempdir().unwrap();
    let other = tempdir().unwrap();
    let destination = tempdir().unwrap();
    for (root, guide) in [
        (source.path(), "kept/AGENTS.md"),
        (other.path(), "foreign/AGENTS.md"),
    ] {
        fs::create_dir_all(root.join(guide).parent().unwrap()).unwrap();
        fs::write(root.join(guide), "repository guide\n").unwrap();
        for args in [vec!["init", "-q"], vec!["add", "."]] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    // A foreign index entry must not make an ignored local scratch guide eligible.
    fs::create_dir(source.path().join("foreign")).unwrap();
    fs::write(source.path().join("foreign/AGENTS.md"), "scratch guide\n").unwrap();
    fs::write(source.path().join(".gitignore"), "foreign/\nkept/\n").unwrap();
    let _git_dir = EnvVarGuard::set("GIT_DIR", other.path().join(".git"));
    let _git_work_tree = EnvVarGuard::set("GIT_WORK_TREE", other.path());
    let _git_index = EnvVarGuard::set("GIT_INDEX_FILE", other.path().join(".git/index"));

    assert_eq!(
        jig_policy::list_agent_guides(source.path()).unwrap(),
        ["kept/AGENTS.md"]
    );
    seed_preview_workspace(source.path(), destination.path()).unwrap();
    assert!(destination.path().join("kept/AGENTS.md").is_file());
    assert!(!destination.path().join("foreign").exists());
}

#[test]
fn update_keeps_ignored_dependency_and_cache_guides_out_of_the_agent_map() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-q", "-m", "adopt"]).unwrap();

    // Local guides that the rendered ignore rules keep out of every clean checkout.
    let ignored_guides = [
        "node_modules/example/AGENTS.md",
        ".agent/.cache/adopt/backups/example/AGENTS.md",
    ];
    for guide in ignored_guides {
        let path = repo.join(guide);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "local guide").unwrap();
        let ignored = git_command(&repo, ["check-ignore", "-q", guide])
            .output()
            .unwrap()
            .status
            .success();
        assert!(ignored, "{guide} should be ignored by the rendered rules");
    }

    run_update(update_opts(&repo, template.path(), true)).unwrap();

    let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
    assert!(
        agent_map.contains("[crates/api](./crates/api/AGENTS.md)"),
        "{agent_map}"
    );
    for guide in ignored_guides {
        assert!(!agent_map.contains(guide), "{agent_map}");
    }
    // Every linked guide is tracked, so the committed map resolves in a clean checkout.
    let links: Vec<&str> = agent_map
        .match_indices("](./")
        .map(|(start, marker)| {
            let rest = &agent_map[start + marker.len()..];
            &rest[..rest.find(')').unwrap()]
        })
        .collect();
    assert!(!links.is_empty(), "{agent_map}");
    for link in links {
        let tracked = git_command(&repo, ["ls-files", "--error-unmatch", link])
            .output()
            .unwrap()
            .status
            .success();
        assert!(
            tracked,
            "{link} is linked from agent-map.md but not tracked"
        );
    }
}
