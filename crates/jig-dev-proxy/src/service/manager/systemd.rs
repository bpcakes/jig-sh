use serde_json::{Value, json};

#[cfg(target_os = "linux")]
use super::command::{
    SERVICE_STATUS_COMMAND_TIMEOUT, command_output_json_with_timeout, command_status_json,
};
use super::{
    ServiceManagerStatus,
    command::{command_completed_without_error_or_timeout, command_succeeded},
};

#[cfg(target_os = "linux")]
pub(super) fn load_service() -> Value {
    let reload_args = vec!["--user".to_string(), "daemon-reload".to_string()];
    let reload = command_status_json("systemctl", &reload_args);
    let enable_args = vec![
        "--user".to_string(),
        "enable".to_string(),
        "--now".to_string(),
        "jig-proxy.service".to_string(),
    ];
    let enable = command_status_json("systemctl", &enable_args);
    json!({
        "ok": reload["ok"].as_bool().unwrap_or(false)
            && enable["ok"].as_bool().unwrap_or(false),
        "daemon_reload": reload,
        "enable_now": enable,
    })
}

#[cfg(target_os = "linux")]
pub(super) fn unload_service() -> Value {
    let disable_args = vec![
        "--user".to_string(),
        "disable".to_string(),
        "--now".to_string(),
        "jig-proxy.service".to_string(),
    ];
    let disable = command_status_json("systemctl", &disable_args);
    json!({
        "ok": disable["ok"].as_bool().unwrap_or(false),
        "disable_now": disable,
    })
}

#[cfg(target_os = "linux")]
pub(super) fn reload_after_remove_service() -> Value {
    let reload_args = vec!["--user".to_string(), "daemon-reload".to_string()];
    let reload = command_status_json("systemctl", &reload_args);
    json!({
        "ok": reload["ok"].as_bool().unwrap_or(false),
        "daemon_reload": reload,
    })
}

#[cfg(target_os = "linux")]
pub(super) fn service_manager_status() -> ServiceManagerStatus {
    let show_args = vec![
        "--user".to_string(),
        "show".to_string(),
        "jig-proxy.service".to_string(),
        "--property=LoadState".to_string(),
        "--value".to_string(),
    ];
    let show =
        command_output_json_with_timeout("systemctl", &show_args, SERVICE_STATUS_COMMAND_TIMEOUT);
    let enabled_args = vec![
        "--user".to_string(),
        "is-enabled".to_string(),
        "jig-proxy.service".to_string(),
    ];
    let enabled = command_output_json_with_timeout(
        "systemctl",
        &enabled_args,
        SERVICE_STATUS_COMMAND_TIMEOUT,
    );
    let active_args = vec![
        "--user".to_string(),
        "is-active".to_string(),
        "jig-proxy.service".to_string(),
    ];
    let active =
        command_output_json_with_timeout("systemctl", &active_args, SERVICE_STATUS_COMMAND_TIMEOUT);
    systemd_manager_status_from_outputs(show, enabled, active)
}

#[cfg(any(target_os = "linux", test))]
pub(in crate::service) fn systemd_manager_status_from_outputs(
    show: Value,
    enabled: Value,
    active: Value,
) -> ServiceManagerStatus {
    let load_state = systemd_load_state(&show);
    let enabled_state = systemd_enabled_state(&enabled);
    let active_state = systemd_active_state(&active);
    let manager_error = systemd_manager_error(load_state, enabled_state);
    let manager_ok = load_state.is_some()
        && enabled_state.is_some()
        && active_state.is_some()
        && manager_error.is_none();
    let loaded = load_state == Some(SystemdLoadState::Loaded);
    let is_enabled = enabled_state.is_some_and(SystemdEnabledState::is_enabled);
    let running = active_state.is_some_and(SystemdActiveState::is_manager_owned);
    let mut value = json!({
        "ok": manager_ok,
        "loaded": loaded,
        "enabled": is_enabled,
        "running": running,
        "show": show,
        "is_enabled": enabled,
        "is_active": active,
        "load_state": load_state.map(SystemdLoadState::as_str),
        "enabled_state": enabled_state.map(SystemdEnabledState::as_str),
        "active_state": active_state.map(SystemdActiveState::as_str),
    });
    if let Some(error) = manager_error {
        value["error"] = json!(error);
    }
    ServiceManagerStatus::from_value(value)
}

