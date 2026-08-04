use super::*;

impl InitMutationTransaction {
    pub(in crate::bootstrap) fn dispose_snapshot_leaf(
        &self,
        relative: &Path,
        inspected_path: &Path,
        expected: &InitPathSnapshot,
    ) -> Result<()> {
        let disposal = self.unique_recovery_path(relative)?;
        path::rename_entry_noreplace(inspected_path, &disposal).with_context(|| {
            format!(
                "Failed to move inspected init quarantine {} into a second no-replace disposal quarantine {}",
                inspected_path.display(),
                disposal.display()
            )
        })?;

        let actual = match self.snapshot_absolute_path(&disposal) {
            Ok(actual) => actual,
            Err(error) => {
                bail!(
                    "Could not verify second disposal quarantine {}; preserving it instead of unlinking: {error:#}",
                    disposal.display()
                );
            }
        };
        if !init_snapshots_match(&actual, expected) {
            return Err(restore_changed_disposal_quarantine(
                &disposal,
                inspected_path,
                anyhow::anyhow!(
                    "Inspected init quarantine changed before disposal; refusing to unlink replacement {}",
                    disposal.display()
                ),
            ));
        }

        remove_snapshot_leaf_unchecked(&disposal, &actual).with_context(|| {
            format!(
                "Failed to remove exact second disposal quarantine {}; it remains available for recovery",
                disposal.display()
            )
        })
    }

