use std::error::Error;
use std::fmt;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::BufReader;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::test_tempdir as tempdir;
#[cfg(unix)]
use http_body_util::Empty;
use hyper::header::HeaderValue;
#[cfg(unix)]
use rustls::pki_types::ServerName;
use tokio::time::{sleep, timeout};
#[cfg(unix)]
use tokio_rustls::TlsConnector;

use super::*;
use crate::state::now_ms;
use crate::types::RouteMode;

#[test]
fn websocket_detection_requires_connection_upgrade() {
    let mut headers = HeaderMap::new();
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    headers.insert("sec-websocket-key", HeaderValue::from_static("abc"));
    headers.insert("sec-websocket-version", HeaderValue::from_static("13"));
    assert!(!is_websocket(&headers));

    headers.insert(CONNECTION, HeaderValue::from_static("keep-alive, Upgrade"));
    assert!(is_websocket(&headers));

    headers.remove("sec-websocket-key");
    assert!(!is_websocket(&headers));
}
#[test]
fn websocket_upgrade_rejects_request_bodies() {
    let mut headers = HeaderMap::new();
    assert!(!websocket_request_has_body(&headers));

    headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
    assert!(!websocket_request_has_body(&headers));

    headers.insert(CONTENT_LENGTH, HeaderValue::from_static("1"));
    assert!(websocket_request_has_body(&headers));

    headers.remove(CONTENT_LENGTH);
    headers.insert(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));
    assert!(websocket_request_has_body(&headers));
}

#[test]
fn websocket_accept_header_must_match_request_key() {
    let expected = websocket_accept_for_key("client-key");
    let valid = vec![(
        HeaderName::from_static("sec-websocket-accept"),
        HeaderValue::from_str(&expected).unwrap(),
    )];
    assert!(websocket_accept_matches(&valid, &expected));

    let invalid = vec![(
        HeaderName::from_static("sec-websocket-accept"),
        HeaderValue::from_static("wrong"),
    )];
    assert!(!websocket_accept_matches(&invalid, &expected));

    let duplicate = vec![
        (
            HeaderName::from_static("sec-websocket-accept"),
            HeaderValue::from_str(&expected).unwrap(),
        ),
        (
            HeaderName::from_static("sec-websocket-accept"),
            HeaderValue::from_str(&expected).unwrap(),
        ),
    ];
    assert!(!websocket_accept_matches(&duplicate, &expected));
}

#[test]
fn websocket_accept_request_requires_single_key() {
    let mut request = HeaderMap::new();
    request.insert("sec-websocket-key", HeaderValue::from_static("a"));
    assert_eq!(
        websocket_accept_for_request(&request).unwrap(),
        websocket_accept_for_key("a")
    );

    request.append("sec-websocket-key", HeaderValue::from_static("b"));
    let error = websocket_accept_for_request(&request)
        .unwrap_err()
        .to_string();
    assert!(error.contains("conflicting Sec-WebSocket-Key"));
}

#[test]
fn websocket_extensions_allow_backend_subset_of_client_request() {
    let mut request = HeaderMap::new();
    request.insert(
        "sec-websocket-extensions",
        HeaderValue::from_static("permessage-deflate; client_max_window_bits, x-dev"),
    );
    let response = vec![(
        HeaderName::from_static("sec-websocket-extensions"),
        HeaderValue::from_static("permessage-deflate"),
    )];

    assert!(websocket_extensions_allowed(&request, &response));
}

#[test]
fn websocket_extensions_reject_unrequested_backend_extension() {
    let request = HeaderMap::new();
    let response = vec![(
        HeaderName::from_static("sec-websocket-extensions"),
        HeaderValue::from_static("permessage-deflate"),
    )];

    assert!(!websocket_extensions_allowed(&request, &response));
}

#[test]
fn websocket_subprotocol_must_match_client_request() {
    let mut request = HeaderMap::new();
    request.insert(
        "sec-websocket-protocol",
        HeaderValue::from_static("chat, superchat"),
    );
    let selected = vec![(
        HeaderName::from_static("sec-websocket-protocol"),
        HeaderValue::from_static("superchat"),
    )];
    let unrequested = vec![(
        HeaderName::from_static("sec-websocket-protocol"),
        HeaderValue::from_static("admin"),
    )];
    let duplicate = vec![
        (
            HeaderName::from_static("sec-websocket-protocol"),
            HeaderValue::from_static("chat"),
        ),
        (
            HeaderName::from_static("sec-websocket-protocol"),
            HeaderValue::from_static("superchat"),
        ),
    ];

    assert!(websocket_subprotocol_allowed(&request, &selected));
    assert!(!websocket_subprotocol_allowed(&request, &unrequested));
    assert!(!websocket_subprotocol_allowed(&request, &duplicate));
}

