use serde_json::json;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Command;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::{Duration, Instant};
use tempfile::tempdir;

use super::file::*;
use super::lifecycle::*;
use super::manager::*;
use super::status::*;
use super::*;

fn manager_status(value: serde_json::Value) -> ServiceManagerStatus {
    ServiceManagerStatus::from_value(value)
}

fn inactive_manager_status(_: &Path) -> ServiceManagerStatus {
    manager_status(json!({
        "ok": true,
        "loaded": false,
        "enabled": false,
        "running": false,
    }))
}

fn command_output(ok: bool, status: i32, stdout: &str, stderr: &str) -> serde_json::Value {
    json!({
        "ok": ok,
        "status": status,
        "timed_out": false,
        "stdout": stdout,
        "stderr": stderr,
    })
}

fn timed_out_command_output() -> serde_json::Value {
    json!({
        "ok": false,
        "timed_out": true,
        "stdout": "",
        "stderr": "",
    })
}

#[test]
fn install_requires_accept_service_scope() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };

    let error = install(
        &settings,
        PathBuf::from("/tmp/jig"),
        temp.path().join("repo"),
        false,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--accept-service-scope"));
    assert!(!settings.state_dir.as_ref().unwrap().exists());
}

#[test]
fn launchctl_print_state_parser_requires_running_state() {
    assert!(launchctl_print_state_is_running(
        "domain = gui/501\nstate = running\n"
    ));
    assert!(!launchctl_print_state_is_running(
        "domain = gui/501\nstate = waiting\n"
    ));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn service_body_rejects_zero_ports() {
    let temp = tempdir().unwrap();
    let mut settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    settings.http_port = 0;
    let error = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("HTTP port must be greater than 0"));

    settings.http_port = 1355;
    settings.https_port = Some(0);
    let error = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("HTTPS port must be greater than 0"));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn service_body_sets_repo_root_environment() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo root");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();

    assert!(body.contains("JIG_PROXY_STATE_DIR"));
    assert!(body.contains("JIG_REPO_ROOT"));
    assert!(body.contains("WorkingDirectory"));
    assert!(body.contains("proxy.log"));
    assert!(body.contains(&repo.to_string_lossy().to_string()));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn service_status_requires_matching_service_state_dir() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let body = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path()).unwrap();
    fs::write(&service_path, body).unwrap();

    let matching = service_status_value(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": true,
        }))
    });
    let mismatched_state_dir = temp.path().join("other-state");
    fs::create_dir_all(&mismatched_state_dir).unwrap();
    let mismatched = service_status_value(&settings, &mismatched_state_dir, &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": true,
        }))
    });

    assert_eq!(matching["installed"].as_bool(), Some(true));
    assert_eq!(matching["may_restart_proxy"].as_bool(), Some(true));
    assert_eq!(matching["service_state_dir_matches"].as_bool(), Some(true));
    assert_eq!(mismatched["installed"].as_bool(), Some(false));
    assert_eq!(mismatched["may_restart_proxy"].as_bool(), Some(false));
    assert_eq!(mismatched["ok"].as_bool(), Some(true));
    assert_eq!(
        mismatched["service_state_dir_matches"].as_bool(),
        Some(false)
    );
    assert!(mismatched["service_state_dir_error"].is_null());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn service_status_canonicalizes_service_state_dir_before_matching() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let aliased_state_dir = store
        .root()
        .parent()
        .unwrap()
        .join(".")
        .join(store.root().file_name().unwrap());
    fs::write(
        &service_path,
        service_body_with_state_dir(&aliased_state_dir),
    )
    .unwrap();

    let output = service_status_value(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": true,
        }))
    });

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["installed"].as_bool(), Some(true));
    assert_eq!(output["may_restart_proxy"].as_bool(), Some(true));
    assert_eq!(output["service_state_dir_matches"].as_bool(), Some(true));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn service_status_reports_canonicalization_failure_details() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let missing_service_state_dir = temp.path().join("missing-state");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    fs::write(
        &service_path,
        service_body_with_state_dir(&missing_service_state_dir),
    )
    .unwrap();

    let snapshot = service_status_snapshot(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": true,
        }))
    })
    .unwrap();

    let value = snapshot.json_value();
    assert!(!snapshot.keepalive_active());
    assert!(snapshot.is_uncertain());
    assert_eq!(value["ok"].as_bool(), Some(false));
    assert_eq!(value["installed"].as_bool(), Some(false));
    assert_eq!(value["may_restart_proxy"].as_bool(), Some(false));
    let error = value["service_state_dir_error"].as_str().unwrap();
    assert!(error.contains(&missing_service_state_dir.display().to_string()));
    assert!(error.contains("Failed to canonicalize"));
}

