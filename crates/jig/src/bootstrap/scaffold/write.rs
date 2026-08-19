use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::bootstrap::path::{
    RepositoryFileCommit, RepositoryFileLeaf, read_repository_regular_file,
    validate_portable_planned_file_collisions, validate_repository_regular_file_leaf,
    write_repository_file_atomic, write_repository_file_atomic_guarded,
    write_repository_file_atomic_staged,
};

use crate::bootstrap::InitMutationTransaction;

use super::{InitScaffoldPlan, ScaffoldDb, ScaffoldPreset};

#[derive(Clone, Debug, Default)]
pub(super) struct ScaffoldReport {
    files_created: Vec<String>,
    files_modified: Vec<String>,
    files_unchanged: Vec<String>,
}

#[derive(Clone, Debug)]
pub(in crate::bootstrap) struct ScaffoldFile {
    pub(in crate::bootstrap) relative: String,
    pub(in crate::bootstrap) contents: String,
}

#[derive(Clone, Debug)]
enum ScaffoldWrite {
    Create(ScaffoldFile),
    Modify(ScaffoldFile),
    Unchanged(String),
}

pub(super) fn scaffold_file(
    relative: impl Into<String>,
    contents: impl Into<String>,
) -> ScaffoldFile {
    ScaffoldFile {
        relative: relative.into(),
        contents: contents.into(),
    }
}

impl ScaffoldReport {
    pub(super) fn preflight_files(
        destination: &Path,
        files: Vec<ScaffoldFile>,
        force: bool,
    ) -> Result<()> {
        preflight_scaffold_writes(destination, files, force).map(|_| ())
    }

    #[cfg(test)]
    pub(super) fn write_files(
        destination: &Path,
        files: Vec<ScaffoldFile>,
        force: bool,
    ) -> Result<Self> {
        Self::write_files_with_transaction(destination, files, force, None)
    }

    pub(super) fn write_files_with_transaction(
        destination: &Path,
        files: Vec<ScaffoldFile>,
        force: bool,
        mut transaction: Option<&mut InitMutationTransaction>,
    ) -> Result<Self> {
        let mut report = Self::default();
        let writes = preflight_scaffold_writes(destination, files, force)?;

        for write in writes {
            match write {
                ScaffoldWrite::Create(file) => {
                    let relative = file.relative.clone();
                    prepare_transaction(transaction.as_deref_mut(), Path::new(&relative))?;
                    let commit = write_scaffold_file(
                        destination,
                        file,
                        transaction.as_deref_mut(),
                        &mut report.files_created,
                    )?;
                    record_transaction_commit(
                        transaction.as_deref_mut(),
                        Path::new(&relative),
                        commit,
                    )?;
                }
                ScaffoldWrite::Modify(file) => {
                    let relative = file.relative.clone();
                    prepare_transaction(transaction.as_deref_mut(), Path::new(&relative))?;
                    let commit = write_scaffold_file(
                        destination,
                        file,
                        transaction.as_deref_mut(),
                        &mut report.files_modified,
                    )?;
                    record_transaction_commit(
                        transaction.as_deref_mut(),
                        Path::new(&relative),
                        commit,
                    )?;
                }
                ScaffoldWrite::Unchanged(relative) => report.files_unchanged.push(relative),
            }
        }
        Ok(report)
    }

    pub(super) fn into_json(self, plan: &InitScaffoldPlan) -> Value {
        json!({
            "preset": match plan.preset {
                ScaffoldPreset::RustReact => "rust-react",
                ScaffoldPreset::GoReact => "go-react",
                ScaffoldPreset::HarnessOnly => unreachable!("harness-only has no scaffold report"),
            },
            "repo_name": &plan.repo_name,
            "repo_name_sanitized_from": (plan.requested_repo_name != plan.repo_name).then_some(&plan.requested_repo_name),
            "db": match plan.db {
                ScaffoldDb::None => "none",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => "sqlite",
            },
            "frontends": plan.frontends.iter().map(|frontend| {
                json!({
                    "name": frontend.name,
                    "dir": frontend.dir,
                    "kind": frontend.dev_kind,
                    "role": frontend.kind.as_str(),
                    "ui": frontend.ui_provenance(),
                })
            }).collect::<Vec<_>>(),
            "frontend_notices": &plan.custom_frontend_notices,
            "files_created": self.files_created,
            "files_modified": self.files_modified,
            "files_unchanged": self.files_unchanged,
        })
    }
}

