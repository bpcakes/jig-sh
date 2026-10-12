use super::*;

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

pub(super) fn publish_existing_transaction_file(
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
                if let Some(expected) = &nested_identity
                    && path::repository_path_identity(&root.join("scripts"))? != *expected
                {
                    bail!("nested parent changed at guarded publication boundary");
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
