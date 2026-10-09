use super::*;

#[test]
fn scaffold_rejects_conflicting_file_unless_forced_and_reports_rerun() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
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
        !preflight.path().join("apps/web/package.json").exists(),
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
fn symlink_test_plan(destination: &Path) -> scaffold::InitScaffoldPlan {
    scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
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

#[cfg(unix)]
fn assert_existing_symlink_rejected(force: bool) {
    let outside = tempdir().unwrap();
    let outside_file = outside.path().join(format!("outside-{force}.toml"));
    fs::write(&outside_file, "outside sentinel\n").unwrap();
    let destination = tempdir().unwrap();
    std::os::unix::fs::symlink(&outside_file, destination.path().join("Cargo.toml")).unwrap();
    let error = symlink_test_plan(destination.path())
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

#[cfg(unix)]
fn assert_broken_symlink_rejected(force: bool) {
    let outside = tempdir().unwrap();
    let outside_file = outside.path().join(format!("missing-{force}.toml"));
    let destination = tempdir().unwrap();
    std::os::unix::fs::symlink(&outside_file, destination.path().join("Cargo.toml")).unwrap();
    let error = symlink_test_plan(destination.path())
        .write(destination.path(), force)
        .unwrap_err()
        .to_string();
    assert!(error.contains("is a symlink"), "{error}");
    assert!(!outside_file.exists(), "broken link target was created");
    assert!(!destination.path().join("apps").exists());
    assert!(!destination.path().join("web").exists());
}

#[cfg(unix)]
fn assert_symlinked_ancestor_rejected(force: bool) {
    let outside = tempdir().unwrap();
    let destination = tempdir().unwrap();
    fs::create_dir(destination.path().join("apps")).unwrap();
    std::os::unix::fs::symlink(outside.path(), destination.path().join("apps/web")).unwrap();
    let error = symlink_test_plan(destination.path())
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

#[cfg(unix)]
fn assert_directory_leaf_rejected(force: bool) {
    let destination = tempdir().unwrap();
    fs::create_dir(destination.path().join("Cargo.toml")).unwrap();
    let error = symlink_test_plan(destination.path())
        .write(destination.path(), force)
        .unwrap_err()
        .to_string();
    assert!(error.contains("destination leaf"), "{error}");
    assert!(error.contains("is a directory"), "{error}");
    assert!(!destination.path().join("apps").exists());
    assert!(!destination.path().join("web").exists());
}

#[cfg(unix)]
#[test]
fn scaffold_preflight_rejects_symlink_boundaries_without_partial_or_outside_writes() {
    for force in [false, true] {
        assert_existing_symlink_rejected(force);
    }

    for force in [false, true] {
        assert_broken_symlink_rejected(force);
    }

    for force in [false, true] {
        assert_symlinked_ancestor_rejected(force);
    }

    for force in [false, true] {
        assert_directory_leaf_rejected(force);
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
                metrics: None,
                jobs: None,
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
                    metrics: None,
                    jobs: None,
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

#[test]
fn init_destination_accepts_an_existing_real_directory_after_create_is_denied() {
    let temp = tempdir().unwrap();
    let existing = temp.path().join("existing");
    fs::create_dir(&existing).unwrap();

    validate_existing_init_directory_after_create_error(
        &existing,
        io::Error::new(io::ErrorKind::PermissionDenied, "root create denied"),
        true,
    )
    .unwrap();

    let file = temp.path().join("file");
    fs::write(&file, "not a directory\n").unwrap();
    let error = validate_existing_init_directory_after_create_error(
        &file,
        io::Error::new(io::ErrorKind::AlreadyExists, "already exists"),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a real directory"), "{error}");
}

#[cfg(unix)]
#[test]
fn init_destination_never_accepts_an_existing_directory_symlink_after_create_fails() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();

    let error = validate_existing_init_directory_after_create_error(
        &link,
        io::Error::new(io::ErrorKind::AlreadyExists, "already exists"),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a real directory"), "{error}");
}
