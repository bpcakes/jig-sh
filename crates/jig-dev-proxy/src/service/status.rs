use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};

use super::file::{installed_service_state_dir, service_file_present};
use super::manager::ServiceManagerStatus;
use crate::types::ProxySettings;

const SERVICE_STATE_DIR_METADATA_ERROR: &str = "Jig proxy service file does not declare a trusted JIG_PROXY_STATE_DIR. Reinstall the proxy service with `jig proxy service install --accept-service-scope` to refresh its metadata.";

#[derive(Clone, Debug)]
pub(crate) struct ServiceStatusSnapshot {
    value: Value,
    restart_risk: ServiceRestartRisk,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServiceRestartRisk {
    None,
    MayRestart,
    Uncertain,
}

impl ServiceStatusSnapshot {
    pub(crate) fn uncertain(error: anyhow::Error) -> Self {
        Self {
            value: json!({
                "ok": false,
                "error": error.to_string(),
            }),
            restart_risk: ServiceRestartRisk::Uncertain,
        }
    }

    pub(crate) const fn from_parts(value: Value, restart_risk: ServiceRestartRisk) -> Self {
        Self {
            value,
            restart_risk,
        }
    }

    pub(crate) const fn json_value(&self) -> &Value {
        &self.value
    }

    pub(crate) fn into_value(self) -> Value {
        self.value
    }

    pub(crate) const fn restart_risk(&self) -> ServiceRestartRisk {
        self.restart_risk
    }

    pub(crate) fn keepalive_active(&self) -> bool {
        self.restart_risk() == ServiceRestartRisk::MayRestart
    }

    pub(crate) fn is_uncertain(&self) -> bool {
        self.restart_risk() == ServiceRestartRisk::Uncertain
    }