fn preflight_scaffold_writes(
    destination: &Path,
    files: Vec<ScaffoldFile>,
    force: bool,
) -> Result<Vec<ScaffoldWrite>> {
    let mut conflicts = Vec::new();
    let mut writes = Vec::new();

    validate_portable_planned_file_collisions(files.iter().map(|file| Path::new(&file.relative)))?;

    for file in files {
        let relative = Path::new(&file.relative);
        match validate_repository_regular_file_leaf(destination, relative)? {
            RepositoryFileLeaf::Missing => writes.push(ScaffoldWrite::Create(file)),
            RepositoryFileLeaf::RegularFile => {
                let existing = read_repository_regular_file(destination, relative)?;
                if existing != file.contents && !force {
                    conflicts.push(file.relative.clone());
                } else if existing == file.contents {
                    writes.push(ScaffoldWrite::Unchanged(file.relative));
                } else {
                    writes.push(ScaffoldWrite::Modify(file));
                }
            }
            RepositoryFileLeaf::Symlink => {
                unreachable!("repository regular-file validation rejects symlink leaves")
            }
        }
    }

    if !conflicts.is_empty() {
        conflicts.sort();
        bail!(
            "Scaffold paths already exist and differ; pass --force to overwrite them in place:\n  {}",
            conflicts.join("\n  ")
        );
    }
    Ok(writes)
}

fn write_scaffold_file(
    destination: &Path,
    file: ScaffoldFile,
    transaction: Option<&mut InitMutationTransaction>,
    completed: &mut Vec<String>,
) -> Result<RepositoryFileCommit> {
    let relative = Path::new(&file.relative);
    let commit = if transaction
        .as_ref()
        .is_some_and(|transaction| transaction.is_privately_staged())
    {
        let expected_leaf = validate_repository_regular_file_leaf(destination, relative)?;
        let transaction = transaction.expect("checked above");
        write_repository_file_atomic_staged(
            destination,
            relative,
            file.contents.as_bytes(),
            expected_leaf,
            || transaction.verify_destination_identity(),
        )?
    } else if let Some(transaction) = transaction {
        let desired_permissions = transaction.publication_permissions(relative)?;
        let temporary_directory = transaction
            .write_staging_path(relative)
            .context("Existing-destination init write staging is unavailable")?
            .to_path_buf();
        write_repository_file_atomic_guarded(
            destination,
            relative,
            file.contents.as_bytes(),
            desired_permissions,
            &temporary_directory,
            || transaction.verify_destination_identity(),
        )?
    } else {
        let expected_leaf = validate_repository_regular_file_leaf(destination, relative)?;
        write_repository_file_atomic(
            destination,
            relative,
            file.contents.as_bytes(),
            expected_leaf,
        )?
    };
    completed.push(file.relative);
    Ok(commit)
}

fn prepare_transaction(
    transaction: Option<&mut InitMutationTransaction>,
    relative: &Path,
) -> Result<()> {
    if let Some(transaction) = transaction {
        transaction.prepare_file_publication(relative)?;
    }
    Ok(())
}

fn record_transaction_commit(
    transaction: Option<&mut InitMutationTransaction>,
    relative: &Path,
    commit: RepositoryFileCommit,
) -> Result<()> {
    if let Some(transaction) = transaction {
        transaction.record_regular_commit(relative, commit)?;
    }
    Ok(())
}