#[test]
fn health_request_requires_loopback_client_and_host() {
    let loopback = "127.0.0.1".parse().unwrap();
    let remote = "192.168.1.50".parse().unwrap();
    let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let localhost = Request::builder()
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let loopback_literal = Request::builder()
        .header(HOST, "127.0.0.1:1355")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let ipv6_loopback = Request::builder()
        .header(HOST, "[::1]:1355")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let malformed_ipv6_loopback = Request::builder()
        .header(HOST, "[::1]evil")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let routed_host = Request::builder()
        .header(HOST, "web.demo.localhost")
        .header("x-jig-proxy-health-token", token)
        .body(())
        .unwrap();
    let wrong_token = Request::builder()
        .header(HOST, "localhost")
        .header("x-jig-proxy-health-token", "wrong")
        .body(())
        .unwrap();
    let missing_token = Request::builder()
        .header(HOST, "localhost")
        .body(())
        .unwrap();

    assert!(health_request_allowed(
        &localhost, loopback, loopback, token
    ));
    assert!(health_request_allowed(
        &loopback_literal,
        loopback,
        loopback,
        token
    ));
    assert!(health_request_allowed(
        &ipv6_loopback,
        "::1".parse().unwrap(),
        "::1".parse().unwrap(),
        token
    ));
    assert!(health_request_allowed(
        &localhost,
        "::ffff:127.0.0.1".parse().unwrap(),
        "::ffff:127.0.0.1".parse().unwrap(),
        token
    ));
    assert!(!health_request_allowed(&localhost, remote, loopback, token));
    assert!(!health_request_allowed(&localhost, loopback, remote, token));
    assert!(!health_request_allowed(
        &malformed_ipv6_loopback,
        loopback,
        loopback,
        token
    ));
    assert!(!health_request_allowed(
        &routed_host,
        loopback,
        loopback,
        token
    ));
    assert!(!health_request_allowed(
        &wrong_token,
        loopback,
        loopback,
        token
    ));
    assert!(!health_request_allowed(
        &missing_token,
        loopback,
        loopback,
        token
    ));
    assert!(!constant_time_ascii_eq("health-token-prefix", token));
    assert!(!constant_time_ascii_eq("short", "short"));
    assert!(constant_time_ascii_eq(token, token));
}

#[test]
fn request_host_rejects_conflicting_authority_and_host() {
    let request = Request::builder()
        .uri("https://web.demo.localhost/")
        .header(HOST, "api.demo.localhost")
        .body(())
        .unwrap();

    let error = request_host(&request).unwrap_err().to_string();
    assert!(error.contains("Conflicting request authority"));
}

#[test]
fn request_host_rejects_duplicate_host_headers() {
    let mut request = Request::builder()
        .header(HOST, "web.demo.localhost")
        .body(())
        .unwrap();
    request
        .headers_mut()
        .append(HOST, HeaderValue::from_static("api.demo.localhost"));

    let error = request_host(&request).unwrap_err().to_string();

    assert!(error.contains("Conflicting request Host headers"));
}

#[test]
fn invalid_request_host_maps_to_bad_request_response() {
    let request = Request::builder().body(()).unwrap();

    let response = request_host_or_bad_request(&request).unwrap_err();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
}

#[test]
fn request_host_prefers_matching_authority() {
    let request = Request::builder()
        .uri("https://web.demo.localhost/")
        .header(HOST, "web.demo.localhost")
        .body(())
        .unwrap();

    assert_eq!(request_host(&request).unwrap(), "web.demo.localhost");
}

#[test]
fn target_authority_brackets_ipv6_literals() {
    assert_eq!(target_authority_host("::1"), "[::1]");
    assert_eq!(target_authority_host("127.0.0.1"), "127.0.0.1");
    assert_eq!(target_authority_host("localhost"), "localhost");
}

#[test]
fn chunked_backend_body_is_decoded() {
    let body = decode_chunked_body(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").unwrap();
    assert_eq!(body, Bytes::from_static(b"hello world"));
}

#[test]
fn backend_status_requires_http_prefix() {
    assert_eq!(
        parse_backend_status(b"HTTP/1.1 204 No Content").unwrap(),
        StatusCode::NO_CONTENT
    );
    let error = parse_backend_status(b"BOGUS 200 OK")
        .unwrap_err()
        .to_string();
    assert!(error.contains("HTTP/"));
}

#[test]
fn backend_status_rejects_non_standard_spacing() {
    assert!(parse_backend_status(b"HTTP/1.1\t204 No Content").is_err());
    assert!(parse_backend_status(b"HTTP/1.1  204 No Content").is_err());
    assert!(parse_backend_status(b"HTTP/1.1 204\tNo Content").is_err());
}

#[test]
fn chunked_scanner_advances_across_complete_chunks() {
    let mut scanner = ChunkedMessageScanner::default();
    assert_eq!(scanner.scan(b"5\r\nhello\r\n").unwrap(), None);
    assert_eq!(
        scanner.scan(b"5\r\nhello\r\n0\r\n\r\n").unwrap(),
        Some("5\r\nhello\r\n0\r\n\r\n".len())
    );
}

#[test]
fn chunked_scanner_does_not_count_pending_data_as_header() {
    let mut scanner = ChunkedMessageScanner::default();
    let mut raw = b"3000\r\n".to_vec();
    raw.extend(std::iter::repeat_n(b'a', MAX_CHUNK_HEADER_BYTES + 1));

    assert_eq!(scanner.scan(&raw).unwrap(), None);

    raw.resize("3000\r\n".len() + 0x3000, b'a');
    raw.extend_from_slice(b"\r\n0\r\n\r\n");

    assert_eq!(scanner.scan(&raw).unwrap(), Some(raw.len()));
}

#[test]
fn backend_content_length_rejects_duplicate_values() {
    let headers = vec![
        (CONTENT_LENGTH, HeaderValue::from_static("5")),
        (CONTENT_LENGTH, HeaderValue::from_static("6")),
    ];

    let error = content_length(&headers).unwrap_err().to_string();

    assert!(error.contains("multiple Content-Length"));

    let error = content_length(&[(CONTENT_LENGTH, HeaderValue::from_static("5, 5"))])
        .unwrap_err()
        .to_string();

    assert!(error.contains("multiple Content-Length"));
}

#[test]
fn disconnect_error_uses_io_error_kind() {
    let broken_pipe = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed");
    assert!(is_disconnect_error(&broken_pipe));

    let misleading_message = std::io::Error::other("contains broken pipe text");
    assert!(!is_disconnect_error(&misleading_message));
}

#[derive(Debug)]
struct WrappedError {
    source: std::io::Error,
}

impl fmt::Display for WrappedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wrapped error")
    }
}

