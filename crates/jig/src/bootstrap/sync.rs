use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[cfg(test)]
use super::ALWAYS_TASK_MUTATED_PATHS;
use super::ANSWERS_FILE;
use super::InitMutationTransaction;
use super::file_copy::{
    copy_file_or_symlink_with_permissions, path_exists, prepare_copy_destination_and_read_metadata,
};
use super::managed_paths::{self, ManagedBlockSpec};
use super::path::{
    RepositoryFileCommit, RepositoryFileLeaf, RepositorySymlinkCommit,
    copy_repository_regular_file_atomic_with_permissions,
    copy_repository_regular_file_atomic_with_permissions_guarded,
    copy_repository_regular_file_atomic_with_permissions_staged, copy_repository_symlink_atomic,
    copy_repository_symlink_atomic_guarded, copy_repository_symlink_atomic_staged,
    validate_portable_planned_file_collisions, validate_repository_relative_file_leaf,
};
use super::staged_render::StagedRender;
use crate::progress::CliProgress;

pub(super) struct ApplyRenderOptions<'a> {
    pub(super) force: bool,
    pub(super) dry_run: bool,
    pub(super) allow_answers_overwrite: bool,
    pub(super) allow_contract_overwrite: bool,
    pub(super) allow_manifest_overwrite: bool,
    pub(super) backup_root: Option<&'a Path>,
    pub(super) conflict_message: &'a str,
    pub(super) progress: CliProgress,
    pub(super) init_transaction: Option<&'a mut InitMutationTransaction>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct ApplyRenderReport {
    pub(super) dry_run: bool,
    pub(super) active_managed_paths: Vec<String>,
    pub(super) retired_managed_paths: Vec<String>,
    pub(super) files_created: Vec<String>,
    pub(super) files_modified: Vec<String>,
    pub(super) files_removed: Vec<String>,
    pub(super) files_unchanged: Vec<String>,
    pub(super) managed_blocks_inserted: Vec<String>,
    pub(super) managed_blocks_rendered: Vec<String>,
    pub(super) backups: Vec<ApplyRenderBackup>,
    pub(super) conflicts: Vec<RenderConflict>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ApplyRenderBackup {
    pub(super) path: String,
    pub(super) backup_path: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) struct RenderConflict {
    pub(super) path: String,
    pub(super) kind: RenderConflictKind,
    pub(super) detail: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RenderConflictKind {
    NonRegularRootAgents,
    ModifiedManagedPath,
    RemovedManagedPath,
}

pub(super) fn apply_staged_render(
    staged: &StagedRender,
    destination: &Path,
    mut options: ApplyRenderOptions<'_>,
) -> Result<ApplyRenderReport> {
    validate_portable_planned_file_collisions(&staged.active_paths)?;
    preflight_apply_paths(staged, destination, options.backup_root)?;

    let conflicts = if options.force {
        options.progress.step(
            "check conflicts",
            "--force supplied; accepting rendered output",
        );
        staged_render_conflicts(
            staged,
            destination,
            options.allow_answers_overwrite,
            options.allow_contract_overwrite,
            options.allow_manifest_overwrite,
        )?
    } else {
        options
            .progress
            .step("check conflicts", "compare rendered managed paths");
        let conflicts = options
            .progress
            .log_blocked_on_err(staged_render_conflicts(
                staged,
                destination,
                options.allow_answers_overwrite,
                options.allow_contract_overwrite,
                options.allow_manifest_overwrite,
            ))?;
        if !conflicts.is_empty() {
            let message = conflict_count_message(conflicts.len());
            if options.dry_run {
                options.progress.info("conflicts", message);
                for line in conflict_lines(&conflicts) {
                    options.progress.info("conflict", line);
                }
            } else {
                options.progress.blocked(message);
                bail!(
                    "{}\n{}",
                    options.conflict_message,
                    conflict_lines(&conflicts).join("\n")
                );
            }
        }
        conflicts
    };

    options.progress.step(
        if options.dry_run {
            "preview managed paths"
        } else {
            "apply managed paths"
        },
        format!("{} path(s)", staged.operation_count()),
    );
    let mut report = ApplyRenderReport {
        dry_run: options.dry_run,
        active_managed_paths: staged
            .active_paths
            .iter()
            .map(|path| repository_relative_path_string(path))
            .collect(),
        retired_managed_paths: staged
            .retirement_paths
            .iter()
            .map(|path| repository_relative_path_string(path))
            .collect(),
        conflicts,
        ..ApplyRenderReport::default()
    };
    for relative in ordered_operation_paths(staged) {
        let rendered_path = staged.destination.join(relative);
        let destination_path = destination.join(relative);
        let relative_text = repository_relative_path_string(relative);
        validate_managed_destination_leaf(destination, relative)?;
        if path_exists(&rendered_path) {
            if path_exists(&destination_path) {
                if files_match(&rendered_path, &destination_path)? {
                    report.files_unchanged.push(relative_text.clone());
                    continue;
                } else {
                    report.files_modified.push(relative_text.clone());
                    if let Some(spec) = managed_paths::managed_block_spec(relative) {
                        if managed_block_inserted(&rendered_path, Some(&destination_path), spec)? {
                            report.managed_blocks_inserted.push(relative_text.clone());
                        }
                    }
                }
            } else {
                report.files_created.push(relative_text.clone());
                if let Some(spec) = managed_paths::managed_block_spec(relative) {
                    if managed_block_inserted(&rendered_path, None, spec)? {
                        report.managed_blocks_rendered.push(relative_text.clone());
                    }
                }
            }
            if !options.dry_run {
                validate_managed_destination_leaf(destination, relative)?;
                if path_exists(&destination_path)
                    && !files_match(&rendered_path, &destination_path)?
                {
                    backup_destination_path(
                        destination,
                        &destination_path,
                        relative,
                        options.backup_root,
                        &mut report,
                    )?;
                }
                if let Some(transaction) = options.init_transaction.as_deref_mut() {
                    transaction.prepare_file_publication(relative)?;
                }
                let published = options.progress.log_blocked_on_err(copy_rendered_path(
                    &rendered_path,
                    destination,
                    &destination_path,
                    relative,
                    options.init_transaction.as_deref_mut(),
                ))?;
                if let Some(transaction) = options.init_transaction.as_deref_mut() {
                    match published {
                        PublishedRepositoryPath::Regular(commit) => {
                            transaction.record_regular_commit(relative, commit)?;
                        }
                        PublishedRepositoryPath::Symlink(commit) => {
                            transaction.record_symlink_commit(relative, commit)?;
                        }
                    }
                }
            }
        } else if path_exists(&destination_path) {
            report.files_removed.push(relative_text.clone());
            if !options.dry_run {
                validate_managed_destination_leaf(destination, relative)?;
                backup_destination_path(
                    destination,
                    &destination_path,
                    relative,
                    options.backup_root,
                    &mut report,
                )?;
                if let Some(transaction) = options.init_transaction.as_deref_mut() {
                    transaction.prepare_file_publication(relative)?;
                    transaction.record_missing_commit(relative)?;
                } else {
                    validate_managed_destination_leaf(destination, relative)?;
                    options
                        .progress
                        .log_blocked_on_err(remove_managed_destination_leaf(
                            destination,
                            relative,
                        ))?;
                }
            }
        }
    }
    Ok(report)
}

fn staged_render_conflicts(
    staged: &StagedRender,
    destination: &Path,
    allow_answers_overwrite: bool,
    allow_contract_overwrite: bool,
    allow_manifest_overwrite: bool,
) -> Result<Vec<RenderConflict>> {
    let mut conflicts = BTreeSet::new();
    for relative in staged
        .active_paths
        .iter()
        .chain(staged.retirement_paths.iter())
    {
        if allow_answers_overwrite && relative == Path::new(ANSWERS_FILE) {
            continue;
        }
        if allow_contract_overwrite && relative == Path::new(".agent/jig-contract.json") {
            continue;
        }
        if allow_manifest_overwrite && relative == Path::new(managed_paths::MANIFEST_PATH) {
            continue;
        }

        let rendered_path = staged.destination.join(relative);
        let destination_path = destination.join(relative);
        if path_exists(&rendered_path) {
            if let Some(spec) = managed_paths::managed_block_spec(relative) {
                if destination_is_regular_file(&destination_path)? {
                    continue;
                }
                if path_exists(&destination_path) {
                    conflicts.insert(RenderConflict {
                        path: repository_relative_path_string(relative),
                        kind: RenderConflictKind::NonRegularRootAgents,
                        detail: format!(
                            "{} exists but is not a regular file; managed block merge cannot apply safely",
                            spec.path,
                        ),
                    });
                    continue;
                }
            }
            if path_exists(&destination_path) && !files_match(&rendered_path, &destination_path)? {
                conflicts.insert(RenderConflict {
                    path: repository_relative_path_string(relative),
                    kind: RenderConflictKind::ModifiedManagedPath,
                    detail: "destination differs from the rendered template-managed path".into(),
                });
            }
        } else if path_exists(&destination_path) {
            conflicts.insert(RenderConflict {
                path: repository_relative_path_string(relative),
                kind: RenderConflictKind::RemovedManagedPath,
                detail: "template no longer renders this managed path".into(),
            });
        }
    }
    Ok(conflicts.into_iter().collect())
}

fn ordered_operation_paths(staged: &StagedRender) -> Vec<&PathBuf> {
    let manifest = Path::new(managed_paths::MANIFEST_PATH);
    staged
        .active_paths
        .iter()
        .filter(|path| path.as_path() != manifest)
        .chain(staged.retirement_paths.iter())
        .chain(staged.active_paths.get(manifest))
        .collect()
}

fn preflight_apply_paths(
    staged: &StagedRender,
    destination: &Path,
    backup_root: Option<&Path>,
) -> Result<()> {
    for relative in ordered_operation_paths(staged) {
        validate_managed_destination_leaf(destination, relative)?;
        if let Some(backup_root) = backup_root {
            let backup_path = backup_root.join(relative);
            let backup_relative = backup_path.strip_prefix(destination).with_context(|| {
                format!(
                    "Backup destination {} must be contained by repository root {}",
                    backup_path.display(),
                    destination.display()
                )
            })?;
            validate_backup_destination_leaf(destination, backup_relative)?;
        }
    }
    Ok(())
}

fn conflict_lines(conflicts: &[RenderConflict]) -> Vec<String> {
    conflicts
        .iter()
        .map(|conflict| format!("{} ({})", conflict.path, conflict.detail))
        .collect()
}

fn conflict_count_message(count: usize) -> String {
    if count == 1 {
        return "1 managed path differs".to_string();
    }
    format!("{count} managed paths differ")
}

fn managed_block_inserted(
    rendered_path: &Path,
    destination_path: Option<&Path>,
    spec: ManagedBlockSpec,
) -> Result<bool> {
    if !file_contains_complete_managed_block(rendered_path, spec)? {
        return Ok(false);
    }
    match destination_path {
        Some(destination_path) => Ok(!file_contains_complete_managed_block(
            destination_path,
            spec,
        )?),
        None => Ok(true),
    }
}

fn file_contains_complete_managed_block(path: &Path, spec: ManagedBlockSpec) -> Result<bool> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(contents.contains(spec.begin) && contents.contains(spec.end))
}

