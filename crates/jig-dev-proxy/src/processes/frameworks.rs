use std::path::Path;

use anyhow::{Context, Result, bail};
pub(super) use jig_typescript::dev_script::{
    is_vite_token, script_looks_like_vite as shell_command_looks_like_vite,
};

use crate::host::{validate_hostname, validate_tld};
use crate::types::{AppKind, AppRunSpec, CommandSpec, ProxySettings};

pub(super) fn vite_allowed_hosts(spec: &AppRunSpec, settings: &ProxySettings) -> Result<String> {
    validate_vite_allowed_host_token(&spec.hostname)?;
    let tld = settings.tld.trim();
    if tld.is_empty() {
        Ok(spec.hostname.clone())
    } else {
        validate_tld(tld)?;
        let wildcard = format!(".{tld}");
        validate_vite_allowed_host_token(&wildcard)?;
        Ok(format!("{},{}", spec.hostname, wildcard))
    }
}

pub(super) fn validate_vite_allowed_host_token(value: &str) -> Result<()> {
    if let Some(hostname) = value.strip_prefix('.') {
        validate_tld(hostname)?;
    } else {
        validate_hostname(value)?;
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
    {
        return Ok(());
    }
    bail!("Vite allowed-host token '{value}' contains invalid characters");
}

pub(super) fn ensure_vite_port_flags_match(argv: &[String], assigned_port: u16) -> Result<()> {
    let Some(configured_port) = configured_vite_port(argv)? else {
        return Ok(());
    };
    if configured_port != assigned_port {
        bail!(
            "Vite command already sets port {configured_port}, but Jig assigned port {assigned_port}. Remove -p/--port from argv or set the Jig app port to {configured_port}."
        );
    }
    Ok(())
}

pub(super) fn configured_vite_port(argv: &[String]) -> Result<Option<u16>> {
    let mut found = None;
    for (index, arg) in argv.iter().enumerate() {
        let value = if arg == "--port" || arg == "-p" {
            argv.get(index + 1)
                .map(String::as_str)
                .filter(|value| !value.starts_with('-'))
                .with_context(|| format!("Vite port flag {arg} must include a numeric value"))?
        } else if let Some(value) = arg.strip_prefix("--port=") {
            value
        } else if let Some(value) = arg.strip_prefix("-p=") {
            value
        } else if let Some(value) = arg.strip_prefix("-p") {
            if value.is_empty() {
                continue;
            }
            value
        } else {
            continue;
        };
        let port = value
            .parse::<u16>()
            .with_context(|| format!("Vite port flag {arg} uses non-numeric value '{value}'"))?;
        if port == 0 {
            bail!("Vite port flag {arg} must be greater than 0");
        }
        if found.is_some_and(|existing| existing != port) {
            bail!("Vite command contains conflicting port flags");
        }
        found = Some(port);
    }
    Ok(found)
}

pub(super) fn command_argv(
    command: &CommandSpec,
    kind: &AppKind,
    port: u16,
) -> Result<Vec<String>> {
    match command {
        CommandSpec::Argv(argv) => {
            let mut argv = argv.clone();
            if kind == &AppKind::Vite || command_looks_like_vite(&argv) {
                ensure_vite_port_flags_match(&argv, port)?;
            }
            inject_framework_flags(&mut argv, kind, port);
            Ok(argv)
        }
        CommandSpec::Shell(command) => {
            // Shell commands are limited to explicit repo configuration. Workspace
            // discovery and CLI passthrough use argv so discovered package names
            // or user-supplied arguments never need shell escaping here.
            validate_shell_command(command)?;
            if kind == &AppKind::Vite || shell_command_looks_like_vite(command) {
                bail!(
                    "Vite development apps must use argv instead of a shell command so Jig can inject --port, --host, and --strictPort safely."
                );
            }
            Ok(shell_command(command))
        }
    }
}

pub(super) fn validate_shell_command(command: &str) -> Result<()> {
    if command
        .bytes()
        .any(|byte| matches!(byte, b'\0' | b'\r' | b'\n'))
    {
        bail!("Shell app commands must be single-line strings without NUL bytes or line breaks.");
    }
    Ok(())
}

pub(super) fn shell_command(command: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), command.into()]
}

pub(crate) fn inject_framework_flags(argv: &mut Vec<String>, kind: &AppKind, port: u16) {
    let detected_vite = command_looks_like_vite(argv);
    if kind != &AppKind::Vite && !detected_vite {
        return;
    }
    if let Some(separator_index) = package_manager_run_separator_index(argv)
        && !argv.iter().any(|arg| arg == "--")
    {
        argv.insert(separator_index, "--".into());
    }
    if !contains_flag(argv, "--port") && !contains_flag(argv, "-p") {
        argv.push("--port".into());
        argv.push(port.to_string());
    }
    if !contains_flag(argv, "--strictPort") {
        argv.push("--strictPort".into());
    }
    if !contains_flag(argv, "--host") {
        argv.push("--host".into());
        argv.push("127.0.0.1".into());
    }
}

pub(super) fn command_looks_like_vite(argv: &[String]) -> bool {
    if argv.is_empty() {
        return false;
    }
    let first = Path::new(&argv[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&argv[0]);
    if is_vite_token(first) {
        return true;
    }
    match first {
        "npx" | "bunx" => argv.get(1).is_some_and(|arg| is_vite_token(arg)),
        "npm" | "pnpm" | "yarn" | "bun" => argv.windows(2).any(|pair| {
            matches!(pair[0].as_str(), "run" | "exec") && is_vite_token(pair[1].as_str())
        }),
        _ => false,
    }
}

pub(super) fn package_manager_run_separator_index(argv: &[String]) -> Option<usize> {
    let first = argv.first()?;
    let binary = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first);
    if !matches!(binary, "npm" | "pnpm" | "bun" | "yarn") {
        return None;
    }
    let subcommand_index = argv
        .iter()
        .position(|arg| matches!(arg.as_str(), "run" | "exec"))?;
    let script_index = subcommand_index + 1;
    (script_index < argv.len()).then_some(script_index + 1)
}

pub(super) fn contains_flag(argv: &[String], flag: &str) -> bool {
    argv.iter().any(|arg| {
        arg == flag
            || arg
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('='))
            || (flag == "-p"
                && arg.strip_prefix(flag).is_some_and(|rest| {
                    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
                }))
    })
}