impl Error for WrappedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

#[test]
fn disconnect_error_checks_nested_sources() {
    let wrapped = WrappedError {
        source: std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"),
    };

    assert!(is_disconnect_error(&wrapped));
}

#[test]
fn removes_standard_and_connection_named_hop_by_hop_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(CONNECTION, HeaderValue::from_static("x-debug-hop"));
    headers.insert("x-debug-hop", HeaderValue::from_static("1"));
    headers.insert("te", HeaderValue::from_static("trailers"));
    headers.insert("trailer", HeaderValue::from_static("x-trailer"));
    headers.insert("proxy-connection", HeaderValue::from_static("keep-alive"));
    headers.insert("proxy-authorization", HeaderValue::from_static("secret"));
    headers.insert("x-end-to-end", HeaderValue::from_static("keep"));

    remove_hop_by_hop_headers(&mut headers);

    assert!(!headers.contains_key(CONNECTION));
    assert!(!headers.contains_key("x-debug-hop"));
    assert!(!headers.contains_key("te"));
    assert!(!headers.contains_key("trailer"));
    assert!(!headers.contains_key("proxy-connection"));
    assert!(!headers.contains_key("proxy-authorization"));
    assert_eq!(
        headers.get("x-end-to-end"),
        Some(&HeaderValue::from_static("keep"))
    );
}

#[test]
fn removes_backend_owned_jig_proxy_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-jig-proxy", HeaderValue::from_static("backend"));
    headers.insert("x-jig-proxy-pid", HeaderValue::from_static("1234"));
    headers.insert("x-jig-proxy-hops", HeaderValue::from_static("7"));
    headers.insert("x-end-to-end", HeaderValue::from_static("keep"));

    remove_jig_proxy_headers(&mut headers);

    assert!(!headers.contains_key("x-jig-proxy"));
    assert!(!headers.contains_key("x-jig-proxy-pid"));
    assert!(!headers.contains_key("x-jig-proxy-hops"));
    assert_eq!(
        headers.get("x-end-to-end"),
        Some(&HeaderValue::from_static("keep"))
    );
}

#[test]
fn rewrite_proxy_headers_increments_inbound_proxy_hop_header() {
    let mut headers = HeaderMap::new();
    headers.insert(CONNECTION, HeaderValue::from_static("x-jig-proxy-hops"));
    headers.insert("x-jig-proxy-hops", HeaderValue::from_static("2"));
    let route = Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    rewrite_proxy_headers(
        &mut headers,
        "127.0.0.1:5000".parse().unwrap(),
        &route,
        false,
        Version::HTTP_11,
    )
    .unwrap();

    assert!(!headers.contains_key(CONNECTION));
    assert_eq!(
        headers.get("x-jig-proxy-hops"),
        Some(&HeaderValue::from_static("3"))
    );
    assert_eq!(headers.get(VIA), Some(&HeaderValue::from_static(VIA_VALUE)));
}

#[test]
fn rewrite_proxy_headers_rejects_excessive_proxy_hops() {
    let mut headers = HeaderMap::new();
    headers.insert("x-jig-proxy-hops", HeaderValue::from_static("8"));
    let route = Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    let error = rewrite_proxy_headers(
        &mut headers,
        "127.0.0.1:5000".parse().unwrap(),
        &route,
        false,
        Version::HTTP_11,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("hop limit"));
}

