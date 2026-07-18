use std::net::IpAddr;
use std::ops::Deref;

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const ALLOWED_TLD_SUFFIXES: &[&str] = &["localhost", "local", "test", "internal"];
const DNS_LABEL_LIMIT: usize = 63;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct RouteHostname(String);

impl RouteHostname {
    pub(crate) fn new(value: impl AsRef<str>) -> Result<Self> {
        let normalized = value.as_ref().to_ascii_lowercase();
        validate_routed_hostname(&normalized)?;
        Ok(Self(normalized))
    }

    #[cfg(test)]
    pub(crate) fn unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl Deref for RouteHostname {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl std::fmt::Display for RouteHostname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for RouteHostname {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<RouteHostname> for &str {
    fn eq(&self, other: &RouteHostname) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<String> for RouteHostname {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<RouteHostname> for String {
    fn eq(&self, other: &RouteHostname) -> bool {
        self == other.as_str()
    }
}

#[cfg(test)]
impl From<&str> for RouteHostname {
    fn from(value: &str) -> Self {
        Self::new(value).expect("route hostname literals must be valid")
    }
}

#[cfg(test)]
impl From<String> for RouteHostname {
    fn from(value: String) -> Self {
        Self::new(value).expect("route hostnames must be valid")
    }
}

impl Serialize for RouteHostname {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RouteHostname {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TargetHost(String);

impl TargetHost {
    pub(crate) fn ip_literal(value: impl AsRef<str>) -> Result<Self> {
        let value = value.as_ref();
        value
            .parse::<IpAddr>()
            .map_err(|_| anyhow!("route target host '{value}' must be an IP literal"))?;
        Ok(Self(value.to_string()))
    }

    pub(crate) fn route_file_value(value: impl AsRef<str>) -> Result<Self> {
        let value = value.as_ref();
        validate_route_target_host(value)?;
        Ok(Self(value.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for TargetHost {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl std::fmt::Display for TargetHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
impl From<&str> for TargetHost {
    fn from(value: &str) -> Self {
        Self::route_file_value(value).expect("route target host literals must be valid")
    }
}

#[cfg(test)]
impl From<String> for TargetHost {
    fn from(value: String) -> Self {
        Self::route_file_value(value).expect("route target hosts must be valid")
    }
}

impl Serialize for TargetHost {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TargetHost {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::route_file_value(value).map_err(serde::de::Error::custom)
    }
}

pub(crate) fn route_hostname(name: &str, repo_name: &str, tld: &str) -> Result<String> {
    let app = sanitize_label(name)?;
    let repo = sanitize_label(repo_name)?;
    validate_tld(tld)?;
    Ok(format!("{app}.{repo}.{tld}"))
}

pub(crate) fn sanitize_label(input: &str) -> Result<String> {
    if !input.is_ascii() {
        bail!(
            "hostname label source '{input}' must be ASCII; configure an explicit ASCII app or repo name"
        );
    }
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !last_dash && !out.is_empty() {
                out.push(mapped);
                last_dash = true;
            }
        } else {
            out.push(mapped);
            last_dash = false;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        bail!("hostname label is empty after sanitizing '{input}'");
    }
    if out.len() > DNS_LABEL_LIMIT {
        // Keep the existing route identity for long app/repository names:
        // these hostnames are user-visible and may already be bookmarked.
        out.truncate(DNS_LABEL_LIMIT);
        while out.ends_with('-') {
            out.pop();
        }
    }
    Ok(out)
}

pub(crate) fn validate_hostname(hostname: &str) -> Result<()> {
    if hostname.is_empty() || hostname.len() > 253 || hostname.contains(':') {
        bail!("invalid hostname '{hostname}'");
    }
    for label in hostname.split('.') {
        if label.is_empty() || label.len() > 63 {
            bail!("invalid hostname '{hostname}'");
        }
        if label.starts_with('-') || label.ends_with('-') {
            bail!("invalid hostname '{hostname}'");
        }
        if !label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            bail!("invalid hostname '{hostname}'");
        }
    }
    Ok(())
}

pub(crate) fn validate_routed_hostname(hostname: &str) -> Result<()> {
    validate_hostname(hostname)?;
    let labels = hostname.split('.').collect::<Vec<_>>();
    if labels.len() < 2 {
        bail!("routed hostname '{hostname}' must include a private/local suffix");
    }
    if labels
        .last()
        .is_some_and(|label| ALLOWED_TLD_SUFFIXES.contains(label))
    {
        return Ok(());
    }
    if labels.len() >= 3 {
        let suffix = labels[labels.len() - 2..].join(".");
        if validate_tld(&suffix).is_ok() {
            return Ok(());
        }
    }
    bail!(
        "routed hostname '{hostname}' must end in a private/local suffix such as localhost, local, test, or internal"
    )
}

pub(crate) fn ip_is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
    }
}

pub(crate) fn target_host_is_loopback(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok_and(ip_is_loopback)
}

pub(crate) fn validate_route_target_host(host: &str) -> Result<()> {
    // Route-file validation is intentionally broad enough to keep older or
    // manually inspected state readable. Public alias/process entrypoints apply
    // stricter IP-literal and LAN-exposure checks before writing new routes.
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if host.contains(':') {
        bail!("invalid route target host '{host}'");
    }
    validate_hostname(host)
}

pub(crate) fn normalize_request_host(value: &str) -> Result<String> {
    if value.contains(['\r', '\n']) {
        bail!("request host must not contain CR/LF");
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("missing host"));
    }
    if trimmed.starts_with('[') {
        bail!("IPv6 literal request hosts are not routed by Jig proxy; use a configured hostname");
    }
    let host = authority_hostname(trimmed).to_ascii_lowercase();
    if host.contains(':') {
        bail!("IPv6 literal request hosts are not routed by Jig proxy; use a configured hostname");
    }
    validate_routed_hostname(&host)?;
    Ok(host)
}

fn authority_hostname(value: &str) -> &str {
    let Some((host, port)) = value.rsplit_once(':') else {
        return value;
    };
    if !host.contains(':') && !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
        host
    } else {
        value
    }
}

pub(crate) fn validate_tld(tld: &str) -> Result<()> {
    validate_hostname(tld)?;
    let tld = tld.to_ascii_lowercase();
    let labels = tld.split('.').collect::<Vec<_>>();
    if labels.len() == 1 && ALLOWED_TLD_SUFFIXES.contains(&labels[0]) {
        return Ok(());
    }
    if labels.len() == 2 && ALLOWED_TLD_SUFFIXES.contains(&labels[1]) {
        return Ok(());
    }
    bail!(
        "dev proxy TLD '{tld}' is not allowed. Use a private/local suffix such as localhost, local, test, or internal."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_hostname_sanitizes_repo_and_app_names() {
        assert_eq!(
            route_hostname("@Org/Web App", "My Repo", "localhost").unwrap(),
            "org-web-app.my-repo.localhost"
        );
    }

    #[test]
    fn route_hostname_rejects_non_ascii_labels() {
        let error = route_hostname("münchen", "demo", "localhost")
            .unwrap_err()
            .to_string();

        assert!(error.contains("must be ASCII"));
    }

    #[test]
    fn long_dns_labels_preserve_the_legacy_bounded_prefix() {
        let vectors = [
            ("my-app".to_string(), "my-app".to_string()),
            ("a".repeat(64), "a".repeat(63)),
            (
                format!("{}project", "project-".repeat(26)),
                "project-project-project-project-project-project-project-project".to_string(),
            ),
        ];

        for (input, expected) in vectors {
            let label = sanitize_label(&input).unwrap();
            assert_eq!(label, expected);
            assert!(label.len() <= DNS_LABEL_LIMIT);
        }

        assert_eq!(
            sanitize_label(&format!("{}-", "a".repeat(63))).unwrap(),
            "a".repeat(63)
        );
    }

    #[test]
    fn rejects_invalid_hostname() {
        assert!(validate_hostname("-bad.localhost").is_err());
        assert!(validate_hostname("bad..localhost").is_err());
        assert!(validate_hostname("bad.localhost:1355").is_err());
    }

    #[test]
    fn routed_hostnames_require_private_suffix() {
        assert!(validate_routed_hostname("web.demo.localhost").is_ok());
        assert!(validate_routed_hostname("web.demo.corp.internal").is_ok());
        assert!(validate_routed_hostname("evil.com").is_err());
        assert!(validate_routed_hostname("localhost").is_err());
    }

    #[test]
    fn normalizes_request_host_port_without_accepting_ipv6_authority() {
        assert_eq!(
            normalize_request_host("Web.Demo.Localhost:1355").unwrap(),
            "web.demo.localhost"
        );
        let error = normalize_request_host("[::1]:1355")
            .unwrap_err()
            .to_string();
        assert!(error.contains("IPv6 literal request hosts"));
    }

    #[test]
    fn request_host_rejects_cr_lf_before_trimming() {
        let error = normalize_request_host("web.demo.localhost\r\n")
            .unwrap_err()
            .to_string();

        assert!(error.contains("CR/LF"));
    }

    #[test]
    fn loopback_targets_require_ip_literals() {
        assert!(target_host_is_loopback("127.0.0.1"));
        assert!(target_host_is_loopback("::1"));
        assert!(target_host_is_loopback("::ffff:127.0.0.1"));
        assert!(!target_host_is_loopback("localhost"));
        assert!(!target_host_is_loopback("example.com"));
    }

    #[test]
    fn tld_must_use_private_local_suffix() {
        assert!(validate_tld("localhost").is_ok());
        assert!(validate_tld("demo.test").is_ok());
        assert!(validate_tld("corp.internal").is_ok());
        assert!(validate_tld("too.deep.test").is_err());
        assert!(validate_tld("dev").is_err());
        assert!(validate_tld("example.com").is_err());
    }
}
