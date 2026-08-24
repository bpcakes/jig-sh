use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::io::{ErrorKind, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints, SerialNumber,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use sha1::Sha1;
use sha2::{Digest, Sha256};
use time::{Duration as TimeDuration, OffsetDateTime};
use x509_parser::oid_registry::{
    OID_EC_P256, OID_KEY_TYPE_EC_PUBLIC_KEY, OID_SIG_ECDSA_WITH_SHA256,
};
use x509_parser::parse_x509_certificate;
use zeroize::Zeroizing;

use crate::file_ops;
use crate::host::{validate_hostname, validate_routed_hostname, validate_tld};
use crate::ports::local_lan_ip_for_ipv4_listener;
use crate::state::{LockOutcome, StateStore};
use crate::types::ProxySettings;

mod trust;

#[cfg(target_os = "linux")]
use trust::linux_current_jig_ca_is_trusted;
#[cfg(target_os = "macos")]
use trust::macos_trusted_ca_fingerprint_exists;
use trust::warn_global_ca_trust;
pub(crate) use trust::{status, trust, untrust};

const CA_VALIDITY_DAYS: i64 = 730;
const LEAF_VALIDITY_DAYS: i64 = 200;
const LEAF_HOSTS_VERSION: u32 = 1;
const TRUSTED_CA_VERSION: u32 = 1;
const MAX_CERTIFICATE_HOSTS: usize = 1024;
const MAX_CERT_PEM_BYTES: u64 = 1024 * 1024;
const MAX_PRIVATE_KEY_PEM_BYTES: u64 = 256 * 1024;
const CERT_FILE_FALLBACK: &str = "jig-proxy-cert";
#[cfg(target_os = "linux")]
const MAX_TRUST_BUNDLE_PEM_BYTES: u64 = 16 * 1024 * 1024;
const CA_SERIAL_NUMBER_BYTES: usize = 16;
const JIG_CA_COMMON_NAME: &str = "Jig Dev Proxy Local CA";
const GLOBAL_CA_TRUST_WARNING: &str = "Trusting the Jig Dev Proxy local CA installs a locally trusted root constrained to configured Jig development DNS names and loopback/IPv4 LAN IP addresses; keep ca-key.pem private because any locally trusted CA key is sensitive machine-local TLS material.";
#[cfg(target_os = "macos")]
const MACOS_UNTRUST_REMOVAL_LIMIT: usize = 64;

pub(crate) fn generate(settings: &ProxySettings, force: bool) -> Result<Value> {
    ensure_certificate_generation_supported()?;
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| {
        remove_stale_cert_temps(&store)?;
        let files = cert_file_state(&store);
        let ca_pair_invalid = ca_pair_invalid(&store, files, settings);
        let leaf_pair_invalid = leaf_pair_invalid(&store, files);
        if !force && files.is_complete() && !ca_pair_invalid && !leaf_pair_invalid {
            bail!(
                "Proxy certificates already exist in {}. Pass --force to regenerate them.",
                store.root().display()
            );
        }

        if force || files.ca_pair_is_partial() || ca_pair_invalid {
            ensure_ca_can_be_replaced(&store)?;
        }
        if force || !files.ca_exists || !files.ca_key_exists || ca_pair_invalid {
            write_ca(&store, settings)?;
        }
        let hosts = certificate_hosts(settings, &store, &[])?;
        write_leaf(&store, &hosts)?;

        warn_global_ca_trust(&store);
        Ok(certificate_paths(&store))
    })
}

pub(crate) fn ensure(settings: &ProxySettings) -> Result<Value> {
    ensure_for_hosts(settings, &[])
}

pub(crate) fn ensure_for_hosts(settings: &ProxySettings, hostnames: &[String]) -> Result<Value> {
    ensure_certificate_generation_supported()?;
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| ensure_for_hosts_locked(&store, settings, hostnames))
}