#[test]
fn rewrite_proxy_headers_replaces_inbound_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.10"));
    headers.insert("x-forwarded-port", HeaderValue::from_static("443"));
    headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.11"));
    headers.insert("forwarded", HeaderValue::from_static("for=203.0.113.10"));
    let route = Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    rewrite_proxy_headers(
        &mut headers,
        "127.0.0.1:5000".parse().unwrap(),
        &route,
        false,
        Version::HTTP_11,
    )
    .unwrap();

    assert_eq!(
        headers.get("x-forwarded-for"),
        Some(&HeaderValue::from_static("127.0.0.1"))
    );
    assert_eq!(
        headers.get(HOST),
        Some(&HeaderValue::from_static("web.demo.localhost"))
    );
    assert!(!headers.contains_key("forwarded"));
    assert!(!headers.contains_key("x-forwarded-port"));
    assert!(!headers.contains_key("x-real-ip"));
}

#[test]
fn rewrite_proxy_headers_removes_inbound_proxy_owned_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-jig-proxy", HeaderValue::from_static("spoofed"));
    headers.insert("x-jig-proxy-pid", HeaderValue::from_static("9999"));
    headers.insert("x-jig-proxy-hops", HeaderValue::from_static("2"));
    let route = Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    rewrite_proxy_headers(
        &mut headers,
        "127.0.0.1:5000".parse().unwrap(),
        &route,
        false,
        Version::HTTP_11,
    )
    .unwrap();

    assert!(!headers.contains_key("x-jig-proxy"));
    assert!(!headers.contains_key("x-jig-proxy-pid"));
    assert_eq!(
        headers.get("x-jig-proxy-hops"),
        Some(&HeaderValue::from_static("3"))
    );
}

#[test]
fn request_content_length_limit_rejects_oversized_requests() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&(MAX_BACKEND_REQUEST_BODY_BYTES as u64 + 1).to_string()).unwrap(),
    );

    assert!(
        request_content_length(&headers).unwrap().unwrap() > MAX_BACKEND_REQUEST_BODY_BYTES as u64
    );

    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&(MAX_BACKEND_REQUEST_BODY_BYTES as u64).to_string()).unwrap(),
    );

    assert!(
        request_content_length(&headers).unwrap().unwrap() <= MAX_BACKEND_REQUEST_BODY_BYTES as u64
    );
}

#[test]
fn request_content_length_rejects_conflicting_duplicates() {
    let mut headers = HeaderMap::new();
    headers.append(CONTENT_LENGTH, HeaderValue::from_static("5"));
    headers.append(CONTENT_LENGTH, HeaderValue::from_static("7"));

    let error = request_content_length(&headers).unwrap_err().to_string();
    assert!(error.contains("conflicting Content-Length"));

    let mut matching = HeaderMap::new();
    matching.append(CONTENT_LENGTH, HeaderValue::from_static("5"));
    matching.append(CONTENT_LENGTH, HeaderValue::from_static("5"));
    assert_eq!(request_content_length(&matching).unwrap(), Some(5));
}

#[test]
fn request_content_length_rejects_transfer_encoding_conflicts() {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_LENGTH, HeaderValue::from_static("5"));
    headers.insert(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));

    let error = request_content_length(&headers).unwrap_err().to_string();

    assert!(error.contains("Content-Length and Transfer-Encoding"));
}

#[test]
fn rewrite_proxy_headers_appends_via() {
    let mut headers = HeaderMap::new();
    headers.insert(VIA, HeaderValue::from_static("1.0 upstream"));
    let route = Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    rewrite_proxy_headers(
        &mut headers,
        "127.0.0.1:5000".parse().unwrap(),
        &route,
        false,
        Version::HTTP_2,
    )
    .unwrap();

    let via = headers
        .get_all(VIA)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(via, vec!["1.0 upstream", "2.0 jig"]);
}

#[test]
fn backend_header_parser_preserves_obs_text_values() {
    let raw = b"HTTP/1.1 200 OK\r\nx-binary: \xFF\r\n\r\n";
    let end = find_header_end(raw, 0).unwrap();
    let header_block = &raw[..end - 4];
    let status_line_end = find_crlf(header_block, 0).unwrap();

    let headers = parse_backend_headers(&header_block[status_line_end..]).unwrap();

    assert_eq!(headers[0].0, HeaderName::from_static("x-binary"));
    assert_eq!(headers[0].1.as_bytes(), b"\xFF");
}

#[test]
fn backend_header_parser_rejects_whitespace_before_colon() {
    let error = parse_backend_headers(b"x-bad : value\r\n")
        .unwrap_err()
        .to_string();

    assert!(error.contains("whitespace before ':'"));
}

#[test]
fn backend_header_parser_rejects_bare_cr_in_values() {
    let error = parse_backend_headers(b"x-bad: value\rcarry\r\n")
        .unwrap_err()
        .to_string();

    assert!(error.contains("bare CR or LF"));
}

#[test]
fn backend_header_parser_caps_header_count() {
    let raw = "x-a: b\r\n".repeat(MAX_BACKEND_HEADER_COUNT + 1);

    let error = parse_backend_headers(raw.as_bytes())
        .unwrap_err()
        .to_string();

    assert!(error.contains("exceeded"));
}