#[cfg(unix)]
#[test]
fn service_status_snapshot_diagnoses_dangling_symlink_service_file() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let missing_target = temp.path().join("missing.service");
    std::os::unix::fs::symlink(&missing_target, &service_path).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let snapshot = service_status_snapshot(&settings, store.root(), &service_path, |_| {
        inactive_manager_status(Path::new(""))
    })
    .unwrap();

    let value = snapshot.json_value();
    assert_eq!(value["ok"].as_bool(), Some(false));
    assert_eq!(value["file_present"].as_bool(), Some(true));
    assert_eq!(value["installed"].as_bool(), Some(false));
    assert_eq!(value["may_restart_proxy"].as_bool(), Some(false));
    assert_eq!(value["service_state_dir_matches"].as_bool(), Some(false));
    assert!(snapshot.is_uncertain());
    let error = value["service_state_dir_error"].as_str().unwrap();
    assert!(error.contains("Failed to read Jig proxy service file"));
    assert!(error.contains(&service_path.display().to_string()));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn matching_service_file_with_manager_failure_is_uncertain() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let body = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path()).unwrap();
    fs::write(&service_path, body).unwrap();

    let snapshot = service_status_snapshot(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": false,
            "loaded": false,
            "enabled": false,
            "running": false,
            "show": command_output(false, 1, "", "Failed to connect to bus: No medium found"),
        }))
    })
    .unwrap();

    let value = snapshot.json_value();
    assert_eq!(value["ok"].as_bool(), Some(false));
    assert_eq!(value["service"]["ok"].as_bool(), Some(false));
    assert!(snapshot.is_uncertain());
    assert!(snapshot.degrades_stop_ok());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn mismatched_service_file_with_manager_timeout_is_uncertain() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let body = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path()).unwrap();
    fs::write(&service_path, body).unwrap();
    let mismatched_state_dir = temp.path().join("other-state");
    fs::create_dir_all(&mismatched_state_dir).unwrap();

    let snapshot = service_status_snapshot(&settings, &mismatched_state_dir, &service_path, |_| {
        manager_status(json!({
            "ok": false,
            "loaded": false,
            "enabled": false,
            "running": false,
            "is_active": timed_out_command_output(),
        }))
    })
    .unwrap();

    let value = snapshot.json_value();
    assert_eq!(value["ok"].as_bool(), Some(false));
    assert_eq!(value["service"]["ok"].as_bool(), Some(false));
    assert_eq!(value["service_state_dir_matches"].as_bool(), Some(false));
    assert!(snapshot.is_uncertain());
    assert!(snapshot.degrades_stop_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn systemd_running_disabled_matching_service_may_restart_proxy() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let body = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path()).unwrap();
    fs::write(&service_path, body).unwrap();

    let snapshot = service_status_snapshot(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": false,
            "running": true,
        }))
    })
    .unwrap();

    let value = snapshot.json_value();
    assert_eq!(value["ok"].as_bool(), Some(true));
    assert_eq!(value["installed"].as_bool(), Some(false));
    assert_eq!(value["may_restart_proxy"].as_bool(), Some(true));
    assert!(snapshot.keepalive_active());
    assert!(snapshot.degrades_stop_ok());
    assert!(!snapshot.is_uncertain());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn absent_service_file_with_active_manager_is_uncertain() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let snapshot = status_if_installed_for_path(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": true,
        }))
    })
    .unwrap()
    .expect("active manager state without a file should be reported");

    let value = snapshot.json_value();
    assert_eq!(value["file_present"].as_bool(), Some(false));
    assert_eq!(value["ok"].as_bool(), Some(false));
    assert_eq!(value["installed"].as_bool(), Some(false));
    assert_eq!(value["may_restart_proxy"].as_bool(), Some(false));
    assert!(!snapshot.keepalive_active());
    assert!(snapshot.is_uncertain());
    assert!(snapshot.degrades_stop_ok());
    assert!(
        value["service_state_dir_error"]
            .as_str()
            .unwrap()
            .contains("JIG_PROXY_STATE_DIR")
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn absent_service_file_with_inactive_manager_returns_none() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let called = std::cell::Cell::new(false);

    let snapshot = status_if_installed_for_path(&settings, store.root(), &service_path, |_| {
        called.set(true);
        manager_status(json!({
            "ok": true,
            "loaded": false,
            "enabled": false,
            "running": false,
        }))
    })
    .unwrap();

    assert!(called.get());
    assert!(snapshot.is_none());
}

