use std::cell::Cell;

use super::*;

mod commit;
mod planning;
mod rollback;

pub(super) use rollback::validate_existing_init_directory_after_create_error;
use rollback::{close_failed_staging, init_snapshots_match};

#[derive(Clone, Debug)]
pub(super) enum InitPathSnapshot {
    Missing,
    Regular(path::RepositoryFileCommit),
    Symlink {
        identity: path::RepositoryEntryIdentity,
        target: PathBuf,
        target_is_directory: bool,
        // The open handle intentionally pins the symlink identity for the snapshot lifetime.
        #[allow(dead_code)]
        handle: Arc<fs::File>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct InitFileMutation {
    pub(super) before: InitPathSnapshot,
    pub(super) expected_jig_states: Vec<InitPathSnapshot>,
    original_quarantine: Option<PathBuf>,
}

pub(super) struct InitMutationTransaction {
    final_destination: PathBuf,
    destination: PathBuf,
    destination_identity: path::RepositoryDirectoryCommit,
    pub(super) staged_publication: Option<StagedInitPublication>,
    write_staging: BTreeMap<PathBuf, InitWriteStagingDirectory>,
    next_snapshot: Cell<u64>,
    pub(super) files: BTreeMap<PathBuf, InitFileMutation>,
    directory_identities: BTreeMap<PathBuf, path::RepositoryDirectoryCommit>,
    owned_directories: BTreeMap<PathBuf, path::RepositoryDirectoryCommit>,
    existing_generation_budget_sealed: bool,
    armed: bool,
}

pub(super) const MAX_EXISTING_INIT_RETAINED_GENERATIONS: usize = 256;
pub(super) const RETAINED_GENERATION_HANDLE_HEADROOM: usize = 32;

#[cfg(test)]
pub(super) fn retained_generation_handle_requirement(
    planned: &BTreeSet<PathBuf>,
    repeated_generation_count: usize,
) -> usize {
    retained_generation_handle_requirement_with_preimages(
        planned,
        repeated_generation_count,
        planned.len(),
    )
}

pub(super) fn retained_generation_handle_requirement_with_preimages(
    planned: &BTreeSet<PathBuf>,
    repeated_generation_count: usize,
    retained_preimage_count: usize,
) -> usize {
    debug_assert!(retained_preimage_count <= planned.len());
    let mut directory_prefixes = BTreeSet::new();
    let mut target_parents = BTreeSet::new();
    for relative in planned {
        let parent = relative.parent().unwrap_or(Path::new(""));
        target_parents.insert(parent.to_path_buf());
        let mut prefix = PathBuf::new();
        for component in parent.components() {
            prefix.push(component.as_os_str());
            directory_prefixes.insert(prefix.clone());
        }
    }
    planned
        .len()
        .saturating_add(retained_preimage_count)
        .saturating_add(repeated_generation_count)
        .saturating_add(directory_prefixes.len())
        .saturating_add(target_parents.len())
        .saturating_add(RETAINED_GENERATION_HANDLE_HEADROOM)
}

#[cfg(unix)]
pub(super) fn process_soft_handle_limit() -> Option<usize> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is valid writable storage for `getrlimit`.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0
        || limit.rlim_cur == libc::RLIM_INFINITY
    {
        return None;
    }
    usize::try_from(limit.rlim_cur).ok()
}

#[cfg(not(unix))]
pub(super) fn process_soft_handle_limit() -> Option<usize> {
    None
}

#[cfg(unix)]
fn current_open_handle_count() -> usize {
    [Path::new("/proc/self/fd"), Path::new("/dev/fd")]
        .into_iter()
        .find_map(|directory| fs::read_dir(directory).ok())
        .map(|entries| entries.filter_map(std::result::Result::ok).count())
        .unwrap_or(RETAINED_GENERATION_HANDLE_HEADROOM)
}

#[cfg(not(unix))]
fn current_open_handle_count() -> usize {
    RETAINED_GENERATION_HANDLE_HEADROOM
}

#[cfg(test)]
pub(super) fn validate_retained_generation_budget(
    planned: &BTreeSet<PathBuf>,
    repeated_generation_count: usize,
    soft_limit: Option<usize>,
    open_handles: usize,
) -> Result<()> {
    validate_retained_generation_budget_with_preimages(
        planned,
        repeated_generation_count,
        planned.len(),
        soft_limit,
        open_handles,
    )
}

pub(super) fn validate_retained_generation_budget_with_preimages(
    planned: &BTreeSet<PathBuf>,
    repeated_generation_count: usize,
    retained_preimage_count: usize,
    soft_limit: Option<usize>,
    open_handles: usize,
) -> Result<()> {
    let planned_generation_count = planned.len().saturating_add(repeated_generation_count);
    if planned_generation_count > MAX_EXISTING_INIT_RETAINED_GENERATIONS {
        bail!(
            "Existing-destination init plans {planned_generation_count} generated file generations, exceeding the safe retained-generation limit of {MAX_EXISTING_INIT_RETAINED_GENERATIONS}. Use a wholly missing destination so Jig can publish one privately staged tree, or reduce the explicit template/scaffold output set."
        );
    }
    // Planning snapshots pin every pre-existing leaf. A leaf that was missing
    // at that boundary cannot acquire a preimage later: publication rejects a
    // concurrently created replacement. Reserve one handle for each first Jig
    // generation plus only the preimages actually retained at planning time.
    // Additional publications are counted explicitly. Current/quarantine/
    // disposal snapshots are processed one leaf at a time and fit within the
    // fixed transient headroom.
    let required = retained_generation_handle_requirement_with_preimages(
        planned,
        repeated_generation_count,
        retained_preimage_count,
    );
    if soft_limit.is_some_and(|limit| open_handles.saturating_add(required) > limit) {
        bail!(
            "Existing-destination init needs capacity for approximately {required} retained file/directory handles in addition to {open_handles} already open handles, but the process soft handle limit is {}. Use a wholly missing destination, reduce the output set, or raise the process file-descriptor limit before retrying.",
            soft_limit.expect("checked above")
        );
    }
    Ok(())
}

pub(super) struct StagedInitPublication {
    staging_root: Option<TempDir>,
    pub(super) publish_source: PathBuf,
    publish_source_identity: path::RepositoryDirectoryCommit,
    publish_destination: PathBuf,
    publish_parent_identity: path::RepositoryDirectoryCommit,
    publish_permissions: fs::Permissions,
}

struct InitWriteStagingDirectory {
    directory: TempDir,
    identity: path::RepositoryDirectoryCommit,
}

fn verify_tracked_init_directories(
    directories: &BTreeMap<PathBuf, path::RepositoryDirectoryCommit>,
) -> Result<()> {
    for (directory, expected) in directories {
        let current = path::repository_directory_commit_matches_path(expected, directory)
            .with_context(|| {
                format!(
                    "Init output ancestor was replaced while init was running: {}",
                    directory.display()
                )
            })?;
        if !current {
            bail!(
                "Init output ancestor was replaced while init was running; refusing to mutate replacement directory {}",
                directory.display()
            );
        }
    }
    Ok(())
}

impl InitMutationTransaction {
    pub(super) fn create(destination: &Path) -> Result<Self> {
        let (existing_ancestor, missing_tail) = path::split_existing_ancestor(destination)?;
        path::ensure_atomic_noreplace_publication_supported(&existing_ancestor)?;
        if !missing_tail.is_empty() {
            let mut staging_builder = TempFileBuilder::new();
            staging_builder.prefix(".jig-init-stage-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                staging_builder.permissions(fs::Permissions::from_mode(0o700));
            }
            let staging_root = staging_builder
                .tempdir_in(&existing_ancestor)
                .with_context(|| {
                    format!(
                        "Failed to create private init staging directory in {}",
                        existing_ancestor.display()
                    )
                })?;
            let publish_source = staging_root.path().to_path_buf();
            let publish_source_identity = match path::repository_directory_commit_at(
                &publish_source,
            ) {
                Ok(identity) => identity,
                Err(primary) => {
                    let preserved = staging_root.keep();
                    bail!(
                        "{primary:#}\nCould not prove ownership of newly created private init staging; preserving it at {}",
                        preserved.display()
                    );
                }
            };
            let mut directory_identities =
                BTreeMap::from([(publish_source.clone(), publish_source_identity.clone())]);
            let setup = (|| -> Result<(
                fs::Permissions,
                path::RepositoryDirectoryCommit,
                PathBuf,
                path::RepositoryDirectoryCommit,
            )> {
                let permission_probe = staging_root.path().join(".jig-directory-mode-probe");
                fs::create_dir(&permission_probe).with_context(|| {
                    format!(
                        "Failed to probe final init directory permissions in {}",
                        staging_root.path().display()
                    )
                })?;
                let publish_permissions = fs::metadata(&permission_probe)
                    .with_context(|| {
                        format!(
                            "Failed to inspect final init directory permission probe {}",
                            permission_probe.display()
                        )
                    })?
                    .permissions();
                fs::remove_dir(&permission_probe).with_context(|| {
                    format!(
                        "Failed to remove final init directory permission probe {}",
                        permission_probe.display()
                    )
                })?;
                let publish_parent_identity =
                    path::repository_directory_commit_at(&existing_ancestor)?;
                let mut work_destination = staging_root.path().to_path_buf();
                for component in missing_tail.iter().skip(1) {
                    verify_tracked_init_directories(&directory_identities)?;
                    work_destination.push(component);
                    fs::create_dir(&work_destination).with_context(|| {
                        format!(
                            "Failed to create private init work-tree ancestor {}",
                            work_destination.display()
                        )
                    })?;
                    let identity =
                        path::repository_directory_commit_at(&work_destination).with_context(
                            || {
                                format!(
                                    "Private init work-tree ancestor is not a stable real directory: {}",
                                    work_destination.display()
                                )
                            },
                        )?;
                    directory_identities.insert(work_destination.clone(), identity);
                }
                verify_tracked_init_directories(&directory_identities)?;
                let destination_identity = directory_identities
                    .get(&work_destination)
                    .context("Private init work destination was not retained")?
                    .clone();
                Ok((
                    publish_permissions,
                    publish_parent_identity,
                    work_destination,
                    destination_identity,
                ))
            })();
            let (
                publish_permissions,
                publish_parent_identity,
                work_destination,
                destination_identity,
            ) = match setup {
                Ok(setup) => setup,
                Err(primary) => {
                    if let Err(boundary) = verify_tracked_init_directories(&directory_identities) {
                        let preserved = staging_root.keep();
                        bail!(
                            "{primary:#}\nPrivate init staging changed during setup ({boundary:#}); preserving the complete staging tree at {}",
                            preserved.display()
                        );
                    }
                    drop(directory_identities);
                    return Err(close_failed_staging(
                        staging_root,
                        &publish_source_identity,
                        primary,
                    ));
                }
            };
            let publish_destination = existing_ancestor.join(&missing_tail[0]);
            return Ok(Self {
                final_destination: destination.to_path_buf(),
                destination: work_destination,
                destination_identity,
                staged_publication: Some(StagedInitPublication {
                    staging_root: Some(staging_root),
                    publish_source,
                    publish_source_identity,
                    publish_destination,
                    publish_parent_identity,
                    publish_permissions,
                }),
                write_staging: BTreeMap::new(),
                next_snapshot: Cell::new(0),
                files: BTreeMap::new(),
                directory_identities,
                owned_directories: BTreeMap::new(),
                existing_generation_budget_sealed: false,
                armed: true,
            });
        }

        let metadata = fs::symlink_metadata(&existing_ancestor).with_context(|| {
            format!(
                "Failed to inspect init destination {}",
                existing_ancestor.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!(
                "Init destination must be a real directory: {}",
                existing_ancestor.display()
            );
        }
        Ok(Self {
            final_destination: destination.to_path_buf(),
            destination: existing_ancestor.clone(),
            destination_identity: path::repository_directory_commit_at(&existing_ancestor)?,
            staged_publication: None,
            write_staging: BTreeMap::new(),
            next_snapshot: Cell::new(0),
            files: BTreeMap::new(),
            directory_identities: BTreeMap::from([(
                existing_ancestor.clone(),
                path::repository_directory_commit_at(&existing_ancestor)?,
            )]),
            owned_directories: BTreeMap::new(),
            existing_generation_budget_sealed: false,
            armed: true,
        })
    }

    pub(super) fn work_destination(&self) -> &Path {
        &self.destination
    }

    pub(super) const fn is_privately_staged(&self) -> bool {
        self.staged_publication.is_some()
    }

    pub(super) fn verify_destination_identity(&self) -> Result<()> {
        let current = path::repository_directory_commit_matches_path(
            &self.destination_identity,
            &self.destination,
        )
        .with_context(|| {
            format!(
                "Init destination was replaced while init was running: {}",
                self.final_destination.display()
            )
        })?;
        if !current {
            bail!(
                "Init destination was replaced while init was running; refusing to mutate replacement path {}",
                self.final_destination.display()
            );
        }
        verify_tracked_init_directories(&self.directory_identities)?;
        for staging in self.write_staging.values() {
            if !path::repository_directory_commit_matches_path(
                &staging.identity,
                staging.directory.path(),
            )? {
                bail!(
                    "Private init write staging was replaced concurrently: {}",
                    staging.directory.path().display()
                );
            }
        }
        Ok(())
    }

    fn verify_rollback_root_and_preexisting_ancestors(&self) -> Result<()> {
        let current = path::repository_directory_commit_matches_path(
            &self.destination_identity,
            &self.destination,
        )?;
        if !current {
            bail!(
                "Init destination was replaced while rollback was starting: {}",
                self.final_destination.display()
            );
        }
        for (directory, expected) in &self.directory_identities {
            if self.owned_directories.contains_key(directory) {
                continue;
            }
            if !path::repository_directory_commit_matches_path(expected, directory)? {
                bail!(
                    "Pre-existing init output ancestor was replaced while rollback was starting: {}",
                    directory.display()
                );
            }
        }
        Ok(())
    }

    fn changed_owned_ancestor(&self, relative: &Path) -> Result<Option<PathBuf>> {
        let output = self.destination.join(relative);
        for (directory, expected) in &self.owned_directories {
            if !output.starts_with(directory) {
                continue;
            }
            match path::repository_directory_commit_matches_path(expected, directory) {
                Ok(true) => {}
                Ok(false) => return Ok(Some(directory.clone())),
                Err(error)
                    if error
                        .downcast_ref::<io::Error>()
                        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
                {
                    return Ok(Some(directory.clone()));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn ensure_write_staging(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        let output = self.destination.join(relative);
        let parent = output
            .parent()
            .with_context(|| format!("Init output has no parent: {}", output.display()))?
            .to_path_buf();
        if self.write_staging.contains_key(&parent) {
            return Ok(());
        }
        let mut builder = TempFileBuilder::new();
        builder.prefix(".jig-init-writes-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let outside_root = self.destination.parent().unwrap_or(&self.destination);
        let directory = if path::repository_paths_same_filesystem(&parent, outside_root)? {
            match builder.tempdir_in(outside_root) {
                Ok(directory) => directory,
                Err(outside_error) => builder.tempdir_in(&parent).with_context(|| {
                    format!(
                        "Failed to create same-filesystem private init write staging in {} after sibling staging in {} failed: {outside_error}",
                        parent.display(),
                        outside_root.display()
                    )
                })?,
            }
        } else {
            builder.tempdir_in(&parent).with_context(|| {
                format!(
                    "Failed to create same-filesystem private init write staging in {}",
                    parent.display()
                )
            })?
        };
        let identity = path::repository_directory_commit_at(directory.path())?;
        self.write_staging.insert(
            parent,
            InitWriteStagingDirectory {
                directory,
                identity,
            },
        );
        Ok(())
    }

    pub(super) fn write_staging_path(&self, relative: &Path) -> Option<&Path> {
        self.destination
            .join(relative)
            .parent()
            .and_then(|parent| self.write_staging.get(parent))
            .map(|staging| staging.directory.path())
    }

    pub(super) fn publication_permissions(
        &self,
        relative: &Path,
    ) -> Result<Option<fs::Permissions>> {
        let Some(mutation) = self.files.get(relative) else {
            return Ok(None);
        };
        let state = mutation
            .expected_jig_states
            .last()
            .unwrap_or(&mutation.before);
        match state {
            InitPathSnapshot::Regular(commit) => commit
                .handle
                .metadata()
                .map(|metadata| Some(metadata.permissions()))
                .with_context(|| {
                    format!(
                        "Failed to inspect retained permissions for {}",
                        relative.display()
                    )
                }),
            InitPathSnapshot::Missing | InitPathSnapshot::Symlink { .. } => Ok(None),
        }
    }

    fn close_write_staging(&mut self) -> Result<()> {
        let staging = std::mem::take(&mut self.write_staging);
        let mut failures = Vec::new();
        for (_, staging) in staging {
            match path::repository_directory_commit_matches_path(
                &staging.identity,
                staging.directory.path(),
            ) {
                Ok(true) => {
                    drop(staging.identity);
                    if let Err(error) = staging.directory.close() {
                        failures.push(format!(
                            "failed to remove private init write staging: {error}"
                        ));
                    }
                }
                Ok(false) => {
                    let preserved = staging.directory.keep();
                    failures.push(format!(
                        "private init write staging was replaced concurrently; preserving foreign replacement {}",
                        preserved.display()
                    ));
                }
                Err(error) => {
                    let preserved = staging.directory.keep();
                    failures.push(format!(
                        "could not verify private init write staging {} for cleanup ({error:#}); preserving it",
                        preserved.display()
                    ));
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }
}
