use crate::bootstrap::scaffold::InitScaffoldPlan;

#[cfg(unix)]
fn rust_only_init_opts(
    path: PathBuf,
    preset: ScaffoldPreset,
    template: Option<&Path>,
    force: bool,
) -> InitOpts {
    let repo_name = match preset {
        ScaffoldPreset::RustLibrary => "ExampleLibrary",
        ScaffoldPreset::RustCli => "ExampleCli",
        _ => unreachable!("Rust-only safety helper received an application preset"),
    };
    InitOpts {
        path,
        scaffold: ScaffoldOpts {
            preset: Some(preset),
            ..ScaffoldOpts::default()
        },
        template: template.map(|path| path.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some(repo_name.into()),
            ..AnswerOpts::default()
        },
    }
}

#[cfg(unix)]
#[test]
fn rust_only_template_scaffold_collisions_fail_before_publication_and_rollback_new_roots() {
    let _guard = lock_env();
    for (template_name, expected_paths) in [
        ("Cargo.toml.jinja", ["Cargo.toml", "Cargo.toml"]),
        ("cargo.toml.jinja", ["Cargo.toml", "cargo.toml"]),
    ] {
        for preset in [ScaffoldPreset::RustLibrary, ScaffoldPreset::RustCli] {
            for force in [false, true] {
                let temp = tempdir().unwrap();
                let template = materialize_template_worktree();
                fs::write(
                    template
                        .path()
                        .join("templates/project")
                        .join(template_name),
                    "template-owned collision\n",
                )
                .unwrap();
                let created_parent = temp.path().join(format!(
                    "{}-{template_name}-{force}",
                    preset.as_str()
                ));
                let destination = created_parent.join("nested/ExampleProject");

                let error = run_init(rust_only_init_opts(
                    destination.clone(),
                    preset,
                    Some(template.path()),
                    force,
                ))
                .unwrap_err()
                .to_string();

                assert!(
                    error.contains("Portable planned repository file collision"),
                    "{} {template_name} force={force}: {error}",
                    preset.as_str()
                );
                for expected in expected_paths {
                    assert!(error.contains(expected), "missing {expected}: {error}");
                }
                assert!(!destination.exists(), "failed init left its destination");
                assert!(
                    !created_parent.exists(),
                    "failed init left transaction-owned parent directories"
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn rust_only_init_rejects_symlink_and_type_replacements_even_with_force() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    for preset in [ScaffoldPreset::RustLibrary, ScaffoldPreset::RustCli] {
        let temp = tempdir().unwrap();
        let template = materialize_template_worktree();
        let destination = temp.path().join(format!("{}-symlink", preset.as_str()));
        let outside = temp.path().join(format!("{}-outside", preset.as_str()));
        fs::create_dir(&destination).unwrap();
        fs::write(&outside, "outside sentinel\n").unwrap();
        symlink(&outside, destination.join("Cargo.toml")).unwrap();

        let error = run_init(rust_only_init_opts(
            destination.clone(),
            preset,
            Some(template.path()),
            true,
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("is a symlink"), "{error}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
        assert!(!destination.join(".jig.toml").exists());

        fs::remove_file(destination.join("Cargo.toml")).unwrap();
        fs::create_dir(destination.join("Cargo.toml")).unwrap();
        let error = run_init(rust_only_init_opts(
            destination.clone(),
            preset,
            Some(template.path()),
            true,
        ))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("Cargo.toml")
                && (error.contains("not a regular file") || error.contains("is a directory")),
            "{error}"
        );
        assert!(destination.join("Cargo.toml").is_dir());
        assert!(!destination.join(".jig.toml").exists());
    }
}

#[cfg(unix)]
#[test]
fn rust_only_public_init_rejects_package_names_beyond_the_cargo_boundary() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    for preset in [ScaffoldPreset::RustLibrary, ScaffoldPreset::RustCli] {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("ExampleProject");
        let mut opts = rust_only_init_opts(
            destination.clone(),
            preset,
            Some(template.path()),
            false,
        );
        opts.answers.repo_name = Some("r".repeat(217));

        let error = run_init(opts).unwrap_err().to_string();
        assert!(error.contains("at most 216 bytes"), "{error}");
        assert!(!destination.exists());
    }
}

#[cfg(unix)]
#[test]
fn rust_only_late_failure_removes_new_destinations_and_restores_forced_preimages() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("failing-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    printf 'fatal: injected Rust-only rollback test\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);

    for preset in [ScaffoldPreset::RustLibrary, ScaffoldPreset::RustCli] {
        let created_parent = temp.path().join(format!("{}-new", preset.as_str()));
        let destination = created_parent.join("nested/ExampleProject");
        let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
            run_init(rust_only_init_opts(destination.clone(), preset, None, false))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("injected Rust-only rollback test"), "{error}");
        assert!(!destination.exists(), "late failure left generated output");
        assert!(
            !created_parent.exists(),
            "late failure left transaction-owned parent directories"
        );

        let existing = temp.path().join(format!("{}-existing", preset.as_str()));
        let package = match preset {
            ScaffoldPreset::RustLibrary => "examplelibrary",
            ScaffoldPreset::RustCli => "examplecli",
            _ => unreachable!(),
        };
        let source = match preset {
            ScaffoldPreset::RustLibrary => format!("crates/{package}/src/lib.rs"),
            ScaffoldPreset::RustCli => format!("crates/{package}/src/main.rs"),
            _ => unreachable!(),
        };
        fs::create_dir_all(existing.join(Path::new(&source).parent().unwrap())).unwrap();
        fs::write(existing.join("Cargo.toml"), "user root manifest\n").unwrap();
        fs::write(existing.join(&source), "user source bytes\n").unwrap();
        fs::write(existing.join("sentinel.txt"), "preserve\n").unwrap();
        fs::set_permissions(
            existing.join("Cargo.toml"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        fs::set_permissions(
            existing.join(&source),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        let before = regular_file_tree_snapshot(&existing);

        let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
            run_init(rust_only_init_opts(existing.clone(), preset, None, true))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("injected Rust-only rollback test"), "{error}");
        assert_eq!(regular_file_tree_snapshot(&existing), before);
        assert_eq!(
            fs::metadata(existing.join("Cargo.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(existing.join(&source))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}

#[test]
fn rust_only_scaffold_generations_fit_exactly_at_the_transaction_budget_boundary() {
    for preset in [ScaffoldPreset::RustLibrary, ScaffoldPreset::RustCli] {
        let destination = tempdir().unwrap();
        let plan = InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(preset),
                ..ScaffoldOpts::default()
            },
            &AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                ..AnswerOpts::default()
            },
            destination.path(),
        )
        .unwrap()
        .unwrap();
        let planned = plan.output_paths().into_iter().collect::<BTreeSet<_>>();
        assert_eq!(planned.len(), 5);
        let admitted_repeats = MAX_EXISTING_INIT_RETAINED_GENERATIONS - planned.len();

        validate_retained_generation_budget(&planned, admitted_repeats, None, 0).unwrap();
        let error = validate_retained_generation_budget(
            &planned,
            admitted_repeats + 1,
            None,
            0,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(&format!(
                "plans {} generated file generations",
                MAX_EXISTING_INIT_RETAINED_GENERATIONS + 1
            )),
            "{}: {error}",
            preset.as_str()
        );
    }
}