#[test]
fn service_status_with_missing_state_dir_inspects_manager_without_creating_state_dir() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let missing_state_dir = temp.path().join("missing-state");
    let settings = ProxySettings {
        state_dir: Some(missing_state_dir.clone()),
        ..ProxySettings::default()
    };
    let called = std::cell::Cell::new(false);

    let output = status_for_path(&settings, &service_path, |_| {
        called.set(true);
        inactive_manager_status(Path::new(""))
    })
    .unwrap();

    assert!(called.get());
    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(
        output["state_dir"].as_str(),
        Some(missing_state_dir.to_str().unwrap())
    );
    assert!(!missing_state_dir.exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn command_output_with_timeout_reports_timed_out_process() {
    let Some(sleep_path) = fixed_sleep_path() else {
        return;
    };
    let mut command = Command::new(sleep_path);
    command.env_clear().arg("2");

    let started = Instant::now();
    let output = command_output_from_command_with_timeout(command, Duration::from_millis(50));

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["timed_out"].as_bool(), Some(true));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn command_output_with_timeout_returns_promptly_when_grandchild_inherits_stdout() {
    let Some(command) = inherited_descriptor_command() else {
        return;
    };

    let started = Instant::now();
    let output = command_output_from_command_with_timeout(command, Duration::from_millis(50));

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["timed_out"].as_bool(), Some(false));
    assert_eq!(output["stdout"].as_str(), Some("done"));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn command_status_with_timeout_reports_timed_out_process() {
    let Some(sleep_path) = fixed_sleep_path() else {
        return;
    };
    let mut command = Command::new(sleep_path);
    command.env_clear().arg("2");

    let started = Instant::now();
    let output = command_status_from_command_with_timeout(command, Duration::from_millis(50));

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["timed_out"].as_bool(), Some(true));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn command_status_with_timeout_returns_promptly_when_grandchild_inherits_stdout() {
    let Some(command) = inherited_descriptor_command() else {
        return;
    };

    let started = Instant::now();
    let output = command_status_from_command_with_timeout(command, Duration::from_millis(50));

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["timed_out"].as_bool(), Some(false));
    assert_eq!(output["stdout"].as_str(), Some("done"));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn fixed_sleep_path() -> Option<PathBuf> {
    ["/bin/sleep", "/usr/bin/sleep"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.try_exists().unwrap_or(false))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn fixed_sh_path() -> Option<PathBuf> {
    ["/bin/sh", "/usr/bin/sh"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.try_exists().unwrap_or(false))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn inherited_descriptor_command() -> Option<Command> {
    let sh_path = fixed_sh_path()?;
    let sleep_path = fixed_sleep_path()?;
    let mut command = Command::new(sh_path);
    command
        .env_clear()
        .arg("-c")
        .arg(format!("{} 2 & printf done", sleep_path.display()));
    Some(command)
}

#[test]
fn launchctl_print_known_not_loaded_failure_is_safe_inactive() {
    let status = launchctl_print_manager_status(command_output(
        false,
        113,
        "",
        "Could not find service \"sh.jig.proxy\" in domain",
    ));

    assert_eq!(status.value["ok"].as_bool(), Some(true));
    assert!(!status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
}

#[test]
fn launchctl_print_unknown_nonzero_failure_is_manager_error() {
    let status = launchctl_print_manager_status(command_output(
        false,
        5,
        "",
        "Bootstrap failed: 5: Input/output error",
    ));

    assert_eq!(status.value["ok"].as_bool(), Some(false));
    assert!(!status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
}

#[test]
fn systemd_accepts_known_disabled_and_inactive_nonzero_states() {
    let status = systemd_manager_status_from_outputs(
        command_output(true, 0, "loaded", ""),
        command_output(false, 1, "disabled", ""),
        command_output(false, 3, "inactive", ""),
    );

    assert_eq!(status.value["ok"].as_bool(), Some(true));
    assert!(status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
}

#[test]
fn systemd_absent_unit_file_diagnostic_is_safe_inactive() {
    let status = systemd_manager_status_from_outputs(
        command_output(true, 0, "not-found", ""),
        command_output(
            false,
            1,
            "",
            "Failed to get unit file state for jig-proxy.service: No such file or directory",
        ),
        command_output(false, 3, "inactive", ""),
    );

    assert_eq!(status.value["ok"].as_bool(), Some(true));
    assert!(!status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
    assert_eq!(status.value["load_state"].as_str(), Some("not-found"));
    assert_eq!(status.value["enabled_state"].as_str(), Some("not-found"));
    assert_eq!(status.value["active_state"].as_str(), Some("inactive"));
    assert!(
        status.value["is_enabled"]["stderr"]
            .as_str()
            .unwrap()
            .contains("Failed to get unit file state for jig-proxy.service")
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn absent_service_file_with_systemd_absent_unit_returns_none() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let snapshot = status_if_installed_for_path(&settings, store.root(), &service_path, |_| {
        systemd_manager_status_from_outputs(
            command_output(true, 0, "not-found", ""),
            command_output(
                false,
                1,
                "",
                "Failed to get unit file state for jig-proxy.service: No such file or directory",
            ),
            command_output(false, 3, "unknown", ""),
        )
    })
    .unwrap();

    assert!(snapshot.is_none());
}

#[test]
fn systemd_bad_setting_load_state_is_manager_error() {
    let status = systemd_manager_status_from_outputs(
        command_output(true, 0, "bad-setting", ""),
        command_output(false, 1, "disabled", ""),
        command_output(false, 3, "inactive", ""),
    );

    assert_systemd_broken_state(
        &status,
        "load_state",
        "bad-setting",
        "LoadState=bad-setting",
    );
    assert!(!status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
    assert_eq!(status.value["enabled_state"].as_str(), Some("disabled"));
    assert_eq!(status.value["active_state"].as_str(), Some("inactive"));
}

#[test]
fn systemd_error_load_state_is_manager_error() {
    let status = systemd_manager_status_from_outputs(
        command_output(true, 0, "error", ""),
        command_output(false, 1, "disabled", ""),
        command_output(false, 3, "inactive", ""),
    );

    assert_systemd_broken_state(&status, "load_state", "error", "LoadState=error");
    assert!(!status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
    assert_eq!(status.value["enabled_state"].as_str(), Some("disabled"));
    assert_eq!(status.value["active_state"].as_str(), Some("inactive"));
}

#[test]
fn systemd_bad_enabled_state_is_manager_error() {
    let status = systemd_manager_status_from_outputs(
        command_output(true, 0, "loaded", ""),
        command_output(false, 1, "bad", ""),
        command_output(false, 3, "inactive", ""),
    );

    assert_systemd_broken_state(&status, "enabled_state", "bad", "is-enabled=bad");
    assert!(status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
    assert_eq!(status.value["load_state"].as_str(), Some("loaded"));
    assert_eq!(status.value["active_state"].as_str(), Some("inactive"));
}

fn assert_systemd_broken_state(
    status: &ServiceManagerStatus,
    state_key: &str,
    state_value: &str,
    error_fragment: &str,
) {
    assert_eq!(status.value["ok"].as_bool(), Some(false));
    assert_eq!(status.value[state_key].as_str(), Some(state_value));
    assert!(status.value["show"].is_object());
    assert!(status.value["is_enabled"].is_object());
    assert!(status.value["is_active"].is_object());
    let error = status.value["error"].as_str().unwrap();
    assert!(error.contains(error_fragment));
    assert!(error.contains("jig proxy service install --accept-service-scope"));
}

#[test]
fn systemd_rejects_empty_bus_error_outputs() {
    let status = systemd_manager_status_from_outputs(
        command_output(true, 0, "loaded", ""),
        command_output(false, 1, "", "Failed to connect to bus: No medium found"),
        command_output(false, 3, "inactive", ""),
    );

    assert_eq!(status.value["ok"].as_bool(), Some(false));
    assert!(status.loaded);
    assert!(!status.enabled);
    assert!(!status.running);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn systemd_activating_disabled_matching_service_may_restart_proxy() {
    assert_systemd_transient_active_state_may_restart_proxy("activating");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn systemd_reloading_disabled_matching_service_may_restart_proxy() {
    assert_systemd_transient_active_state_may_restart_proxy("reloading");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn assert_systemd_transient_active_state_may_restart_proxy(active_state: &str) {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    fs::write(&service_path, service_body_with_state_dir(store.root())).unwrap();

    let service = systemd_manager_status_from_outputs(
        command_output(true, 0, "loaded", ""),
        command_output(false, 1, "disabled", ""),
        command_output(false, 3, active_state, ""),
    );
    assert_eq!(service.value["ok"].as_bool(), Some(true));
    assert!(service.loaded);
    assert!(!service.enabled);
    assert!(service.running);
    assert_eq!(service.value["active_state"].as_str(), Some(active_state));

    let snapshot =
        service_status_snapshot(&settings, store.root(), &service_path, |_| service).unwrap();

    let value = snapshot.json_value();
    assert_eq!(value["ok"].as_bool(), Some(true));
    assert_eq!(value["installed"].as_bool(), Some(false));
    assert_eq!(value["may_restart_proxy"].as_bool(), Some(true));
    assert!(snapshot.keepalive_active());
    assert!(snapshot.degrades_stop_ok());
    assert!(!snapshot.is_uncertain());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn active_service_without_state_dir_metadata_is_uncertain() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    fs::write(
        &service_path,
        "ExecStart=/tmp/jig proxy start --foreground\n",
    )
    .unwrap();

    let output = service_status_value(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": true,
            "running": true,
        }))
    });

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["installed"].as_bool(), Some(false));
    assert_eq!(output["may_restart_proxy"].as_bool(), Some(false));
    assert_eq!(output["service_state_dir_matches"].as_bool(), Some(false));
    assert!(
        output["service_state_dir_error"]
            .as_str()
            .unwrap()
            .contains("JIG_PROXY_STATE_DIR")
    );
}

#[test]
fn service_state_dir_parsers_decode_generated_values() {
    let state_dir = "/tmp/jig state/#1/%/\"quoted\"/&";
    let plist = format!("<key>JIG_PROXY_STATE_DIR</key>{}", plist_string(state_dir));
    let systemd = format!(
        "Environment={}",
        systemd_quote(&format!("JIG_PROXY_STATE_DIR={state_dir}")).unwrap()
    );

    assert_eq!(plist_service_state_dir(&plist).as_deref(), Some(state_dir));
    assert_eq!(
        systemd_service_state_dir(&systemd).unwrap().as_deref(),
        Some(state_dir)
    );
}

#[cfg(target_os = "macos")]
fn service_body_with_state_dir(state_dir: &Path) -> String {
    format!(
        "<key>JIG_PROXY_STATE_DIR</key>{}",
        plist_string(&state_dir.to_string_lossy())
    )
}

#[cfg(target_os = "linux")]
fn service_body_with_state_dir(state_dir: &Path) -> String {
    format!(
        "Environment={}",
        systemd_quote(&format!("JIG_PROXY_STATE_DIR={}", state_dir.display())).unwrap()
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn service_body_preserves_http2_runtime_setting() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let mut settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();
    assert!(!body.contains("--no-http2"));

    settings.http2 = false;
    let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();
    assert!(body.contains("--no-http2"));
}

#[test]
fn service_temp_paths_are_unique_within_process() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");

    let first = temp_service_path(&service_path);
    let second = temp_service_path(&service_path);

    assert_ne!(first, second);
    assert!(
        first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".tmp")
    );
    assert!(
        second
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".tmp")
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn install_response_reports_load_failure_but_written_file() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let service_path = temp.path().join("jig-proxy.service");

    let output = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": false, "error": "load failed" }),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["installed"].as_bool(), Some(false));
    assert_eq!(output["file_written"].as_bool(), Some(true));
    assert!(service_path.exists());
    assert_eq!(output["load"]["error"].as_str(), Some("load failed"));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn install_refuses_to_overwrite_different_service_file() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    fs::write(&service_path, "custom service").unwrap();

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Refusing to overwrite existing Jig proxy service file"));
    assert_eq!(fs::read_to_string(service_path).unwrap(), "custom service");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn install_refuses_to_reuse_group_writable_service_file() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();
    fs::write(&service_path, body).unwrap();
    fs::set_permissions(&service_path, fs::Permissions::from_mode(0o664)).unwrap();

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("group/world write bits"));
}

#[cfg(unix)]
#[test]
fn install_refuses_to_reuse_symlinked_service_file() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let target = temp.path().join("target.service");
    fs::write(&target, "service").unwrap();
    std::os::unix::fs::symlink(&target, &service_path).unwrap();

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("because it is a symlink"));
}

#[cfg(unix)]
#[test]
fn install_refuses_symlinked_service_parent() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let real_parent = temp.path().join("real-services");
    let linked_parent = temp.path().join("linked-services");
    fs::create_dir_all(&real_parent).unwrap();
    std::os::unix::fs::symlink(&real_parent, &linked_parent).unwrap();
    let service_path = linked_parent.join("jig-proxy.service");

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("symlinked directory"));
    assert!(!real_parent.join("jig-proxy.service").exists());
}

#[cfg(unix)]
#[test]
fn install_refuses_group_writable_service_parent() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let service_parent = temp.path().join("services");
    fs::create_dir_all(&service_parent).unwrap();
    fs::set_permissions(&service_parent, fs::Permissions::from_mode(0o775)).unwrap();
    let service_path = service_parent.join("jig-proxy.service");

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("group/world writable directory"));
    assert!(!service_path.exists());
}

#[cfg(unix)]
#[test]
fn install_refuses_world_writable_service_ancestor() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let shared = temp.path().join("shared");
    fs::create_dir_all(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
    let service_path = shared.join("systemd/user/jig-proxy.service");

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("group/world writable directory"));
    assert!(!service_path.exists());
}

