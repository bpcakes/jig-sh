#[test]
fn scaffold_rejects_conflicting_file_unless_forced_and_reports_rerun() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();
    fs::write(temp.path().join("Cargo.toml"), "project-owned\n").unwrap();

    let error = plan.write(temp.path(), false).unwrap_err().to_string();
    assert!(error.contains("already exist and differ"));
    assert!(error.contains("pass --force"));

    let preflight = tempdir().unwrap();
    fs::write(preflight.path().join("Cargo.toml"), "project-owned\n").unwrap();
    let error = plan.write(preflight.path(), false).unwrap_err().to_string();
    assert!(error.contains("Cargo.toml"));
    assert!(
        !preflight.path().join("web/package.json").exists(),
        "scaffold conflict preflight should fail before writing later files"
    );

    let forced = plan.write(temp.path(), true).unwrap();
    assert!(
        forced["files_modified"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "Cargo.toml")
    );
    assert_ne!(
        fs::read_to_string(temp.path().join("Cargo.toml")).unwrap(),
        "project-owned\n"
    );

    let rerun = plan.write(temp.path(), false).unwrap();
    assert!(
        rerun["files_unchanged"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "Cargo.toml")
    );
}

#[cfg(unix)]
#[test]
fn scaffold_preflight_rejects_symlink_boundaries_without_partial_or_outside_writes() {
    use std::os::unix::fs::symlink;

    fn plan_for(destination: &Path) -> scaffold::InitScaffoldPlan {
        scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                ..AnswerOpts::default()
            },
            destination,
        )
        .unwrap()
        .unwrap()
    }

    for force in [false, true] {
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join(format!("outside-{force}.toml"));
        fs::write(&outside_file, "outside sentinel\n").unwrap();
        let destination = tempdir().unwrap();
        symlink(&outside_file, destination.path().join("Cargo.toml")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("is a symlink"), "{error}");
        assert_eq!(
            fs::read_to_string(&outside_file).unwrap(),
            "outside sentinel\n"
        );
        assert!(!destination.path().join("apps").exists());
        assert!(!destination.path().join("web").exists());
    }

    for force in [false, true] {
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join(format!("missing-{force}.toml"));
        let destination = tempdir().unwrap();
        symlink(&outside_file, destination.path().join("Cargo.toml")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("is a symlink"), "{error}");
        assert!(!outside_file.exists(), "broken link target was created");
        assert!(!destination.path().join("apps").exists());
        assert!(!destination.path().join("web").exists());
    }

    for force in [false, true] {
        let outside = tempdir().unwrap();
        let destination = tempdir().unwrap();
        symlink(outside.path(), destination.path().join("web")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("ancestor"), "{error}");
        assert!(error.contains("is a symlink"), "{error}");
        assert!(
            !destination.path().join("Cargo.toml").exists(),
            "a late unsafe output must fail before earlier scaffold files are published"
        );
        assert!(
            fs::read_dir(outside.path()).unwrap().next().is_none(),
            "scaffold wrote through a symlinked output ancestor"
        );
    }

    for force in [false, true] {
        let destination = tempdir().unwrap();
        fs::create_dir(destination.path().join("Cargo.toml")).unwrap();

        let error = plan_for(destination.path())
            .write(destination.path(), force)
            .unwrap_err()
            .to_string();

        assert!(error.contains("destination leaf"), "{error}");
        assert!(error.contains("is a directory"), "{error}");
        assert!(!destination.path().join("apps").exists());
        assert!(!destination.path().join("web").exists());
    }
}

#[cfg(unix)]
#[test]
fn init_preflights_scaffold_and_agent_map_outputs_before_rendering_the_harness() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for relative in ["Cargo.toml", managed_paths::AGENT_MAP_PATH] {
        let destination = temp.path().join(relative.replace(['/', '.'], "-"));
        fs::create_dir(&destination).unwrap();
        let outside = temp
            .path()
            .join(format!("outside-{}", relative.replace('/', "-")));
        fs::write(&outside, "outside sentinel\n").unwrap();
        symlink(&outside, destination.join(relative)).unwrap();

        let error = run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: Some(ScaffoldDb::None),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: true,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("is a symlink"), "{relative}: {error}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
        assert!(
            !destination.join(".jig.toml").exists(),
            "managed rendering started before {relative} was rejected"
        );
        assert!(!destination.join("scripts/jig").exists());
    }
}

