#[cfg(target_os = "linux")]
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::net::{SocketAddr, ToSocketAddrs};
use std::process::Child;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::ports::{is_port_free, is_tcp_listening};
use crate::state::{process_start_token, process_start_tokens_supported};
use crate::types::AppRunSpec;

#[cfg(unix)]
use super::child_lifecycle::unix_pid;
use super::child_lifecycle::{terminate_and_reap_logged, try_wait_preserving_process_group};
use super::cleanup::{TerminationReason, termination_requested};
use super::interruption_error;

const APP_READY_CHECK_INTERVAL: Duration = Duration::from_millis(100);
const LISTENER_OWNER_RECHECK_ATTEMPTS: usize = 3;
const LISTENER_OWNER_RECHECK_DELAY: Duration = Duration::from_millis(50);

struct ReadinessProbes<Listens, PortFree, Interrupted> {
    is_listening: Listens,
    port_is_free: PortFree,
    interrupt_requested: Interrupted,
}

pub(super) fn wait_for_app_ready(
    spec: &AppRunSpec,
    port: u16,
    child: &mut Child,
) -> Result<Option<String>> {
    wait_for_app_ready_interruptible(spec, port, child, &termination_requested)
}

pub(super) fn wait_for_app_ready_interruptible(
    spec: &AppRunSpec,
    port: u16,
    child: &mut Child,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<Option<String>> {
    wait_for_app_ready_until(
        &spec.name,
        &spec.target_host,
        port,
        child,
        None,
        interrupt_requested,
    )
}

pub(super) fn verify_process_route_owner(
    name: &str,
    target_host: &str,
    port: u16,
    child_pid: u32,
    expected_start_token: Option<&str>,
) -> Result<()> {
    // Package-manager dev commands often leave the actual listener in a
    // supervised grandchild. The contract is process-group ownership plus
    // stable process start tokens, not exact ownership by the direct child PID.
    let before = owner_start_token_for_child(child_pid)?;
    if before.as_deref() != expected_start_token {
        bail!(
            "Process identity for app '{name}' changed before listener ownership could be verified; refusing to publish process route"
        );
    }
    verify_app_listener_owner(name, target_host, port, child_pid)?;
    let after = owner_start_token_for_child(child_pid)?;
    if after.as_deref() != expected_start_token {
        bail!(
            "Process identity for app '{name}' changed while verifying listener ownership; refusing to publish process route"
        );
    }
    Ok(())
}

fn owner_start_token_for_child(pid: u32) -> Result<Option<String>> {
    if !process_start_tokens_supported() {
        bail!(
            "Process route start identity is unavailable on this platform; use --no-proxy or a proxy alias instead."
        );
    }
    let token = process_start_token(pid);
    if token.is_none() {
        bail!(
            "Could not verify start identity for child process {pid}; refusing to publish process route"
        );
    }
    Ok(token)
}

#[cfg(test)]
pub(super) fn wait_for_app_ready_with_timeout(
    name: &str,
    target_host: &str,
    port: u16,
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<String>> {
    wait_for_app_ready_until(
        name,
        target_host,
        port,
        child,
        Some(timeout),
        &termination_requested,
    )
}

#[cfg(test)]
pub(super) fn wait_for_app_ready_with_timeout_and_test_probe(
    name: &str,
    target_host: &str,
    port: u16,
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<String>> {
    wait_for_app_ready_until_with_probe(
        name,
        target_host,
        port,
        child,
        Some(timeout),
        ReadinessProbes {
            is_listening: |_: &str, _: u16| false,
            port_is_free: |_: &str, _: u16| true,
            interrupt_requested: termination_requested,
        },
    )
}

fn wait_for_app_ready_until(
    name: &str,
    target_host: &str,
    port: u16,
    child: &mut Child,
    timeout: Option<Duration>,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<Option<String>> {
    wait_for_app_ready_until_with_probe(
        name,
        target_host,
        port,
        child,
        timeout,
        ReadinessProbes {
            is_listening: is_tcp_listening,
            port_is_free: is_port_free,
            interrupt_requested,
        },
    )
}

fn wait_for_app_ready_until_with_probe(
    name: &str,
    target_host: &str,
    port: u16,
    child: &mut Child,
    timeout: Option<Duration>,
    probes: ReadinessProbes<
        impl Fn(&str, u16) -> bool,
        impl Fn(&str, u16) -> bool,
        impl Fn() -> Option<TerminationReason>,
    >,
) -> Result<Option<String>> {
    if let Some(status) = try_wait_preserving_process_group(child)? {
        bail!("App '{name}' exited before listening on {target_host}:{port} with status {status}");
    }
    let child_pid = child.id();
    let expected_start_token = owner_start_token_for_child(child_pid)?;
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    loop {
        if let Some(status) = try_wait_preserving_process_group(child)? {
            bail!(
                "App '{name}' exited before listening on {target_host}:{port} with status {status}"
            );
        }
        if let Some(reason) = (probes.interrupt_requested)() {
            // Prefer an already-observable child result over a late signal.
            if let Some(status) = try_wait_preserving_process_group(child)? {
                bail!(
                    "App '{name}' exited before listening on {target_host}:{port} with status {status}"
                );
            }
            return Err(interruption_error(reason));
        }
        if (probes.is_listening)(target_host, port) {
            verify_process_route_owner(
                name,
                target_host,
                port,
                child_pid,
                expected_start_token.as_deref(),
            )?;
            return Ok(expected_start_token);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let timeout = timeout.expect("a readiness deadline always has a timeout");
            let message = if (probes.port_is_free)(target_host, port) {
                format!(
                    "App '{name}' did not listen on {target_host}:{port} within {timeout:?}. The process may have ignored PORT/HOST or rebound to a different port. Likely fix: configure the app to honor PORT={port} and HOST={target_host}, and make it fail when the requested port is unavailable."
                )
            } else {
                format!(
                    "App '{name}' did not listen on {target_host}:{port} within {timeout:?}, and that port is now in use by another process. Likely fix: stop the process using that port or configure a different [[dev.apps]].port."
                )
            };
            terminate_and_reap_logged(child, "could not clean up after app readiness timeout");
            bail!(message);
        }
        thread::sleep(APP_READY_CHECK_INTERVAL);
    }
}

#[cfg(unix)]
pub(super) fn verify_app_listener_owner(
    name: &str,
    target_host: &str,
    port: u16,
    child_pid: u32,
) -> Result<()> {
    for attempt in 0..LISTENER_OWNER_RECHECK_ATTEMPTS {
        let listener_pids = tcp_listener_pids(target_host, port)
            .with_context(|| format!("Could not verify listener owner for app '{name}'"))?;
        if listener_pids.is_empty() {
            bail!(
                "Could not identify the process listening on {target_host}:{port} for app '{name}'; refusing to publish process route"
            );
        }
        let child_pgid = process_group_id(child_pid).with_context(|| {
            format!("Could not read spawned process group for app '{name}' process {child_pid}")
        })?;
        let mut outside_group = Vec::new();
        for pid in &listener_pids {
            let before = owner_start_token_for_child(*pid).with_context(|| {
                format!(
                    "Could not verify start identity for listener process {pid} on {target_host}:{port}"
                )
            })?;
            match process_group_id(*pid) {
                Some(pgid) if pgid == child_pgid => {}
                Some(_) => {
                    outside_group.push(*pid);
                    continue;
                }
                None => bail!(
                    "Listener process {pid} for app '{name}' vanished or could not be inspected during verification; refusing to publish process route"
                ),
            }
            #[cfg(target_os = "macos")]
            if !macos_pid_listens_on_target(*pid, target_host, port)? {
                bail!(
                    "Listener process {pid} for app '{name}' no longer owns {target_host}:{port}; refusing to publish process route"
                );
            }
            let after = owner_start_token_for_child(*pid).with_context(|| {
                format!(
                    "Could not recheck start identity for listener process {pid} on {target_host}:{port}"
                )
            })?;
            if before != after {
                bail!(
                    "Listener process {pid} for app '{name}' changed identity during verification; refusing to publish process route"
                );
            }
        }
        if outside_group.is_empty() {
            let after_listener_pids = tcp_listener_pids(target_host, port)
                .with_context(|| format!("Could not recheck listener owner for app '{name}'"))?;
            if after_listener_pids != listener_pids {
                bail!(
                    "Listener owner set for app '{name}' on {target_host}:{port} changed during verification; refusing to publish process route"
                );
            }
            return Ok(());
        }
        if attempt + 1 < LISTENER_OWNER_RECHECK_ATTEMPTS {
            thread::sleep(LISTENER_OWNER_RECHECK_DELAY);
            continue;
        }
        bail!(
            "App '{name}' listener on {target_host}:{port} is owned by process(es) {}, not spawned process group {child_pid}; refusing to publish process route",
            outside_group
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    bail!(
        "Listener ownership verification for app '{name}' on {target_host}:{port} exhausted without a result"
    )
}

#[cfg(not(unix))]
pub(super) fn verify_app_listener_owner(
    name: &str,
    target_host: &str,
    port: u16,
    _child_pid: u32,
) -> Result<()> {
    bail!(
        "Cannot verify listener owner for app '{name}' on {target_host}:{port}; refusing to publish process route on this platform"
    )
}

#[cfg(unix)]
fn process_group_id(pid: u32) -> Option<i32> {
    let pid = unix_pid(pid)?;
    let pgid = unsafe {
        // SAFETY: pid was range-checked by unix_pid. getpgid does not write
        // through pointers and reports missing or inaccessible processes as -1.
        libc::getpgid(pid)
    };
    (pgid != -1).then_some(pgid)
}

fn listener_target_addrs(target_host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    let addrs = (target_host, port)
        .to_socket_addrs()
        .with_context(|| format!("Failed to resolve app target {target_host}:{port}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        bail!("App target {target_host}:{port} did not resolve to any socket addresses");
    }
    Ok(addrs)
}

#[cfg(target_os = "linux")]
fn tcp_listener_pids(target_host: &str, port: u16) -> Result<Vec<u32>> {
    let target_addrs = listener_target_addrs(target_host, port)?;
    let socket_inodes = linux_tcp_listener_inodes(&target_addrs, port)?;
    if socket_inodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut pids = HashSet::new();
    for entry in fs::read_dir("/proc").context("Failed to read /proc")? {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = fs::read_dir(fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = fs::read_link(fd.path()) else {
                continue;
            };
            let Some(target) = target.to_str() else {
                continue;
            };
            let Some(inode) = target
                .strip_prefix("socket:[")
                .and_then(|rest| rest.strip_suffix(']'))
            else {
                continue;
            };
            if socket_inodes.contains(inode) {
                pids.insert(pid);
                break;
            }
        }
    }
    let mut pids: Vec<_> = pids.into_iter().collect();
    pids.sort_unstable();
    Ok(pids)
}

#[cfg(target_os = "linux")]
fn linux_tcp_listener_inodes(target_addrs: &[SocketAddr], port: u16) -> Result<HashSet<String>> {
    let mut inodes = HashSet::new();
    collect_linux_tcp_listener_inodes("/proc/net/tcp", target_addrs, port, &mut inodes)?;
    collect_linux_tcp_listener_inodes("/proc/net/tcp6", target_addrs, port, &mut inodes)?;
    Ok(inodes)
}

#[cfg(target_os = "linux")]
fn collect_linux_tcp_listener_inodes(
    path: &str,
    target_addrs: &[SocketAddr],
    port: u16,
    inodes: &mut HashSet<String>,
) -> Result<()> {
    let table = fs::read_to_string(path).with_context(|| format!("Failed to read {path}"))?;
    for line in table.lines().skip(1) {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() <= 9 || parts[3] != "0A" {
            continue;
        }
        let Some((local_ip_hex, local_port_hex)) = parts[1].rsplit_once(':') else {
            continue;
        };
        let Some(local_ip) = parse_linux_tcp_ip(local_ip_hex) else {
            continue;
        };
        let Ok(local_port) = u16::from_str_radix(local_port_hex, 16) else {
            continue;
        };
        if local_port == port && listen_ip_matches_targets(local_ip, target_addrs) {
            inodes.insert(parts[9].to_string());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn parse_linux_tcp_ip(hex: &str) -> Option<IpAddr> {
    match hex.len() {
        8 => {
            let raw = u32::from_str_radix(hex, 16).ok()?;
            // /proc/net/tcp[6] prints each __be32 word as a native-endian u32.
            // Native bytes recover the network-order address on both little-
            // and big-endian Linux hosts.
            Some(IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes())))
        }
        32 => {
            let mut bytes = [0u8; 16];
            for (index, chunk) in hex.as_bytes().chunks_exact(8).enumerate() {
                let chunk = std::str::from_utf8(chunk).ok()?;
                let word = u32::from_str_radix(chunk, 16).ok()?;
                bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_ne_bytes());
            }
            Some(IpAddr::V6(Ipv6Addr::from(bytes)))
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
pub(super) fn listen_ip_matches_targets(local_ip: IpAddr, target_addrs: &[SocketAddr]) -> bool {
    match local_ip {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            target_addrs.iter().any(|addr| addr.ip().is_ipv4())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            target_addrs.iter().any(|addr| addr.ip().is_ipv6())
        }
        _ => target_addrs.iter().any(|addr| addr.ip() == local_ip),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn tcp_listener_pids(target_host: &str, port: u16) -> Result<Vec<u32>> {
    #[cfg(not(target_os = "macos"))]
    {
        bail!(
            "Listener owner verification is not implemented on {} for {target_host}:{port}; refusing to publish process route. Use --no-proxy or `scripts/jig proxy alias` for manually managed loopback services on this platform.",
            std::env::consts::OS
        );
    }
    #[cfg(target_os = "macos")]
    {
        let mut pids = Vec::new();
        for selector in macos_lsof_selectors(target_host, port)? {
            let output = Command::new(lsof_command())
                .env_clear()
                .env("LC_ALL", "C")
                .args(["-nP", "-sTCP:LISTEN", "-Fp", selector.as_str()])
                .output()
                .with_context(|| {
                    format!(
                        "Failed to run lsof to inspect TCP listener owner for {target_host}:{port}"
                    )
                })?;
            if !output.status.success() {
                continue;
            }
            pids.extend(
                output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .filter_map(|line| line.strip_prefix(b"p"))
                    .filter_map(|pid| std::str::from_utf8(pid).ok())
                    .filter_map(|pid| pid.parse::<u32>().ok()),
            );
        }
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    }
}

#[cfg(target_os = "macos")]
fn lsof_command() -> &'static str {
    "/usr/sbin/lsof"
}

#[cfg(target_os = "macos")]
fn macos_lsof_selectors(target_host: &str, port: u16) -> Result<Vec<String>> {
    let mut ips = Vec::new();
    for addr in listener_target_addrs(target_host, port)? {
        let ip = addr.ip();
        if !ips.contains(&ip) {
            ips.push(ip);
        }
        let unspecified = match ip {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        };
        if !ips.contains(&unspecified) {
            ips.push(unspecified);
        }
    }
    Ok(ips
        .into_iter()
        .map(|ip| format!("-iTCP@{ip}:{port}"))
        .collect())
}

#[cfg(target_os = "macos")]
fn macos_pid_listens_on_target(pid: u32, target_host: &str, port: u16) -> Result<bool> {
    let pid_text = pid.to_string();
    for selector in macos_lsof_selectors(target_host, port)? {
        let output = Command::new(lsof_command())
            .env_clear()
            .env("LC_ALL", "C")
            .args([
                "-nP",
                "-sTCP:LISTEN",
                "-Fp",
                "-a",
                "-p",
                &pid_text,
                selector.as_str(),
            ])
            .output()
            .with_context(|| {
                format!("Failed to run lsof to recheck TCP listener owner for {target_host}:{port}")
            })?;
        if output.status.success()
            && output.stdout.split(|byte| *byte == b'\n').any(|line| {
                line.strip_prefix(b"p")
                    .and_then(|value| std::str::from_utf8(value).ok())
                    .is_some_and(|value| value == pid_text)
            })
        {
            return Ok(true);
        }
    }
    Ok(false)
}
