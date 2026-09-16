use std::fs;
use std::path::{Component, Path, PathBuf};

use super::{InvalidWorkspaceReason, TrackerError, profile_0_5_7::BeadsInfo};

const TRACKER_ROOT: &str = ".beads";

pub(super) fn canonical_repository_root(root: &Path) -> Result<PathBuf, TrackerError> {
    let metadata =
        fs::symlink_metadata(root).map_err(|_| invalid(InvalidWorkspaceReason::RepositoryRoot))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid(InvalidWorkspaceReason::RepositoryRoot));
    }
    root.canonicalize()
        .map_err(|_| invalid(InvalidWorkspaceReason::RepositoryRoot))
}

pub(super) fn validate_store(root: &Path) -> Result<(), TrackerError> {
    crate::repository_path::validate_repository_directory_path(root, Path::new(TRACKER_ROOT))
        .map_err(|_| invalid(InvalidWorkspaceReason::TrackerStore))?;
    let store = root.join(TRACKER_ROOT);
    let metadata =
        fs::symlink_metadata(&store).map_err(|_| invalid(InvalidWorkspaceReason::TrackerStore))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid(InvalidWorkspaceReason::TrackerStore));
    }
    validate_no_provider_routing(root, &store)?;
    Ok(())
}

fn validate_no_provider_routing(root: &Path, store: &Path) -> Result<(), TrackerError> {
    for local_artifact in [store.join("routes.jsonl"), store.join("redirect")] {
        if is_provider_routing_file(&local_artifact) {
            return Err(invalid(InvalidWorkspaceReason::Routing));
        }
    }

    for ancestor in root.ancestors().skip(1) {
        if is_provider_routing_file(&ancestor.join("mayor/town.json"))
            && is_provider_routing_file(&ancestor.join(".beads/routes.jsonl"))
        {
            return Err(invalid(InvalidWorkspaceReason::Routing));
        }
    }
    Ok(())
}

fn is_provider_routing_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

pub(super) fn validate_discovered_paths(
    root: &Path,
    info: &BeadsInfo,
) -> Result<(PathBuf, PathBuf), TrackerError> {
    validate_store(root)?;
    let store = root.join(TRACKER_ROOT);
    let canonical_store = store
        .canonicalize()
        .map_err(|_| invalid(InvalidWorkspaceReason::TrackerStore))?;
    let discovered_store =
        canonical_existing_absolute(&info.beads_dir, InvalidWorkspaceReason::TrackerStore)?;
    if discovered_store != canonical_store {
        return Err(invalid(InvalidWorkspaceReason::TrackerStore));
    }
    validate_descendant(
        &canonical_store,
        &info.database_path,
        InvalidWorkspaceReason::Database,
        true,
    )?;
    validate_descendant(
        &canonical_store,
        &info.jsonl_path,
        InvalidWorkspaceReason::JsonlExport,
        false,
    )?;
    Ok((info.database_path.clone(), info.jsonl_path.clone()))
}

pub(super) fn revalidate_paths(
    root: &Path,
    database_path: &Path,
    jsonl_path: &Path,
) -> Result<(), TrackerError> {
    validate_store(root)?;
    let store = root
        .join(TRACKER_ROOT)
        .canonicalize()
        .map_err(|_| invalid(InvalidWorkspaceReason::TrackerStore))?;
    validate_descendant(
        &store,
        database_path,
        InvalidWorkspaceReason::Database,
        true,
    )?;
    validate_descendant(
        &store,
        jsonl_path,
        InvalidWorkspaceReason::JsonlExport,
        false,
    )
}

fn canonical_existing_absolute(
    path: &Path,
    reason: InvalidWorkspaceReason,
) -> Result<PathBuf, TrackerError> {
    if !absolute_normal_path(path) {
        return Err(invalid(reason));
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid(reason))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid(reason));
    }
    path.canonicalize().map_err(|_| invalid(reason))
}

fn validate_descendant(
    store: &Path,
    path: &Path,
    reason: InvalidWorkspaceReason,
    must_exist: bool,
) -> Result<(), TrackerError> {
    if !absolute_normal_path(path) || !path.starts_with(store) || path == store {
        return Err(invalid(reason));
    }
    let relative = path.strip_prefix(store).map_err(|_| invalid(reason))?;
    let components = relative.components().collect::<Vec<_>>();
    let mut current = store.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return Err(invalid(reason));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(invalid(reason));
                }
                let is_leaf = index + 1 == components.len();
                if (is_leaf && !metadata.is_file()) || (!is_leaf && !metadata.is_dir()) {
                    return Err(invalid(reason));
                }
                if is_leaf && has_external_hard_link(&metadata) {
                    return Err(invalid(InvalidWorkspaceReason::HardLinkedAuthority));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if must_exist || index + 1 != components.len() {
                    return Err(invalid(reason));
                }
            }
            Err(_) => return Err(invalid(reason)),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn has_external_hard_link(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
const fn has_external_hard_link(_metadata: &fs::Metadata) -> bool {
    false
}

fn absolute_normal_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

const fn invalid(reason: InvalidWorkspaceReason) -> TrackerError {
    TrackerError::InvalidWorkspace { reason }
}
