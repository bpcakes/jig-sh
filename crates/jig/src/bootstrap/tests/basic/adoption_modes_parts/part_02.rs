#[test]
fn forced_full_to_minimal_adoption_retires_full_harness_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    add_project_runtime_tables(&repo);
    let full_manifest = managed_manifest_paths(&repo)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(repo.join(".mcp.json").is_file());
    assert!(repo.join("scripts/jig").is_file());
    assert!(repo.join(".github/workflows/rust-tests.yml").is_file());

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();
    let minimal_manifest = managed_manifest_paths(&repo)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let expected_retirements = full_manifest
        .difference(&minimal_manifest)
        .cloned()
        .collect::<Vec<_>>();
    let reported_retirements = output["adoption_profile"]["retired_managed_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(reported_retirements, expected_retirements);
    assert_eq!(
        reported_retirements,
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );

    assert_eq!(output["harness_footprint"], "minimal");
    assert!(!repo.join(".mcp.json").exists());
    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join(".github/workflows/rust-tests.yml").exists());
    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert_eq!(root_guide, "# Repository Guidelines\n");
    assert!(
        output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".mcp.json")
    );
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_project_runtime_tables(&config);
    crate::context::RepoContext::load_from(&repo).unwrap();
}

#[cfg(unix)]
#[test]
fn minimal_adoption_rejects_managed_symlink_ancestors_in_preview_write_and_force_modes() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for ancestor in [".agent", ".github", "scripts"] {
        for (label, write, force) in [
            ("preview", false, false),
            ("write", true, false),
            ("force", true, true),
        ] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
            let config_before = fs::read(repo.join(".jig.toml")).unwrap();
            let outside = temp.path().join(format!(
                "outside-{}-{label}",
                ancestor.trim_start_matches('.')
            ));
            fs::rename(repo.join(ancestor), &outside).unwrap();
            fs::write(outside.join("project-sentinel"), "outside\n").unwrap();
            let protected_relative = match ancestor {
                ".agent" => managed_paths::MANIFEST_PATH
                    .strip_prefix(".agent/")
                    .unwrap(),
                ".github" => "workflows/rust-tests.yml",
                "scripts" => "jig",
                _ => unreachable!(),
            };
            let protected_before = fs::read(outside.join(protected_relative)).unwrap();
            let outside_before = regular_file_tree_snapshot(&outside);
            create_symlink(&outside, &repo.join(ancestor)).unwrap();
            let mut opts = footprint_adopt_opts(&repo, template.path(), true, force);
            opts.write = write;

            let error = run_adopt(opts).unwrap_err().to_string();

            assert!(
                error.contains("is a symlink"),
                "{ancestor}/{label}: {error}"
            );
            assert_eq!(fs::read(repo.join(".jig.toml")).unwrap(), config_before);
            assert_eq!(
                fs::read(outside.join(protected_relative)).unwrap(),
                protected_before,
                "{ancestor}/{label} changed an outside managed path"
            );
            assert_eq!(
                fs::read_to_string(outside.join("project-sentinel")).unwrap(),
                "outside\n"
            );
            assert_eq!(regular_file_tree_snapshot(&outside), outside_before);
            assert!(
                fs::symlink_metadata(repo.join(ancestor))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}

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

#[test]
fn adopt_minimal_preview_keeps_write_flag_in_next_steps() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: true,
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

    assert_eq!(output["render_mode"], "preview");
    assert_eq!(output["harness_footprint"], "minimal");
    assert!(!repo.join(".jig.toml").exists());
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| !command.as_str().unwrap().contains("scripts/jig"))
    );
    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(output["next_steps"].as_array().unwrap().iter().all(|step| {
        !step
            .as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write --force")
    }));
}

#[test]
fn full_to_minimal_preview_requires_force_in_the_emitted_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| { step.as_str() == Some("jig adopt . --minimal --write --force") })
    );
}

#[test]
fn minimal_to_minimal_preview_does_not_add_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| { !step.as_str().unwrap().contains("--force") })
    );
}

#[test]
fn invalid_prior_minimal_preview_does_not_add_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        "harness_footprint = \"not-a-footprint\"\n",
    )
    .unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| { !step.as_str().unwrap().contains("--force") })
    );
}

#[test]
fn adopt_preserves_existing_vault_scope_id() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    let first_scope = rendered_vault_scope_id(&repo);

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(rendered_vault_scope_id(&repo), first_scope);
}

#[test]
fn adopt_reports_legacy_vault_scope_migration_note() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("Existing .jig.toml had no [vault] block")
    }));
}

#[test]
fn adopt_rejects_existing_repo_vault_scope_without_scope_id() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].scope_id is required"));
}