fn destination_is_regular_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("Failed to stat {}", path.display())),
    }
}

fn copy_rendered_path(
    rendered_path: &Path,
    destination: &Path,
    destination_path: &Path,
    relative: &Path,
    transaction: Option<&mut InitMutationTransaction>,
) -> Result<PublishedRepositoryPath> {
    validate_managed_destination_leaf(destination, relative)?;
    let guarded = transaction
        .as_ref()
        .is_some_and(|transaction| !transaction.is_privately_staged());
    let privately_staged = transaction
        .as_ref()
        .is_some_and(|transaction| transaction.is_privately_staged());
    let metadata = if transaction.is_some() {
        fs::symlink_metadata(rendered_path)
            .with_context(|| format!("Failed to stat {}", rendered_path.display()))?
    } else {
        prepare_copy_destination_and_read_metadata(rendered_path, destination_path)?
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        bail!(
            "Cannot apply rendered directory {}; managed render paths must be files or symlinks",
            rendered_path.display()
        );
    }
    let expected_leaf = validate_managed_destination_leaf(destination, relative)?;
    if !metadata.file_type().is_symlink() {
        if !metadata.is_file() {
            bail!(
                "Cannot apply rendered special file {}; managed render paths must be files or symlinks",
                rendered_path.display()
            );
        }
        return if guarded {
            let transaction = transaction.expect("checked above");
            let temporary_directory = transaction
                .write_staging_path(relative)
                .context("Existing-destination init write staging is unavailable")?
                .to_path_buf();
            copy_repository_regular_file_atomic_with_permissions_guarded(
                destination,
                relative,
                rendered_path,
                metadata.permissions(),
                &temporary_directory,
                || transaction.verify_destination_identity(),
            )
            .map(PublishedRepositoryPath::Regular)
        } else if privately_staged {
            let transaction = transaction.expect("checked above");
            copy_repository_regular_file_atomic_with_permissions_staged(
                destination,
                relative,
                rendered_path,
                metadata.permissions(),
                expected_leaf,
                || transaction.verify_destination_identity(),
            )
            .map(PublishedRepositoryPath::Regular)
        } else {
            copy_repository_regular_file_atomic_with_permissions(
                destination,
                relative,
                rendered_path,
                metadata.permissions(),
                expected_leaf,
            )
            .map(PublishedRepositoryPath::Regular)
        };
    }

    if !guarded && path_exists(destination_path) {
        if let Some(transaction) = transaction.as_deref() {
            transaction.verify_destination_identity()?;
        }
        remove_managed_destination_leaf(destination, relative)?;
        if let Some(transaction) = transaction.as_deref() {
            transaction.verify_destination_identity()?;
        }
    }
    if validate_managed_destination_leaf(destination, relative)? != RepositoryFileLeaf::Missing {
        bail!(
            "Managed destination {} changed while it was being replaced",
            destination_path.display()
        );
    }
    if guarded {
        let transaction = transaction.expect("checked above");
        let temporary_directory = transaction
            .write_staging_path(relative)
            .context("Existing-destination init write staging is unavailable")?
            .to_path_buf();
        copy_repository_symlink_atomic_guarded(
            destination,
            relative,
            rendered_path,
            &temporary_directory,
            || transaction.verify_destination_identity(),
        )
        .map(PublishedRepositoryPath::Symlink)
    } else if privately_staged {
        let transaction = transaction.expect("checked above");
        copy_repository_symlink_atomic_staged(destination, relative, rendered_path, || {
            transaction.verify_destination_identity()
        })
        .map(PublishedRepositoryPath::Symlink)
    } else {
        copy_repository_symlink_atomic(destination, relative, rendered_path)
            .map(PublishedRepositoryPath::Symlink)
    }
}