#[test]
fn chunked_body_parser_rejects_overflowing_size() {
    let error = decode_chunked_body(b"ffffffffffffffff\r\n")
        .unwrap_err()
        .to_string();

    assert!(error.contains("overflowed"));
}

#[test]
fn non_loopback_clients_cannot_use_remote_target_aliases() {
    let route = Route {
        hostname: "remote.demo.localhost".into(),
        target_host: "10.0.0.5".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    assert!(!route_allowed_for_remote_client(
        &route,
        "192.168.1.50".parse().unwrap()
    ));
    assert!(route_allowed_for_remote_client(
        &route,
        "127.0.0.1".parse().unwrap()
    ));
}

#[test]
fn route_targets_must_be_ip_literals_at_serve_time() {
    let route = Route {
        hostname: "remote.demo.localhost".into(),
        target_host: "example.com".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    assert!(!route_allowed_for_remote_client(
        &route,
        "127.0.0.1".parse().unwrap()
    ));
    assert!(!route_allowed_for_remote_client(
        &route,
        "192.168.1.50".parse().unwrap()
    ));
}

#[test]
fn non_loopback_clients_cannot_use_loopback_target_aliases() {
    let route = Route {
        hostname: "api.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    assert!(!route_allowed_for_remote_client(
        &route,
        "192.168.1.50".parse().unwrap()
    ));
}

#[test]
fn non_loopback_clients_can_use_loopback_target_process_routes() {
    let route = Route {
        hostname: "api.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: Some(std::process::id()),
        owner_start_token: None,
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };

    assert!(route_allowed_for_remote_client(
        &route,
        "192.168.1.50".parse().unwrap()
    ));
}

#[test]
fn route_targets_active_proxy_listener_rejects_loopback_self_route() {
    let route = Route {
        hostname: "api.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4300,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    assert!(route_targets_active_proxy_listener(
        &route,
        "127.0.0.1".parse().unwrap(),
        None,
        &[4300]
    ));
    assert!(!route_targets_active_proxy_listener(
        &route,
        "127.0.0.1".parse().unwrap(),
        None,
        &[4301]
    ));
}

#[test]
fn route_targets_active_proxy_listener_uses_cached_lan_ip() {
    let route = Route {
        hostname: "api.demo.localhost".into(),
        target_host: "192.0.2.10".into(),
        target_port: 4300,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    };

    assert!(route_targets_active_proxy_listener(
        &route,
        "127.0.0.1".parse().unwrap(),
        Some("192.0.2.10".parse().unwrap()),
        &[4300]
    ));
}

#[test]
fn non_loopback_clients_cannot_use_remote_target_process_routes() {
    let route = Route {
        hostname: "web.demo.localhost".into(),
        target_host: "10.0.0.5".into(),
        target_port: 4000,
        owner_pid: Some(std::process::id()),
        owner_start_token: None,
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };

    assert!(!route_allowed_for_remote_client(
        &route,
        "192.168.1.50".parse().unwrap()
    ));
    assert!(route_allowed_for_remote_client(
        &route,
        "127.0.0.1".parse().unwrap()
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn tls_cache_reloads_after_freshness_ttl() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    crate::certs::ensure_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let cache = TlsCache::new(store, true);
    cache.acceptor().await.unwrap();

    let stale_loaded_at = Instant::now() - ROUTE_CACHE_MAX_AGE - Duration::from_millis(1);
    {
        let mut inner = cache.inner.write().unwrap();
        inner.loaded_at = Some(stale_loaded_at);
    }

    cache.acceptor().await.unwrap();
    assert!(cache.inner.read().unwrap().loaded_at.unwrap() > stale_loaded_at);
}

#[cfg(unix)]
#[test]
fn tls_acceptor_rejects_symlinked_key_file() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    crate::certs::ensure_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();
    let store = StateStore::resolve(settings.state_dir).unwrap();
    let key_path = store.leaf_key_path();
    let outside_key = temp.path().join("outside-leaf-key.pem");
    fs::rename(&key_path, &outside_key).unwrap();
    symlink(&outside_key, &key_path).unwrap();

    let result = tls_acceptor(&store, false);

    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("Failed to open TLS certificate file")
    );
}

#[cfg(unix)]
#[test]
fn tls_acceptor_retries_transient_key_pair_mismatch() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    crate::certs::ensure_for_hosts(&settings, &["web.demo.localhost".into()]).unwrap();
    let store = StateStore::resolve(settings.state_dir).unwrap();
    let key_path = store.leaf_key_path();
    let original_key = fs::read_to_string(&key_path).unwrap();
    fs::write(
        &key_path,
        rcgen::KeyPair::generate().unwrap().serialize_pem(),
    )
    .unwrap();

    let mut retry_count = 0;
    tls::tls_acceptor_with_retry(&store, false, || {
        retry_count += 1;
        fs::write(&key_path, &original_key).unwrap();
    })
    .unwrap();

    assert_eq!(retry_count, 1);
}