    pub(crate) fn degrades_stop_ok(&self) -> bool {
        // Fail closed when service status is unknown: `jig proxy stop` cannot
        // confirm the proxy will stay down if a user service may restart it.
        self.restart_risk() != ServiceRestartRisk::None
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn status_if_installed_for_path(
    settings: &ProxySettings,
    state_dir: &Path,
    path: &Path,
    manager_status: impl FnOnce(&Path) -> ServiceManagerStatus,
) -> Result<Option<ServiceStatusSnapshot>> {
    let snapshot = service_status_snapshot(settings, state_dir, path, manager_status)?;
    let file_present = snapshot.value["file_present"].as_bool().unwrap_or(false);
    let manager_active = snapshot.value["service"]["loaded"]
        .as_bool()
        .unwrap_or(false)
        || snapshot.value["service"]["enabled"]
            .as_bool()
            .unwrap_or(false)
        || snapshot.value["service"]["running"]
            .as_bool()
            .unwrap_or(false);
    if !file_present && !manager_active && !snapshot.is_uncertain() {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub(super) fn service_status_snapshot(
    settings: &ProxySettings,
    state_dir: &Path,
    path: &Path,
    manager_status: impl FnOnce(&Path) -> ServiceManagerStatus,
) -> Result<ServiceStatusSnapshot> {
    let file_present = service_file_present(path)?;
    let service_state_dir = if file_present {
        match installed_service_state_dir(path) {
            Ok(service_state_dir) => service_state_dir,
            Err(error) => {
                let service = manager_status(path);
                let error = error.to_string();
                return Ok(ServiceStatusSnapshot {
                    value: json!({
                        "ok": false,
                        "error": error,
                        "installed": false,
                        "may_restart_proxy": false,
                        "file_present": file_present,
                        "path": path,
                        "state_dir": state_dir,
                        "platform": std::env::consts::OS,
                        "service": service.value,
                        "service_state_dir": null,
                        "service_state_dir_matches": false,
                        "service_state_dir_error": error,
                        "privileged_port_note": privileged_port_note(settings),
                    }),
                    restart_risk: ServiceRestartRisk::Uncertain,
                });
            }
        }
    } else {
        None
    };
    let service_state_dir_match = service_state_dir
        .as_deref()
        .map_or(ServiceStateDirMatch::MissingMetadata, |service_state_dir| {
            service_state_dir_match(service_state_dir, state_dir)
        });
    let service_state_dir_matches = service_state_dir_match == ServiceStateDirMatch::Match;
    let service = manager_status(path);
    let manager_active = service_manager_active(&service);
    let restart_candidate = if file_present {
        service.loaded && (service.enabled || service.running)
    } else {
        manager_active
    };
    let service_state_dir_error =
        service_state_dir_error(&service_state_dir_match, restart_candidate);
    let installed = file_present && service.loaded && service.enabled && service_state_dir_matches;
    let may_restart_proxy = file_present
        && service.loaded
        && (service.enabled || service.running)
        && service_state_dir_matches;
    let uncertain = !service.ok
        || (!matches!(service_state_dir_match, ServiceStateDirMatch::Mismatch)
            && restart_candidate
            && !service_state_dir_matches);
    let value = json!({
        "ok": service.ok && service_state_dir_error.is_none(),
        "error": service_state_dir_error,
        "installed": installed,
        "may_restart_proxy": may_restart_proxy,
        "file_present": file_present,
        "path": path,
        "state_dir": state_dir,
        "platform": std::env::consts::OS,
        "service": service.value,
        "service_state_dir": service_state_dir,
        "service_state_dir_matches": service_state_dir_matches,
        "service_state_dir_error": service_state_dir_error,
        "privileged_port_note": privileged_port_note(settings),
    });
    let restart_risk = if uncertain {
        ServiceRestartRisk::Uncertain
    } else if may_restart_proxy {
        ServiceRestartRisk::MayRestart
    } else {
        ServiceRestartRisk::None
    };
    Ok(ServiceStatusSnapshot::from_parts(value, restart_risk))
}

#[cfg(test)]
pub(super) fn service_status_value(
    settings: &ProxySettings,
    state_dir: &Path,
    path: &Path,
    manager_status: impl FnOnce(&Path) -> ServiceManagerStatus,
) -> Value {
    service_status_snapshot(settings, state_dir, path, manager_status)
        .expect("service status test path should be inspectable")
        .into_value()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ServiceStateDirMatch {
    Match,
    Mismatch,
    MissingMetadata,
    Unknown { reason: String },
}

fn service_state_dir_match(
    service_state_dir: &Path,
    expected_state_dir: &Path,
) -> ServiceStateDirMatch {
    if service_state_dir == expected_state_dir {
        return ServiceStateDirMatch::Match;
    }
    let service_state_dir = match fs::canonicalize(service_state_dir) {
        Ok(path) => path,
        Err(error) => {
            return ServiceStateDirMatch::Unknown {
                reason: format!(
                    "Failed to canonicalize Jig proxy service state dir {} while comparing it with expected state dir {}: {error}",
                    service_state_dir.display(),
                    expected_state_dir.display()
                ),
            };
        }
    };
    let expected_state_dir = match fs::canonicalize(expected_state_dir) {
        Ok(path) => path,
        Err(error) => {
            return ServiceStateDirMatch::Unknown {
                reason: format!(
                    "Failed to canonicalize expected Jig proxy state dir {} while comparing it with service state dir {}: {error}",
                    expected_state_dir.display(),
                    service_state_dir.display()
                ),
            };
        }
    };
    if service_state_dir == expected_state_dir {
        ServiceStateDirMatch::Match
    } else {
        ServiceStateDirMatch::Mismatch
    }
}

fn service_state_dir_error(
    service_state_dir_match: &ServiceStateDirMatch,
    restart_candidate: bool,
) -> Option<String> {
    if !restart_candidate {
        return match service_state_dir_match {
            ServiceStateDirMatch::Unknown { reason } => Some(reason.clone()),
            _ => None,
        };
    }
    match service_state_dir_match {
        ServiceStateDirMatch::Match | ServiceStateDirMatch::Mismatch => None,
        ServiceStateDirMatch::MissingMetadata => Some(SERVICE_STATE_DIR_METADATA_ERROR.into()),
        ServiceStateDirMatch::Unknown { reason } => Some(reason.clone()),
    }
}

const fn service_manager_active(service: &ServiceManagerStatus) -> bool {
    service.loaded || service.enabled || service.running
}

pub(super) const fn service_reload_hint() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Jig attempts to load the LaunchAgent with launchctl. If that fails, run launchctl bootstrap gui/$UID ~/Library/LaunchAgents/sh.jig.proxy.plist."
    }
    #[cfg(target_os = "linux")]
    {
        "Jig attempts systemctl --user daemon-reload and enable --now. If that fails, run systemctl --user daemon-reload && systemctl --user enable --now jig-proxy.service."
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Service management is unsupported on this platform."
    }
}

pub(super) fn privileged_port_note(settings: &ProxySettings) -> Option<&'static str> {
    let uses_privileged =
        settings.http_port < 1024 || settings.https_port.is_some_and(|port| port < 1024);
    if !uses_privileged {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        Some(
            "Ports below 1024 require root or cap_net_bind_service. For a user service, grant the installed jig binary with: sudo setcap 'cap_net_bind_service=+ep' <path-to-jig>.",
        )
    }
    #[cfg(target_os = "macos")]
    {
        Some(
            "Ports below 1024 require a root-owned LaunchDaemon or a local port-forward from 80/443 to an unprivileged Jig proxy port.",
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Some("Ports below 1024 may require elevated privileges on this platform.")
    }
}