#[test]
fn init_rejects_portable_scaffold_output_collisions_before_any_repository_write() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let app = |name: &str, dir: &str| FrontendApp {
        name: name.into(),
        dir: dir.into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    };

    for force in [false, true] {
        for (case_name, frontend_apps, expected_paths) in [
            (
                "scaffold-file-ancestor",
                vec![app("client", "package.json")],
                ["package.json", "package.json/.gitignore"],
            ),
            (
                "template-file-ancestor",
                vec![app("client", "scripts/jig")],
                ["scripts/jig", "scripts/jig/.gitignore"],
            ),
            (
                "case-folded-frontends",
                vec![app("first", "Web"), app("second", "web")],
                ["Web/", "web/"],
            ),
        ] {
            let destination = temp.path().join(format!("{case_name}-{force}"));
            fs::create_dir(&destination).unwrap();
            let outside = temp.path().join(format!("outside-{case_name}-{force}"));
            fs::write(&outside, "outside sentinel\n").unwrap();

            let error = run_init(InitOpts {
                path: destination.clone(),
                scaffold: ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(ScaffoldDb::None),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some("demo".into()),
                    frontend_apps,
                    ..AnswerOpts::default()
                },
            })
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("Portable planned repository file collision"),
                "{case_name}/{force}: {error}"
            );
            for expected in expected_paths {
                assert!(
                    error.contains(expected),
                    "{case_name}/{force}: missing {expected:?} in {error}"
                );
            }
            assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
            assert!(
                destination.is_dir(),
                "{case_name}/{force}: a pre-existing empty destination must remain"
            );
            assert!(
                fs::read_dir(&destination).unwrap().next().is_none(),
                "{case_name}/{force}: collision preflight partially mutated the destination"
            );
            assert!(!destination.join(".jig.toml").exists());
            assert!(!destination.join("scripts/jig").exists());
            assert!(!destination.join("Cargo.toml").exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn harness_only_init_rejects_win32_forbidden_managed_template_paths_before_publication() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    fs::write(
        template.path().join("templates/project/bad:name.jinja"),
        "nonportable\n",
    )
    .unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
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

    assert!(error.contains("not portable to Windows"), "{error}");
    assert!(error.contains("bad:name"), "{error}");
    assert!(!destination.exists());
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn harness_only_init_rejects_non_unicode_managed_parent_before_publication() {
    use std::os::unix::ffi::OsStringExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let invalid_parent =
        template
            .path()
            .join("templates/project")
            .join(std::ffi::OsString::from_vec(
                b"invalid-\xff-parent".to_vec(),
            ));
    fs::create_dir(&invalid_parent).unwrap();
    fs::write(invalid_parent.join("valid-leaf.jinja"), "nonportable\n").unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
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

    assert!(error.contains("valid Unicode"), "{error}");
    assert!(error.contains("valid-leaf"), "{error}");
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn adopt_rejects_win32_forbidden_managed_template_paths_before_repository_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    fs::write(
        template.path().join("templates/project/bad?name.jinja"),
        "nonportable\n",
    )
    .unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("sentinel"), "preserve\n").unwrap();

    let error = run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("not portable to Windows"), "{error}");
    assert_eq!(
        fs::read_to_string(repo.join("sentinel")).unwrap(),
        "preserve\n"
    );
    assert!(!repo.join("bad?name").exists());
    assert!(!repo.join(".jig.toml").exists());
}

#[cfg(unix)]
#[test]
fn update_rejects_new_control_bearing_managed_template_paths_before_repository_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("sentinel"), "preserve\n").unwrap();
    let template_path = template.path().display().to_string();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template_path.clone()),
        template_mode: None,
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
    let answers_before = fs::read(repo.join(".jig.toml")).unwrap();
    fs::write(
        template
            .path()
            .join("templates/project/bad\u{1f}name.jinja"),
        "nonportable\n",
    )
    .unwrap();

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template_path),
        template_mode: None,
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("not portable to Windows"), "{error}");
    assert_eq!(fs::read(repo.join(".jig.toml")).unwrap(), answers_before);
    assert_eq!(
        fs::read_to_string(repo.join("sentinel")).unwrap(),
        "preserve\n"
    );
    assert!(!repo.join("bad\u{1f}name").exists());
}

#[test]
fn init_rolls_back_new_destination_after_planned_output_collision() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let app = |name: &str, dir: &str| FrontendApp {
        name: name.into(),
        dir: dir.into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    };

    for force in [false, true] {
        for (case_name, frontend_apps) in [
            (
                "internal-case-folded-frontends",
                vec![app("first", "Web"), app("second", "web")],
            ),
            (
                "managed-scaffold-ancestor",
                vec![app("client", "scripts/jig")],
            ),
        ] {
            let created_ancestor = temp.path().join(format!("{case_name}-{force}"));
            let destination = created_ancestor.join("nested/new-repo");
            assert!(!destination.exists());

            let error = run_init(InitOpts {
                path: destination.clone(),
                scaffold: ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(ScaffoldDb::None),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some("demo".into()),
                    frontend_apps,
                    ..AnswerOpts::default()
                },
            })
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("Portable planned repository file collision"),
                "{case_name}/{force}: {error}"
            );
            assert!(
                !destination.exists(),
                "{case_name}/{force}: failed init left its new destination behind"
            );
            assert!(
                !created_ancestor.exists(),
                "{case_name}/{force}: failed init left created parent directories behind"
            );
        }
    }
}