#[tokio::test]
async fn backend_headers_reject_truncated_response() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_backend_headers(&mut stream)
            .await
            .unwrap_err()
            .to_string()
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"HTTP/1.1 101 Switching").await.unwrap();
    drop(client);

    let error = server.await.unwrap();
    assert!(error.contains("before completing response headers"));
}

#[tokio::test]
async fn backend_headers_reject_malformed_status_line() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_backend_headers(&mut stream)
            .await
            .unwrap_err()
            .to_string()
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"NOTHTTP\r\n\r\n").await.unwrap();

    let error = server.await.unwrap();
    assert!(error.contains("HTTP/"));
}

#[cfg(unix)]
#[tokio::test]
async fn https_proxy_serves_h2_requests() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        http2: true,
        ..ProxySettings::default()
    };
    let hostname = "web.demo.localhost";
    crate::certs::ensure_for_hosts(&settings, &[hostname.to_string()]).unwrap();
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let backend_port = backend.local_addr().unwrap().port();
    let backend_task = tokio::spawn(async move {
        let (mut stream, _) = backend.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\n\r\nh2-ok\n")
            .await
            .unwrap();
    });
    store
        .add_route(Route {
            hostname: hostname.into(),
            target_host: "127.0.0.1".into(),
            target_port: backend_port,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let route_cache = RouteCache::new(store.clone());
    let tls_cache = TlsCache::new(store.clone(), true);
    let server = tokio::spawn(async move {
        serve_https(
            listener,
            route_cache,
            tls_cache,
            ProxyLimits::new(),
            true,
            ListenerContext {
                proxy_port: addr.port(),
                lan_ip: None,
                health_token: Arc::from("test-health-token"),
            },
        )
        .await
        .unwrap();
    });

    let mut roots = rustls::RootCertStore::empty();
    let cert_file = std::fs::File::open(store.ca_path()).unwrap();
    for cert in rustls_pemfile::certs(&mut BufReader::new(cert_file)) {
        roots.add(cert.unwrap()).unwrap();
    }
    let mut client_config = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::aws_lc_rs::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    client_config.alpn_protocols = vec![b"h2".to_vec()];

    let connector = TlsConnector::from(Arc::new(client_config));
    let server_name = ServerName::try_from(hostname).unwrap().to_owned();
    let stream = TcpStream::connect(addr).await.unwrap();
    let tls = connector.connect(server_name, stream).await.unwrap();
    assert_eq!(tls.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tls))
            .await
            .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri(format!("https://{hostname}/"))
        .header(HOST, hostname)
        .body(Empty::<Bytes>::new())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"h2-ok\n"));
    backend_task.await.unwrap();
    server.abort();
}

#[tokio::test]
async fn https_not_found_uses_bound_proxy_port() {
    let routes = vec![Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    }];

    let response = not_found_response(&routes, "missing.demo.localhost", 1443, true, true);
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    let collected = response.into_body().collect().await.unwrap();
    let text = String::from_utf8(collected.to_bytes().to_vec()).unwrap();
    assert!(text.contains("<h1>ROUTE<span>NOT FOUND.</span></h1>"));
    assert!(text.contains("<span class=\"brand-sub\">development proxy</span>"));
    assert!(!text.contains("harness · runtime · contract"));
    assert!(!text.contains("class=\"tape\""));
    assert!(text.contains("href=\"https://web.demo.localhost:1443\""));
    assert!(!text.contains(":443"));
}

#[tokio::test]
async fn error_pages_escape_untrusted_message_content() {
    let response = error_response(
        StatusCode::BAD_REQUEST,
        "Invalid host <script>alert('nope')</script>",
    );
    let collected = response.into_body().collect().await.unwrap();
    let text = String::from_utf8(collected.to_bytes().to_vec()).unwrap();

    assert!(text.contains("&#60;script&#62;alert(&#39;nope&#39;)&#60;/script&#62;"));
    assert!(!text.contains("<script>alert"));
}

#[tokio::test]
async fn not_found_escapes_requested_host() {
    let response = not_found_response(&[], "<script.example>", 1355, false, true);
    let collected = response.into_body().collect().await.unwrap();
    let text = String::from_utf8(collected.to_bytes().to_vec()).unwrap();

    assert!(text.contains("<code>&#60;script.example&#62;</code>"));
    assert!(!text.contains("<code><script"));
}

#[tokio::test]
async fn not_found_hides_routes_for_non_loopback_clients() {
    let routes = vec![Route {
        hostname: "web.demo.localhost".into(),
        target_host: "127.0.0.1".into(),
        target_port: 4000,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    }];

    let response = not_found_response(&routes, "missing.demo.localhost", 1355, false, false);
    let collected = response.into_body().collect().await.unwrap();
    let text = String::from_utf8(collected.to_bytes().to_vec()).unwrap();
    assert!(text.contains("hidden"));
    assert!(!text.contains("web.demo.localhost"));
}