#[cfg(unix)]
#[test]
fn install_refuses_symlinked_service_ancestor() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let real = temp.path().join("real");
    let linked = temp.path().join("linked");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let service_path = linked.join("systemd/user/jig-proxy.service");

    let error = write_and_load_service(
        &settings,
        &store,
        Path::new("/tmp/jig"),
        &repo,
        &service_path,
        |_| json!({ "ok": true }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("symlinked directory"));
    assert!(!real.join("systemd/user/jig-proxy.service").exists());
}

#[test]
fn uninstall_keeps_service_file_when_unload_fails() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    fs::write(&service_path, "service").unwrap();

    let output = unload_and_remove_service(
        &service_path,
        inactive_manager_status,
        |_| json!({ "ok": false, "error": "unload failed" }),
        |_| json!({ "ok": true }),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["removed"].as_bool(), Some(false));
    assert!(service_path.exists());
}

#[test]
fn uninstall_removes_file_only_after_successful_unload_then_reloads() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    fs::write(&service_path, "service").unwrap();

    let output = unload_and_remove_service(
        &service_path,
        inactive_manager_status,
        |_| json!({ "ok": true }),
        |_| json!({ "ok": true, "daemon_reload": { "ok": true } }),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["removed"].as_bool(), Some(true));
    assert_eq!(output["reload"]["ok"].as_bool(), Some(true));
    assert!(!service_path.exists());
}

