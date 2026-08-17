
#[test]
fn second_disposal_quarantine_preserves_post_inspection_replacements() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();
    let transaction = InitMutationTransaction::create(&root).unwrap();

    let inspected_file = root.join("inspected-file");
    let retained_file = root.join("retained-file");
    fs::write(&inspected_file, "jig\n").unwrap();
    let expected_file = transaction.snapshot_absolute_path(&inspected_file).unwrap();
    fs::rename(&inspected_file, &retained_file).unwrap();
    fs::write(&inspected_file, "foreign\n").unwrap();
    let error = transaction
        .dispose_snapshot_leaf(Path::new("managed"), &inspected_file, &expected_file)
        .unwrap_err()
        .to_string();
    assert!(error.contains("refusing to unlink replacement"), "{error}");
    assert_eq!(fs::read_to_string(&inspected_file).unwrap(), "foreign\n");
    assert_eq!(fs::read_to_string(&retained_file).unwrap(), "jig\n");

    let inspected_directory = root.join("inspected-directory");
    let retained_directory = root.join("retained-directory");
    fs::create_dir(&inspected_directory).unwrap();
    let expected_directory = path::repository_directory_commit_at(&inspected_directory).unwrap();
    fs::rename(&inspected_directory, &retained_directory).unwrap();
    fs::create_dir(&inspected_directory).unwrap();
    fs::write(inspected_directory.join("foreign"), "preserve\n").unwrap();
    let error = transaction
        .dispose_empty_owned_directory(
            Path::new("owned"),
            &inspected_directory,
            &inspected_directory,
            expected_directory,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("refusing to remove replacement"), "{error}");
    assert_eq!(
        fs::read_to_string(inspected_directory.join("foreign")).unwrap(),
        "preserve\n"
    );
    assert!(retained_directory.is_dir());
}

#[test]
fn retained_generation_budget_fails_before_a_low_soft_handle_limit() {
    let planned = (0..12)
        .map(|index| PathBuf::from(format!("nested/{index}/generated")))
        .collect::<BTreeSet<_>>();
    let repeated_generation_count = 2;
    let required = retained_generation_handle_requirement(&planned, repeated_generation_count);

    let error = validate_retained_generation_budget(
        &planned,
        repeated_generation_count,
        Some(required + 9),
        10,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("soft handle limit"), "{error}");
    validate_retained_generation_budget(
        &planned,
        repeated_generation_count,
        Some(required + 10),
        10,
    )
    .unwrap();
}

#[test]
fn retained_generation_budget_only_reserves_preimages_that_were_snapshotted() {
    let planned = (0..12)
        .map(|index| PathBuf::from(format!("nested/{index}/generated")))
        .collect::<BTreeSet<_>>();
    let repeated_generation_count = 2;
    let pessimistic = retained_generation_handle_requirement(&planned, repeated_generation_count);
    let all_missing = retained_generation_handle_requirement_with_preimages(
        &planned,
        repeated_generation_count,
        0,
    );

    assert_eq!(pessimistic - all_missing, planned.len());
    validate_retained_generation_budget_with_preimages(
        &planned,
        repeated_generation_count,
        0,
        Some(all_missing + 10),
        10,
    )
    .unwrap();
}

#[test]
fn retained_generation_budget_caps_planned_and_repeated_generations_together() {
    let planned = (0..MAX_EXISTING_INIT_RETAINED_GENERATIONS)
        .map(|index| PathBuf::from(format!("generated-{index}")))
        .collect::<BTreeSet<_>>();

    validate_retained_generation_budget(&planned, 0, None, 0).unwrap();
    let error = validate_retained_generation_budget(&planned, 1, None, 0)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!(
            "plans {} generated file generations",
            MAX_EXISTING_INIT_RETAINED_GENERATIONS + 1
        )),
        "{error}"
    );
}

#[test]
fn retained_generation_model_matches_preimages_first_outputs_and_explicit_repeats() {
    fn snapshot_handle_count(snapshot: &InitPathSnapshot) -> usize {
        usize::from(!matches!(snapshot, InitPathSnapshot::Missing))
    }

    let temp = tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("nested")).unwrap();
    let first = Path::new("nested/first");
    let second = Path::new("nested/second");
    fs::write(root.join(first), "first preimage\n").unwrap();
    fs::write(root.join(second), "second preimage\n").unwrap();

    let mut transaction = InitMutationTransaction::create(&root).unwrap();
    publish_existing_transaction_file(&mut transaction, first, b"first Jig generation\n");
    publish_existing_transaction_file(&mut transaction, second, b"second Jig generation\n");
    publish_existing_transaction_file(&mut transaction, first, b"repeated Jig generation\n");

    let retained_file_generations = transaction
        .files
        .values()
        .map(|mutation| {
            snapshot_handle_count(&mutation.before)
                + mutation
                    .expected_jig_states
                    .iter()
                    .map(snapshot_handle_count)
                    .sum::<usize>()
        })
        .sum::<usize>();
    assert_eq!(retained_file_generations, 2 * 2 + 1);

    let planned = BTreeSet::from([first.to_path_buf(), second.to_path_buf()]);
    assert_eq!(
        retained_generation_handle_requirement(&planned, 1),
        retained_file_generations
            + 1 // one retained directory prefix: nested
            + 1 // one private write-staging directory for that parent
            + RETAINED_GENERATION_HANDLE_HEADROOM
    );

    transaction.rollback().unwrap();
    assert_eq!(
        fs::read_to_string(root.join(first)).unwrap(),
        "first preimage\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(second)).unwrap(),
        "second preimage\n"
    );
}

