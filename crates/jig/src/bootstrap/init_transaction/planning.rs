use super::*;

impl InitMutationTransaction {
    pub(in crate::bootstrap) fn plan_staged_render(
        &mut self,
        staged: &staged_render::StagedRender,
        reserved_output_paths: &[PathBuf],
    ) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        let authored_seed_paths = staged.authored_seed_paths();
        let mut planned = staged
            .active_paths
            .iter()
            .chain(staged.retirement_paths.iter())
            .chain(authored_seed_paths.iter())
            .chain(reserved_output_paths)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !reserved_output_paths.is_empty() {
            planned.insert(PathBuf::from(managed_paths::AGENT_MAP_PATH));
        }
        // A scaffold refreshes agent-map.md after crate guides exist, retaining
        // one additional Jig generation beyond its staged-render publication.
        let repeated_generation_count = usize::from(!reserved_output_paths.is_empty());
        self.ensure_planned_noreplace_filesystems(&planned)?;
        // Enforce the generation-count ceiling before retaining any snapshots.
        validate_retained_generation_budget_with_preimages(
            &planned,
            repeated_generation_count,
            0,
            None,
            0,
        )?;
        let soft_limit = process_soft_handle_limit();
        let open_handles = current_open_handle_count();
        for relative in &planned {
            // Snapshotting an existing leaf retains one handle. Check one
            // increment at a time so a large existing tree fails cleanly,
            // while missing leaves do not consume a pessimistic preimage slot.
            let current_open_handles = current_open_handle_count();
            if soft_limit.is_some_and(|limit| {
                current_open_handles
                    .saturating_add(1)
                    .saturating_add(RETAINED_GENERATION_HANDLE_HEADROOM)
                    > limit
            }) {
                bail!(
                    "Existing-destination init cannot safely retain another planning snapshot under the process soft handle limit of {}. Use a wholly missing destination, reduce the output set, or raise the process file-descriptor limit before retrying.",
                    soft_limit.expect("checked above")
                );
            }
            self.ensure_file_mutation(relative)?;
        }
        let retained_preimage_count = planned
            .iter()
            .filter_map(|relative| self.files.get(relative))
            .filter(|mutation| !matches!(mutation.before, InitPathSnapshot::Missing))
            .count();
        validate_retained_generation_budget_with_preimages(
            &planned,
            repeated_generation_count,
            retained_preimage_count,
            soft_limit,
            open_handles,
        )?;
        self.existing_generation_budget_sealed = true;
        Ok(())
    }

    fn ensure_planned_noreplace_filesystems(&self, planned: &BTreeSet<PathBuf>) -> Result<()> {
        let mut verified_filesystems = Vec::<PathBuf>::new();
        for relative in planned {
            validate_repository_relative_ancestors(&self.destination, relative)?;
            let output = self.destination.join(relative);
            let parent = output
                .parent()
                .with_context(|| format!("Init output has no parent: {}", output.display()))?;
            let (existing_ancestor, _) = path::split_existing_ancestor(parent)?;
            let mut already_verified = false;
            for verified in &verified_filesystems {
                if path::repository_paths_same_filesystem(verified, &existing_ancestor)? {
                    already_verified = true;
                    break;
                }
            }
            if already_verified {
                continue;
            }
            path::ensure_atomic_noreplace_publication_supported(&existing_ancestor).with_context(
                || {
                    format!(
                        "Init output {} is on a filesystem without safe transactional publication",
                        relative.display()
                    )
                },
            )?;
            verified_filesystems.push(existing_ancestor);
        }
        Ok(())
    }

    pub(in crate::bootstrap) fn plan_scaffold_files(
        &mut self,
        files: &[scaffold::ScaffoldFile],
    ) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        for file in files {
            if self.existing_generation_budget_sealed
                && !self.files.contains_key(Path::new(&file.relative))
            {
                bail!(
                    "Scaffold output {} was not included in the up-front existing-destination generation budget",
                    file.relative
                );
            }
            self.ensure_file_mutation(Path::new(&file.relative))?;
        }
        Ok(())
    }

    pub(in crate::bootstrap) fn plan_regular_file_bytes(
        &mut self,
        relative: &Path,
        _contents: &[u8],
    ) -> Result<()> {
        if self.existing_generation_budget_sealed && !self.files.contains_key(relative) {
            bail!(
                "Generated output {} was not included in the up-front existing-destination generation budget",
                relative.display()
            );
        }
        self.ensure_file_mutation(relative)
    }

    fn ensure_file_mutation(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        self.verify_destination_identity()?;
        if !self.files.contains_key(relative) {
            let before = self.snapshot_path(&self.destination, relative)?;
            self.files.insert(
                relative.to_path_buf(),
                InitFileMutation {
                    before,
                    expected_jig_states: Vec::new(),
                    original_quarantine: None,
                },
            );
        }
        Ok(())
    }

    fn ensure_parent_directories(&mut self, relative: &Path) -> Result<()> {
        self.verify_destination_identity()?;
        validate_repository_relative_ancestors(&self.destination, relative)?;
        let Some(parent) = relative.parent() else {
            return Ok(());
        };
        let mut current = self.destination.clone();
        for component in parent.components() {
            let std::path::Component::Normal(component) = component else {
                bail!(
                    "Init output path must contain only normal relative components: {}",
                    relative.display()
                );
            };
            current.push(component);
            self.verify_destination_identity()?;
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let identity = path::repository_directory_commit_at(&current)?;
                    if let Some(expected) = self.directory_identities.get(&current) {
                        if expected.identity != identity.identity {
                            bail!(
                                "Init output ancestor changed concurrently: {}",
                                current.display()
                            );
                        }
                    } else {
                        self.directory_identities.insert(current.clone(), identity);
                    }
                    self.verify_destination_identity()?;
                }
                Ok(_) => bail!(
                    "Init output parent is not a real directory: {}",
                    current.display()
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => {
                            let identity = path::repository_directory_commit_at(&current)?;
                            self.directory_identities
                                .insert(current.clone(), identity.clone());
                            self.owned_directories
                                .insert(current.clone(), identity.clone());
                            if let Err(primary) = self.verify_destination_identity() {
                                self.directory_identities.remove(&current);
                                self.owned_directories.remove(&current);
                                let cleanup = self.cleanup_owned_directory_after_failed_boundary(
                                    &current, identity,
                                );
                                return match cleanup {
                                    Ok(()) => Err(primary),
                                    Err(cleanup) => Err(anyhow::anyhow!(
                                        "{primary:#}\nAdditionally failed to clean the just-created init directory safely: {cleanup:#}"
                                    )),
                                };
                            }
                        }
                        Err(error) => {
                            validate_existing_init_directory_after_create_error(
                                &current, error, true,
                            )?;
                            let identity = path::repository_directory_commit_at(&current)?;
                            if let Some(expected) = self.directory_identities.get(&current) {
                                if expected.identity != identity.identity {
                                    bail!(
                                        "Init output ancestor changed concurrently: {}",
                                        current.display()
                                    );
                                }
                            } else {
                                self.directory_identities.insert(current.clone(), identity);
                            }
                            self.verify_destination_identity()?;
                        }
                    }
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to inspect init output parent {}", current.display())
                    });
                }
            }
        }
        validate_repository_relative_ancestors(&self.destination, relative)?;
        self.verify_destination_identity()
    }

    fn cleanup_owned_directory_after_failed_boundary(
        &self,
        directory: &Path,
        expected_directory: path::RepositoryDirectoryCommit,
    ) -> Result<()> {
        let relative = directory.strip_prefix(&self.destination).with_context(|| {
            format!(
                "Created init directory {} is outside destination {}",
                directory.display(),
                self.destination.display()
            )
        })?;
        let quarantine = self.unique_recovery_path(relative)?;
        match path::rename_entry_noreplace(directory, &quarantine) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to quarantine just-created directory {}",
                        directory.display()
                    )
                });
            }
        }
        let identity = path::repository_path_identity(&quarantine)?;
        let empty = fs::read_dir(&quarantine)
            .with_context(|| format!("Failed to inspect {}", quarantine.display()))?
            .next()
            .is_none();
        if identity == expected_directory.identity && empty {
            return self.dispose_empty_owned_directory(
                relative,
                &quarantine,
                directory,
                expected_directory,
            );
        }
        path::rename_entry_noreplace(&quarantine, directory).with_context(|| {
            format!(
                "Created directory changed concurrently; preserved it at {} but could not restore {}",
                quarantine.display(),
                directory.display()
            )
        })?;
        bail!(
            "Created init directory changed concurrently; preserved {}",
            directory.display()
        )
    }

    pub(in crate::bootstrap) fn prepare_file_publication(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            self.ensure_parent_directories(relative)?;
            return self.verify_destination_identity();
        }
        self.ensure_file_mutation(relative)?;
        self.ensure_parent_directories(relative)?;
        self.verify_destination_identity()?;
        self.ensure_write_staging(relative)?;
        self.verify_destination_identity()?;

        let expected = {
            let mutation = self
                .files
                .get(relative)
                .expect("init mutation was preflighted");
            mutation
                .expected_jig_states
                .last()
                .unwrap_or(&mutation.before)
                .clone()
        };
        let current = self.snapshot_destination_path(relative)?;
        if matches!(expected, InitPathSnapshot::Missing) {
            if matches!(current, InitPathSnapshot::Missing) {
                return Ok(());
            }
            bail!(
                "Init output {} appeared concurrently; refusing to replace it",
                self.destination.join(relative).display()
            );
        }
        if matches!(current, InitPathSnapshot::Missing) {
            bail!(
                "Init output {} disappeared concurrently; refusing to publish",
                self.destination.join(relative).display()
            );
        }

        let quarantine = self.unique_recovery_path(relative)?;
        path::rename_entry_noreplace(&self.destination.join(relative), &quarantine).with_context(
            || {
                format!(
                    "Failed to quarantine current init output {} before replacement",
                    self.destination.join(relative).display()
                )
            },
        )?;
        let quarantined = self.snapshot_absolute_path(&quarantine)?;
        let root_check = self.verify_destination_identity();
        let matches_expected = init_snapshots_match(&quarantined, &expected);
        if root_check.is_err() || !matches_expected {
            let restore =
                path::rename_entry_noreplace(&quarantine, &self.destination.join(relative));
            if let Err(error) = restore {
                bail!(
                    "Init output changed at the publication boundary; preserved the quarantined entry at {} but could not restore its original path: {error}",
                    quarantine.display()
                );
            }
            root_check?;
            bail!(
                "Init output {} changed concurrently; preserved it and refused to replace it",
                self.destination.join(relative).display()
            );
        }

        let retain_as_preimage = self.files.get(relative).is_some_and(|mutation| {
            mutation.original_quarantine.is_none()
                && mutation.expected_jig_states.is_empty()
                && !matches!(mutation.before, InitPathSnapshot::Missing)
        });
        if retain_as_preimage {
            self.files
                .get_mut(relative)
                .expect("init mutation was preflighted")
                .original_quarantine = Some(quarantine);
        } else {
            self.dispose_snapshot_leaf(relative, &quarantine, &quarantined)?;
        }
        Ok(())
    }

    pub(in crate::bootstrap) fn record_regular_commit(
        &mut self,
        relative: &Path,
        commit: path::RepositoryFileCommit,
    ) -> Result<()> {
        if self.is_privately_staged() {
            self.verify_destination_identity()?;
            let current = path::repository_file_fingerprint_at(&self.destination.join(relative))?;
            if !path::repository_file_commits_match(&current, &commit) {
                bail!(
                    "Private init output {} was replaced immediately after publication",
                    self.destination.join(relative).display()
                );
            }
            return Ok(());
        }
        self.verify_destination_identity()?;
        let current = path::repository_file_fingerprint_at(&self.destination.join(relative))?;
        if !path::repository_file_commits_match(&current, &commit) {
            bail!(
                "Init output {} was replaced immediately after publication",
                self.destination.join(relative).display()
            );
        }
        self.files
            .get_mut(relative)
            .with_context(|| format!("Init transaction did not preflight {}", relative.display()))?
            .expected_jig_states
            .push(InitPathSnapshot::Regular(commit));
        Ok(())
    }

    pub(in crate::bootstrap) fn record_symlink_commit(
        &mut self,
        relative: &Path,
        commit: path::RepositorySymlinkCommit,
    ) -> Result<()> {
        if self.is_privately_staged() {
            self.verify_destination_identity()?;
            let current = self.snapshot_destination_path(relative)?;
            let committed = InitPathSnapshot::Symlink {
                identity: commit.identity,
                target: commit.target,
                target_is_directory: commit.target_is_directory,
                handle: commit.handle,
            };
            if !init_snapshots_match(&current, &committed) {
                bail!(
                    "Private init symlink output {} was replaced immediately after publication",
                    self.destination.join(relative).display()
                );
            }
            return Ok(());
        }
        self.verify_destination_identity()?;
        let current = self.snapshot_destination_path(relative)?;
        let committed = InitPathSnapshot::Symlink {
            identity: commit.identity,
            target: commit.target,
            target_is_directory: commit.target_is_directory,
            handle: commit.handle,
        };
        if !init_snapshots_match(&current, &committed) {
            bail!(
                "Init symlink output {} was replaced immediately after publication",
                self.destination.join(relative).display()
            );
        }
        self.files
            .get_mut(relative)
            .with_context(|| format!("Init transaction did not preflight {}", relative.display()))?
            .expected_jig_states
            .push(committed);
        Ok(())
    }

    pub(in crate::bootstrap) fn record_missing_commit(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            self.verify_destination_identity()?;
            if !matches!(
                self.snapshot_destination_path(relative)?,
                InitPathSnapshot::Missing
            ) {
                bail!(
                    "Private init output {} reappeared immediately after removal",
                    self.destination.join(relative).display()
                );
            }
            return Ok(());
        }
        self.verify_destination_identity()?;
        if !matches!(
            self.snapshot_destination_path(relative)?,
            InitPathSnapshot::Missing
        ) {
            bail!(
                "Init output {} reappeared immediately after removal",
                self.destination.join(relative).display()
            );
        }
        self.files
            .get_mut(relative)
            .with_context(|| format!("Init transaction did not preflight {}", relative.display()))?
            .expected_jig_states
            .push(InitPathSnapshot::Missing);
        Ok(())
    }

    pub(super) fn unique_recovery_path(&self, relative: &Path) -> Result<PathBuf> {
        let path = self.destination.join(relative);
        let parent = path
            .parent()
            .with_context(|| format!("Init output has no parent: {}", path.display()))?;
        let name = path
            .file_name()
            .with_context(|| format!("Init output has no file name: {}", path.display()))?
            .to_string_lossy();
        for _ in 0..1024 {
            let index = self.next_snapshot.get();
            self.next_snapshot.set(index.saturating_add(1));
            let candidate = parent.join(format!(
                ".{name}.jig-init-recovery-{}-{index}",
                std::process::id()
            ));
            match fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
                Ok(_) => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to inspect recovery path {}", candidate.display())
                    });
                }
            }
        }
        bail!(
            "Failed to allocate a unique init recovery path beside {}",
            path.display()
        )
    }
}