#[test]
fn uninstall_reports_reload_failure_after_file_removal() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    fs::write(&service_path, "service").unwrap();

    let output = unload_and_remove_service(
        &service_path,
        inactive_manager_status,
        |_| json!({ "ok": true }),
        |_| json!({ "ok": false, "error": "reload failed" }),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["removed"].as_bool(), Some(true));
    assert_eq!(output["installed"].as_bool(), Some(false));
    assert!(!service_path.exists());
}

#[cfg(unix)]
#[test]
fn uninstall_removes_dangling_symlink_after_successful_unload() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let missing_target = temp.path().join("missing.service");
    std::os::unix::fs::symlink(&missing_target, &service_path).unwrap();
    let unloaded = std::cell::Cell::new(false);
    let reloaded = std::cell::Cell::new(false);

    let output = unload_and_remove_service(
        &service_path,
        inactive_manager_status,
        |_| {
            unloaded.set(true);
            json!({ "ok": true })
        },
        |_| {
            reloaded.set(true);
            json!({ "ok": true })
        },
    )
    .unwrap();

    assert!(unloaded.get());
    assert!(reloaded.get());
    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["removed"].as_bool(), Some(true));
    assert_eq!(output["installed"].as_bool(), Some(false));
    assert_eq!(
        fs::symlink_metadata(&service_path).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
}