#[tokio::test]
async fn request_limit_exhaustion_returns_service_unavailable() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_http(
        listener,
        RouteCache::new(store),
        ProxyLimits {
            connections: Arc::new(Semaphore::new(8)),
            requests: Arc::new(Semaphore::new(0)),
            websockets: Arc::new(Semaphore::new(8)),
        },
        ListenerContext {
            proxy_port: addr.port(),
            lan_ip: None,
            health_token: Arc::from("test-health-token"),
        },
    ));

    let mut client = TcpStream::connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: web.demo.localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), client.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8_lossy(&received);

    assert!(response.contains("503 Service Unavailable"));
    assert!(response.contains("Too many proxy requests are active."));
    server.abort();
}

#[tokio::test]
async fn http_proxy_streams_backend_response() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let backend_port = backend.local_addr().unwrap().port();
    let backend_task = tokio::spawn(async move {
        let (mut stream, _) = backend.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n",
                )
                .await
                .unwrap();
        sleep(Duration::from_millis(500)).await;
        stream.write_all(b"6\r\n world\r\n0\r\n\r\n").await.unwrap();
    });

    let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
    let proxy_port = wait_for_http_port(&store).await;
    store
        .add_route(Route {
            hostname: "stream.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: backend_port,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: stream.demo.localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_millis(250), async {
        let mut buffer = [0u8; 1024];
        loop {
            let n = client.read(&mut buffer).await.unwrap();
            assert_ne!(n, 0, "proxy closed before first chunk");
            received.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&received).contains("hello") {
                break;
            }
        }
    })
    .await
    .expect("proxy buffered the response instead of streaming the first chunk");

    proxy_task.abort();
    backend_task.abort();
}

#[tokio::test]
async fn inbound_proxy_owned_headers_do_not_trigger_loop_detection() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let backend_port = backend.local_addr().unwrap().port();
    let backend_task = tokio::spawn(async move {
        let (mut stream, _) = backend.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 256];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..n]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.contains("x-jig-proxy-hops: 3\r\n"));
        assert!(
            !request_text
                .to_ascii_lowercase()
                .contains("x-jig-proxy-pid")
        );
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
    let proxy_port = wait_for_http_port(&store).await;
    store
        .add_route(Route {
            hostname: "spoof.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: backend_port,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: spoof.demo.localhost\r\nX-Jig-Proxy-Hops: 2\r\nX-Jig-Proxy-Pid: 9999\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), client.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8_lossy(&received);

    assert!(response.contains("204 No Content"));

    backend_task.await.unwrap();
    proxy_task.abort();
}

#[tokio::test]
async fn malformed_proxy_hop_header_returns_bad_request() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
    let proxy_port = wait_for_http_port(&store).await;
    store
        .add_route(Route {
            hostname: "bad-hop.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 9,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    client
            .write_all(
                b"GET / HTTP/1.1\r\nHost: bad-hop.demo.localhost\r\nX-Jig-Proxy-Hops: 1\r\nX-Jig-Proxy-Hops: 2\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), client.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8_lossy(&received);

    assert!(response.contains("400 Bad Request"));
    assert!(response.contains("Invalid Jig proxy headers"));
    proxy_task.abort();
}

#[tokio::test]
async fn connect_method_is_rejected_before_proxying() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
    let proxy_port = wait_for_http_port(&store).await;

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    client
            .write_all(
                b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), client.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8_lossy(&received);

    assert!(response.contains("405 Method Not Allowed"));
    assert!(response.contains("CONNECT requests are not supported"));
    proxy_task.abort();
}

#[tokio::test]
async fn bind_failure_preserves_existing_runtime_files() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(4242).unwrap();
    store.write_http_port(31337).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let occupied = listener.local_addr().unwrap().port();
    let settings = ProxySettings {
        http_port: occupied,
        ..settings
    };

    let error = run_async(settings, PathBuf::from("test-jig"))
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("Failed to bind") || error.contains("Address already in use"));
    assert_eq!(store.read_pid().unwrap(), Some(4242));
    assert_eq!(store.read_http_port().unwrap(), Some(31337));
}

#[tokio::test]
async fn route_cache_invalidates_after_route_file_changes() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let cache = RouteCache::new(store.clone());

    assert!(cache.routes().await.unwrap().is_empty());

    store
        .add_route(Route {
            hostname: "cache.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let routes = cache.routes().await.unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].hostname, "cache.demo.localhost");
}

#[tokio::test]
async fn backend_body_rejects_content_length_with_chunked_encoding() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accept = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        stream
    });
    let mut client = TcpStream::connect(addr).await.unwrap();
    let _server = accept.await.unwrap();
    let headers = vec![
        (CONTENT_LENGTH, HeaderValue::from_static("0")),
        (
            HeaderName::from_static("transfer-encoding"),
            HeaderValue::from_static("chunked"),
        ),
    ];

    let error = complete_backend_body(&mut client, &headers, Bytes::new())
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("both Content-Length and Transfer-Encoding"));
}

