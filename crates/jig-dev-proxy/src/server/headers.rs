use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::{
    CONNECTION, CONTENT_LENGTH, HOST, HeaderMap, HeaderName, HeaderValue, TRANSFER_ENCODING,
    UPGRADE, VIA,
};
use hyper::{Request, Response, StatusCode, Version};
use subtle::ConstantTimeEq;

use crate::host::{ip_is_loopback, normalize_request_host};
use crate::types::Route;

use super::error_page::error_response;
use super::{HEALTH_TOKEN_BYTES, MAX_PROXY_HOPS, ProxyBody, VIA_VALUE};

pub(super) fn rewrite_proxy_headers(
    headers: &mut hyper::HeaderMap,
    remote_addr: SocketAddr,
    route: &Route,
    tls: bool,
    version: Version,
) -> Result<()> {
    let proxy_hops = next_proxy_hops(headers)?;
    remove_hop_by_hop_headers(headers);
    remove_forwarded_headers(headers);
    remove_jig_proxy_headers(headers);
    headers.insert(HOST, HeaderValue::from_str(&route.hostname)?);
    append_via(headers, version);
    set_forwarded_for(headers, remote_addr);
    headers.insert(
        "x-forwarded-proto",
        HeaderValue::from_static(if tls { "https" } else { "http" }),
    );
    // Use the normalized route hostname so backends see the same stable
    // development hostname regardless of inbound Host casing.
    headers.insert("x-forwarded-host", HeaderValue::from_str(&route.hostname)?);
    headers.insert("x-jig-proxy-hops", proxy_hops);
    Ok(())
}

pub(super) fn next_proxy_hops(headers: &HeaderMap) -> Result<HeaderValue> {
    let mut values = headers.get_all("x-jig-proxy-hops").iter();
    let current = match (values.next(), values.next()) {
        (None, _) => 0,
        (Some(value), None) => value
            .to_str()
            .context("Invalid Jig proxy hop header")?
            .parse::<u8>()
            .context("Invalid Jig proxy hop count")?,
        (Some(_), Some(_)) => anyhow::bail!("Conflicting Jig proxy hop headers"),
    };
    let next = current
        .checked_add(1)
        .context("Jig proxy hop count overflowed")?;
    if next > MAX_PROXY_HOPS {
        anyhow::bail!("Jig proxy hop limit exceeded");
    }
    HeaderValue::from_str(&next.to_string()).context("Invalid Jig proxy hop count")
}

pub(super) fn request_host<B>(req: &Request<B>) -> Result<String> {
    let authority = req
        .uri()
        .authority()
        .map(|authority| normalize_request_host(authority.as_str()))
        .transpose()?;
    if req.headers().get_all(HOST).iter().count() > 1 {
        anyhow::bail!("Conflicting request Host headers");
    }
    let host = req
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .map(normalize_request_host)
        .transpose()?;
    match (authority, host) {
        (Some(authority), Some(host)) if authority != host => {
            // Jig routes by a single normalized host. Rejecting conflicting
            // absolute-form authority and Host values avoids ambiguous proxy
            // routing even though ordinary HTTP/1.1 clients usually send only
            // Host.
            anyhow::bail!("Conflicting request authority and Host header");
        }
        (Some(authority), _) => Ok(authority),
        (None, Some(host)) => Ok(host),
        (None, None) => anyhow::bail!("Missing Host header"),
    }
}

pub(super) fn request_host_or_bad_request<B>(
    req: &Request<B>,
) -> std::result::Result<String, Box<Response<ProxyBody>>> {
    request_host(req).map_err(|error| {
        Box::new(error_response(
            StatusCode::BAD_REQUEST,
            &format!("Invalid request host: {error}"),
        ))
    })
}

pub(super) fn health_request_allowed<B>(
    req: &Request<B>,
    remote_ip: IpAddr,
    local_ip: IpAddr,
    health_token: &str,
) -> bool {
    // Health checks are intentionally narrower than routed requests: they only
    // accept the advertised loopback authorities and a valid private token, and
    // they bypass route-host normalization because they do not select a route.
    let supplied_token = req
        .headers()
        .get("x-jig-proxy-health-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    ip_is_loopback(remote_ip)
        && ip_is_loopback(local_ip)
        && req
            .headers()
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .or_else(|| req.uri().authority().map(|authority| authority.as_str()))
            .is_some_and(loopback_health_host)
        && constant_time_ascii_eq(supplied_token, health_token)
}

pub(super) fn constant_time_ascii_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != HEALTH_TOKEN_BYTES || right.len() != HEALTH_TOKEN_BYTES {
        return false;
    }
    left.ct_eq(right).into()
}

pub(super) fn loopback_health_host(value: &str) -> bool {
    // Keep this pinned to the authorities the proxy advertises in its own
    // health probes. Other 127/8 loopback aliases are valid IPs but are not
    // part of the proxy identity contract.
    let host = authority_host(value.trim());
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub(super) fn authority_host(value: &str) -> &str {
    if let Some(rest) = value.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return value;
        };
        if suffix.is_empty()
            || suffix
                .strip_prefix(':')
                .is_some_and(|port| !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()))
        {
            return host;
        }
        return value;
    }
    let Some((host, port)) = value.rsplit_once(':') else {
        return value;
    };
    if !host.contains(':') && !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
        host
    } else {
        value
    }
}

pub(super) fn find_route<'a>(routes: &'a [Route], host: &str) -> Option<&'a Route> {
    routes.iter().find(|route| route.hostname == host)
}