#[cfg(all(target_os = "macos", unix))]
#[test]
fn macos_unload_service_reports_symlinked_service_file_error() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let missing_target = temp.path().join("missing.service");
    std::os::unix::fs::symlink(&missing_target, &service_path).unwrap();

    let output = macos_unload_service_with_domain(&service_path, "gui/501".to_string());

    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["missing_file_label_bootout"].as_bool(), None);
    let error = output["error"].as_str().unwrap();
    assert!(error.contains("symlink"));
    assert!(error.contains(&service_path.display().to_string()));
}

#[test]
fn uninstall_missing_file_with_active_manager_attempts_unload_and_reload() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let unloaded = std::cell::Cell::new(false);
    let reloaded = std::cell::Cell::new(false);

    let output = unload_and_remove_service(
        &service_path,
        |_| {
            manager_status(json!({
                "ok": true,
                "loaded": true,
                "enabled": true,
                "running": true,
            }))
        },
        |_| {
            unloaded.set(true);
            json!({ "ok": true })
        },
        |_| {
            reloaded.set(true);
            json!({ "ok": true })
        },
    )
    .unwrap();

    assert!(unloaded.get());
    assert!(reloaded.get());
    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["removed"].as_bool(), Some(false));
    assert_eq!(output["file_present"].as_bool(), Some(false));
    assert_eq!(output["service"]["loaded"].as_bool(), Some(true));
    assert_ne!(output["unload"]["skipped"].as_bool(), Some(true));
}