enum PublishedRepositoryPath {
    Regular(RepositoryFileCommit),
    Symlink(RepositorySymlinkCommit),
}

fn backup_destination_path(
    destination: &Path,
    destination_path: &Path,
    relative: &Path,
    backup_root: Option<&Path>,
    report: &mut ApplyRenderReport,
) -> Result<()> {
    let Some(backup_root) = backup_root else {
        return Ok(());
    };
    let backup_path = backup_root.join(relative);
    let backup_relative = backup_path.strip_prefix(destination).with_context(|| {
        format!(
            "Backup destination {} must be contained by repository root {}",
            backup_path.display(),
            destination.display()
        )
    })?;
    validate_managed_destination_leaf(destination, relative)?;
    validate_backup_destination_leaf(destination, backup_relative)?;
    let metadata = prepare_copy_destination_and_read_metadata(destination_path, &backup_path)?;
    validate_managed_destination_leaf(destination, relative)?;
    validate_backup_destination_leaf(destination, backup_relative)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        bail!(
            "Cannot back up directory managed path {}; expected file or symlink",
            destination_path.display()
        );
    }
    copy_file_or_symlink_with_permissions(destination_path, &backup_path, &metadata)?;
    report.backups.push(ApplyRenderBackup {
        path: repository_relative_path_string(relative),
        backup_path: backup_path.display().to_string(),
    });
    Ok(())
}