#[cfg(any(target_os = "linux", test))]
fn systemd_manager_error(
    load_state: Option<SystemdLoadState>,
    enabled_state: Option<SystemdEnabledState>,
) -> Option<String> {
    match load_state {
        Some(SystemdLoadState::BadSetting) => Some(
            "systemd reports jig-proxy.service LoadState=bad-setting. Inspect the unit with `systemctl --user status jig-proxy.service` and reinstall with `jig proxy service install --accept-service-scope`.".to_string(),
        ),
        Some(SystemdLoadState::Error) => Some(
            "systemd reports jig-proxy.service LoadState=error. Inspect the unit with `systemctl --user status jig-proxy.service` and reinstall with `jig proxy service install --accept-service-scope`.".to_string(),
        ),
        _ if enabled_state == Some(SystemdEnabledState::Bad) => Some(
            "systemd reports jig-proxy.service is-enabled=bad. Inspect the unit with `systemctl --user status jig-proxy.service` and reinstall with `jig proxy service install --accept-service-scope`.".to_string(),
        ),
        _ => None,
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemdLoadState {
    Loaded,
    NotFound,
    Masked,
    BadSetting,
    Error,
    Merged,
    Stub,
}

#[cfg(any(target_os = "linux", test))]
impl SystemdLoadState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "loaded" => Some(Self::Loaded),
            "not-found" => Some(Self::NotFound),
            "masked" => Some(Self::Masked),
            "bad-setting" => Some(Self::BadSetting),
            "error" => Some(Self::Error),
            "merged" => Some(Self::Merged),
            "stub" => Some(Self::Stub),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Loaded => "loaded",
            Self::NotFound => "not-found",
            Self::Masked => "masked",
            Self::BadSetting => "bad-setting",
            Self::Error => "error",
            Self::Merged => "merged",
            Self::Stub => "stub",
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemdEnabledState {
    Enabled,
    EnabledRuntime,
    Linked,
    LinkedRuntime,
    Alias,
    Masked,
    MaskedRuntime,
    Static,
    Disabled,
    Indirect,
    Generated,
    Transient,
    Bad,
    NotFound,
}

#[cfg(any(target_os = "linux", test))]
impl SystemdEnabledState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "enabled" => Some(Self::Enabled),
            "enabled-runtime" => Some(Self::EnabledRuntime),
            "linked" => Some(Self::Linked),
            "linked-runtime" => Some(Self::LinkedRuntime),
            "alias" => Some(Self::Alias),
            "masked" => Some(Self::Masked),
            "masked-runtime" => Some(Self::MaskedRuntime),
            "static" => Some(Self::Static),
            "disabled" => Some(Self::Disabled),
            "indirect" => Some(Self::Indirect),
            "generated" => Some(Self::Generated),
            "transient" => Some(Self::Transient),
            "bad" => Some(Self::Bad),
            "not-found" => Some(Self::NotFound),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::EnabledRuntime => "enabled-runtime",
            Self::Linked => "linked",
            Self::LinkedRuntime => "linked-runtime",
            Self::Alias => "alias",
            Self::Masked => "masked",
            Self::MaskedRuntime => "masked-runtime",
            Self::Static => "static",
            Self::Disabled => "disabled",
            Self::Indirect => "indirect",
            Self::Generated => "generated",
            Self::Transient => "transient",
            Self::Bad => "bad",
            Self::NotFound => "not-found",
        }
    }

    const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled | Self::EnabledRuntime)
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemdActiveState {
    Active,
    Reloading,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Maintenance,
    Unknown,
}

#[cfg(any(target_os = "linux", test))]
impl SystemdActiveState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "reloading" => Some(Self::Reloading),
            "inactive" => Some(Self::Inactive),
            "failed" => Some(Self::Failed),
            "activating" => Some(Self::Activating),
            "deactivating" => Some(Self::Deactivating),
            "maintenance" => Some(Self::Maintenance),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Reloading => "reloading",
            Self::Inactive => "inactive",
            Self::Failed => "failed",
            Self::Activating => "activating",
            Self::Deactivating => "deactivating",
            Self::Maintenance => "maintenance",
            Self::Unknown => "unknown",
        }
    }

    const fn is_manager_owned(self) -> bool {
        matches!(self, Self::Active | Self::Reloading | Self::Activating)
    }
}

#[cfg(any(target_os = "linux", test))]
fn systemd_load_state(output: &Value) -> Option<SystemdLoadState> {
    if !command_succeeded(output) {
        return None;
    }
    systemd_state_from_stdout(output, SystemdLoadState::parse)
}

#[cfg(any(target_os = "linux", test))]
fn systemd_enabled_state(output: &Value) -> Option<SystemdEnabledState> {
    if !command_completed_without_error_or_timeout(output) {
        return None;
    }
    let state = systemd_state_from_stdout_or_stderr(output, SystemdEnabledState::parse)
        .or_else(|| systemd_absent_unit_enabled_state(output))?;
    if state.is_enabled() && !command_succeeded(output) {
        return None;
    }
    Some(state)
}

#[cfg(any(target_os = "linux", test))]
fn systemd_active_state(output: &Value) -> Option<SystemdActiveState> {
    if !command_completed_without_error_or_timeout(output) {
        return None;
    }
    let state = systemd_state_from_stdout_or_stderr(output, SystemdActiveState::parse)?;
    if state == SystemdActiveState::Active && !command_succeeded(output) {
        return None;
    }
    Some(state)
}

#[cfg(any(target_os = "linux", test))]
fn systemd_state_from_stdout<T>(output: &Value, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
    systemd_single_state_text(output["stdout"].as_str().unwrap_or_default()).and_then(parse)
}

#[cfg(any(target_os = "linux", test))]
fn systemd_state_from_stdout_or_stderr<T>(
    output: &Value,
    parse: impl Fn(&str) -> Option<T> + Copy,
) -> Option<T> {
    systemd_state_from_stdout(output, parse).or_else(|| {
        systemd_single_state_text(output["stderr"].as_str().unwrap_or_default()).and_then(parse)
    })
}

#[cfg(any(target_os = "linux", test))]
fn systemd_absent_unit_enabled_state(output: &Value) -> Option<SystemdEnabledState> {
    let stdout = output["stdout"].as_str().unwrap_or_default();
    let stderr = output["stderr"].as_str().unwrap_or_default();
    (systemd_enabled_output_means_unit_not_found(stdout)
        || systemd_enabled_output_means_unit_not_found(stderr))
    .then_some(SystemdEnabledState::NotFound)
}

#[cfg(any(target_os = "linux", test))]
fn systemd_enabled_output_means_unit_not_found(text: &str) -> bool {
    let text = text.trim();
    text.contains("jig-proxy.service")
        && (text.contains("Failed to get unit file state")
            || text.contains("Unit file")
            || text.contains("unit file"))
        && (text.contains("No such file or directory")
            || text.contains("does not exist")
            || text.contains("not found"))
}

#[cfg(any(target_os = "linux", test))]
fn systemd_single_state_text(text: &str) -> Option<&str> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let state = lines.next()?;
    if lines.next().is_none() {
        Some(state)
    } else {
        None
    }
}