#[cfg(unix)]
const EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV: &str =
    "JIG_TEST_EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER";
#[cfg(unix)]
const EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_TEST: &str = "bootstrap::tests::basic::init_safety::existing_empty_default_init_succeeds_with_256_soft_handle_limit_helper";

#[cfg(unix)]
#[test]
fn existing_empty_default_init_succeeds_with_256_soft_handle_limit() {
    let _guard = lock_env();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_TEST,
            "--nocapture",
        ])
        .env(EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV, "1")
        .env_remove(GIT_BIN_ENV)
        .env_remove(path::INVOCATION_CWD_ENV)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "soft-limit init helper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn existing_empty_default_init_succeeds_with_256_soft_handle_limit_helper() {
    if std::env::var_os(EXISTING_INIT_SOFT_HANDLE_LIMIT_HELPER_ENV).is_none() {
        return;
    }

    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: the isolated helper owns this process and `limit` is writable.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0,
        "failed to read helper descriptor limit: {}",
        std::io::Error::last_os_error()
    );
    let requested: libc::rlim_t = 256;
    assert!(
        limit.rlim_max == libc::RLIM_INFINITY || limit.rlim_max >= requested,
        "helper hard descriptor limit {} is below {requested}",
        limit.rlim_max
    );
    limit.rlim_cur = requested;
    // SAFETY: this limit change is confined to the isolated helper subprocess.
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) },
        0,
        "failed to set helper descriptor limit: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(process_soft_handle_limit(), Some(256));

    let temp = tempdir().unwrap();
    let destination = temp.path().join("existing-empty");
    fs::create_dir(&destination).unwrap();
    let report = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_init(rollback_test_init_opts(destination.clone(), false))
    })
    .unwrap();

    assert_eq!(report["scaffold"]["preset"], "rust-react");
    assert!(destination.join(".jig.toml").is_file());
    assert!(
        destination
            .join("apps/rollback-demo-api/Cargo.toml")
            .is_file()
    );
    assert!(destination.join("web/e2e/app.spec.ts").is_file());
}

fn publish_existing_transaction_file(
    transaction: &mut InitMutationTransaction,
    relative: &Path,
    contents: &[u8],
) {
    transaction
        .plan_regular_file_bytes(relative, contents)
        .unwrap();
    transaction.prepare_file_publication(relative).unwrap();
    let permissions = transaction.publication_permissions(relative).unwrap();
    let staging = transaction
        .write_staging_path(relative)
        .unwrap()
        .to_path_buf();
    let commit = path::write_repository_file_atomic_guarded(
        transaction.work_destination(),
        relative,
        contents,
        permissions,
        &staging,
        || transaction.verify_destination_identity(),
    )
    .unwrap();
    transaction.record_regular_commit(relative, commit).unwrap();
}

#[cfg(unix)]
#[test]
fn guarded_publication_rejects_root_and_nested_parent_swaps_without_touching_foreign_trees() {
    for swap_nested_parent in [false, true] {
        let temp = tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        let relative = if swap_nested_parent {
            fs::create_dir(root.join("scripts")).unwrap();
            Path::new("scripts/generated")
        } else {
            Path::new("generated")
        };
        let mut transaction = InitMutationTransaction::create(&root).unwrap();
        transaction
            .plan_regular_file_bytes(relative, b"jig\n")
            .unwrap();
        transaction.prepare_file_publication(relative).unwrap();
        let staging = transaction
            .write_staging_path(relative)
            .unwrap()
            .to_path_buf();
        let root_identity = path::repository_path_identity(&root).unwrap();
        let nested_identity = swap_nested_parent
            .then(|| path::repository_path_identity(&root.join("scripts")).unwrap());
        let moved = temp.path().join(if swap_nested_parent {
            "moved-scripts"
        } else {
            "moved-repo"
        });
        let mut checks = 0;
        let error = path::write_repository_file_atomic_guarded(
            &root,
            relative,
            b"jig\n",
            None,
            &staging,
            || {
                checks += 1;
                if checks == 2 {
                    if swap_nested_parent {
                        fs::rename(root.join("scripts"), &moved)?;
                        fs::create_dir(root.join("scripts"))?;
                        fs::write(root.join("scripts/foreign"), "preserve\n")?;
                    } else {
                        fs::rename(&root, &moved)?;
                        fs::create_dir(&root)?;
                        fs::write(root.join("foreign"), "preserve\n")?;
                    }
                }
                if path::repository_path_identity(&root)? != root_identity {
                    bail!("root changed at guarded publication boundary");
                }
                if let Some(expected) = &nested_identity {
                    if path::repository_path_identity(&root.join("scripts"))? != *expected {
                        bail!("nested parent changed at guarded publication boundary");
                    }
                }
                Ok(())
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("changed at guarded publication boundary"),
            "{error}"
        );
        let foreign_root = if swap_nested_parent {
            root.join("scripts")
        } else {
            root.clone()
        };
        assert_eq!(
            fs::read_to_string(foreign_root.join("foreign")).unwrap(),
            "preserve\n"
        );
        assert!(!foreign_root.join("generated").exists());
        assert!(!moved.join("generated").exists());
        let _ = transaction.rollback();
    }
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
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);

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
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);
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
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);
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
    let _git = EnvVarGuard::set(GIT_BIN_ENV, &git);
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
