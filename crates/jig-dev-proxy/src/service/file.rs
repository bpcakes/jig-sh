use std::fs::{self, File};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::file_ops;
use crate::host::validate_tld;
use crate::state::StateStore;
use crate::types::ProxySettings;

pub(super) fn service_file_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to inspect Jig proxy service file {}",
                path.display()
            )
        }),
    }
}

pub(super) fn installed_service_state_dir(path: &Path) -> Result<Option<PathBuf>> {
    let Some(body) = file_ops::read_text_no_follow(path).with_context(|| {
        format!(
            "Failed to read Jig proxy service file {} while inspecting its state dir",
            path.display()
        )
    })?
    else {
        return Ok(None);
    };
    Ok(service_state_dir_from_body(&body)?.map(PathBuf::from))
}

// The platform-neutral boundary remains fallible because the systemd parser
// validates quoting and escaping even though the macOS plist parser is lenient.
#[allow(clippy::unnecessary_wraps)]
fn service_state_dir_from_body(body: &str) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        Ok(plist_service_state_dir(body))
    }

    #[cfg(target_os = "linux")]
    {
        systemd_service_state_dir(body)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = body;
        Ok(None)
    }
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn plist_service_state_dir(body: &str) -> Option<String> {
    let (_, after_key) = body.split_once("<key>JIG_PROXY_STATE_DIR</key>")?;
    let (_, after_string_open) = after_key.split_once("<string>")?;
    let (raw, _) = after_string_open.split_once("</string>")?;
    Some(xml_unescape(raw))
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn systemd_service_state_dir(body: &str) -> Result<Option<String>> {
    for line in body.lines() {
        let Some(raw) = line.strip_prefix("Environment=") else {
            continue;
        };
        let value = systemd_unquote(raw)?;
        if let Some(state_dir) = value.strip_prefix("JIG_PROXY_STATE_DIR=") {
            return Ok(Some(state_dir.to_string()));
        }
    }
    Ok(None)
}

pub(super) fn service_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Ok(dirs::home_dir()
            .context("Could not resolve home directory")?
            .join("Library/LaunchAgents/sh.jig.proxy.plist"))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(dirs::home_dir()
            .context("Could not resolve home directory")?
            .join(".config/systemd/user/jig-proxy.service"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    anyhow::bail!("Jig proxy user services are not supported on this platform.");
}

pub(super) fn service_body(
    settings: &ProxySettings,
    store: &StateStore,
    current_exe: &Path,
    repo_root: &Path,
) -> Result<String> {
    if settings.http_port == 0 {
        bail!("proxy HTTP port must be greater than 0 for service files");
    }
    if settings.https_port == Some(0) {
        bail!("proxy HTTPS port must be greater than 0 for service files");
    }
    if settings.https && settings.https_port == Some(settings.http_port) {
        bail!("proxy HTTP and HTTPS ports must be different for service files");
    }
    validate_tld(&settings.tld)?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let current_exe = service_path_text(current_exe, "current executable")?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let repo_root = service_path_text(repo_root, "repo root")?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let state_dir = service_path_text(store.root(), "proxy state dir")?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let log_path = service_path_text(&store.log_path(), "proxy log path")?;

    #[cfg(target_os = "macos")]
    {
        let mut args = vec![
            current_exe,
            "proxy".to_string(),
            "start".to_string(),
            "--foreground".to_string(),
            "--http-port".to_string(),
            settings.http_port.to_string(),
            "--tld".to_string(),
            settings.tld.clone(),
        ];
        if settings.https {
            args.push("--https".to_string());
            args.push("--https-port".to_string());
            args.push(settings.https_port.unwrap_or(1443).to_string());
        }
        if !settings.http2 {
            args.push("--no-http2".to_string());
        }
        if settings.lan {
            args.push("--lan".to_string());
        }
        let program_args = plist_string_array_entries(&args);
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>{label}
  <key>ProgramArguments</key>
  <array>
{program_args}
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>JIG_PROXY_STATE_DIR</key>{state_dir}
    <key>JIG_REPO_ROOT</key>{repo_root}
  </dict>
  <key>WorkingDirectory</key>{repo_root}
  <key>StandardOutPath</key>{log_path}
  <key>StandardErrorPath</key>{log_path}
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
</dict>
</plist>
"#,
            label = plist_string("sh.jig.proxy"),
            program_args = program_args,
            state_dir = plist_string(&state_dir),
            repo_root = plist_string(&repo_root),
            log_path = plist_string(&log_path),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        let exe = systemd_exec_quote(&current_exe)?;
        let tld = systemd_exec_quote(&settings.tld)?;
        let state_dir_env = systemd_quote(&format!("JIG_PROXY_STATE_DIR={state_dir}"))?;
        let repo_root_env = systemd_quote(&format!("JIG_REPO_ROOT={repo_root}"))?;
        let repo_root = systemd_quote(&repo_root)?;
        let log_output = systemd_quote(&format!("append:{log_path}"))?;
        Ok(format!(
            r#"[Unit]
Description=Jig local development proxy

[Service]
ExecStart={exe} proxy start --foreground --http-port {http_port} --tld {tld}{https_args}{http2_args}{lan_args}
Environment={state_dir_env}
Environment={repo_root_env}
WorkingDirectory={repo_root}
StandardOutput={log_output}
StandardError={log_output}
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=default.target
"#,
            exe = exe,
            http_port = settings.http_port,
            tld = tld,
            https_args = if settings.https {
                format!(
                    " --https --https-port {}",
                    settings.https_port.unwrap_or(1443)
                )
            } else {
                String::new()
            },
            http2_args = if settings.http2 { "" } else { " --no-http2" },
            lan_args = if settings.lan { " --lan" } else { "" },
            state_dir_env = state_dir_env,
            repo_root_env = repo_root_env,
            repo_root = repo_root,
            log_output = log_output,
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (store, current_exe, repo_root);
        anyhow::bail!("Jig proxy service install is not supported on this platform.");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub(super) fn service_path_text(path: &Path, label: &str) -> Result<String> {
    if !path.is_absolute() {
        anyhow::bail!("{label} path must be absolute for service files");
    }
    let text = path.to_string_lossy().into_owned();
    if text.chars().any(char::is_control) {
        anyhow::bail!("{label} path cannot contain control characters for service files");
    }
    Ok(text)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn plist_string(input: &str) -> String {
    // Keep generated escaping in sync with xml_unescape; service status parses
    // Jig-authored service files, not arbitrary plist syntax.
    format!("<string>{}</string>", xml_escape(input))
}

#[cfg(target_os = "macos")]
fn plist_string_array_entries(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("    {}", plist_string(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn write_service_file_if_safe(path: &Path, body: &str) -> Result<bool> {
    if let Some(mut file) = open_existing_service_file(path)? {
        let mut existing = String::new();
        file.read_to_string(&mut existing)?;
        if existing != body {
            anyhow::bail!(
                "Refusing to overwrite existing Jig proxy service file {} because its contents differ. Run `scripts/jig proxy service uninstall` first or remove the file manually.",
                path.display()
            );
        }
        ensure_existing_service_file_permissions(path, &file)?;
        return Ok(false);
    }
    write_service_file(path, body)?;
    Ok(true)
}

fn write_service_file(path: &Path, body: &str) -> Result<()> {
    let tmp = file_ops::temp_path(path, "jig-proxy-service");
    let mut file = create_service_file(&tmp)?;
    file.write_all(body.as_bytes())?;
    file.sync_data()?;
    drop(file);
    file_ops::replace_file(&tmp, path)
}

pub(super) fn prepare_service_parent_directory(parent: &Path) -> Result<()> {
    ensure_service_directory_chain_is_safe(parent, MissingDirectoryPolicy::Allow)?;
    create_service_parent_dir_all(parent)?;
    ensure_service_directory_chain_is_safe(parent, MissingDirectoryPolicy::Deny)
}

pub(super) fn ensure_service_parent_directory_is_safe(parent: &Path) -> Result<()> {
    ensure_service_directory_chain_is_safe(parent, MissingDirectoryPolicy::Deny)
}

fn create_service_parent_dir_all(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o755);
        builder.create(parent).with_context(|| {
            format!(
                "Failed to create Jig proxy service directory {}",
                parent.display()
            )
        })
    }

    #[cfg(not(unix))]
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create Jig proxy service directory {}",
                parent.display()
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MissingDirectoryPolicy {
    Allow,
    Deny,
}

#[cfg(unix)]
fn ensure_service_directory_chain_is_safe(
    parent: &Path,
    missing_policy: MissingDirectoryPolicy,
) -> Result<()> {
    if !parent.is_absolute() {
        anyhow::bail!(
            "Refusing to write Jig proxy service file under relative directory {}",
            parent.display()
        );
    }
    let root = service_directory_chain_root(parent)?;
    for path in service_directory_chain(parent, &root) {
        ensure_service_directory_is_safe(&path, missing_policy)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_service_directory_chain_is_safe(
    parent: &Path,
    missing_policy: MissingDirectoryPolicy,
) -> Result<()> {
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!(
                "Refusing to write Jig proxy service file under symlinked directory {}",
                parent.display()
            );
        }
        Ok(metadata) if !metadata.is_dir() => {
            anyhow::bail!(
                "Refusing to write Jig proxy service file under non-directory {}",
                parent.display()
            );
        }
        Ok(_) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && missing_policy == MissingDirectoryPolicy::Allow =>
        {
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "Jig proxy service directory {} is missing after creation",
                parent.display()
            )
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to inspect Jig proxy service directory {}",
                parent.display()
            )
        }),
    }
}

#[cfg(unix)]
fn service_directory_chain_root(parent: &Path) -> Result<PathBuf> {
    if let Some(home_dir) = dirs::home_dir()
        && parent.starts_with(&home_dir)
    {
        return Ok(home_dir);
    }

    let euid = current_euid();
    let mut root = parent.to_path_buf();
    for path in parent.ancestors() {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.uid() == euid => root = path.to_path_buf(),
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect Jig proxy service directory {}",
                        path.display()
                    )
                });
            }
        }
    }
    Ok(root)
}

#[cfg(unix)]
fn service_directory_chain(parent: &Path, root: &Path) -> Vec<PathBuf> {
    let mut chain = Vec::new();
    for path in parent.ancestors() {
        chain.push(path.to_path_buf());
        if path == root {
            break;
        }
    }
    chain.reverse();
    chain
}

#[cfg(unix)]
fn ensure_service_directory_is_safe(
    path: &Path,
    missing_policy: MissingDirectoryPolicy,
) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && missing_policy == MissingDirectoryPolicy::Allow =>
        {
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "Jig proxy service directory {} is missing after creation",
                path.display()
            );
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect Jig proxy service directory {}",
                    path.display()
                )
            });
        }
    };
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "Refusing to write Jig proxy service file under symlinked directory {}",
            path.display()
        );
    }
    if !metadata.is_dir() {
        anyhow::bail!(
            "Refusing to write Jig proxy service file under non-directory {}",
            path.display()
        );
    }
    let euid = current_euid();
    if metadata.uid() != euid {
        anyhow::bail!(
            "Refusing to write Jig proxy service file under directory {} owned by uid {}; expected current uid {}.",
            path.display(),
            metadata.uid(),
            euid
        );
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o022 != 0 {
        anyhow::bail!(
            "Refusing to write Jig proxy service file under group/world writable directory {} with permissions {:o}; remove group/world write bits first.",
            path.display(),
            mode
        );
    }
    Ok(())
}

