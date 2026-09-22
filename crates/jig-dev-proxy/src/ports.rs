use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use anyhow::{Context, Result, bail};

const MIN_APP_PORT: u16 = 4000;
const MAX_APP_PORT: u16 = 4999;

pub(crate) fn find_free_app_port_excluding(host: &str, reserved: &HashSet<u16>) -> Result<u16> {
    let target_ips = resolve_target_ips(host, MIN_APP_PORT)
        .with_context(|| format!("Could not resolve app target host '{host}'"))?;
    for port in MIN_APP_PORT..=MAX_APP_PORT {
        if !reserved.contains(&port) && target_ips.iter().all(|ip| port_is_free_on_ip(*ip, port)) {
            return Ok(port);
        }
    }
    bail!("No free app port found in range {MIN_APP_PORT}-{MAX_APP_PORT}")
}

pub(crate) fn is_port_free(host: &str, port: u16) -> bool {
    port_is_free(host, port).unwrap_or(false)
}

pub(crate) fn port_is_free(host: &str, port: u16) -> Result<bool> {
    let target_ips = resolve_target_ips(host, port)
        .with_context(|| format!("Could not resolve app target host '{host}'"))?;
    Ok(target_ips.iter().all(|ip| port_is_free_on_ip(*ip, port)))
}

fn resolve_target_ips(host: &str, port: u16) -> Result<Vec<IpAddr>> {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        bail!("host did not resolve");
    };
    let mut ips = Vec::new();
    for addr in addrs {
        if !ips.contains(&addr.ip()) {
            ips.push(addr.ip());
        }
    }
    if ips.is_empty() {
        bail!("host did not resolve to any socket addresses");
    }
    Ok(ips)
}

fn port_is_free_on_ip(ip: IpAddr, port: u16) -> bool {
    TcpListener::bind(SocketAddr::new(ip, port)).is_ok()
}

pub(crate) fn is_tcp_listening(host: &str, port: u16) -> bool {
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok())
}

mod probe;
pub(crate) use probe::{
    CAPABILITIES_PATH, CapabilityProbe, ProxyCapabilities, is_any_jig_proxy_http,
    is_jig_proxy_http, jig_proxy_capabilities, jig_proxy_http_pid,
};

pub(crate) fn local_lan_ip_for_ipv4_listener() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // UDP connect selects the outbound interface for this route without
    // sending application data to the address. The LAN listener currently
    // binds 0.0.0.0, so do not advertise IPv6 addresses here.
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip_is_link_local(ip) {
        None
    } else {
        Some(ip)
    }
}

const fn ip_is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn finds_free_app_port() {
        let port = find_free_app_port_excluding("127.0.0.1", &HashSet::new()).unwrap();
        assert!((MIN_APP_PORT..=MAX_APP_PORT).contains(&port));
    }

    #[test]
    fn free_port_search_reports_resolution_errors() {
        let error = find_free_app_port_excluding("bad host name", &HashSet::new())
            .unwrap_err()
            .to_string();

        assert!(error.contains("Could not resolve app target host"));
    }

    #[test]
    fn port_probe_checks_all_resolved_target_addresses() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let localhost_resolves_to_bound_addr = ("localhost", port)
            .to_socket_addrs()
            .unwrap()
            .any(|addr| addr.ip().is_loopback() && addr.is_ipv4());

        if localhost_resolves_to_bound_addr {
            assert!(!is_port_free("localhost", port));
        }
    }

    #[test]
    fn health_probe_reads_fragmented_response_headers() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 512];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\n")
                .unwrap();
            thread::sleep(Duration::from_millis(20));
            stream
                .write_all(b"x-jig-proxy-pid: 4242\r\ncontent-length: 0\r\n\r\n")
                .unwrap();
        });

        assert_eq!(jig_proxy_http_pid("127.0.0.1", port, None), Some(4242));
        handle.join().unwrap();
    }

    #[test]
    fn health_probe_sends_token_header() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 512];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.contains("x-jig-proxy-health-token: abc123\r\n"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: 4242\r\ncontent-length: 0\r\n\r\n",
                )
                .unwrap();
        });

        assert_eq!(
            jig_proxy_http_pid("127.0.0.1", port, Some("abc123")),
            Some(4242)
        );
        handle.join().unwrap();
    }

    #[test]
    fn health_probe_rejects_header_breaks_in_token() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();

        assert_eq!(
            jig_proxy_http_pid("127.0.0.1", port, Some("abc\r\nx-bad: 1")),
            None
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn unauthenticated_health_probe_detects_jig_proxy_header_without_pid() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 512];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nx-jig-proxy: 1\r\ncontent-length: 9\r\n\r\nForbidden",
                )
                .unwrap();
        });

        assert!(is_any_jig_proxy_http("127.0.0.1", port));
        assert_eq!(jig_proxy_http_pid("127.0.0.1", port, None), None);
        handle.join().unwrap();
    }

    #[test]
    fn health_probe_requires_loopback_ip_literal() {
        assert_eq!(jig_proxy_http_pid("localhost", 1355, None), None);
        assert_eq!(jig_proxy_http_pid("192.0.2.10", 1355, None), None);
    }
}