#[test]
fn uninstall_missing_file_with_inactive_manager_skips_successfully() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let unloaded = std::cell::Cell::new(false);
    let reloaded = std::cell::Cell::new(false);

    let output = unload_and_remove_service(
        &service_path,
        inactive_manager_status,
        |_| {
            unloaded.set(true);
            json!({ "ok": true })
        },
        |_| {
            reloaded.set(true);
            json!({ "ok": true })
        },
    )
    .unwrap();

    assert!(!unloaded.get());
    assert!(!reloaded.get());
    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["removed"].as_bool(), Some(false));
    assert_eq!(output["file_present"].as_bool(), Some(false));
    assert_eq!(output["unload"]["skipped"].as_bool(), Some(true));
    assert_eq!(output["reload"]["skipped"].as_bool(), Some(true));
    assert_eq!(output["service"]["loaded"].as_bool(), Some(false));
}

#[test]
fn uninstall_missing_file_with_uncertain_manager_attempts_unload_and_reload() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    let unloaded = std::cell::Cell::new(false);
    let reloaded = std::cell::Cell::new(false);

    let output = unload_and_remove_service(
        &service_path,
        |_| {
            manager_status(json!({
                "ok": false,
                "loaded": false,
                "enabled": false,
                "running": false,
                "error": "manager unavailable",
            }))
        },
        |_| {
            unloaded.set(true);
            json!({ "ok": true })
        },
        |_| {
            reloaded.set(true);
            json!({ "ok": true })
        },
    )
    .unwrap();

    assert!(unloaded.get());
    assert!(reloaded.get());
    assert_eq!(output["ok"].as_bool(), Some(false));
    assert_eq!(output["removed"].as_bool(), Some(false));
    assert_eq!(output["file_present"].as_bool(), Some(false));
    assert_eq!(output["service"]["ok"].as_bool(), Some(false));
    assert_eq!(output["status_uncertain"].as_bool(), Some(true));
    assert_eq!(output["unload"]["ok"].as_bool(), Some(true));
    assert_eq!(output["reload"]["ok"].as_bool(), Some(true));
    assert!(
        output["warning"]
            .as_str()
            .unwrap()
            .contains("could not confidently inspect")
    );
}