#[tokio::test]
async fn websocket_non_switching_response_drains_backend_body() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let backend_port = backend.local_addr().unwrap().port();
    let backend_task = tokio::spawn(async move {
        let (mut stream, _) = backend.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 11\r\nconnection: close, x-secret\r\nx-secret: leak\r\nx-jig-proxy-pid: spoofed\r\n\r\nhello",
                )
                .await
                .unwrap();
        sleep(Duration::from_millis(100)).await;
        stream.write_all(b" world").await.unwrap();
    });

    let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
    let proxy_port = wait_for_http_port(&store).await;
    store
        .add_route(Route {
            hostname: "ws.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: backend_port,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    client
            .write_all(
                b"GET /socket HTTP/1.1\r\nHost: ws.demo.localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: x\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .await
            .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), async {
        let mut buffer = [0u8; 1024];
        loop {
            let n = client.read(&mut buffer).await.unwrap();
            assert_ne!(n, 0, "proxy closed before returning the backend body");
            received.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&received).contains("hello world") {
                break;
            }
        }
    })
    .await
    .expect("proxy did not drain the backend non-101 WebSocket response");
    let text = String::from_utf8(received).unwrap();

    assert!(text.contains("401 Unauthorized"));
    assert!(text.contains("hello world"));
    assert!(!text.to_ascii_lowercase().contains("x-secret"));
    assert!(!text.to_ascii_lowercase().contains("x-jig-proxy-pid"));

    proxy_task.abort();
    backend_task.abort();
}

#[tokio::test]
async fn websocket_switching_protocols_tunnels_bytes() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: 0,
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    let backend = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let backend_port = backend.local_addr().unwrap().port();
    let backend_task = tokio::spawn(async move {
        let (mut stream, _) = backend.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 256];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..n]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.starts_with("GET /socket?x=%0AInjected:%20yes HTTP/1.1\r\n"));
        assert!(!request_text.contains("\r\nInjected:"));
        assert!(request_text.contains("x-jig-proxy-hops: 1\r\n"));
        assert!(request_text.contains("via: 1.1 jig\r\n"));
        assert!(!request_text.to_ascii_lowercase().contains("content-length"));
        assert!(
            !request_text
                .to_ascii_lowercase()
                .contains("proxy-authorization")
        );
        assert!(!request_text.to_ascii_lowercase().contains("x-secret"));
        assert!(
            !request_text
                .to_ascii_lowercase()
                .contains("x-forwarded-port")
        );
        assert!(!request_text.to_ascii_lowercase().contains("x-real-ip"));
        assert!(
            !request_text
                .to_ascii_lowercase()
                .contains("x-jig-proxy-pid")
        );
        let accept = websocket_accept_for_key("x");
        stream
                .write_all(
                    format!(
                        "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade, X-Secret\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\nX-Secret: leak\r\nX-Jig-Proxy-Pid: spoofed\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        let mut ping = [0u8; 4];
        stream.read_exact(&mut ping).await.unwrap();
        assert_eq!(&ping, b"ping");
        stream.write_all(b"pong").await.unwrap();
    });

    let proxy_task = tokio::spawn(run_async(settings, PathBuf::from("test-jig")));
    let proxy_port = wait_for_http_port(&store).await;
    store
        .add_route(Route {
            hostname: "ws-ok.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: backend_port,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: now_ms(),
        })
        .unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
    client
            .write_all(
                b"GET /socket?x=%0AInjected:%20yes HTTP/1.1\r\nHost: ws-ok.demo.localhost\r\nConnection: Upgrade, X-Secret\r\nUpgrade: websocket\r\nSec-WebSocket-Key: x\r\nSec-WebSocket-Version: 13\r\nContent-Length: 0\r\nProxy-Authorization: Basic bad\r\nX-Secret: leak\r\nX-Forwarded-Port: 443\r\nX-Real-IP: 203.0.113.1\r\nX-Jig-Proxy-Pid: spoofed\r\n\r\n",
            )
            .await
            .unwrap();
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), async {
        let mut buffer = [0u8; 256];
        loop {
            let n = client.read(&mut buffer).await.unwrap();
            assert_ne!(n, 0, "proxy closed before 101 response");
            received.extend_from_slice(&buffer[..n]);
            if received.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
    })
    .await
    .expect("proxy did not return the 101 response");
    let response_text = String::from_utf8_lossy(&received);
    assert!(response_text.contains("101 Switching Protocols"));
    assert!(!response_text.to_ascii_lowercase().contains("x-secret"));
    assert!(
        !response_text
            .to_ascii_lowercase()
            .contains("x-jig-proxy-pid")
    );

    client.write_all(b"ping").await.unwrap();
    let mut pong = [0u8; 4];
    timeout(Duration::from_secs(2), client.read_exact(&mut pong))
        .await
        .expect("proxy did not tunnel backend bytes")
        .unwrap();
    assert_eq!(&pong, b"pong");

    proxy_task.abort();
    backend_task.abort();
}
async fn wait_for_http_port(store: &StateStore) -> u16 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(port) = store.read_http_port().unwrap() {
            return port;
        }
        assert!(Instant::now() < deadline, "proxy did not start listening");
        sleep(Duration::from_millis(25)).await;
    }
}
