#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::ffi::OsString;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::ErrorKind;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::{Command, ExitStatus, Output, Stdio};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::time::{Duration as StdDuration, Instant};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use anyhow::Context;
use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::*;

#[cfg(any(target_os = "macos", target_os = "linux"))]
const TRUST_COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(30);

pub(crate) fn status(settings: &ProxySettings) -> Result<Value> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    warn_global_ca_trust(&store);
    Ok(json!({
        "ok": true,
        "state_dir": store.root(),
        "ca_exists": store.ca_path().exists(),
        "certificate_exists": store.leaf_path().exists(),
        "key_exists": store.leaf_key_path().exists(),
        "trust_check": trust_check(&store),
        "trust_warning": GLOBAL_CA_TRUST_WARNING,
    }))
}

pub(crate) fn trust(settings: &ProxySettings, accept_trust_scope: bool) -> Result<Value> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| trust_locked(&store, accept_trust_scope))
}

fn trust_locked(store: &StateStore, accept_trust_scope: bool) -> Result<Value> {
    if !store.ca_path().exists() {
        bail!(
            "CA certificate does not exist. Likely fix: run `scripts/jig proxy cert generate` first."
        );
    }
    ensure_jig_ca_certificate(store)?;
    ensure_jig_ca_private_key(store)?;
    if !accept_trust_scope {
        bail!(
            "Refusing to trust the Jig Dev Proxy local CA without --accept-trust-scope. {GLOBAL_CA_TRUST_WARNING}"
        );
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    warn_global_ca_trust(store);

    #[cfg(target_os = "macos")]
    {
        let keychain = login_keychain_path()?;
        let mut command = macos_security_command();
        command
            .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
            .arg(&keychain)
            .arg(command_path_arg(&store.ca_path()));
        let status = command_status_with_timeout(&mut command, "security add-trusted-cert")?;
        if !status.success() {
            bail!("security add-trusted-cert failed with status {status}");
        }
        ensure_macos_current_ca_is_trusted(store)?;
        // The marker is written only after platform trust succeeds. If the
        // process crashes between those steps, untrust falls back to scanning
        // Jig-labelled trusted roots instead of trusting only the marker.
        write_trusted_ca_marker(store)?;
        Ok(json!({
            "ok": true,
            "trusted": true,
            "platform": "macos",
            "warning": GLOBAL_CA_TRUST_WARNING,
        }))
    }

    #[cfg(target_os = "linux")]
    {
        if linux_command_available("trust") {
            let mut command = linux_system_command("trust")?;
            command
                .arg("anchor")
                .arg(command_path_arg(&store.ca_path()));
            let status = command_status_with_timeout(&mut command, "trust anchor")?;
            if !status.success() {
                bail!("trust anchor failed with status {status}");
            }
            let bundle_update = match linux_refresh_ca_bundles() {
                Ok(value) => value,
                Err(error) => {
                    return Err(error).context(
                        "trust anchor succeeded, but refreshing system CA bundles failed",
                    );
                }
            };
            // The marker is written only after platform trust succeeds. If the
            // process crashes between those steps, untrust falls back to
            // scanning Jig-labelled trusted roots instead of trusting only the marker.
            write_trusted_ca_marker(store)?;
            Ok(json!({
                "ok": true,
                "trusted": true,
                "platform": "linux",
                "system_bundle_update": bundle_update,
                "warning": GLOBAL_CA_TRUST_WARNING,
            }))
        } else {
            bail!(
                "Automatic trust requires the `trust` command. Install p11-kit or import {} manually.",
                store.ca_path().display()
            );
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    bail!("Automatic certificate trust is not supported on this platform.");
}

pub(super) fn warn_global_ca_trust(store: &StateStore) {
    eprintln!(
        "Warning: {} CA path: {}",
        GLOBAL_CA_TRUST_WARNING,
        store.ca_path().display()
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn command_path_arg(path: &Path) -> OsString {
    if !path.is_absolute() && path.to_string_lossy().starts_with('-') {
        return Path::new(".").join(path).into_os_string();
    }
    path.as_os_str().to_owned()
}

#[cfg(target_os = "macos")]
fn macos_security_command() -> Command {
    let mut command = Command::new("/usr/bin/security");
    command.env_clear();
    command
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_status_with_timeout(command: &mut Command, action: &str) -> Result<ExitStatus> {
    command.stdin(Stdio::null());
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to run {action}"))?;
    wait_child_with_timeout(&mut child, action)
}

#[cfg(target_os = "macos")]
fn ensure_macos_current_ca_is_trusted(store: &StateStore) -> Result<()> {
    let fingerprints = ca_fingerprints(&store.ca_path()).with_context(|| {
        format!(
            "Failed to inspect Jig proxy CA certificate {} after trust install",
            store.ca_path().display()
        )
    })?;
    if !macos_trusted_ca_fingerprint_exists(&fingerprints)
        .context("Failed to verify macOS trust store after installing Jig proxy CA")?
    {
        bail!(
            "security add-trusted-cert completed, but the Jig proxy CA was not found in the macOS trust store"
        );
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_output_with_timeout(command: &mut Command, action: &str) -> Result<Output> {
    let temp_dir = std::env::temp_dir();
    // TMPDIR is allowed to redirect these captures. The files themselves are
    // still created with O_NOFOLLOW | O_EXCL and mode 0600 by file_ops.
    fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "Failed to create temporary command-output directory {}",
            temp_dir.display()
        )
    })?;
    let temp_base = temp_dir.join("jig-proxy-command-output");
    let stdout_path = file_ops::temp_path(&temp_base, "jig-proxy-command-output");
    let stderr_path = file_ops::temp_path(&temp_base, "jig-proxy-command-output");
    let stdout_file = file_ops::create_new_file(&stdout_path, 0o600)
        .with_context(|| format!("Failed to create temporary stdout file for {action}"))?;
    let stderr_file = file_ops::create_new_file(&stderr_path, 0o600)
        .with_context(|| format!("Failed to create temporary stderr file for {action}"))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to run {action}"))?;
    let status = wait_child_with_timeout(&mut child, action);
    let child_completed = status.is_ok();
    let stdout = fs::read(&stdout_path)
        .with_context(|| capture_read_context(action, "stdout", child_completed));
    let stderr = fs::read(&stderr_path)
        .with_context(|| capture_read_context(action, "stderr", child_completed));
    remove_temp_file_best_effort(&stdout_path);
    remove_temp_file_best_effort(&stderr_path);
    if status.is_err() {
        log_capture_read_error("stdout", &stdout);
        log_capture_read_error("stderr", &stderr);
    }
    Ok(Output {
        status: status?,
        stdout: stdout?,
        stderr: stderr?,
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn capture_read_context(action: &str, stream: &str, child_completed: bool) -> String {
    if child_completed {
        format!("Trust helper {action} completed, but captured {stream} could not be read")
    } else {
        format!("Failed to read captured {stream} for {action}")
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn log_capture_read_error(stream: &str, result: &Result<Vec<u8>>) {
    if let Err(error) = result {
        eprintln!(
            "jig proxy could not read captured {stream} after trust helper failure: {error:#}"
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn wait_child_with_timeout(child: &mut std::process::Child, action: &str) -> Result<ExitStatus> {
    let deadline = Instant::now() + TRUST_COMMAND_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("Failed to wait for {action}"))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            // Preserve the timeout as the primary error. Kill/wait only try to
            // prevent a platform trust helper from continuing after timeout.
            let _ = child.kill();
            let _ = child.wait();
            bail!("{action} timed out after {TRUST_COMMAND_TIMEOUT:?}");
        }
        std::thread::sleep(StdDuration::from_millis(50));
    }
}

pub(crate) fn untrust(settings: &ProxySettings, accept_trust_scope: bool) -> Result<Value> {
    if !accept_trust_scope {
        bail!(
            "Refusing to mutate platform trust settings without --accept-trust-scope. This command removes matching Jig Dev Proxy local CA certificates from the platform trust store."
        );
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let store = StateStore::resolve(settings.state_dir.clone())?;
        store.with_cert_lock(|| untrust_locked(&store))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = settings;
        bail!("Automatic certificate untrust is not supported on this platform.");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn untrust_locked(store: &StateStore) -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        let target_fingerprint = if let Some(fingerprint) = trusted_ca_marker_fingerprints(store)? {
            Some(fingerprint)
        } else if store.ca_path().exists() {
            ensure_jig_ca_certificate(store)?;
            Some(ca_fingerprints(&store.ca_path())?)
        } else {
            None
        };
        let mut removed = 0usize;
        if let Some(target_fingerprint) = target_fingerprint {
            while removed < MACOS_UNTRUST_REMOVAL_LIMIT {
                let fingerprints = macos_trusted_jig_ca_fingerprints()?;
                let Some(fingerprint) = fingerprints
                    .iter()
                    .find(|fingerprint| fingerprint.matches(&target_fingerprint))
                    .cloned()
                else {
                    break;
                };
                macos_delete_trusted_certificate(&fingerprint)?;
                removed += 1;
            }
            if removed >= MACOS_UNTRUST_REMOVAL_LIMIT
                && macos_trusted_jig_ca_fingerprints()?
                    .iter()
                    .any(|fingerprint| fingerprint.matches(&target_fingerprint))
            {
                bail!(
                    "Removed {removed} matching certificates, but more trusted copies remain. Run untrust again."
                );
            }
        }
        remove_trusted_ca_marker(store);
        Ok(json!({
            "ok": true,
            "platform": "macos",
            "removed": removed,
            "warning": macos_untrust_warning(removed),
        }))
    }

    #[cfg(target_os = "linux")]
    {
        let ca_exists = store.ca_path().exists();
        if ca_exists {
            ensure_jig_ca_certificate(store)?;
        }
        if !linux_command_available("trust") {
            bail!("Automatic certificate untrust requires the `trust` command.");
        }
        {
            let mut removed = 0usize;
            let current_trusted = if ca_exists {
                let current_der = first_certificate_der(&store.ca_path())?;
                linux_trust_anchors_contain_der(store, &current_der)?
            } else {
                false
            };
            let trusted_uris = linux_trusted_jig_ca_uris_result()?;
            let marker_authorizes_label_removal = if current_trusted {
                false
            } else if ca_exists {
                trusted_ca_marker_matches(store)?
            } else {
                trusted_ca_marker_owned_by_current_platform(store)?
            };
            if !ca_exists && !marker_authorizes_label_removal {
                bail!(
                    "CA certificate does not exist in {}, and no Jig-installed Linux trust marker was found.",
                    store.ca_path().display()
                );
            }
            if current_trusted || marker_authorizes_label_removal {
                if trusted_uris.is_empty() {
                    if current_trusted {
                        linux_remove_trust_anchor(command_path_arg(&store.ca_path()))?;
                        removed = 1;
                    }
                } else {
                    for uri in trusted_uris {
                        linux_remove_trust_anchor(OsString::from(uri))?;
                        removed += 1;
                    }
                }
            } else {
                bail!(
                    "No exact Jig CA trust anchor or Jig-installed trust marker was found. Refusing to remove label-matched Linux trust anchors."
                );
            }
            let bundle_update = if removed > 0 {
                linux_refresh_ca_bundles().context(
                    "trust anchor --remove succeeded, but refreshing system CA bundles failed",
                )?
            } else {
                json!({ "ok": true, "skipped": true })
            };
            remove_trusted_ca_marker(store);
            Ok(json!({
                "ok": true,
                "platform": "linux",
                "removed": removed,
                "system_bundle_update": bundle_update,
            }))
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) const fn macos_untrust_warning(removed: usize) -> Option<&'static str> {
    if removed == 0 {
        Some("No trusted Jig Dev Proxy Local CA certificate was removed.")
    } else if removed >= MACOS_UNTRUST_REMOVAL_LIMIT {
        Some("Removed many matching certificates; run untrust again if more copies remain.")
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
pub(super) fn macos_trusted_ca_fingerprint_exists(
    fingerprint: &CertificateFingerprints,
) -> Result<bool> {
    Ok(macos_trusted_jig_ca_fingerprints()?
        .iter()
        .any(|hash| hash.matches(fingerprint)))
}

#[cfg(target_os = "macos")]
fn macos_trusted_jig_ca_fingerprints() -> Result<Vec<CertificateFingerprints>> {
    let Ok(keychain) = login_keychain_path() else {
        return Ok(Vec::new());
    };
    let mut command = macos_security_command();
    command
        .args([
            "find-certificate",
            "-a",
            "-Z",
            "-p",
            "-c",
            JIG_CA_COMMON_NAME,
        ])
        .arg(keychain);
    let output = command_output_with_timeout(&mut command, "security find-certificate")?;
    if output.status.success() {
        Ok(security_find_certificate_fingerprints(&output.stdout))
    } else {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "macos")]
pub(super) fn security_find_certificate_fingerprints(
    output: &[u8],
) -> Vec<CertificateFingerprints> {
    let text = String::from_utf8_lossy(output);
    let mut fingerprints = Vec::new();
    let mut pending_sha1 = None;
    let mut pem = String::new();
    let mut in_pem = false;
    for line in text.lines() {
        if let Some(hash) = line.trim().strip_prefix("SHA-1 hash:") {
            let hash = hash.trim().to_ascii_uppercase();
            pending_sha1 = sha1_fingerprint_is_valid(&hash).then_some(hash);
            continue;
        }
        if line == "-----BEGIN CERTIFICATE-----" {
            pem.clear();
            pem.push_str(line);
            pem.push('\n');
            in_pem = true;
            continue;
        }
        if in_pem {
            pem.push_str(line);
            pem.push('\n');
            if line == "-----END CERTIFICATE-----" {
                if let Some(sha1) = pending_sha1.take() {
                    if let Some(sha256) = pem_sha256_hex(&pem) {
                        fingerprints.push(CertificateFingerprints { sha1, sha256 });
                    }
                }
                in_pem = false;
            }
        }
    }
    fingerprints
}

#[cfg(target_os = "macos")]
pub(super) fn pem_sha256_hex(pem: &str) -> Option<String> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let cert = rustls_pemfile::certs(&mut reader).next()?.ok()?;
    Some(hex_upper(&Sha256::digest(cert.as_ref())))
}

#[cfg(target_os = "macos")]
fn macos_delete_trusted_certificate(fingerprint: &CertificateFingerprints) -> Result<()> {
    if !sha1_fingerprint_is_valid(&fingerprint.sha1) {
        bail!("Refusing to pass invalid SHA-1 certificate fingerprint to security");
    }
    let keychain = login_keychain_path()?;
    let mut command = macos_security_command();
    // The candidate was selected by a paired SHA-256 PEM digest above; the
    // `security delete-certificate -Z` interface itself accepts the SHA-1 hash.
    command
        .args(["delete-certificate", "-Z", &fingerprint.sha1])
        .arg(&keychain);
    let output = command_output_with_timeout(&mut command, "security delete-certificate")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("could not be found") || stderr.contains("not found") {
        bail!(
            "security reported a matching Jig CA certificate but could not delete it; run `scripts/jig proxy cert untrust --accept-trust-scope` again."
        );
    }
    bail!("security delete-certificate failed: {}", stderr.trim())
}

#[cfg(target_os = "macos")]
fn sha1_fingerprint_is_valid(fingerprint: &str) -> bool {
    fingerprint.len() == 40
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase())
}

fn trust_check(store: &StateStore) -> Value {
    #[cfg(target_os = "macos")]
    {
        let mut error = None::<String>;
        let fingerprints = if store.ca_path().exists() {
            match ca_fingerprints(&store.ca_path()) {
                Ok(fingerprints) => Some(fingerprints),
                Err(err) => {
                    error = Some(err.to_string());
                    None
                }
            }
        } else {
            None
        };
        let trusted = if let Some(fingerprints) = fingerprints.as_ref() {
            match macos_trusted_ca_fingerprint_exists(fingerprints) {
                Ok(trusted) => Some(trusted),
                Err(err) => {
                    error = Some(err.to_string());
                    None
                }
            }
        } else if store.ca_path().exists() {
            None
        } else {
            Some(false)
        };
        json!({
            "platform": "macos",
            "trusted": trusted,
            "fingerprint_sha256": fingerprints.as_ref().map(|fingerprint| &fingerprint.sha256),
            "fingerprint_sha1": fingerprints.as_ref().map(|fingerprint| &fingerprint.sha1),
            "error": error,
        })
    }

    #[cfg(target_os = "linux")]
    {
        let has_trust = linux_command_available("trust");
        let mut error = None::<String>;
        let trusted = if has_trust {
            if store.ca_path().exists() {
                match linux_current_jig_ca_is_trusted(store) {
                    Ok(trusted) => Some(trusted),
                    Err(err) => {
                        error = Some(err.to_string());
                        None
                    }
                }
            } else {
                match linux_trusted_jig_ca_uris_result() {
                    Ok(uris) => Some(!uris.is_empty()),
                    Err(err) => {
                        error = Some(err.to_string());
                        None
                    }
                }
            }
        } else {
            None
        };
        json!({ "platform": "linux", "trusted": trusted, "trust_command": has_trust, "error": error })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = store;
        json!({ "platform": std::env::consts::OS, "trusted": null })
    }
}

#[cfg(target_os = "linux")]
fn linux_refresh_ca_bundles() -> Result<Value> {
    if linux_command_available("update-ca-trust") {
        let mut command = linux_system_command("update-ca-trust")?;
        command.arg("extract");
        let status = command_status_with_timeout(&mut command, "update-ca-trust extract")?;
        if !status.success() {
            bail!(
                "update-ca-trust extract failed with status {status}. Run with the privileges required by your distribution or refresh system CA bundles manually."
            );
        }
        return Ok(json!({
            "ok": true,
            "command": "update-ca-trust extract",
            "status": status.code(),
        }));
    }
    if linux_command_available("update-ca-certificates") {
        let mut command = linux_system_command("update-ca-certificates")?;
        let status = command_status_with_timeout(&mut command, "update-ca-certificates")?;
        if !status.success() {
            bail!(
                "update-ca-certificates failed with status {status}. Run with the privileges required by your distribution or refresh system CA bundles manually."
            );
        }
        return Ok(json!({
            "ok": true,
            "command": "update-ca-certificates",
            "status": status.code(),
        }));
    }
    bail!(
        "No supported system CA bundle refresh command found. Install update-ca-trust/update-ca-certificates, run with privileges when required, or refresh system CA bundles manually."
    )
}

#[cfg(target_os = "linux")]
fn linux_command_available(program: &str) -> bool {
    linux_system_tool(program).is_some()
}

#[cfg(target_os = "linux")]
fn linux_system_command(program: &str) -> Result<Command> {
    let path = linux_system_tool(program)
        .with_context(|| format!("Could not find supported system command `{program}`"))?;
    let mut command = Command::new(path);
    command.env_clear();
    Ok(command)
}

#[cfg(target_os = "linux")]
fn linux_system_tool(program: &str) -> Option<PathBuf> {
    linux_system_tool_candidates(program)
        .iter()
        .map(PathBuf::from)
        .find(|path| executable_file(path))
}

#[cfg(target_os = "linux")]
fn linux_system_tool_candidates(program: &str) -> &'static [&'static str] {
    match program {
        "trust" => &["/usr/bin/trust", "/bin/trust"],
        "update-ca-trust" => &["/usr/bin/update-ca-trust", "/usr/sbin/update-ca-trust"],
        "update-ca-certificates" => &[
            "/usr/sbin/update-ca-certificates",
            "/usr/bin/update-ca-certificates",
        ],
        _ => &[],
    }
}

#[cfg(target_os = "linux")]
fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(target_os = "linux")]
fn linux_trusted_jig_ca_uris_result() -> Result<Vec<String>> {
    if !linux_command_available("trust") {
        return Ok(Vec::new());
    }
    let mut command = linux_system_command("trust")?;
    command.args(["list", "--filter=ca-anchors"]);
    let output = command_output_with_timeout(&mut command, "trust list")?;
    if !output.status.success() {
        bail!(
            "trust list --filter=ca-anchors failed with status {}",
            output.status
        );
    }
    Ok(trust_list_jig_ca_uris(&output.stdout))
}

#[cfg(target_os = "linux")]
fn linux_remove_trust_anchor(anchor: OsString) -> Result<()> {
    let mut command = linux_system_command("trust")?;
    command.arg("anchor").arg("--remove").arg(anchor);
    let status = command_status_with_timeout(&mut command, "trust anchor --remove")?;
    if !status.success() {
        bail!("trust anchor --remove failed with status {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn linux_current_jig_ca_is_trusted(store: &StateStore) -> Result<bool> {
    if !linux_command_available("trust") {
        return Ok(false);
    }
    let current_der = first_certificate_der(&store.ca_path())?;
    if linux_trust_anchors_contain_der(store, &current_der)? {
        return Ok(true);
    }
    // Older p11-kit deployments may not support the extract format consistently.
    // Fall back to Jig's owned CA label so forced regeneration still refuses
    // when a prior Jig root might be trusted.
    Ok(!linux_trusted_jig_ca_uris_result()?.is_empty())
}

#[cfg(target_os = "linux")]
fn linux_trust_anchors_contain_der(store: &StateStore, expected_der: &[u8]) -> Result<bool> {
    let tmp_dir = file_ops::temp_path(&store.root().join("trusted-anchors"), "jig-proxy-cert");
    fs::create_dir(&tmp_dir)?;
    #[cfg(unix)]
    fs::set_permissions(&tmp_dir, fs::Permissions::from_mode(0o700))?;
    let tmp = tmp_dir.join("anchors.pem");
    let mut command = match linux_system_command("trust") {
        Ok(command) => command,
        Err(error) => {
            remove_temp_dir_best_effort(&tmp_dir);
            return Err(error);
        }
    };
    command
        .args([
            "extract",
            "--overwrite",
            "--format=pem-bundle",
            "--filter=ca-anchors",
        ])
        .arg(command_path_arg(&tmp));
    let status = match command_status_with_timeout(&mut command, "trust extract") {
        Ok(status) => status,
        Err(error) => {
            remove_temp_file_best_effort(&tmp);
            remove_temp_dir_best_effort(&tmp_dir);
            return Err(error);
        }
    };
    if !status.success() {
        remove_temp_file_best_effort(&tmp);
        remove_temp_dir_best_effort(&tmp_dir);
        bail!("trust extract failed with status {status}; refusing to assume no Jig CA is trusted");
    }
    let result = pem_bundle_contains_der(&tmp, expected_der);
    remove_temp_file_best_effort(&tmp);
    remove_temp_dir_best_effort(&tmp_dir);
    result
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_temp_file_best_effort(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            eprintln!(
                "jig proxy could not remove temporary file {}: {error}",
                path.display()
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn remove_temp_dir_best_effort(path: &Path) {
    match fs::remove_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            eprintln!(
                "jig proxy could not remove temporary directory {}: {error}",
                path.display()
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn pem_bundle_contains_der(path: &Path, expected_der: &[u8]) -> Result<bool> {
    let file = open_required_read_no_follow(path, MAX_TRUST_BUNDLE_PEM_BYTES)?;
    let mut reader = std::io::BufReader::new(file);
    for cert in rustls_pemfile::certs(&mut reader) {
        let cert = cert.context("Failed to parse trust anchor PEM bundle")?;
        if cert.as_ref() == expected_der {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
pub(super) fn trust_list_jig_ca_uris(output: &[u8]) -> Vec<String> {
    let mut uris = Vec::new();
    let mut current_uri = None::<String>;
    let mut current_label_matches = false;
    for line in String::from_utf8_lossy(output).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pkcs11:") {
            if current_label_matches {
                if let Some(uri) = current_uri.take() {
                    uris.push(uri);
                }
            }
            current_uri = Some(trimmed.to_string());
            current_label_matches = false;
            continue;
        }
        if trimmed
            .strip_prefix("label:")
            .map(str::trim)
            .map(|label| label.trim_matches('"'))
            .is_some_and(|label| label == JIG_CA_COMMON_NAME)
        {
            current_label_matches = true;
        }
    }
    if current_label_matches {
        if let Some(uri) = current_uri {
            uris.push(uri);
        }
    }
    uris
}

#[cfg(target_os = "macos")]
fn login_keychain_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not resolve home directory for login keychain")?;
    Ok(home.join("Library/Keychains/login.keychain-db"))
}
