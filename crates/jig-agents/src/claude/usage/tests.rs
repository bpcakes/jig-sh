use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use serde_json::json;

use super::{account, credentials, http, inspected, normalize};

#[test]
fn subscription_windows_are_normalized_without_forwarding_unrelated_fields() {
    let limits = normalize::limits(&json!({
        "five_hour":{"utilization":25.5,"resets_at":"2026-09-10T12:30:00Z"},
        "seven_day":{"utilization":80,"resets_at":"2026-09-15T12:30:00+00:00"},
        "seven_day_sonnet":{"utilization":4,"resets_at":null},
        "seven_day_opus":null,
        "extra_usage":{"is_enabled":true, "used_credits":500},
        "unknown":"untrusted response text"
    }))
    .unwrap();
    assert_eq!(limits.len(), 2);
    assert_eq!(limits[0]["id"], "claude");
    assert_eq!(limits[0]["primary"]["used_percent"], 25.5);
    assert_eq!(limits[0]["primary"]["duration_minutes"], 300);
    assert_eq!(limits[0]["primary"]["resets_at"], 1789043400_i64);
    assert_eq!(limits[0]["secondary"]["duration_minutes"], 10080);
    assert_eq!(limits[1]["name"], "Sonnet");
    assert!(limits[1]["primary"]["resets_at"].is_null());
    assert!(
        !serde_json::to_string(&limits)
            .unwrap()
            .contains("untrusted")
    );
}

#[test]
fn missing_and_malformed_limits_are_unavailable_instead_of_zero() {
    for value in [json!(null), json!({})] {
        assert!(normalize::limits(&value).is_err());
    }
    for value in [
        json!({"five_hour":{"utilization":-1}}),
        json!({"seven_day":{"utilization":"10"}}),
        json!({"five_hour":{"utilization":10},"seven_day":{"utilization":"10"}}),
    ] {
        let limits = json!(normalize::limits(&value).unwrap());
        assert!(normalize::is_partial(&limits));
        let window = if limits[0]["secondary"].is_null() {
            &limits[0]["primary"]
        } else {
            &limits[0]["secondary"]
        };
        assert!(window["used_percent"].is_null());
        assert_eq!(window["invalid"], true);
    }
    let limits =
        normalize::limits(&json!({"five_hour":{"utilization":0,"resets_at":"invalid"}})).unwrap();
    assert_eq!(limits[0]["primary"]["used_percent"], 0.0);
    assert!(limits[0]["primary"]["resets_at"].is_null());
    assert_eq!(limits[0]["primary"]["invalid"], true);
}

#[test]
fn fable_scoped_weekly_limits_survive_missing_model_ids_and_inactive_markers() {
    let limits = normalize::limits(&json!({
        "five_hour":{"utilization":5,"resets_at":"2026-10-11T12:00:00Z"},
        "limits":[
            {"kind":"weekly_scoped","group":"weekly","percent":95,
             "resets_at":"2026-10-15T12:00:00Z","is_active":false,
             "scope":{"model":{"id":null,"display_name":"Fable"}},
             "unknown":"example-private-field"},
            {"kind":"weekly_scoped","percent":20,"resets_at":null,
             "scope":{"surface":{"display_name":"Example surface"}}}
        ]
    }))
    .unwrap();
    assert_eq!(limits.len(), 3);
    assert_eq!(limits[1]["name"], "Fable");
    assert_eq!(limits[1]["primary"]["used_percent"], 95.0);
    assert_eq!(limits[1]["primary"]["duration_minutes"], 10_080);
    assert_eq!(limits[2]["name"], "Example surface");
    assert_ne!(limits[1]["id"], limits[2]["id"]);
    assert!(
        !serde_json::to_string(&limits)
            .unwrap()
            .contains("example-private-field")
    );
}