    pub(in crate::bootstrap) fn dispose_empty_owned_directory(
        &self,
        relative: &Path,
        inspected_path: &Path,
        restore_path: &Path,
        expected_directory: path::RepositoryDirectoryCommit,
    ) -> Result<()> {
        let expected_identity = expected_directory.identity.clone();
        let disposal = self.unique_recovery_path(relative)?;
        path::rename_entry_noreplace(inspected_path, &disposal).with_context(|| {
            format!(
                "Failed to move inspected owned directory {} into a second no-replace disposal quarantine {}",
                inspected_path.display(),
                disposal.display()
            )
        })?;

        let exact_empty_directory = (|| -> Result<bool> {
            let metadata = fs::symlink_metadata(&disposal).with_context(|| {
                format!(
                    "Failed to inspect disposal quarantine {}",
                    disposal.display()
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Ok(false);
            }
            let identity = path::repository_path_identity(&disposal)?;
            let empty = fs::read_dir(&disposal)
                .with_context(|| {
                    format!("Failed to read disposal quarantine {}", disposal.display())
                })?
                .next()
                .is_none();
            Ok(identity == expected_identity && empty)
        })();
        match exact_empty_directory {
            Ok(true) => {
                drop(expected_directory);
                fs::remove_dir(&disposal).with_context(|| {
                format!(
                    "Failed to remove exact empty directory disposal quarantine {}; it remains available for recovery",
                    disposal.display()
                )
                })
            }
            Ok(false) => Err(restore_changed_disposal_quarantine(
                &disposal,
                restore_path,
                anyhow::anyhow!(
                    "Owned init directory changed before disposal; refusing to remove replacement {}",
                    disposal.display()
                ),
            )),
            Err(error) => Err(anyhow::anyhow!(
                "Could not verify owned-directory disposal quarantine {}; preserving it instead of removing: {error:#}",
                disposal.display()
            )),
        }
    }

    pub(in crate::bootstrap) fn rollback(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        self.armed = false;
        self.rollback_armed()
    }

    fn rollback_armed(&mut self) -> Result<()> {
        let staged_boundary = self
            .staged_publication
            .as_ref()
            .map(|_| verify_tracked_init_directories(&self.directory_identities));
        if let Some(publication) = self.staged_publication.as_mut() {
            if let Some(staging_root) = publication.staging_root.take() {
                if let Some(Err(error)) = staged_boundary {
                    let preserved = staging_root.keep();
                    bail!(
                        "Private init work tree changed before cleanup: {error:#}. Preserving the complete staging tree at {}",
                        preserved.display()
                    );
                }
                return cleanup_private_staging(staging_root, &publication.publish_source_identity)
                    .with_context(|| {
                        format!(
                            "Failed to remove private failed-init staging tree beside {}",
                            self.final_destination.display()
                        )
                    });
            }
            return Ok(());
        }

        let mut failures = Vec::new();
        if let Err(error) = self.verify_rollback_root_and_preexisting_ancestors() {
            failures.push(format!(
                "{error}; refusing to touch replacement root. Any retained preimages remain at their .jig-init-recovery paths"
            ));
            if let Err(cleanup) = self.close_write_staging() {
                failures.push(format!("private write staging cleanup failed: {cleanup:#}"));
            }
            return Err(anyhow::anyhow!(failures.join("\n")));
        }

        let mutations = self
            .files
            .iter()
            .rev()
            .map(|(relative, mutation)| (relative.clone(), mutation.clone()))
            .collect::<Vec<_>>();
        for (relative, mutation) in mutations {
            match self.changed_owned_ancestor(&relative) {
                Ok(Some(directory)) => {
                    failures.push(format!(
                        "{}: owned ancestor {} changed; preserving its subtree for directory-level recovery",
                        relative.display(),
                        directory.display()
                    ));
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("{}: {error:#}", relative.display()));
                    continue;
                }
            }
            let current = match self.snapshot_destination_path(&relative) {
                Ok(current) => current,
                Err(error) => {
                    failures.push(format!("{}: {error:#}", relative.display()));
                    continue;
                }
            };
            let matches_before = init_snapshots_match(&current, &mutation.before);
            if matches_before && mutation.original_quarantine.is_none() {
                continue;
            }

            let current_quarantine = if matches!(current, InitPathSnapshot::Missing) {
                None
            } else {
                let quarantine = match self.unique_recovery_path(&relative) {
                    Ok(path) => path,
                    Err(error) => {
                        failures.push(format!("{}: {error:#}", relative.display()));
                        continue;
                    }
                };
                if let Err(error) =
                    path::rename_entry_noreplace(&self.destination.join(&relative), &quarantine)
                {
                    failures.push(format!(
                        "{}: failed to quarantine current rollback leaf: {error}",
                        relative.display()
                    ));
                    continue;
                }
                let quarantined = match self.snapshot_absolute_path(&quarantine) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        failures.push(format!(
                            "{}: could not inspect rollback quarantine {}: {error:#}",
                            relative.display(),
                            quarantine.display()
                        ));
                        continue;
                    }
                };
                let is_jig = any_init_snapshot_matches(&quarantined, &mutation.expected_jig_states);
                if !is_jig {
                    let retained_preimage = mutation
                        .original_quarantine
                        .as_ref()
                        .map(|path| format!("; original preimage remains at {}", path.display()))
                        .unwrap_or_default();
                    match path::rename_entry_noreplace(
                        &quarantine,
                        &self.destination.join(&relative),
                    ) {
                        Ok(()) => failures.push(format!(
                            "{} changed after Jig wrote it; preserved the current path{}",
                            relative.display(),
                            retained_preimage
                        )),
                        Err(error) => failures.push(format!(
                            "{} changed after Jig wrote it; preserved it at recovery path {} because its original path became occupied: {error}{}",
                            relative.display(),
                            quarantine.display(),
                            retained_preimage
                        )),
                    }
                    continue;
                }
                Some((quarantine, quarantined))
            };

            let restore_result = match (&mutation.before, &mutation.original_quarantine) {
                (InitPathSnapshot::Missing, _) => Ok(()),
                (_, Some(preimage)) => path::rename_entry_noreplace(
                    preimage,
                    &self.destination.join(&relative),
                )
                .with_context(|| {
                    format!(
                        "Failed to restore retained preimage from {}; it remains available for recovery",
                        preimage.display()
                    )
                }),
                (_, None) if matches_before => Ok(()),
                (_, None) => Err(anyhow::anyhow!(
                    "Original preimage for {} is unavailable; preserving recovery artifacts",
                    relative.display()
                )),
            };
            if let Err(error) = restore_result {
                failures.push(format!("{}: {error:#}", relative.display()));
                continue;
            }
            if let Some((quarantine, snapshot)) = current_quarantine {
                if let Err(error) = self.dispose_snapshot_leaf(&relative, &quarantine, &snapshot) {
                    failures.push(format!(
                        "{}: restored the preimage but failed to remove quarantined Jig output {}: {error:#}",
                        relative.display(),
                        quarantine.display()
                    ));
                }
            }
        }

        if let Err(error) = self.close_write_staging() {
            failures.push(format!("private write staging cleanup failed: {error:#}"));
        }

