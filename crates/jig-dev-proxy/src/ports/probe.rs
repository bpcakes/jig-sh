use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::host::ip_is_loopback;

const MAX_HEALTH_RESPONSE_HEADER_BYTES: usize = 2048;
pub(crate) const CAPABILITIES_PATH: &str = "/__jig_proxy_capabilities";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProxyCapabilities {
    pub(crate) pid: u32,
    pub(crate) lan: bool,
    pub(crate) https: bool,
    pub(crate) http2: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilityProbe {
    Available(ProxyCapabilities),
    Unsupported,
    Unavailable,
    Invalid,
}

pub(crate) fn is_jig_proxy_http(host: &str, port: u16, health_token: Option<&str>) -> bool {
    jig_proxy_http_pid(host, port, health_token).is_some()
}

pub(crate) fn is_any_jig_proxy_http(host: &str, port: u16) -> bool {
    jig_proxy_http_probe(host, port, None).is_some_and(|probe| probe.is_jig_proxy)
}

pub(crate) fn jig_proxy_http_pid(host: &str, port: u16, health_token: Option<&str>) -> Option<u32> {
    let ip = host.parse::<IpAddr>().ok()?;
    if !ip_is_loopback(ip) {
        return None;
    }
    jig_proxy_http_probe_at(SocketAddr::new(ip, port), health_token)?.pid
}

pub(crate) fn jig_proxy_capabilities(host: &str, port: u16, health_token: &str) -> CapabilityProbe {
    let Ok(ip) = host.parse::<IpAddr>() else {
        return CapabilityProbe::Unavailable;
    };
    if !ip_is_loopback(ip) {
        return CapabilityProbe::Unavailable;
    }
    let Some(response) = read_probe_headers(
        SocketAddr::new(ip, port),
        CAPABILITIES_PATH,
        Some(health_token),
    ) else {
        return CapabilityProbe::Unavailable;
    };
    let status = response.lines().next().unwrap_or_default();
    let mut status_parts = status.split_ascii_whitespace();
    if !matches!(status_parts.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return CapabilityProbe::Invalid;
    }
    match status_parts.next() {
        Some("200") => parse_capabilities_response(&response)
            .map(CapabilityProbe::Available)
            .unwrap_or(CapabilityProbe::Invalid),
        Some("404") => CapabilityProbe::Unsupported,
        Some("503") | Some("403") => CapabilityProbe::Unavailable,
        _ => CapabilityProbe::Invalid,
    }
}

fn parse_capabilities_response(response: &str) -> Option<ProxyCapabilities> {
    let status = response.lines().next()?;
    let mut status_parts = status.split_ascii_whitespace();
    if !matches!(status_parts.next(), Some("HTTP/1.0" | "HTTP/1.1"))
        || status_parts.next() != Some("200")
        || header_value(response, "x-jig-proxy")? != "1"
        || header_value(response, "x-jig-proxy-capabilities-version")? != "1"
    {
        return None;
    }
    let pid = header_value(response, "x-jig-proxy-pid")?.parse().ok()?;
    let lan = parse_bool(header_value(response, "x-jig-proxy-lan")?)?;
    let https = parse_bool(header_value(response, "x-jig-proxy-https")?)?;
    let http2 = parse_bool(header_value(response, "x-jig-proxy-http2")?)?;
    if http2 && !https {
        return None;
    }
    Some(ProxyCapabilities {
        pid,
        lan,
        https,
        http2,
    })
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

struct JigProxyHealthProbe {
    is_jig_proxy: bool,
    pid: Option<u32>,
}

fn jig_proxy_http_probe(
    host: &str,
    port: u16,
    health_token: Option<&str>,
) -> Option<JigProxyHealthProbe> {
    let ip = host.parse::<IpAddr>().ok()?;
    if !ip_is_loopback(ip) {
        return None;
    }
    jig_proxy_http_probe_at(SocketAddr::new(ip, port), health_token)
}

fn jig_proxy_http_probe_at(
    addr: SocketAddr,
    health_token: Option<&str>,
) -> Option<JigProxyHealthProbe> {
    let response = read_probe_headers(addr, "/__jig_proxy_health", health_token)?;
    let is_jig_proxy = header_value(&response, "x-jig-proxy") == Some("1");
    let pid = is_jig_proxy
        .then(|| header_value(&response, "x-jig-proxy-pid"))
        .flatten()
        .and_then(|value| value.parse().ok());
    Some(JigProxyHealthProbe { is_jig_proxy, pid })
}

fn read_probe_headers(addr: SocketAddr, path: &str, health_token: Option<&str>) -> Option<String> {
    if health_token.is_some_and(|token| token.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))) {
        return None;
    }
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) else {
        return None;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let request = if let Some(token) = health_token {
        format!(
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nx-jig-proxy-health-token: {token}\r\n\r\n"
        )
    } else {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    };
    if stream.write_all(request.as_bytes()).is_err() {
        return None;
    }
    let mut response_bytes = Vec::new();
    let mut buffer = [0u8; 512];
    while response_bytes.len() < MAX_HEALTH_RESPONSE_HEADER_BYTES {
        let remaining = MAX_HEALTH_RESPONSE_HEADER_BYTES - response_bytes.len();
        let read_len = buffer.len().min(remaining);
        let Ok(n) = stream.read(&mut buffer[..read_len]) else {
            return None;
        };
        if n == 0 {
            break;
        }
        response_bytes.extend_from_slice(&buffer[..n]);
        if response_bytes
            .windows(4)
            .any(|window| window == b"\r\n\r\n")
        {
            break;
        }
    }
    let header_end = response_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)?;
    String::from_utf8(response_bytes[..header_end].to_vec()).ok()
}

fn header_value<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    let mut found = None;
    for line in response.split("\r\n").skip(1) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.eq_ignore_ascii_case(name) {
            if found.is_some() {
                return None;
            }
            found = Some(value.trim());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_parser_rejects_missing_duplicate_and_invalid_evidence() {
        let valid = "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: 42\r\nx-jig-proxy-capabilities-version: 1\r\nx-jig-proxy-lan: 0\r\nx-jig-proxy-https: 1\r\nx-jig-proxy-http2: 1\r\n\r\n";
        assert_eq!(
            parse_capabilities_response(valid),
            Some(ProxyCapabilities {
                pid: 42,
                lan: false,
                https: true,
                http2: true,
            })
        );
        for invalid in [
            valid.replace("200 OK", "404 Not Found"),
            valid.replace("capabilities-version: 1", "capabilities-version: 2"),
            valid.replace("x-jig-proxy-lan: 0\r\n", ""),
            valid.replace("x-jig-proxy-lan: 0", "x-jig-proxy-lan: true"),
            valid.replace(
                "x-jig-proxy-pid: 42",
                "x-jig-proxy-pid: 42\r\nx-jig-proxy-pid: 43",
            ),
            valid.replace("x-jig-proxy-https: 1", "x-jig-proxy-https: 0"),
        ] {
            assert!(parse_capabilities_response(&invalid).is_none(), "{invalid}");
        }
    }
}
