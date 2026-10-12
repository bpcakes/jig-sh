use super::*;

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

#[test]
fn rollback_preserves_same_inode_foreign_rewrite_and_recreated_owned_directory() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();

    let mut file_transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(&mut file_transaction, Path::new("managed"), b"jig-state\n");
    fs::write(root.join("managed"), b"foreign!!\n").unwrap();
    let error = file_transaction.rollback().unwrap_err().to_string();
    assert!(error.contains("changed after Jig wrote it"), "{error}");
    assert_eq!(fs::read(root.join("managed")).unwrap(), b"foreign!!\n");

    let mut directory_transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(
        &mut directory_transaction,
        Path::new("owned/generated"),
        b"jig\n",
    );
    fs::remove_file(root.join("owned/generated")).unwrap();
    fs::remove_dir(root.join("owned")).unwrap();
    fs::create_dir(root.join("owned")).unwrap();
    fs::write(root.join("owned/foreign"), "preserve\n").unwrap();
    let error = directory_transaction.rollback().unwrap_err().to_string();
    assert!(error.contains("owned ancestor"), "{error}");
    assert_eq!(
        fs::read_to_string(root.join("owned/foreign")).unwrap(),
        "preserve\n"
    );
}

#[cfg(unix)]
#[test]
fn late_init_failure_removes_managed_scaffold_agent_map_and_partial_git_outputs() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("failing-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    mkdir -p .git/objects/aa\n    printf 'ref: refs/heads/main\\n' > .git/HEAD\n    printf 'partial\\n' > .git/objects/aa/object\n    printf 'fatal: injected late failure\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(jig_git::GIT_BIN_ENV, &git);

    let created_parent = temp.path().join("created-parent");
    let destination = created_parent.join("nested/repo");
    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("git init -b main failed"), "{error}");
    assert!(error.contains("injected late failure"), "{error}");
    assert!(
        !destination.exists(),
        "late failure left generated repo output"
    );
    assert!(
        !created_parent.exists(),
        "late failure left transaction-owned parent directories"
    );

    let existing = temp.path().join("existing-empty");
    fs::create_dir(&existing).unwrap();
    with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(existing.clone(), false))
    })
    .unwrap_err();
    assert!(existing.is_dir());
    assert!(fs::read_dir(&existing).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn late_forced_init_failure_restores_user_files_bytes_and_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("failing-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    printf 'fatal: injected rollback test\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(jig_git::GIT_BIN_ENV, &git);
    let destination = temp.path().join("existing");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join(".gitignore"), b"user bytes\n").unwrap();
    fs::set_permissions(
        destination.join(".gitignore"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::write(destination.join("sentinel.txt"), "keep me\n").unwrap();
    let before = regular_file_tree_snapshot(&destination);

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), true))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("injected rollback test"), "{error}");
    assert_eq!(regular_file_tree_snapshot(&destination), before);
    assert_eq!(
        fs::metadata(destination.join(".gitignore"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn late_init_rollback_preserves_foreign_file_changes_and_surfaces_both_failures() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let git = temp.path().join("mutating-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    printf 'foreign concurrent contents\\n' > \"$JIG_TEST_FOREIGN_DESTINATION/.jig.toml\"\n    printf 'fatal: injected primary failure\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(jig_git::GIT_BIN_ENV, &git);
    let destination = temp.path().join("existing");
    fs::create_dir(&destination).unwrap();
    let _destination = EnvVarGuard::set("JIG_TEST_FOREIGN_DESTINATION", destination.as_os_str());

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("injected primary failure"), "{error}");
    assert!(
        error.contains("failed to roll back init changes"),
        "{error}"
    );
    assert!(
        error.contains(".jig.toml changed after Jig wrote it"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(destination.join(".jig.toml")).unwrap(),
        "foreign concurrent contents\n"
    );
    let remaining = regular_file_tree_snapshot(&destination);
    assert_eq!(remaining.len(), 1, "{remaining:?}");
    assert!(remaining.contains_key(Path::new(".jig.toml")));
}

#[cfg(unix)]
#[test]
fn failed_staged_git_init_never_claims_concurrent_destination_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let destination = temp.path().join("existing");
    fs::create_dir(&destination).unwrap();
    let git = temp.path().join("concurrent-git");
    write_executable_test_script(
        &git,
        "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"init\" ]; then\n    mkdir -p \"$JIG_TEST_CONCURRENT_GIT_DESTINATION/.git\"\n    printf 'foreign git metadata\\n' > \"$JIG_TEST_CONCURRENT_GIT_DESTINATION/.git/foreign\"\n    mkdir -p .git/objects\n    printf 'partial staged metadata\\n' > .git/HEAD\n    printf 'fatal: staged git failure\\n' >&2\n    exit 1\n  fi\ndone\nexec git \"$@\"\n",
    );
    let _git = EnvVarGuard::set(jig_git::GIT_BIN_ENV, &git);
    let _destination = EnvVarGuard::set(
        "JIG_TEST_CONCURRENT_GIT_DESTINATION",
        destination.as_os_str(),
    );

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("staged git failure"), "{error}");
    assert!(
        !error.contains("failed to roll back init changes"),
        "foreign .git must not be transaction-owned: {error}"
    );
    assert_eq!(
        fs::read_to_string(destination.join(".git/foreign")).unwrap(),
        "foreign git metadata\n"
    );
    assert!(!destination.join(".jig.toml").exists());
    assert!(!destination.join("Cargo.toml").exists());
    assert!(fs::read_dir(&destination).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".jig-git-init-")
    }));
}