#[cfg(unix)]
fn current_euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn create_service_file(path: &Path) -> Result<File> {
    file_ops::create_new_file(path, 0o644)
}

fn open_existing_service_file(path: &Path) -> Result<Option<File>> {
    match file_ops::open_read_no_follow(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        #[cfg(unix)]
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            anyhow::bail!(
                "Refusing to reuse existing Jig proxy service file {} because it is a symlink.",
                path.display()
            )
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Refusing to reuse existing Jig proxy service file {}",
                path.display()
            )
        }),
    }
}

fn ensure_existing_service_file_permissions(path: &Path, file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            anyhow::bail!(
                "Refusing to reuse existing Jig proxy service file {} with permissions {:o}; remove group/world write bits first.",
                path.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (path, file);
    Ok(())
}

#[cfg(test)]
pub(super) fn temp_service_path(path: &Path) -> PathBuf {
    file_ops::temp_path(path, "jig-proxy-service")
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn systemd_quote(input: &str) -> Result<String> {
    // Keep generated escaping in sync with systemd_unquote; service status only
    // needs to parse the service files this generator writes.
    if input.contains('\r') || input.contains('\n') {
        anyhow::bail!("systemd unit value cannot contain CR or LF characters");
    }
    Ok(format!(
        "\"{}\"",
        input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('#', "\\x23")
            .replace('%', "%%")
    ))
}

#[cfg(any(target_os = "linux", test))]
fn systemd_unquote(input: &str) -> Result<String> {
    let Some(rest) = input.strip_prefix('"') else {
        return Ok(input.to_string());
    };
    let mut output = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if chars.next().is_some() {
                anyhow::bail!("systemd quoted value has trailing characters");
            }
            return Ok(output);
        }
        if ch == '\\' {
            let Some(escaped) = chars.next() else {
                anyhow::bail!("systemd quoted value ends with an escape");
            };
            if escaped == 'x' {
                let hex = [chars.next(), chars.next()];
                if hex != [Some('2'), Some('3')] {
                    anyhow::bail!("unsupported systemd hex escape in generated service file");
                }
                output.push('#');
            } else {
                output.push(escaped);
            }
            continue;
        }
        if ch == '%' && chars.peek() == Some(&'%') {
            let _ = chars.next();
            output.push('%');
            continue;
        }
        output.push(ch);
    }
    anyhow::bail!("systemd quoted value is missing closing quote")
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn systemd_exec_quote(input: &str) -> Result<String> {
    Ok(systemd_quote(input)?.replace('$', "$$"))
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "macos", test))]
fn xml_unescape(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
