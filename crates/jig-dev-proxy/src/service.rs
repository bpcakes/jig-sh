mod file;
mod lifecycle;
mod manager;
mod status;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::state::StateStore;
use crate::types::ProxySettings;

use file::{
    ensure_service_parent_directory_is_safe, prepare_service_parent_directory, service_body,
    service_file_present, service_path,
};
use lifecycle::unload_and_remove_service;
use manager::{
    ServiceManagerStatus, load_service, reload_after_remove_service, service_manager_status,
    unload_service,
};
use status::{
    privileged_port_note, service_reload_hint, service_status_snapshot,
    status_if_installed_for_path,
};

#[cfg(test)]
pub(crate) use status::ServiceRestartRisk;
pub(crate) use status::ServiceStatusSnapshot;

pub(crate) fn install(
    settings: &ProxySettings,
    current_exe: PathBuf,
    repo_root: PathBuf,
    accept_service_scope: bool,
) -> Result<Value> {
    if !accept_service_scope {
        bail!(
            "Refusing to install the Jig proxy service without --accept-service-scope. This command writes and loads a per-user service for the local development proxy."
        );
    }
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let service_path = service_path()?;
    write_and_load_service(
        settings,
        &store,
        &current_exe,
        &repo_root,
        &service_path,
        load_service,
    )
}

fn write_and_load_service(
    settings: &ProxySettings,
    store: &StateStore,
    current_exe: &Path,
    repo_root: &Path,
    service_path: &Path,
    load: impl FnOnce(&Path) -> Value,
) -> Result<Value> {
    if let Some(parent) = service_path.parent() {
        prepare_service_parent_directory(parent)?;
    }
    let body = service_body(settings, store, current_exe, repo_root)?;
    let file_written = file::write_service_file_if_safe(service_path, &body)?;
    if let Some(parent) = service_path.parent() {
        ensure_service_parent_directory_is_safe(parent)?;
    }
    let load = load(service_path);
    let loaded = load["ok"].as_bool().unwrap_or(false);
    let file_present = service_file_present(service_path)?;
    Ok(json!({
        "ok": loaded,
        "installed": file_present && loaded,
        "file_present": file_present,
        "file_written": file_written,
        "load": load,
        "path": service_path,
        "state_dir": store.root(),
        "repo_root": repo_root,
        "log_path": store.log_path(),
        "note": service_reload_hint(),
        "privileged_port_note": privileged_port_note(settings),
    }))
}

pub(crate) fn uninstall(settings: &ProxySettings) -> Result<Value> {
    let _ = settings;
    let path = service_path()?;
    unload_and_remove_service(
        &path,
        service_manager_status,
        unload_service,
        reload_after_remove_service,
    )
}

pub(crate) fn status(settings: &ProxySettings) -> Result<Value> {
    let path = service_path()?;
    status_for_path(settings, &path, service_manager_status)
}

fn status_for_path(
    settings: &ProxySettings,
    path: &Path,
    service_manager_status: impl FnOnce(&Path) -> ServiceManagerStatus,
) -> Result<Value> {
    let state_dir = crate::resolve_state_dir(settings.state_dir.clone())?;
    Ok(service_status_snapshot(settings, &state_dir, path, service_manager_status)?.into_value())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn status_if_installed_for_state_dir(
    settings: &ProxySettings,
    state_dir: &Path,
) -> Result<Option<ServiceStatusSnapshot>> {
    let path = service_path()?;
    status_if_installed_for_path(settings, state_dir, &path, service_manager_status)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn status_if_installed_for_state_dir(
    _settings: &ProxySettings,
    _state_dir: &Path,
) -> Result<Option<ServiceStatusSnapshot>> {
    Ok(None)
}

#[cfg(test)]
mod tests;