fn validate_managed_destination_leaf(
    destination: &Path,
    relative: &Path,
) -> Result<RepositoryFileLeaf> {
    validate_repository_relative_file_leaf(destination, relative)
}

fn validate_backup_destination_leaf(
    destination: &Path,
    backup_relative: &Path,
) -> Result<RepositoryFileLeaf> {
    let leaf = validate_repository_relative_file_leaf(destination, backup_relative)?;
    if leaf == RepositoryFileLeaf::Symlink {
        bail!(
            "Unsafe backup destination {}: backup leaf must be missing or a regular file, not a symlink",
            destination.join(backup_relative).display()
        );
    }
    Ok(leaf)
}

fn remove_managed_destination_leaf(destination: &Path, relative: &Path) -> Result<()> {
    let path = destination.join(relative);
    match validate_managed_destination_leaf(destination, relative)? {
        RepositoryFileLeaf::RegularFile | RepositoryFileLeaf::Symlink => {
            fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))
        }
        RepositoryFileLeaf::Missing => Ok(()),
    }
}

#[cfg(test)]
pub(super) fn rendered_conflicts(rendered_root: &Path, destination: &Path) -> Result<Vec<String>> {
    let mut conflicts = BTreeSet::new();
    collect_sync_conflicts(rendered_root, destination, rendered_root, &mut conflicts)?;
    for relative in ALWAYS_TASK_MUTATED_PATHS {
        let path = destination.join(relative);
        if path.exists() {
            conflicts.insert((*relative).to_string());
        }
    }
    Ok(conflicts.into_iter().collect())
}