        let mut directories = self.owned_directories.keys().cloned().collect::<Vec<_>>();
        directories.sort_by(|left, right| {
            right
                .components()
                .count()
                .cmp(&left.components().count())
                .then_with(|| right.cmp(left))
        });
        for directory in directories {
            let expected_directory = self
                .owned_directories
                .remove(&directory)
                .expect("owned directory disappeared from transaction state");
            self.directory_identities.remove(&directory);
            match fs::symlink_metadata(&directory) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    failures.push(format!(
                        "{}: failed to inspect owned init directory: {error}",
                        directory.display()
                    ));
                    continue;
                }
            }
            let relative = match directory.strip_prefix(&self.destination) {
                Ok(relative) => relative,
                Err(error) => {
                    failures.push(format!("{}: {error}", directory.display()));
                    continue;
                }
            };
            let quarantine = match self.unique_recovery_path(relative) {
                Ok(path) => path,
                Err(error) => {
                    failures.push(format!("{}: {error:#}", directory.display()));
                    continue;
                }
            };
            if let Err(error) = path::rename_entry_noreplace(&directory, &quarantine) {
                failures.push(format!(
                    "{}: preserving changed init directory ({error})",
                    directory.display()
                ));
                continue;
            }
            let identity = path::repository_path_identity(&quarantine);
            let empty = fs::read_dir(&quarantine)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if identity
                .as_ref()
                .is_ok_and(|identity| identity == &expected_directory.identity)
                && empty
            {
                if let Err(error) = self.dispose_empty_owned_directory(
                    relative,
                    &quarantine,
                    &directory,
                    expected_directory,
                ) {
                    failures.push(format!(
                        "{}: failed to dispose exact empty owned directory quarantine {} safely ({error:#})",
                        directory.display(),
                        quarantine.display()
                    ));
                }
            } else {
                match path::rename_entry_noreplace(&quarantine, &directory) {
                    Ok(()) => failures.push(format!(
                        "{}: preserving non-empty or changed init directory",
                        directory.display()
                    )),
                    Err(error) => failures.push(format!(
                        "{}: preserving non-empty or changed init directory at recovery path {} ({error})",
                        directory.display(),
                        quarantine.display()
                    )),
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }

    pub(super) fn snapshot_path(&self, root: &Path, relative: &Path) -> Result<InitPathSnapshot> {
        use path::RepositoryFileLeaf;

        match path::validate_repository_relative_file_leaf(root, relative)? {
            RepositoryFileLeaf::Missing => Ok(InitPathSnapshot::Missing),
            RepositoryFileLeaf::RegularFile => Ok(InitPathSnapshot::Regular(
                path::repository_file_fingerprint_at(&root.join(relative))?,
            )),
            RepositoryFileLeaf::Symlink => {
                let commit = path::repository_symlink_commit_at(&root.join(relative))?;
                Ok(InitPathSnapshot::Symlink {
                    identity: commit.identity,
                    target: commit.target,
                    target_is_directory: commit.target_is_directory,
                    handle: commit.handle,
                })
            }
        }
    }

    pub(super) fn snapshot_destination_path(&self, relative: &Path) -> Result<InitPathSnapshot> {
        self.snapshot_path(&self.destination, relative)
    }

    pub(in crate::bootstrap) fn snapshot_absolute_path(
        &self,
        absolute: &Path,
    ) -> Result<InitPathSnapshot> {
        let metadata = fs::symlink_metadata(absolute)
            .with_context(|| format!("Failed to inspect {}", absolute.display()))?;
        if metadata.file_type().is_symlink() {
            let commit = path::repository_symlink_commit_at(absolute)?;
            return Ok(InitPathSnapshot::Symlink {
                identity: commit.identity,
                target: commit.target,
                target_is_directory: commit.target_is_directory,
                handle: commit.handle,
            });
        }
        if metadata.is_file() {
            return Ok(InitPathSnapshot::Regular(
                path::repository_file_fingerprint_at(absolute)?,
            ));
        }
        bail!(
            "Rollback leaf is not a file or symlink: {}",
            absolute.display()
        )
    }

    pub(in crate::bootstrap) fn finish_failed_init(
        &mut self,
        primary: anyhow::Error,
    ) -> anyhow::Error {
        match self.rollback() {
            Ok(()) => primary,
            Err(rollback) => anyhow::anyhow!(
                "{primary:#}\nAdditionally, failed to roll back init changes:\n{rollback:#}"
            ),
        }
    }
}

pub(super) fn close_failed_staging(
    staging_root: TempDir,
    expected_identity: &path::RepositoryDirectoryCommit,
    primary: anyhow::Error,
) -> anyhow::Error {
    match cleanup_private_staging(staging_root, expected_identity) {
        Ok(()) => primary,
        Err(cleanup) => anyhow::anyhow!(
            "{primary:#}\nAdditionally, staging cleanup was incomplete:\n{cleanup:#}"
        ),
    }
}

