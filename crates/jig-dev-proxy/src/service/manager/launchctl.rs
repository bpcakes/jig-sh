#[cfg(target_os = "macos")]
use std::path::Path;

#[cfg(target_os = "macos")]
use anyhow::{Result, bail};
use serde_json::{Value, json};

#[cfg(target_os = "macos")]
use super::command::{
    SERVICE_STATUS_COMMAND_TIMEOUT, command_output_json, command_output_json_with_timeout,
    command_status_json,
};
use super::{
    ServiceManagerStatus,
    command::{command_completed_without_error_or_timeout, command_succeeded},
};

#[cfg(target_os = "macos")]
pub(super) fn load_service(path: &Path) -> Value {
    let domain = match launchctl_domain() {
        Ok(domain) => domain,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    let bootout_args = vec![
        "bootout".to_string(),
        domain.clone(),
        path.to_string_lossy().into_owned(),
    ];
    let bootout = launchctl_bootout_json(&bootout_args);
    let bootstrap_args = vec![
        "bootstrap".to_string(),
        domain,
        path.to_string_lossy().into_owned(),
    ];
    let bootstrap = command_status_json("launchctl", &bootstrap_args);
    json!({
        "ok": bootstrap["ok"].as_bool().unwrap_or(false),
        "bootout": bootout,
        "bootstrap": bootstrap,
    })
}

#[cfg(target_os = "macos")]
pub(super) fn unload_service(path: &Path) -> Value {
    let domain = match launchctl_domain() {
        Ok(domain) => domain,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    macos_unload_service_with_domain(path, domain)
}

#[cfg(target_os = "macos")]
pub(in crate::service) fn macos_unload_service_with_domain(path: &Path, domain: String) -> Value {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let label_args = vec!["bootout".to_string(), format!("{domain}/sh.jig.proxy")];
            let bootout = launchctl_bootout_json(&label_args);
            json!({
                "ok": bootout["ok"].as_bool().unwrap_or(false),
                "missing_file_label_bootout": true,
                "bootout": bootout,
            })
        }
        Err(error) => json!({
            "ok": false,
            "error": format!(
                "Failed to inspect Jig proxy service file {}: {error}",
                path.display()
            ),
        }),
        Ok(metadata) if metadata.file_type().is_symlink() => json!({
            "ok": false,
            "error": format!(
                "Refusing to unload Jig proxy service file {} because it is a symlink. Remove the symlink after confirming the service is disabled.",
                path.display()
            ),
        }),
        Ok(_) => {
            let bootout_args = vec![
                "bootout".to_string(),
                domain,
                path.to_string_lossy().into_owned(),
            ];
            launchctl_bootout_json(&bootout_args)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn reload_after_remove_service(_path: &Path) -> Value {
    json!({ "ok": true, "skipped": true })
}

#[cfg(target_os = "macos")]
pub(super) fn service_manager_status() -> ServiceManagerStatus {
    let domain = match launchctl_domain() {
        Ok(domain) => domain,
        Err(error) => {
            return ServiceManagerStatus::from_value(json!({
                "ok": false,
                "error": error.to_string(),
                "loaded": false,
                "enabled": false,
                "running": false,
            }));
        }
    };
    let print_args = vec!["print".to_string(), format!("{domain}/sh.jig.proxy")];
    let print =
        command_output_json_with_timeout("launchctl", &print_args, SERVICE_STATUS_COMMAND_TIMEOUT);
    launchctl_print_manager_status(print)
}

#[cfg(any(target_os = "macos", test))]
pub(in crate::service) fn launchctl_print_manager_status(print: Value) -> ServiceManagerStatus {
    if command_succeeded(&print) {
        let running = print["stdout"]
            .as_str()
            .is_some_and(launchctl_print_state_is_running);
        return ServiceManagerStatus::from_value(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": running,
            "print": print,
        }));
    }

    if command_completed_without_error_or_timeout(&print)
        && launchctl_output_means_not_loaded(&print)
    {
        return ServiceManagerStatus::from_value(json!({
            "ok": true,
            "loaded": false,
            "enabled": false,
            "running": false,
            "print": print,
        }));
    }

    ServiceManagerStatus::from_value(json!({
        "ok": false,
        "loaded": false,
        "enabled": false,
        "running": false,
        "print": print,
    }))
}

#[cfg(target_os = "macos")]
fn launchctl_domain() -> Result<String> {
    let uid = unsafe {
        // SAFETY: geteuid takes no pointers and has no preconditions.
        libc::geteuid()
    };
    if uid == 0 {
        bail!("Jig proxy user services must be managed as the login user, not with sudo/root.");
    }
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
fn launchctl_bootout_json(args: &[String]) -> Value {
    let output = command_output_json("launchctl", args);
    if output["ok"].as_bool().unwrap_or(false) || !launchctl_output_means_not_loaded(&output) {
        return output;
    }
    json!({
        "ok": true,
        "status": output["status"].clone(),
        "skipped_not_loaded": true,
        "stdout": output["stdout"].clone(),
        "stderr": output["stderr"].clone(),
    })
}

#[cfg(any(target_os = "macos", test))]
pub(in crate::service) fn launchctl_output_means_not_loaded(output: &Value) -> bool {
    let text = format!(
        "{}\n{}",
        output["stdout"].as_str().unwrap_or_default(),
        output["stderr"].as_str().unwrap_or_default()
    )
    .to_ascii_lowercase();
    text.contains("no such process")
        || text.contains("not loaded")
        || text.contains("could not find service")
        || text.contains("service is not loaded")
}

#[cfg(any(target_os = "macos", test))]
pub(in crate::service) fn launchctl_print_state_is_running(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("state = running"))
}
