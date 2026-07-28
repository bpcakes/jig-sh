use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::file::service_file_present;
use super::manager::ServiceManagerStatus;
use super::status::service_reload_hint;

pub(super) fn unload_and_remove_service(
    path: &Path,
    manager_status: impl FnOnce(&Path) -> ServiceManagerStatus,
    unload: impl FnOnce(&Path) -> Value,
    reload_after_remove: impl FnOnce(&Path) -> Value,
) -> Result<Value> {
    let existed = service_file_present(path)?;
    if !existed {
        let service = manager_status(path);
        return Ok(unload_missing_service_file(
            path,
            service,
            unload,
            reload_after_remove,
        ));
    }
    let unload = unload(path);
    let unload_ok = unload["ok"].as_bool().unwrap_or(false);
    if !unload_ok {
        return Ok(json!({
            "ok": false,
            "installed": true,
            "removed": false,
            "unload": unload,
            "path": path,
            "note": service_reload_hint(),
        }));
    }
    fs::remove_file(path)
        .with_context(|| format!("Failed to remove Jig proxy service file {}", path.display()))?;
    let reload = reload_after_remove(path);
    let reload_ok = reload["ok"].as_bool().unwrap_or(false);
    Ok(json!({
        "ok": reload_ok,
        "installed": false,
        "removed": true,
        "unload": unload,
        "reload": reload,
        "path": path,
        "note": service_reload_hint(),
    }))
}

fn unload_missing_service_file(
    path: &Path,
    service: ServiceManagerStatus,
    unload: impl FnOnce(&Path) -> Value,
    reload_after_remove: impl FnOnce(&Path) -> Value,
) -> Value {
    let manager_active = service_manager_active(&service);
    if !manager_active && service.ok {
        return json!({
            "ok": true,
            "installed": false,
            "removed": false,
            "file_present": false,
            "unload": {
                "ok": true,
                "skipped": true,
                "reason": "service file is absent and service manager reports the unit inactive",
            },
            "reload": {
                "ok": true,
                "skipped": true,
                "reason": "service file is absent and service manager reports the unit inactive",
            },
            "service": service.value,
            "path": path,
            "note": service_reload_hint(),
        });
    }

    let service_ok = service.ok;
    let service_value = service.value;
    let unload = unload(path);
    let unload_ok = unload["ok"].as_bool().unwrap_or(false);
    let reload = if unload_ok {
        reload_after_remove(path)
    } else {
        json!({
            "ok": false,
            "skipped": true,
            "skipped_unload_failed": true,
        })
    };
    let reload_ok = reload["ok"].as_bool().unwrap_or(false);
    let status_uncertain = !service_ok;
    json!({
        "ok": service_ok && unload_ok && reload_ok,
        "installed": false,
        "removed": false,
        "file_present": false,
        "unload": unload,
        "reload": reload,
        "service": service_value,
        "status_uncertain": status_uncertain,
        "path": path,
        "warning": missing_service_file_uninstall_warning(
            true,
            status_uncertain,
            unload_ok,
            reload_ok,
        ),
        "note": service_reload_hint(),
    })
}

const fn service_manager_active(service: &ServiceManagerStatus) -> bool {
    service.loaded || service.enabled || service.running
}

const fn missing_service_file_uninstall_warning(
    attempted_unload: bool,
    status_uncertain: bool,
    unload_ok: bool,
    reload_ok: bool,
) -> Option<&'static str> {
    if status_uncertain {
        return Some(
            "Jig proxy service file is missing, and Jig could not confidently inspect service manager state. Verify with `jig proxy service status` before assuming the service is disabled.",
        );
    }
    if !attempted_unload {
        return None;
    }
    if !unload_ok {
        return Some(
            "Jig proxy service file is missing, and the service manager reported an active unit that Jig could not unload. Disable the Jig proxy service manually and re-run `jig proxy service status`.",
        );
    }
    if !reload_ok {
        return Some(
            "Jig proxy service file is missing, and Jig unloaded an active unit but could not reload the service manager. Re-run `jig proxy service status` to verify the unit is gone.",
        );
    }
    None
}