#[test]
fn init_destination_rollback_preserves_existing_and_concurrently_created_destinations() {
    let temp = tempdir().unwrap();

    let pre_existing = temp.path().join("pre-existing");
    fs::create_dir(&pre_existing).unwrap();
    InitMutationTransaction::create(&pre_existing)
        .unwrap()
        .rollback()
        .unwrap();
    assert!(pre_existing.is_dir());

    let with_content = temp.path().join("created/with-content");
    let mut rollback = InitMutationTransaction::create(&with_content).unwrap();
    fs::create_dir_all(&with_content).unwrap();
    fs::write(with_content.join("concurrent.txt"), "preserve\n").unwrap();
    rollback.rollback().unwrap();
    assert_eq!(
        fs::read_to_string(with_content.join("concurrent.txt")).unwrap(),
        "preserve\n"
    );
    assert!(temp.path().join("created").is_dir());
}

#[cfg(unix)]
#[test]
fn init_rejects_an_existing_final_symlink_destination() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();
    let resolved = path::resolve_init_destination(&link, temp.path()).unwrap();
    assert_eq!(resolved, link);
    let error = validate_init_destination(&resolved, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a real directory"), "{error}");
}

#[cfg(unix)]
#[test]
fn missing_init_tree_is_private_then_published_with_normal_directory_mode() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let probe = temp.path().join("mode-probe");
    fs::create_dir(&probe).unwrap();
    let expected_mode = fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
    fs::remove_dir(&probe).unwrap();

    let destination = temp.path().join("new-top/nested/repo");
    let mut transaction = InitMutationTransaction::create(&destination).unwrap();
    let staging = transaction
        .staged_publication
        .as_ref()
        .unwrap()
        .publish_source
        .clone();
    assert_eq!(
        fs::metadata(&staging).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::write(
        transaction.work_destination().join("sentinel"),
        "complete\n",
    )
    .unwrap();
    assert!(!destination.exists());

    transaction.commit().unwrap();
    assert_eq!(
        fs::metadata(temp.path().join("new-top"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        expected_mode
    );
    assert_eq!(
        fs::read_to_string(destination.join("sentinel")).unwrap(),
        "complete\n"
    );
    assert!(!staging.exists());
}

#[test]
fn missing_init_tree_publication_never_replaces_concurrent_top_component() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("contended/nested/repo");
    let mut transaction = InitMutationTransaction::create(&destination).unwrap();
    let staging = transaction
        .staged_publication
        .as_ref()
        .unwrap()
        .publish_source
        .clone();
    fs::write(transaction.work_destination().join("generated"), "jig\n").unwrap();
    fs::create_dir(temp.path().join("contended")).unwrap();
    fs::write(temp.path().join("contended/foreign"), "preserve\n").unwrap();

    let error = transaction.commit().unwrap_err().to_string();
    assert!(
        error.contains("without replacing concurrent path"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("contended/foreign")).unwrap(),
        "preserve\n"
    );
    assert!(!destination.exists());
    assert!(!staging.exists());
}

#[cfg(unix)]
#[test]
fn missing_init_tree_rejects_an_intermediate_symlink_swap_before_file_publication() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let destination = temp.path().join("new-top/nested/repo");
    let mut transaction = InitMutationTransaction::create(&destination).unwrap();
    let relative = Path::new("generated");
    transaction.prepare_file_publication(relative).unwrap();

    let staging = transaction
        .staged_publication
        .as_ref()
        .unwrap()
        .publish_source
        .clone();
    let intermediate = staging.join("nested");
    let retained_intermediate = staging.join("nested-original");
    let foreign_intermediate = temp.path().join("foreign-nested");
    fs::create_dir(&foreign_intermediate).unwrap();
    fs::create_dir(foreign_intermediate.join("repo")).unwrap();
    fs::write(foreign_intermediate.join("marker"), "preserve\n").unwrap();
    fs::rename(&intermediate, &retained_intermediate).unwrap();
    symlink(&foreign_intermediate, &intermediate).unwrap();

    let error = path::write_repository_file_atomic_staged(
        transaction.work_destination(),
        relative,
        b"jig\n",
        path::RepositoryFileLeaf::Missing,
        || transaction.verify_destination_identity(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("replaced while init was running"), "{error}");
    assert!(!foreign_intermediate.join("repo/generated").exists());
    assert_eq!(
        fs::read_to_string(foreign_intermediate.join("marker")).unwrap(),
        "preserve\n"
    );

    let rollback = transaction.rollback().unwrap_err().to_string();
    assert!(rollback.contains("Preserving the complete staging tree"));
    fs::remove_file(&intermediate).unwrap();
    fs::rename(&retained_intermediate, &intermediate).unwrap();
    fs::remove_dir_all(&staging).unwrap();
}