#[cfg(test)]
fn collect_sync_conflicts(
    rendered_root: &Path,
    destination_root: &Path,
    current_rendered: &Path,
    conflicts: &mut BTreeSet<String>,
) -> Result<()> {
    for entry in fs::read_dir(current_rendered)? {
        let entry = entry?;
        let rendered_path = entry.path();
        let relative = rendered_path.strip_prefix(rendered_root).with_context(|| {
            format!(
                "{} is not under {}",
                rendered_path.display(),
                rendered_root.display()
            )
        })?;
        let destination_path = destination_root.join(relative);
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if destination_path.exists() && !destination_path.is_dir() {
                conflicts.insert(repository_relative_path_string(relative));
                continue;
            }
            collect_sync_conflicts(rendered_root, destination_root, &rendered_path, conflicts)?;
            continue;
        }

        if destination_path.exists() && !files_match(&rendered_path, &destination_path)? {
            conflicts.insert(repository_relative_path_string(relative));
        }
    }
    Ok(())
}

fn files_match(rendered_path: &Path, destination_path: &Path) -> Result<bool> {
    let rendered_meta = fs::symlink_metadata(rendered_path)
        .with_context(|| format!("Failed to stat {}", rendered_path.display()))?;
    let destination_meta = fs::symlink_metadata(destination_path)
        .with_context(|| format!("Failed to stat {}", destination_path.display()))?;

    if either_is_symlink(&rendered_meta, &destination_meta) {
        return symlinks_match(
            rendered_path,
            &rendered_meta,
            destination_path,
            &destination_meta,
        );
    }

    if !both_are_files(&rendered_meta, &destination_meta) {
        return Ok(false);
    }
    if !executable_bits_match(&rendered_meta, &destination_meta) {
        return Ok(false);
    }

    file_contents_match(rendered_path, destination_path)
}

fn repository_relative_path_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn either_is_symlink(rendered_meta: &fs::Metadata, destination_meta: &fs::Metadata) -> bool {
    rendered_meta.file_type().is_symlink() || destination_meta.file_type().is_symlink()
}

fn symlinks_match(
    rendered_path: &Path,
    rendered_meta: &fs::Metadata,
    destination_path: &Path,
    destination_meta: &fs::Metadata,
) -> Result<bool> {
    if !(rendered_meta.file_type().is_symlink() && destination_meta.file_type().is_symlink()) {
        return Ok(false);
    }

    let rendered_target = fs::read_link(rendered_path)
        .with_context(|| format!("Failed to read symlink {}", rendered_path.display()))?;
    let destination_target = fs::read_link(destination_path)
        .with_context(|| format!("Failed to read symlink {}", destination_path.display()))?;
    Ok(rendered_target == destination_target)
}

fn both_are_files(rendered_meta: &fs::Metadata, destination_meta: &fs::Metadata) -> bool {
    rendered_meta.is_file() && destination_meta.is_file()
}

fn file_contents_match(rendered_path: &Path, destination_path: &Path) -> Result<bool> {
    let rendered = fs::read(rendered_path)
        .with_context(|| format!("Failed to read {}", rendered_path.display()))?;
    let destination = fs::read(destination_path)
        .with_context(|| format!("Failed to read {}", destination_path.display()))?;
    Ok(rendered == destination)
}

#[cfg(unix)]
fn executable_bits_match(rendered_meta: &fs::Metadata, destination_meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    (rendered_meta.permissions().mode() & 0o111) == (destination_meta.permissions().mode() & 0o111)
}

#[cfg(not(unix))]
fn executable_bits_match(_rendered_meta: &fs::Metadata, _destination_meta: &fs::Metadata) -> bool {
    true
}
