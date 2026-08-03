mod command;
#[cfg(any(target_os = "macos", test))]
mod launchctl;
#[cfg(any(target_os = "linux", test))]
mod systemd;

use std::path::Path;

use serde_json::Value;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
pub(super) use command::{
    command_output_from_command_with_timeout, command_status_from_command_with_timeout,
};
#[cfg(all(test, target_os = "macos"))]
pub(super) use launchctl::macos_unload_service_with_domain;
#[cfg(test)]
pub(super) use launchctl::{
    launchctl_output_means_not_loaded, launchctl_print_manager_status,
    launchctl_print_state_is_running,
};
#[cfg(test)]
pub(super) use systemd::systemd_manager_status_from_outputs;

#[derive(Clone, Debug)]
pub(super) struct ServiceManagerStatus {
    pub(super) value: Value,
    pub(super) ok: bool,
    pub(super) loaded: bool,
    pub(super) enabled: bool,
    pub(super) running: bool,
}

impl ServiceManagerStatus {
    pub(super) fn from_value(value: Value) -> Self {
        let loaded = value["loaded"].as_bool().unwrap_or(false);
        Self {
            ok: value["ok"].as_bool().unwrap_or(false),
            loaded,
            enabled: value["enabled"].as_bool().unwrap_or(loaded),
            running: value["running"].as_bool().unwrap_or(false),
            value,
        }
    }
}

pub(super) fn load_service(path: &Path) -> Value {
    #[cfg(target_os = "macos")]
    {
        launchctl::load_service(path)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        systemd::load_service()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        serde_json::json!({ "ok": false, "unsupported": true })
    }
}

pub(super) fn unload_service(path: &Path) -> Value {
    #[cfg(target_os = "macos")]
    {
        launchctl::unload_service(path)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        systemd::unload_service()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        serde_json::json!({ "ok": false, "unsupported": true })
    }
}

pub(super) fn reload_after_remove_service(path: &Path) -> Value {
    #[cfg(target_os = "macos")]
    {
        launchctl::reload_after_remove_service(path)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        systemd::reload_after_remove_service()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        serde_json::json!({ "ok": false, "unsupported": true })
    }
}

pub(super) fn service_manager_status(path: &Path) -> ServiceManagerStatus {
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        launchctl::service_manager_status()
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        systemd::service_manager_status()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        ServiceManagerStatus::from_value(serde_json::json!({
            "ok": false,
            "unsupported": true,
            "loaded": false,
            "enabled": false,
            "running": false,
        }))
    }
}