#[test]
fn malformed_scoped_limits_preserve_valid_usage_and_mark_unknown_capacity() {
    let credential =
        credentials::parse(br#"{"claudeAiOauth":{"accessToken":"example-token"}}"#).unwrap();
    for scoped in [
        json!({}),
        json!(null),
        json!({"kind":"weekly_scoped","percent":null}),
    ] {
        let limits = json!(
            normalize::limits(&json!({
                "five_hour":{"utilization":25}, "limits":[scoped],
            }))
            .unwrap()
        );
        assert_eq!(limits[0]["primary"]["used_percent"], 25.0);
        assert_eq!(limits[1]["primary"]["invalid"], true);
        assert!(limits[1]["primary"]["used_percent"].is_null());
        assert!(normalize::is_partial(&limits));
        let report = inspected(&credential, None, Ok(limits.as_array().unwrap().clone()));
        assert_eq!(report["rate_limits"][0]["primary"]["used_percent"], 25.0);
        assert!(report["usage_error"].is_null());
    }
}

#[test]
fn credentials_are_read_without_exposing_tokens_and_expiry_is_checked_in_milliseconds() {
    let credential = credentials::parse(br#"{"claudeAiOauth":{"accessToken":"example-secret-token","refreshToken":"example-refresh-token","expiresAt":2000,"subscriptionType":"max","scopes":["user:profile"]}}"#).unwrap();
    assert!(credential.usage_error(1999).is_none());
    assert!(credential.usage_error(2000).unwrap().contains("expired"));
    let report = inspected(&credential, None, Err("Usage unavailable".into()));
    assert_eq!(report["account"]["plan_type"], "max");
    assert!(report["account"]["email"].is_null());
    assert_eq!(report["usage_error"], "Usage unavailable");
    assert!(!report.to_string().contains("secret-token"));
    assert!(!report.to_string().contains("refresh-token"));
    for input in [
        b"example-secret-token".as_slice(),
        br#"{}"#,
        br#"{"claudeAiOauth":{"accessToken":""}}"#,
    ] {
        let error = credentials::parse(input).err().unwrap();
        assert!(!error.contains("example-secret-token"));
    }
    let limited = credentials::parse(
        br#"{"claudeAiOauth":{"accessToken":"example-token","scopes":["user:inference"]}}"#,
    )
    .unwrap();
    assert!(limited.usage_error(0).unwrap().contains("user:profile"));
}

#[test]
fn credential_file_is_bounded_and_non_regular_files_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let home = crate::claude::Home {
        path: root.path().into(),
        default_config: false,
    };
    let path = root.path().join(".credentials.json");
    std::fs::write(
        &path,
        br#"{"claudeAiOauth":{"accessToken":"example-token"}}"#,
    )
    .unwrap();
    assert!(credentials::read_file(&home).is_ok());
    std::fs::write(&path, vec![b'x'; 65537]).unwrap();
    assert!(
        credentials::read_file(&home)
            .err()
            .unwrap()
            .contains("size limit")
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(credentials::read_file(&home).is_err());
}

fn http_response(
    status: &str,
    body: &str,
    extra: &str,
) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    );
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        let _ = socket.write_all(response.as_bytes());
        String::from_utf8(request).unwrap()
    });
    (address, handle)
}

fn fetch(url: &str) -> Result<serde_json::Value, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(http::fetch(
        &http::client().unwrap(),
        "example-token",
        url,
        &|| false,
    ))
}