pub(super) fn health_response() -> Response<ProxyBody> {
    let pid = std::process::id();
    let mut response = Response::new(full_body(Bytes::from(format!(
        r#"{{"ok":true,"pid":{pid}}}"#
    ))));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    if let Ok(value) = HeaderValue::from_str(&pid.to_string()) {
        response.headers_mut().insert("x-jig-proxy-pid", value);
    }
    response
        .headers_mut()
        .insert("x-jig-proxy", HeaderValue::from_static("1"));
    response
}

pub(super) fn full_body(bytes: Bytes) -> ProxyBody {
    Full::new(bytes)
        .map_err(|never| match never {})
        .boxed_unsync()
}

pub(super) fn empty_body() -> ProxyBody {
    full_body(Bytes::new())
}

pub(super) fn is_websocket(headers: &HeaderMap) -> bool {
    let has_upgrade = headers
        .get(UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let connection_upgrade = headers
        .get(CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
    let has_key = headers
        .get("sec-websocket-key")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().is_empty());
    let has_supported_version = headers
        .get("sec-websocket-version")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "13");
    has_upgrade && connection_upgrade && has_key && has_supported_version
}

pub(super) fn websocket_request_has_body(headers: &HeaderMap) -> bool {
    if headers.contains_key(TRANSFER_ENCODING) {
        return true;
    }
    headers.get_all(CONTENT_LENGTH).iter().any(|value| {
        value
            .to_str()
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .any(|item| item.trim().parse::<u64>().ok() != Some(0))
            })
            .unwrap_or(true)
    })
}

pub(super) fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

pub(super) fn remove_hop_by_hop_headers(headers: &mut HeaderMap) {
    let connection_headers = connection_header_names(headers);
    let names: Vec<_> = headers
        .keys()
        .filter(|name| {
            is_hop_by_hop(name.as_str())
                || connection_headers
                    .iter()
                    .any(|connection_header| connection_header == *name)
        })
        .cloned()
        .collect();
    for name in names {
        headers.remove(name);
    }
}

pub(super) fn remove_forwarded_headers(headers: &mut HeaderMap) {
    let names: Vec<_> = headers
        .keys()
        .filter(|name| {
            let lower = name.as_str().to_ascii_lowercase();
            lower == "forwarded" || lower == "x-real-ip" || lower.starts_with("x-forwarded-")
        })
        .cloned()
        .collect();
    for name in names {
        headers.remove(name);
    }
}

pub(super) fn remove_jig_proxy_headers(headers: &mut HeaderMap) {
    let names: Vec<_> = headers
        .keys()
        .filter(|name| is_jig_proxy_header(name))
        .cloned()
        .collect();
    for name in names {
        headers.remove(name);
    }
}

pub(super) fn is_jig_proxy_header(name: &HeaderName) -> bool {
    // Strip the full proxy-owned namespace so backends cannot spoof future
    // x-jig-proxy-* control headers back to clients.
    name.as_str().starts_with("x-jig-proxy")
}

pub(super) fn raw_header_is_hop_by_hop(
    name: &HeaderName,
    connection_headers: &[HeaderName],
) -> bool {
    is_hop_by_hop(name.as_str())
        || connection_headers
            .iter()
            .any(|connection_header| connection_header == name)
}

pub(super) fn raw_connection_header_names(
    headers: &[(HeaderName, HeaderValue)],
) -> Vec<HeaderName> {
    headers
        .iter()
        .filter(|(name, _)| *name == CONNECTION)
        .filter_map(|(_, value)| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
        .collect()
}

pub(super) fn connection_header_names(headers: &HeaderMap) -> Vec<HeaderName> {
    headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        // Invalid tokens cannot name a header present in Hyper's HeaderMap,
        // whose keys are already validated HeaderName values.
        .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
        .collect()
}

pub(super) fn set_forwarded_for(headers: &mut HeaderMap, remote_addr: SocketAddr) {
    // This proxy is the local edge. Inbound forwarded headers were stripped, so
    // expose only the directly connected client rather than appending spoofable
    // upstream hops.
    let value = remote_addr.ip().to_string();
    let value = HeaderValue::from_str(&value).expect("IP address strings are valid header values");
    headers.insert("x-forwarded-for", value);
}

pub(super) fn request_content_length(headers: &HeaderMap) -> Result<Option<u64>> {
    let mut parsed = None;
    for value in headers.get_all(CONTENT_LENGTH) {
        let value = value
            .to_str()
            .context("Request Content-Length was not valid header text")?;
        for item in value.split(',') {
            let length = item
                .trim()
                .parse::<u64>()
                .context("Invalid request Content-Length")?;
            if parsed.is_some_and(|existing| existing != length) {
                anyhow::bail!("Request used conflicting Content-Length values");
            }
            parsed = Some(length);
        }
    }
    if parsed.is_some() && headers.contains_key(TRANSFER_ENCODING) {
        anyhow::bail!("Request used both Content-Length and Transfer-Encoding");
    }
    Ok(parsed)
}

pub(super) fn append_via(headers: &mut HeaderMap, version: Version) {
    headers.append(VIA, header_value_or_default(via_value(version), VIA_VALUE));
}

pub(super) fn via_value(version: Version) -> &'static str {
    match version {
        Version::HTTP_2 => "2.0 jig",
        _ => VIA_VALUE,
    }
}

pub(super) fn header_value_or_default(value: &str, default: &'static str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static(default))
}