fn cleanup_private_staging(
    staging_root: TempDir,
    expected_identity: &path::RepositoryDirectoryCommit,
) -> Result<()> {
    let staging_path = staging_root.path().to_path_buf();
    match path::repository_directory_commit_matches_path(expected_identity, &staging_path) {
        Ok(false) => {
            let preserved = staging_root.keep();
            bail!(
                "Private staging path was replaced concurrently; preserving foreign replacement {}",
                preserved.display()
            );
        }
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(());
        }
        Err(error) => {
            let preserved = staging_root.keep();
            bail!(
                "Could not verify private staging path {} for cleanup ({error:#}); preserving it",
                preserved.display()
            );
        }
        Ok(true) => {}
    }

    let mut cleanup_failures = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Err(error) = fs::set_permissions(&staging_path, fs::Permissions::from_mode(0o700)) {
            cleanup_failures.push(format!(
                "failed to restore private staging permissions on {}: {error}",
                staging_path.display()
            ));
        }
    }
    if let Err(error) = staging_root.close() {
        cleanup_failures.push(format!(
            "failed to remove private staging tree {}: {error}",
            staging_path.display()
        ));
    }
    if cleanup_failures.is_empty() {
        Ok(())
    } else {
        bail!("{}", cleanup_failures.join("\n"))
    }
}

fn any_init_snapshot_matches(state: &InitPathSnapshot, candidates: &[InitPathSnapshot]) -> bool {
    for candidate in candidates {
        if init_snapshots_match(state, candidate) {
            return true;
        }
    }
    false
}

pub(super) fn init_snapshots_match(left: &InitPathSnapshot, right: &InitPathSnapshot) -> bool {
    match (left, right) {
        (InitPathSnapshot::Missing, InitPathSnapshot::Missing) => true,
        (InitPathSnapshot::Regular(left), InitPathSnapshot::Regular(right)) => {
            path::repository_file_commits_match(left, right)
        }
        (
            InitPathSnapshot::Symlink {
                identity: left_identity,
                target: left_target,
                target_is_directory: left_is_directory,
                ..
            },
            InitPathSnapshot::Symlink {
                identity: right_identity,
                target: right_target,
                target_is_directory: right_is_directory,
                ..
            },
        ) => {
            left_identity == right_identity
                && left_target == right_target
                && left_is_directory == right_is_directory
        }
        _ => false,
    }
}

fn restore_changed_disposal_quarantine(
    disposal: &Path,
    inspected_path: &Path,
    primary: anyhow::Error,
) -> anyhow::Error {
    match path::rename_entry_noreplace(disposal, inspected_path) {
        Ok(()) => anyhow::anyhow!(
            "{primary:#}\nRestored the changed entry to {}",
            inspected_path.display()
        ),
        Err(error) => anyhow::anyhow!(
            "{primary:#}\nPreserved the changed entry at {} because {} became occupied: {error}",
            disposal.display(),
            inspected_path.display()
        ),
    }
}

fn remove_snapshot_leaf_unchecked(path: &Path, snapshot: &InitPathSnapshot) -> Result<()> {
    match snapshot {
        InitPathSnapshot::Missing => Ok(()),
        InitPathSnapshot::Regular(_) | InitPathSnapshot::Symlink { .. } => fs::remove_file(path)
            .with_context(|| format!("Failed to remove quarantined init leaf {}", path.display())),
    }
}

pub(in crate::bootstrap) fn validate_existing_init_directory_after_create_error(
    directory: &Path,
    create_error: io::Error,
    require_real_directory: bool,
) -> Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() && !require_real_directory => {
            match fs::metadata(directory) {
                Ok(target_metadata) if target_metadata.is_dir() => Ok(()),
                Ok(_) => bail!(
                    "Init destination path component does not resolve to a directory: {}",
                    directory.display()
                ),
                Err(inspect_error) => Err(inspect_error).with_context(|| {
                    format!(
                        "Failed to inspect init destination path component {}",
                        directory.display()
                    )
                }),
            }
        }
        Ok(_) => bail!(
            "Init destination path component is not a real directory: {}",
            directory.display()
        ),
        Err(inspect_error) if create_error.kind() == io::ErrorKind::AlreadyExists => {
            Err(inspect_error).with_context(|| {
                format!(
                    "Failed to inspect existing init destination directory {}",
                    directory.display()
                )
            })
        }
        Err(_) => Err(create_error).with_context(|| {
            format!(
                "Failed to create init destination directory {}",
                directory.display()
            )
        }),
    }
}

impl Drop for InitMutationTransaction {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            let _ = self.rollback_armed();
        }
    }
}