#[test]
fn http_uses_bearer_auth_and_preserves_usage_payload() {
    let (url, server) = http_response("200 OK", r#"{"five_hour":{"utilization":12}}"#, "");
    assert_eq!(fetch(&url).unwrap()["five_hour"]["utilization"], 12);
    let request = server.join().unwrap().to_lowercase();
    assert!(request.contains("authorization: bearer example-token\r\n"));
    assert!(request.contains("anthropic-beta: oauth-2025-04-20\r\n"));
}

#[test]
fn http_failures_do_not_echo_server_content_or_follow_redirects() {
    for (status, expected) in [
        ("401 Unauthorized", "expired"),
        ("403 Forbidden", "cannot read"),
        ("429 Too Many Requests", "rate limited"),
        ("302 Found", "HTTP 302"),
        ("500 Internal Server Error", "HTTP 500"),
    ] {
        let (url, server) = http_response(
            status,
            "example-sensitive-response",
            "Location: http://127.0.0.1:1/\r\n",
        );
        let error = fetch(&url).unwrap_err();
        server.join().unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("sensitive"));
    }
    for (body, expected) in [
        ("example-invalid-body".to_owned(), "valid JSON"),
        ("x".repeat(65537), "size limit"),
    ] {
        let (url, server) = http_response("200 OK", &body, "");
        let error = fetch(&url).unwrap_err();
        server.join().unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn cancellation_drops_an_inflight_http_request_promptly() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut data = [0; 4096];
        assert!(stream.read(&mut data).unwrap() > 0);
        signal.store(true, Ordering::Release);
        while stream.read(&mut data).unwrap() > 0 {}
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let start = Instant::now();
    let error = runtime
        .block_on(http::fetch(
            &http::client().unwrap(),
            "example-token",
            &url,
            &|| cancelled.load(Ordering::Acquire),
        ))
        .unwrap_err();
    // Inspection owns and drops this runtime when cancellation ends the pass.
    // Hyper's connection tasks are released with it even while the server stalls.
    drop(runtime);
    server.join().unwrap();
    assert!(error.contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[cfg(target_os = "macos")]
#[test]
fn keychain_names_keep_native_and_explicit_default_modes_separate() {
    let mut home = crate::claude::Home {
        path: "/tmp/ExampleHome/.claude".into(),
        default_config: true,
    };
    assert_eq!(
        super::keychain::service(&home).unwrap(),
        "Claude Code-credentials"
    );
    home.default_config = false;
    let explicit = super::keychain::service(&home).unwrap();
    assert!(explicit.starts_with("Claude Code-credentials-"));
    assert_eq!(explicit.len(), "Claude Code-credentials-".len() + 8);
    home.path = "/tmp/ExampleHome/caf\u{e9}".into();
    let composed = super::keychain::service(&home).unwrap();
    home.path = "/tmp/ExampleHome/cafe\u{301}".into();
    assert_eq!(super::keychain::service(&home).unwrap(), composed);
}

#[test]
fn account_email_comes_from_the_recorded_oauth_account_only() {
    assert_eq!(
        account::parse_email(
            br#"{"projects":{"/tmp/ExampleProject":{}},"oauthAccount":{"emailAddress":"person@example.com","organizationName":"ExampleOrg"}}"#
        )
        .as_deref(),
        Some("person@example.com")
    );
    for config in [
        br#"{}"#.as_slice(),
        br#"{"oauthAccount":null}"#,
        br#"{"oauthAccount":{"emailAddress":"  "}}"#,
        br#"{"oauthAccount":{"emailAddress":42}}"#,
        br#"{"emailAddress":"person@example.com"}"#,
        b"not json",
    ] {
        assert_eq!(account::parse_email(config), None);
    }

    let credential = credentials::parse(
        br#"{"claudeAiOauth":{"accessToken":"example-token","subscriptionType":"max"}}"#,
    )
    .unwrap();
    let report = inspected(
        &credential,
        Some("person@example.com".into()),
        Ok(Vec::new()),
    );
    assert_eq!(report["account"]["email"], "person@example.com");
}

#[test]
fn account_config_reads_only_bounded_regular_files() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join(".claude.json");
    std::fs::write(
        &config,
        br#"{"oauthAccount":{"emailAddress":"person@example.com"}}"#,
    )
    .unwrap();
    assert!(account::read_bounded(&config).is_some());
    assert!(account::read_bounded(directory.path()).is_none());
    assert!(account::read_bounded(&directory.path().join("missing.json")).is_none());
    std::fs::File::create(&config)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    assert!(account::read_bounded(&config).is_none());
}