#[test]
fn service_status_requires_file_and_loaded_enabled_manager_state() {
    let temp = tempdir().unwrap();
    let service_path = temp.path().join("jig-proxy.service");
    fs::write(&service_path, "service").unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let output = service_status_value(&settings, store.root(), &service_path, |_| {
        manager_status(json!({
            "ok": true,
            "loaded": true,
            "enabled": false,
            "running": false,
        }))
    });

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(output["file_present"].as_bool(), Some(true));
    assert_eq!(output["installed"].as_bool(), Some(false));
}

#[test]
fn service_path_text_rejects_line_breaks() {
    let error = service_path_text(Path::new("/tmp/jig\nbin"), "current executable")
        .unwrap_err()
        .to_string();

    assert!(error.contains("cannot contain control characters"));
}

#[test]
fn service_path_text_rejects_nul() {
    let error = service_path_text(Path::new("/tmp/jig\0bin"), "current executable")
        .unwrap_err()
        .to_string();

    assert!(error.contains("cannot contain control characters"));
}

#[test]
fn service_path_text_rejects_relative_paths() {
    let error = service_path_text(Path::new("target/debug/jig"), "current executable")
        .unwrap_err()
        .to_string();

    assert!(error.contains("must be absolute"));
}

#[test]
fn launchctl_not_loaded_output_is_not_uninstall_failure() {
    let output = json!({
        "ok": false,
        "status": 5,
        "stdout": "",
        "stderr": "Bootstrap failed: 5: Input/output error\nservice is not loaded"
    });

    assert!(launchctl_output_means_not_loaded(&output));
}

#[test]
fn xml_escape_covers_apostrophes() {
    assert_eq!(
        xml_escape("a&b<c>d\"e'f"),
        "a&amp;b&lt;c&gt;d&quot;e&apos;f"
    );
}

#[test]
fn plist_string_escapes_body_text() {
    assert_eq!(
        plist_string("a&b<c>d\"e'f"),
        "<string>a&amp;b&lt;c&gt;d&quot;e&apos;f</string>"
    );
}

#[test]
fn systemd_quote_escapes_comment_markers() {
    assert_eq!(
        systemd_quote("JIG_REPO_ROOT=/tmp/repo#1%$").unwrap(),
        "\"JIG_REPO_ROOT=/tmp/repo\\x231%%$\""
    );
}

#[test]
fn systemd_exec_quote_escapes_command_dollars() {
    assert_eq!(
        systemd_exec_quote("/tmp/repo$1/bin/jig").unwrap(),
        "\"/tmp/repo$$1/bin/jig\""
    );
}

#[test]
fn systemd_quote_handles_quotes_and_backslashes() {
    assert_eq!(
        systemd_quote(r#"JIG_REPO_ROOT=/tmp/repo "one" \ user's"#).unwrap(),
        r#""JIG_REPO_ROOT=/tmp/repo \"one\" \\ user's""#
    );
}

#[test]
fn systemd_quote_rejects_line_breaks() {
    let error = systemd_quote("JIG_REPO_ROOT=/tmp/repo\nbad")
        .unwrap_err()
        .to_string();

    assert!(error.contains("cannot contain CR or LF"));
}

#[cfg(target_os = "linux")]
#[test]
fn service_body_quotes_systemd_paths_with_spaces() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo root");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state dir")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let body = service_body(&settings, &store, Path::new("/tmp/jig bin/jig"), &repo).unwrap();

    assert!(body.contains("ExecStart=\"/tmp/jig bin/jig\" proxy start"));
    assert!(body.contains("Environment=\"JIG_REPO_ROOT="));
}

#[cfg(target_os = "linux")]
#[test]
fn service_body_systemd_lines_start_at_column_zero() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let settings = ProxySettings {
        state_dir: Some(temp.path().join("state")),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

    let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();

    for line in body.lines().filter(|line| !line.is_empty()) {
        assert!(
            !line.chars().next().is_some_and(|ch| ch.is_whitespace()),
            "systemd unit line must start at column zero: {line:?}"
        );
        if line.starts_with('[') {
            assert!(
                line.ends_with(']'),
                "systemd section header must close on the same line: {line:?}"
            );
        } else {
            assert!(
                line.contains('='),
                "systemd directive must contain '=': {line:?}"
            );
        }
    }
}