pub(crate) fn ensure_for_hosts_interruptible(
    settings: &ProxySettings,
    hostnames: &[String],
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<Value>> {
    ensure_certificate_generation_supported()?;
    let store = match StateStore::resolve_interruptible(settings.state_dir.clone(), cancelled)? {
        LockOutcome::Acquired(store) => store,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    match store.with_cert_lock_interruptible(cancelled, || {
        ensure_for_hosts_locked_interruptible(&store, settings, hostnames, cancelled)
    })? {
        LockOutcome::Acquired(outcome) => Ok(outcome),
        LockOutcome::Cancelled => Ok(LockOutcome::Cancelled),
    }
}

fn ensure_for_hosts_locked(
    store: &StateStore,
    settings: &ProxySettings,
    hostnames: &[String],
) -> Result<Value> {
    match ensure_for_hosts_locked_interruptible(store, settings, hostnames, &|| false)? {
        LockOutcome::Acquired(value) => Ok(value),
        LockOutcome::Cancelled => bail!("uncancelled certificate preparation was cancelled"),
    }
}

fn ensure_for_hosts_locked_interruptible(
    store: &StateStore,
    settings: &ProxySettings,
    hostnames: &[String],
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<Value>> {
    remove_stale_cert_temps(store)?;
    let files = cert_file_state(store);
    let ca_pair_invalid = ca_pair_invalid(store, files, settings);
    if files.ca_pair_is_partial() || ca_pair_invalid {
        ensure_ca_can_be_replaced(store)?;
    }
    // Existing valid CAs, including ones trusted by Jig, may continue to issue
    // refreshed leaf certificates. Only CA replacement paths require
    // untrusting first so old trusted roots are not orphaned.
    if !files.ca_exists || !files.ca_key_exists || ca_pair_invalid {
        write_ca(store, settings)?;
    }
    let hosts = match certificate_hosts_interruptible(settings, store, hostnames, cancelled)? {
        LockOutcome::Acquired(hosts) => hosts,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    ensure_leaf_hosts_within_ca_constraints(&store.ca_path(), &hosts)?;
    if leaf_matches_hosts(store, &hosts) {
        restrict_private_key(&store.ca_key_path())?;
        restrict_private_key(&store.leaf_key_path())?;
        return Ok(LockOutcome::Acquired(certificate_paths(store)));
    }
    write_leaf(store, &hosts)?;
    Ok(LockOutcome::Acquired(certificate_paths(store)))
}

fn ensure_certificate_generation_supported() -> Result<()> {
    Ok(())
}

fn write_ca(store: &StateStore, settings: &ProxySettings) -> Result<()> {
    let ca_params = ca_params(settings)?;
    let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let ca = ca_params.self_signed(&ca_key)?;
    let ca_key_pem = Zeroizing::new(ca_key.serialize_pem());

    write_private_key(store.ca_key_path(), &ca_key_pem)?;
    write_public_pem(store.ca_path(), &ca.pem())?;
    remove_leaf_material_after_ca_rotation(store)?;
    Ok(())
}

fn remove_leaf_material_after_ca_rotation(store: &StateStore) -> Result<()> {
    remove_optional_cert_file(store.leaf_path())?;
    remove_optional_cert_file(store.leaf_key_path())?;
    remove_optional_cert_file(store.leaf_hosts_path())?;
    Ok(())
}

fn remove_optional_cert_file(path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to remove stale certificate file {}", path.display())),
    }
}

fn ensure_ca_can_be_replaced(store: &StateStore) -> Result<()> {
    if trusted_ca_marker_matches(store)? {
        bail!(
            "Current Jig proxy CA was trusted by Jig. Run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating it with --force so the old trusted root is not orphaned."
        );
    }
    #[cfg(target_os = "macos")]
    {
        if store.ca_path().exists() {
            let fingerprints = ca_fingerprints(&store.ca_path()).with_context(|| {
                format!(
                    "Failed to inspect current Jig proxy CA certificate {} before replacing it",
                    store.ca_path().display()
                )
            })?;
            if macos_trusted_ca_fingerprint_exists(&fingerprints).context(
                "Failed to inspect macOS trust store before replacing current Jig proxy CA",
            )? {
                bail!(
                    "Current Jig proxy CA appears to be trusted. Run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating it with --force so the old trusted root is not orphaned."
                );
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if store.ca_path().exists() && linux_current_jig_ca_is_trusted(store)? {
            bail!(
                "A Jig Dev Proxy Local CA appears to be trusted by the Linux trust store. Run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating it with --force so the old trusted root is not orphaned."
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct CertFileState {
    ca_exists: bool,
    ca_key_exists: bool,
    leaf_exists: bool,
    leaf_key_exists: bool,
}

impl CertFileState {
    const fn is_complete(self) -> bool {
        self.ca_exists && self.ca_key_exists && self.leaf_exists && self.leaf_key_exists
    }

    const fn ca_pair_is_partial(self) -> bool {
        self.ca_exists != self.ca_key_exists
    }
}

fn cert_file_state(store: &StateStore) -> CertFileState {
    CertFileState {
        ca_exists: store.ca_path().exists(),
        ca_key_exists: store.ca_key_path().exists(),
        leaf_exists: store.leaf_path().exists(),
        leaf_key_exists: store.leaf_key_path().exists(),
    }
}

fn ca_pair_invalid(store: &StateStore, files: CertFileState, settings: &ProxySettings) -> bool {
    if !files.ca_exists || !files.ca_key_exists {
        return false;
    }
    match certificate_is_current(&store.ca_path()) {
        Ok(true) => {}
        Ok(false) => return true,
        Err(error) => {
            eprintln!(
                "jig proxy CA certificate validity could not be checked; regenerating it: {error:#}"
            );
            return true;
        }
    }
    match ca_name_constraints_cover_settings(&store.ca_path(), settings) {
        Ok(true) => {}
        Ok(false) => return true,
        Err(error) => {
            eprintln!(
                "jig proxy CA certificate name constraints could not be checked; regenerating it: {error:#}"
            );
            return true;
        }
    }
    match private_key_matches_certificate(&store.ca_key_path(), &store.ca_path()) {
        Ok(true) => false,
        Ok(false) => true,
        Err(error) => {
            eprintln!("jig proxy CA key/certificate pair is invalid; regenerating it: {error:#}");
            true
        }
    }
}

fn ca_name_constraints_cover_settings(path: &Path, settings: &ProxySettings) -> Result<bool> {
    let der = first_certificate_der(path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    let Some(constraints) = cert
        .name_constraints()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA name constraints: {error}"))?
    else {
        return Ok(false);
    };
    let Some(permitted) = &constraints.value.permitted_subtrees else {
        return Ok(false);
    };
    let (required_dns, required_ip_constraints) = required_ca_name_constraints(settings)?;
    let dns_covered = required_dns.iter().all(|required| {
        permitted.iter().any(|subtree| {
            matches!(
                subtree.base,
                x509_parser::extensions::GeneralName::DNSName(name)
                    if dns_constraint_covers(name, required)
            )
        })
    });
    let ip_covered = required_ip_constraints.iter().all(|required| {
        permitted.iter().any(|subtree| {
            matches!(
                subtree.base,
                x509_parser::extensions::GeneralName::IPAddress(bytes)
                    if bytes == required.as_slice()
            )
        })
    });
    Ok(dns_covered && ip_covered)
}

fn dns_constraint_covers(permitted: &str, required: &str) -> bool {
    let permitted = permitted.to_ascii_lowercase();
    let required = required.to_ascii_lowercase();
    if permitted.is_empty() {
        return false;
    }
    if permitted.starts_with('.') {
        return required.ends_with(&permitted) && required.len() > permitted.len();
    }
    required == permitted || required.ends_with(&format!(".{permitted}"))
}

fn leaf_pair_invalid(store: &StateStore, files: CertFileState) -> bool {
    if !files.leaf_exists || !files.leaf_key_exists {
        return false;
    }
    match certificate_is_current(&store.leaf_path()) {
        Ok(true) => {}
        Ok(false) => return true,
        Err(error) => {
            eprintln!(
                "jig proxy leaf certificate validity could not be checked; regenerating it: {error:#}"
            );
            return true;
        }
    }
    match private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()) {
        Ok(true) => false,
        Ok(false) => true,
        Err(error) => {
            eprintln!("jig proxy leaf key/certificate pair is invalid; regenerating it: {error:#}");
            true
        }
    }
}

fn write_leaf(store: &StateStore, hosts: &[String]) -> Result<()> {
    ensure_leaf_hosts_within_ca_constraints(&store.ca_path(), hosts)?;
    let mut leaf_params = CertificateParams::new(hosts.to_vec())?;
    set_validity(&mut leaf_params, LEAF_VALIDITY_DAYS);
    leaf_params.distinguished_name = DistinguishedName::new();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "Jig Dev Proxy");
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let (leaf_key, leaf_key_pem) = leaf_key_for_write(store)?;
    let leaf = {
        let ca_cert_pem = fs::read_to_string(store.ca_path())?;
        let ca_key_pem = read_private_key(&store.ca_key_path())?;
        // rcgen::KeyPair does not zeroize on drop. Keep the parsed CA key in
        // the smallest practical scope; the PEM buffer itself is zeroized.
        let ca_key = KeyPair::from_pem(&ca_key_pem).context("Failed to parse Jig proxy CA key")?;
        let ca = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key)
            .context("Failed to parse Jig proxy CA certificate")?;
        leaf_params.signed_by(&leaf_key, &ca)?
    };

    write_public_pem(store.leaf_path(), &leaf.pem())?;
    if let Some(leaf_key_pem) = leaf_key_pem {
        write_private_key(store.leaf_key_path(), &leaf_key_pem)?;
    } else {
        restrict_private_key(&store.leaf_key_path())?;
    }
    write_leaf_hosts(store, hosts)?;
    Ok(())
}

fn leaf_key_for_write(store: &StateStore) -> Result<(KeyPair, Option<Zeroizing<String>>)> {
    match read_optional_private_key(&store.leaf_key_path()) {
        Ok(Some(pem)) => match KeyPair::from_pem(&pem) {
            Ok(key) => return Ok((key, None)),
            Err(error) => {
                eprintln!("jig proxy failed to parse existing leaf key; regenerating it: {error}");
            }
        },
        Ok(None) => {}
        Err(error) => return Err(error).context("Failed to read Jig proxy leaf private key"),
    }

    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let leaf_key_pem = Zeroizing::new(leaf_key.serialize_pem());
    Ok((leaf_key, Some(leaf_key_pem)))
}

pub(crate) fn certificate_hosts(
    settings: &ProxySettings,
    store: &StateStore,
    hostnames: &[String],
) -> Result<Vec<String>> {
    match certificate_hosts_interruptible(settings, store, hostnames, &|| false)? {
        LockOutcome::Acquired(hosts) => Ok(hosts),
        LockOutcome::Cancelled => bail!("uncancelled certificate host collection was cancelled"),
    }
}

fn certificate_hosts_interruptible(
    settings: &ProxySettings,
    store: &StateStore,
    hostnames: &[String],
    cancelled: &impl Fn() -> bool,
) -> Result<LockOutcome<Vec<String>>> {
    let mut hosts = vec!["localhost".to_string()];
    if settings.lan && settings.tld.eq_ignore_ascii_case("local") {
        eprintln!(
            "jig proxy LAN mode with tld=local omits the broad *.local certificate SAN; use repo-scoped route hostnames or explicit additional DNS names."
        );
    }
    hosts.extend(settings.additional_dns_names.clone());
    if settings.lan
        && let Some(ip) = bindable_lan_ip()
    {
        hosts.push(ip.to_string());
    }
    hosts.extend(hostnames.iter().cloned());
    // Keep certificate generation on the cert-lock -> route-lock ordering.
    // Route mutation paths do not acquire the cert lock while holding routes.
    // Do not call certificate generation or certificate refresh from inside a
    // route-lock closure; that would invert this order and can deadlock.
    let routes = match store.read_routes_interruptible(true, cancelled)? {
        LockOutcome::Acquired(routes) => routes,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    hosts.extend(routes.into_iter().map(|route| route.hostname.into_string()));
    for host in &mut hosts {
        if host.parse::<IpAddr>().is_err() {
            *host = host.to_ascii_lowercase();
        }
    }
    hosts.sort();
    hosts.dedup();
    if hosts.len() > MAX_CERTIFICATE_HOSTS {
        bail!(
            "Jig proxy certificate would contain {} SAN entries, above the limit of {MAX_CERTIFICATE_HOSTS}. Prune stale routes or use a dedicated JIG_PROXY_STATE_DIR.",
            hosts.len()
        );
    }
    for host in &hosts {
        validate_certificate_host(host)?;
    }
    Ok(LockOutcome::Acquired(hosts))
}

fn validate_certificate_host(host: &str) -> Result<()> {
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if let Some(rest) = host.strip_prefix("*.") {
        validate_hostname(rest)
    } else {
        validate_hostname(host)
    }
}

fn ensure_leaf_hosts_within_ca_constraints(ca_path: &Path, hosts: &[String]) -> Result<()> {
    let der = first_certificate_der(ca_path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    let Some(constraints) = cert
        .name_constraints()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA name constraints: {error}"))?
    else {
        bail!(
            "Jig proxy CA certificate has no name constraints; refusing to issue a leaf certificate"
        );
    };
    let Some(permitted) = &constraints.value.permitted_subtrees else {
        bail!(
            "Jig proxy CA certificate has no permitted name constraints; refusing to issue a leaf certificate"
        );
    };
    for host in hosts {
        let allowed = if let Ok(ip) = host.parse::<IpAddr>() {
            permitted.iter().any(|subtree| {
                matches!(
                    subtree.base,
                    x509_parser::extensions::GeneralName::IPAddress(bytes)
                        if ip_constraint_covers(bytes, ip)
                )
            })
        } else {
            permitted.iter().any(|subtree| {
                matches!(
                    subtree.base,
                    x509_parser::extensions::GeneralName::DNSName(name)
                        if dns_constraint_covers(name, host)
                )
            })
        };
        if !allowed {
            bail!("Jig proxy leaf certificate host '{host}' is outside the CA name constraints");
        }
    }
    Ok(())
}

fn ip_constraint_covers(constraint: &[u8], ip: IpAddr) -> bool {
    match (ip, constraint.len()) {
        (IpAddr::V4(ip), 8) => ip_matches_mask(&ip.octets(), &constraint[..4], &constraint[4..]),
        (IpAddr::V4(ip), 4) => constraint == ip.octets(),
        (IpAddr::V6(ip), 32) => ip_matches_mask(&ip.octets(), &constraint[..16], &constraint[16..]),
        (IpAddr::V6(ip), 16) => constraint == ip.octets(),
        _ => false,
    }
}

fn ip_matches_mask(ip: &[u8], base: &[u8], mask: &[u8]) -> bool {
    ip.len() == base.len()
        && base.len() == mask.len()
        && ip
            .iter()
            .zip(base)
            .zip(mask)
            .all(|((ip, base), mask)| (ip & mask) == (base & mask))
}

fn write_public_pem(path: PathBuf, pem: &str) -> Result<()> {
    file_ops::write_atomic_text(path, pem, CERT_FILE_FALLBACK)
}

fn write_leaf_hosts(store: &StateStore, hosts: &[String]) -> Result<()> {
    let record = LeafHostsRecord {
        version: LEAF_HOSTS_VERSION,
        hosts: hosts.to_vec(),
        ca_hash: file_hash(&store.ca_path()).context("Failed to hash Jig proxy CA certificate")?,
        certificate_hash: file_hash(&store.leaf_path())
            .context("Failed to hash Jig proxy leaf certificate")?,
    };
    let text = serde_json::to_string_pretty(&record)?;
    file_ops::write_atomic_text(store.leaf_hosts_path(), &text, CERT_FILE_FALLBACK)
}

fn leaf_matches_hosts(store: &StateStore, desired_hosts: &[String]) -> bool {
    // This sidecar is written in the same locked regeneration path as the leaf
    // certificate. Treat parse/read failure as a cache miss and regenerate.
    if !store.leaf_path().exists()
        || !store.leaf_key_path().exists()
        || !store.leaf_hosts_path().exists()
    {
        return false;
    }
    let Ok(text) = fs::read_to_string(store.leaf_hosts_path()) else {
        return false;
    };
    let Ok(existing) = serde_json::from_str::<LeafHostsFile>(&text) else {
        return false;
    };
    match existing {
        LeafHostsFile::Record(record) if record.version == LEAF_HOSTS_VERSION => {
            if !hosts_cover(&record.hosts, desired_hosts)
                || Some(record.ca_hash) != file_hash(&store.ca_path())
                || Some(record.certificate_hash) != file_hash(&store.leaf_path())
                || !certificate_is_current(&store.ca_path()).unwrap_or(false)
                || !certificate_is_current(&store.leaf_path()).unwrap_or(false)
            {
                return false;
            }
            private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path())
                .unwrap_or(false)
        }
        LeafHostsFile::Record(_) => false,
        LeafHostsFile::Legacy(hosts) => {
            let _ = hosts;
            false
        }
    }
}

fn hosts_cover(existing_hosts: &[String], desired_hosts: &[String]) -> bool {
    desired_hosts
        .iter()
        .all(|host| existing_hosts.iter().any(|existing| existing == host))
}

fn certificate_is_current(path: &Path) -> Result<bool> {
    let der = first_certificate_der(path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse certificate DER: {error}"))?;
    Ok(cert.validity().is_valid())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LeafHostsFile {
    Record(LeafHostsRecord),
    Legacy(Vec<String>),
}

#[derive(Deserialize, Serialize)]
struct LeafHostsRecord {
    version: u32,
    hosts: Vec<String>,
    ca_hash: String,
    certificate_hash: String,
}

#[derive(Deserialize, Serialize)]
struct TrustedCaRecord {
    version: u32,
    platform: String,
    ca_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ca_sha256_fingerprint: Option<String>,
}

fn write_private_key(path: PathBuf, pem: &str) -> Result<()> {
    write_owner_only_text(path, pem)
}

fn write_owner_only_text(path: PathBuf, contents: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let tmp = file_ops::temp_path(&path, CERT_FILE_FALLBACK);
        let mut file = file_ops::create_new_file(&tmp, 0o600)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(contents.as_bytes())?;
        file.sync_data()?;
        drop(file);
        file_ops::replace_file(&tmp, &path)?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = (path, contents);
        bail!("Jig proxy private key writes are not supported on this platform")
    }
}

fn read_private_key(path: &Path) -> Result<Zeroizing<String>> {
    read_optional_private_key(path)?
        .with_context(|| format!("Failed to read Jig proxy private key {}", path.display()))
}

fn read_optional_private_key(path: &Path) -> Result<Option<Zeroizing<String>>> {
    let Some(mut file) = open_optional_read_no_follow(path, MAX_PRIVATE_KEY_PEM_BYTES)? else {
        return Ok(None);
    };
    ensure_private_key_permissions(path, &file)?;
    let mut pem = Zeroizing::new(String::new());
    file.read_to_string(&mut pem)?;
    Ok(Some(pem))
}

fn ensure_private_key_permissions(path: &Path, file: &fs::File) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o7777;
        if mode != 0o600 {
            bail!(
                "Refusing to read Jig proxy private key {} with permissions {:o}; tighten it to mode 600 first.",
                path.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (path, file);
    Ok(())
}

fn restrict_private_key(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let file = match file_ops::open_read_no_follow(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn ca_params(settings: &ProxySettings) -> Result<CertificateParams> {
    let mut params = CertificateParams::default();
    params.serial_number = Some(random_serial_number()?);
    set_validity(&mut params, CA_VALIDITY_DAYS);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.name_constraints = Some(ca_name_constraints(settings)?);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, JIG_CA_COMMON_NAME);
    Ok(params)
}

fn ca_name_constraints(settings: &ProxySettings) -> Result<NameConstraints> {
    let mut permitted_subtrees = Vec::new();
    for dns_name in ca_dns_name_constraints(settings)? {
        permitted_subtrees.push(GeneralSubtree::DnsName(dns_name));
    }
    for (ip, prefix) in ca_ip_name_constraints(settings) {
        permitted_subtrees.push(GeneralSubtree::IpAddress(CidrSubnet::from_addr_prefix(
            ip, prefix,
        )));
    }
    permitted_subtrees.sort_by_key(|subtree| format!("{subtree:?}"));
    permitted_subtrees.dedup();
    Ok(NameConstraints {
        permitted_subtrees,
        excluded_subtrees: Vec::new(),
    })
}

fn required_ca_name_constraints(settings: &ProxySettings) -> Result<(Vec<String>, Vec<Vec<u8>>)> {
    let dns_names = ca_dns_name_constraints(settings)?;
    let ip_constraints = ca_ip_name_constraints(settings)
        .into_iter()
        .map(|(ip, prefix)| ip_name_constraint_bytes(ip, prefix))
        .collect();
    Ok((dns_names, ip_constraints))
}

fn ca_dns_name_constraints(settings: &ProxySettings) -> Result<Vec<String>> {
    validate_tld(&settings.tld).with_context(|| {
        format!(
            "Invalid dev proxy TLD '{}' for CA name constraints",
            settings.tld
        )
    })?;
    let tld = settings.tld.to_ascii_lowercase();
    let mut dns_names = vec!["localhost".to_string()];
    if tld == "localhost" {
        dns_names.push(tld);
    }
    for name in &settings.additional_dns_names {
        if name.parse::<IpAddr>().is_err() {
            let name = name.strip_prefix("*.").unwrap_or(name).to_ascii_lowercase();
            validate_routed_hostname(&name).with_context(|| {
                format!(
                    "Additional DNS name '{name}' must be a configured Jig development DNS name"
                )
            })?;
            dns_names.push(name);
        }
    }
    dns_names.sort();
    dns_names.dedup();
    Ok(dns_names)
}

fn ca_ip_name_constraints(settings: &ProxySettings) -> Vec<(IpAddr, u8)> {
    let mut ip_constraints = vec![
        (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8),
        (IpAddr::V6(Ipv6Addr::LOCALHOST), 128),
    ];
    ip_constraints.extend(settings.additional_dns_names.iter().filter_map(|name| {
        let ip = name.parse::<IpAddr>().ok()?;
        let prefix = match ip {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        Some((ip, prefix))
    }));
    if settings.lan
        && let Some(ip) = bindable_lan_ip()
    {
        let prefix = match ip {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        ip_constraints.push((ip, prefix));
    }
    ip_constraints.sort();
    ip_constraints.dedup();
    ip_constraints
}

fn ip_name_constraint_bytes(ip: IpAddr, prefix: u8) -> Vec<u8> {
    let (mut bytes, len) = match ip {
        IpAddr::V4(ip) => (ip.octets().to_vec(), 4usize),
        IpAddr::V6(ip) => (ip.octets().to_vec(), 16usize),
    };
    bytes.extend(prefix_mask_bytes(len, prefix));
    bytes
}

fn prefix_mask_bytes(len: usize, prefix: u8) -> Vec<u8> {
    let mut mask = vec![0u8; len];
    for bit in 0..usize::from(prefix).min(len * 8) {
        mask[bit / 8] |= 0x80 >> (bit % 8);
    }
    mask
}

fn bindable_lan_ip() -> Option<IpAddr> {
    local_lan_ip_for_ipv4_listener().filter(|ip| TcpListener::bind((*ip, 0)).is_ok())
}

fn random_serial_number() -> Result<SerialNumber> {
    let mut bytes = [0u8; CA_SERIAL_NUMBER_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow!("Failed to generate random CA serial number: {error}"))?;
    // X.509 serials are positive DER integers. Keep the high bit clear and
    // the first byte nonzero so DER preserves the full 16-byte serial while
    // retaining roughly 127 bits of entropy.
    bytes[0] = (bytes[0] & 0x7f) | 0x01;
    Ok(SerialNumber::from_slice(&bytes))
}

fn set_validity(params: &mut CertificateParams, days: i64) {
    let not_before = OffsetDateTime::now_utc() - TimeDuration::days(1);
    params.not_before = not_before;
    params.not_after = not_before + TimeDuration::days(days);
}

fn remove_stale_cert_temps(store: &StateStore) -> Result<()> {
    for entry in fs::read_dir(store.root())? {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_cert_temp_name)
            && let Err(error) = fs::remove_file(&path)
        {
            eprintln!(
                "jig proxy could not remove stale certificate temp file {}: {error}",
                path.display()
            );
        }
    }
    Ok(())
}

fn is_cert_temp_name(name: &str) -> bool {
    [
        "ca.pem.",
        "ca-key.pem.",
        "leaf.pem.",
        "leaf-key.pem.",
        "leaf-hosts.json.",
        "trusted-ca.json.",
        "trusted-anchors.pem.",
    ]
    .iter()
    .any(|prefix| name.strip_prefix(prefix).is_some_and(generated_temp_suffix))
}

fn generated_temp_suffix(suffix: &str) -> bool {
    let mut parts = suffix.split('.');
    let Some(pid) = parts.next() else {
        return false;
    };
    let Some(timestamp_ms) = parts.next() else {
        return false;
    };
    let Some(counter) = parts.next() else {
        return false;
    };
    matches!(parts.next(), Some("tmp"))
        && parts.next().is_none()
        && !pid.is_empty()
        && !timestamp_ms.is_empty()
        && !counter.is_empty()
        && pid.bytes().all(|ch| ch.is_ascii_digit())
        && timestamp_ms.bytes().all(|ch| ch.is_ascii_digit())
        && counter.bytes().all(|ch| ch.is_ascii_digit())
}

fn file_hash(path: &Path) -> Option<String> {
    let mut file = open_optional_read_no_follow(path, MAX_CERT_PEM_BYTES)
        .ok()
        .flatten()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(hex_lower(&Sha256::digest(bytes)))
}

fn certificate_paths(store: &StateStore) -> Value {
    json!({
        "ok": true,
        "state_dir": store.root(),
        "ca": store.ca_path(),
        "certificate": store.leaf_path(),
        "key": store.leaf_key_path(),
        "trust_warning": GLOBAL_CA_TRUST_WARNING,
    })
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_trusted_ca_marker(store: &StateStore) -> Result<()> {
    let record = TrustedCaRecord {
        version: TRUSTED_CA_VERSION,
        platform: std::env::consts::OS.to_string(),
        ca_hash: file_hash(&store.ca_path()).context("Failed to hash Jig proxy CA certificate")?,
        ca_fingerprint: trusted_ca_marker_fingerprint_for_current_platform(store)?,
        ca_sha256_fingerprint: trusted_ca_marker_sha256_fingerprint_for_current_platform(store)?,
    };
    write_owner_only_text(
        store.trusted_ca_path(),
        &serde_json::to_string_pretty(&record)?,
    )
}

fn ensure_jig_ca_certificate(store: &StateStore) -> Result<()> {
    let der = first_certificate_der(&store.ca_path())?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    let basic_constraints = cert
        .basic_constraints()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA basic constraints: {error}"))?
        .context("Refusing to trust a certificate without CA basic constraints")?;
    if !basic_constraints.value.ca {
        bail!("Refusing to trust a certificate that is not a CA");
    }
    ensure_jig_ca_key_usages(&cert)?;
    let common_name = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|common_name| common_name.as_str().ok());
    if common_name != Some(JIG_CA_COMMON_NAME) {
        bail!(
            "Refusing to trust CA certificate {} because it was not issued as a Jig Dev Proxy Local CA",
            store.ca_path().display()
        );
    }
    ensure_jig_ca_crypto_provenance(store)?;
    Ok(())
}

fn ensure_jig_ca_key_usages(cert: &x509_parser::certificate::X509Certificate<'_>) -> Result<()> {
    let key_usage = cert
        .key_usage()
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA key usage: {error}"))?
        .context("Refusing to trust Jig proxy CA because it is missing key usage")?;
    let key_usage = key_usage.value;
    if !key_usage.key_cert_sign() || !key_usage.crl_sign() || !key_usage.digital_signature() {
        bail!("Refusing to trust Jig proxy CA because it is missing expected key usage");
    }
    Ok(())
}

fn ensure_jig_ca_crypto_provenance(store: &StateStore) -> Result<()> {
    let der = first_certificate_der(&store.ca_path())?;
    let (remaining, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse Jig proxy CA certificate DER: {error}"))?;
    if !remaining.is_empty() {
        bail!("Refusing to trust Jig proxy CA certificate with trailing DER data");
    }
    if !cert.validity().is_valid() {
        bail!(
            "Refusing to trust Jig proxy CA certificate outside its validity period. Run `scripts/jig proxy cert generate --force` after untrusting the expired CA."
        );
    }
    if cert.subject() != cert.issuer() {
        bail!("Refusing to trust Jig proxy CA certificate that is not self-issued");
    }
    let public_key_algorithm = &cert.public_key().algorithm;
    if public_key_algorithm.algorithm != OID_KEY_TYPE_EC_PUBLIC_KEY {
        bail!("Refusing to trust Jig proxy CA certificate that is not an ECDSA P-256 CA");
    }
    let curve_oid = public_key_algorithm
        .parameters
        .as_ref()
        .and_then(|parameters| parameters.as_oid().ok());
    if curve_oid != Some(OID_EC_P256) {
        bail!("Refusing to trust Jig proxy CA certificate that is not an ECDSA P-256 CA");
    }
    if cert.signature_algorithm.algorithm != OID_SIG_ECDSA_WITH_SHA256 {
        bail!("Refusing to trust Jig proxy CA certificate without an ECDSA-SHA256 self-signature");
    }
    cert.verify_signature(None)
        .map_err(|error| anyhow!("Refusing to trust Jig proxy CA certificate: {error}"))?;
    Ok(())
}

fn ensure_jig_ca_private_key(store: &StateStore) -> Result<()> {
    if !private_key_matches_certificate(&store.ca_key_path(), &store.ca_path())? {
        bail!(
            "Refusing to trust Jig proxy CA because ca-key.pem does not match the CA certificate"
        );
    }
    Ok(())
}

fn private_key_matches_certificate(key_path: &Path, cert_path: &Path) -> Result<bool> {
    let key_pem = read_private_key(key_path)?;
    // rcgen::KeyPair does not zeroize its parsed key material on drop; the
    // zeroized PEM buffer keeps the readable copy scoped to this validation.
    let key_pair = KeyPair::from_pem(&key_pem)
        .with_context(|| format!("Failed to parse private key {}", key_path.display()))?;
    let der = first_certificate_der(cert_path)?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse certificate DER: {error}"))?;
    Ok(cert.public_key().subject_public_key.data.as_ref() == key_pair.public_key_raw())
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_trusted_ca_marker(store: &StateStore) {
    match fs::remove_file(store.trusted_ca_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "jig proxy could not remove trusted CA marker {}: {error}",
            store.trusted_ca_path().display()
        ),
    }
}

fn trusted_ca_marker_matches(store: &StateStore) -> Result<bool> {
    let Some(record) = read_trusted_ca_marker(store)? else {
        return Ok(false);
    };
    Ok(record.version == TRUSTED_CA_VERSION && Some(record.ca_hash) == file_hash(&store.ca_path()))
}

fn read_trusted_ca_marker(store: &StateStore) -> Result<Option<TrustedCaRecord>> {
    let Some(text) = file_ops::read_text_no_follow(&store.trusted_ca_path())? else {
        return Ok(None);
    };
    let record = serde_json::from_str::<TrustedCaRecord>(&text).with_context(|| {
        format!(
            "Failed to parse trusted CA marker {}",
            store.trusted_ca_path().display()
        )
    })?;
    Ok(Some(record))
}

#[cfg(target_os = "linux")]
fn trusted_ca_marker_owned_by_current_platform(store: &StateStore) -> Result<bool> {
    let Some(record) = read_trusted_ca_marker(store)? else {
        return Ok(false);
    };
    Ok(record.version == TRUSTED_CA_VERSION && record.platform == std::env::consts::OS)
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn trusted_ca_marker_fingerprint_for_current_platform(
    store: &StateStore,
) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Some(ca_sha1_hex(&store.ca_path())?))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = store;
        Ok(None)
    }
}
#[cfg(target_os = "macos")]
fn trusted_ca_marker_sha256_fingerprint_for_current_platform(
    store: &StateStore,
) -> Result<Option<String>> {
    Ok(Some(ca_sha256_hex(&store.ca_path())?))
}
#[cfg(target_os = "linux")]
fn trusted_ca_marker_sha256_fingerprint_for_current_platform(
    store: &StateStore,
) -> Result<Option<String>> {
    let _ = store;
    Ok(None)
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct CertificateFingerprints {
    sha1: String,
    sha256: String,
}

#[cfg(target_os = "macos")]
impl CertificateFingerprints {
    fn matches(&self, other: &Self) -> bool {
        self.sha1.eq_ignore_ascii_case(&other.sha1)
            && self.sha256.eq_ignore_ascii_case(&other.sha256)
    }
}

#[cfg(target_os = "macos")]
fn trusted_ca_marker_fingerprints(store: &StateStore) -> Result<Option<CertificateFingerprints>> {
    let Some(record) = read_trusted_ca_marker(store)? else {
        return Ok(None);
    };
    if record.version != TRUSTED_CA_VERSION || record.platform != "macos" {
        return Ok(None);
    }
    let Some(sha1) = record.ca_fingerprint else {
        return Ok(None);
    };
    let Some(sha256) = record.ca_sha256_fingerprint else {
        return Ok(None);
    };
    Ok(Some(CertificateFingerprints { sha1, sha256 }))
}

#[cfg(target_os = "macos")]
fn ca_sha1_hex(path: &Path) -> Result<String> {
    Ok(hex_upper(&Sha1::digest(first_certificate_der(path)?)))
}

#[cfg(target_os = "macos")]
fn ca_sha256_hex(path: &Path) -> Result<String> {
    Ok(hex_upper(&Sha256::digest(first_certificate_der(path)?)))
}

#[cfg(target_os = "macos")]
fn ca_fingerprints(path: &Path) -> Result<CertificateFingerprints> {
    Ok(CertificateFingerprints {
        sha1: ca_sha1_hex(path)?,
        sha256: ca_sha256_hex(path)?,
    })
}

fn first_certificate_der(path: &Path) -> Result<Vec<u8>> {
    let file = open_required_read_no_follow(path, MAX_CERT_PEM_BYTES)?;
    let mut reader = std::io::BufReader::new(file);
    let mut certs = rustls_pemfile::certs(&mut reader);
    let Some(cert) = certs.next() else {
        bail!(
            "CA certificate does not contain a PEM certificate: {}",
            path.display()
        );
    };
    let cert = cert.context("Failed to parse CA certificate PEM")?;
    if certs
        .next()
        .transpose()
        .context("Failed to parse CA certificate PEM")?
        .is_some()
    {
        bail!(
            "CA certificate contains more than one PEM certificate: {}",
            path.display()
        );
    }
    Ok(cert.as_ref().to_vec())
}

fn open_required_read_no_follow(path: &Path, max_bytes: u64) -> Result<fs::File> {
    open_optional_read_no_follow(path, max_bytes)?
        .with_context(|| format!("File does not exist: {}", path.display()))
}

fn open_optional_read_no_follow(path: &Path, max_bytes: u64) -> Result<Option<fs::File>> {
    let file = match file_ops::open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    let len = metadata.len();
    if len > max_bytes {
        bail!(
            "Refusing to read {} because it is {} bytes, above the {} byte limit",
            path.display(),
            len,
            max_bytes
        );
    }
    Ok(Some(file))
}

#[cfg(target_os = "macos")]
fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(target_os = "macos")]
#[cfg(test)]
mod tests;
