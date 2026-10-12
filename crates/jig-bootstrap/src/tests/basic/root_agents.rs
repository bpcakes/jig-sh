use super::*;

#[test]
fn full_to_minimal_removes_only_the_root_agents_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("AGENTS.md"),
        "# Project Guide\n\nKeep this project-owned guidance.\n",
    )
    .unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "# Project Guide\n\nKeep this project-owned guidance.\n"
    );
}

#[test]
fn full_to_minimal_preserves_root_agents_bytes_around_the_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let rendered = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    let spec = managed_paths::managed_block_spec(Path::new("AGENTS.md")).unwrap();
    let start = rendered.find(spec.begin).unwrap();
    let end = rendered.find(spec.end).unwrap() + spec.end.len();
    let block = &rendered[start..end];
    let before = "# Project Guide\n\nKeep two trailing spaces.  \n\tindented tab\t\n\n";
    let after = "\n\n    indented code\n\ttrailing tab\t\n";
    fs::write(repo.join("AGENTS.md"), format!("{before}{block}{after}")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        format!("{}{}", &before[..before.len() - 1], &after[1..])
    );
}

#[test]
fn full_to_minimal_preserves_crlf_root_agents_bytes_around_the_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let rendered = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    let spec = managed_paths::managed_block_spec(Path::new("AGENTS.md")).unwrap();
    let start = rendered.find(spec.begin).unwrap();
    let end = rendered.find(spec.end).unwrap() + spec.end.len();
    let block = rendered[start..end].replace('\n', "\r\n");
    let before = b"# Project Guide\r\n\r\n";
    let after = b"\r\nPreserve tail spaces.  \r\n";
    let mut contents = before.to_vec();
    contents.extend_from_slice(block.as_bytes());
    contents.extend_from_slice(after);
    fs::write(repo.join("AGENTS.md"), contents).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    let mut expected = before[..before.len() - 2].to_vec();
    expected.extend_from_slice(&after[2..]);
    assert_eq!(fs::read(repo.join("AGENTS.md")).unwrap(), expected);
}

#[test]
fn full_to_minimal_writes_an_empty_root_agents_residual() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let rendered = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    let spec = managed_paths::managed_block_spec(Path::new("AGENTS.md")).unwrap();
    let start = rendered.find(spec.begin).unwrap();
    let end = rendered.find(spec.end).unwrap() + spec.end.len();
    let mut block_only = rendered.as_bytes()[start..end].to_vec();
    block_only.push(b'\n');
    fs::write(repo.join("AGENTS.md"), block_only).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(repo.join("AGENTS.md").is_file());
    assert_eq!(fs::read(repo.join("AGENTS.md")).unwrap(), b"");
}

#[test]
fn full_to_minimal_preserves_project_owned_root_agents_without_managed_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::write(repo.join("AGENTS.md"), "# Project Guide\n\nProject only.\n").unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        "# Project Guide\n\nProject only.\n"
    );
}

#[test]
fn forced_full_to_minimal_rejects_malformed_root_agents_block_without_deleting_it() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let malformed = "# Project Guide\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nmissing end\n";
    fs::write(repo.join("AGENTS.md"), malformed).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Malformed Jig managed block"));
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md")).unwrap(),
        malformed
    );
}

#[test]
fn forced_full_to_minimal_preserves_nonregular_root_agents_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::remove_file(repo.join("AGENTS.md")).unwrap();
    fs::create_dir(repo.join("AGENTS.md")).unwrap();
    fs::write(repo.join("AGENTS.md/project.txt"), "project-owned\n").unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(repo.join("AGENTS.md").is_dir());
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.md/project.txt")).unwrap(),
        "project-owned\n"
    );
}

#[cfg(unix)]
#[test]
fn forced_full_to_minimal_preserves_symlinked_root_agents_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::remove_file(repo.join("AGENTS.md")).unwrap();
    fs::write(repo.join("AGENTS.shared.md"), "# Shared Project Guide\n").unwrap();
    create_symlink(Path::new("AGENTS.shared.md"), &repo.join("AGENTS.md")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(
        fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Shared Project Guide\n"
    );
}

#[test]
fn adopt_appends_jig_block_to_existing_root_agents() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n",
    )
    .unwrap();
    fs::write(
        repo.join(".gitignore"),
        "# Project ignores\nproject-owned-cache/\n",
    )
    .unwrap();

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
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.starts_with("# Existing Agent Guide"));
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
    assert!(root_guide.contains("Use `scripts/jig` for the typed repo contract"));
    assert_eq!(
        root_guide
            .matches("<!-- BEGIN JIG MANAGED BLOCK -->")
            .count(),
        1
    );

    let gitignore = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.starts_with("# Project ignores"));
    assert!(gitignore.contains("project-owned-cache/"));
    assert!(gitignore.contains("# BEGIN JIG MANAGED BLOCK"));
    assert!(gitignore.contains("node_modules/"));
    assert_eq!(gitignore.matches("# BEGIN JIG MANAGED BLOCK").count(), 1);
}

#[cfg(unix)]
#[test]
fn adopt_refuses_to_replace_symlinked_root_agents_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.shared.md"),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n",
    )
    .unwrap();
    create_symlink(Path::new("AGENTS.shared.md"), &repo.join("AGENTS.md")).unwrap();

    let error = run_adopt(AdoptOpts {
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
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Adopt would overwrite template-managed paths"));
    assert!(error.contains("AGENTS.md"));
    assert!(
        fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n"
    );

    run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: true,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(
        !fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n"
    );
}

#[test]
fn adopt_rejects_malformed_existing_root_agents_jig_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nmissing end\n",
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo,
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
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Malformed Jig managed block"));
}
