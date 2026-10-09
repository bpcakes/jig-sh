use super::*;

#[test]
fn custom_template_retires_git_blocks_to_exact_project_residuals() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    let gitignore_spec = managed_paths::managed_block_spec(Path::new(".gitignore")).unwrap();
    let gitignore_rendered = fs::read_to_string(repo.join(".gitignore")).unwrap();
    let gitignore_start = gitignore_rendered.find(gitignore_spec.begin).unwrap();
    let gitignore_end =
        gitignore_rendered.find(gitignore_spec.end).unwrap() + gitignore_spec.end.len();
    let gitignore_block = &gitignore_rendered.as_bytes()[gitignore_start..gitignore_end];
    let mut gitignore = b"project-cache/  \n\tproject-tab\t\n\n".to_vec();
    gitignore.extend_from_slice(gitignore_block);
    gitignore.extend_from_slice(b"\nkeep-after/  \n");
    fs::write(repo.join(".gitignore"), gitignore).unwrap();

    let attributes_spec = managed_paths::managed_block_spec(Path::new(".gitattributes")).unwrap();
    let attributes_rendered = fs::read_to_string(repo.join(".gitattributes")).unwrap();
    let attributes_start = attributes_rendered.find(attributes_spec.begin).unwrap();
    let attributes_end =
        attributes_rendered.find(attributes_spec.end).unwrap() + attributes_spec.end.len();
    let mut attributes = attributes_rendered.as_bytes()[attributes_start..attributes_end].to_vec();
    attributes.push(b'\n');
    fs::write(repo.join(".gitattributes"), attributes).unwrap();

    fs::remove_file(template.path().join("templates/project/.gitignore.jinja")).unwrap();
    fs::remove_file(
        template
            .path()
            .join("templates/project/.gitattributes.jinja"),
    )
    .unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert_eq!(
        fs::read(repo.join(".gitignore")).unwrap(),
        b"project-cache/  \n\tproject-tab\t\nkeep-after/  \n"
    );
    assert!(repo.join(".gitattributes").is_file());
    assert_eq!(fs::read(repo.join(".gitattributes")).unwrap(), b"");
    let manifest = managed_manifest_paths(&repo);
    assert!(manifest.iter().all(|path| path != ".gitignore"));
    assert!(manifest.iter().all(|path| path != ".gitattributes"));
    for retired in [".gitignore", ".gitattributes"] {
        assert!(
            output["render_report"]["retired_managed_paths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == retired)
        );
        assert!(
            output["render_report"]["files_modified"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == retired)
        );
        assert!(
            output["render_report"]["files_removed"]
                .as_array()
                .unwrap()
                .iter()
                .all(|path| path != retired)
        );
    }
}

#[test]
fn custom_template_preserves_git_block_paths_without_valid_blocks() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::write(repo.join(".gitignore"), "project-only/\n").unwrap();
    fs::remove_file(repo.join(".gitattributes")).unwrap();
    fs::create_dir(repo.join(".gitattributes")).unwrap();
    fs::write(
        repo.join(".gitattributes/project-owned"),
        "directory sentinel\n",
    )
    .unwrap();
    fs::remove_file(template.path().join("templates/project/.gitignore.jinja")).unwrap();
    fs::remove_file(
        template
            .path()
            .join("templates/project/.gitattributes.jinja"),
    )
    .unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join(".gitignore")).unwrap(),
        "project-only/\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join(".gitattributes/project-owned")).unwrap(),
        "directory sentinel\n"
    );
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| path != ".gitignore" && path != ".gitattributes")
    );
    assert!(
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| path != ".gitignore" && path != ".gitattributes")
    );
}

#[cfg(unix)]
#[test]
fn custom_template_preserves_symlinked_retired_git_block_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    for (relative, target) in [
        (".gitignore", "project.gitignore"),
        (".gitattributes", "project.gitattributes"),
    ] {
        fs::remove_file(repo.join(relative)).unwrap();
        fs::write(repo.join(target), format!("project-owned {relative}\n")).unwrap();
        create_symlink(Path::new(target), &repo.join(relative)).unwrap();
        fs::remove_file(
            template
                .path()
                .join(format!("templates/project/{relative}.jinja")),
        )
        .unwrap();
    }

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    for (relative, target) in [
        (".gitignore", "project.gitignore"),
        (".gitattributes", "project.gitattributes"),
    ] {
        assert!(
            fs::symlink_metadata(repo.join(relative))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(repo.join(target)).unwrap(),
            format!("project-owned {relative}\n")
        );
    }
}

#[test]
fn malformed_retired_git_block_fails_before_apply_and_preserves_prior_manifest() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let manifest_before = fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
    let attributes_before = fs::read(repo.join(".gitattributes")).unwrap();
    let malformed = b"project-only/\n# BEGIN JIG MANAGED BLOCK\nmissing end\n";
    fs::write(repo.join(".gitignore"), malformed).unwrap();
    fs::remove_file(template.path().join("templates/project/.gitignore.jinja")).unwrap();
    fs::remove_file(
        template
            .path()
            .join("templates/project/.gitattributes.jinja"),
    )
    .unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Malformed Jig managed block"), "{error}");
    assert_eq!(fs::read(repo.join(".gitignore")).unwrap(), malformed);
    assert_eq!(
        fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
        manifest_before
    );
    assert_eq!(
        fs::read(repo.join(".gitattributes")).unwrap(),
        attributes_before
    );
}
