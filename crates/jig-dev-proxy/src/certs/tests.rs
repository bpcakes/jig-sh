use crate::test_tempdir as tempdir;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::trust::command_path_arg;
use super::trust::{
    macos_untrust_warning, pem_sha256_hex, security_find_certificate_fingerprints,
    trust_list_jig_ca_uris,
};
use super::*;
use crate::types::{Route, RouteMode};

#[test]
fn ca_provenance_rejects_non_jig_common_name() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let mut params = ca_params(&ProxySettings::default()).unwrap();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "Not Jig");
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    fs::write(store.ca_path(), cert.pem()).unwrap();
    fs::write(store.ca_key_path(), key.serialize_pem()).unwrap();

    let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

    assert!(error.contains("was not issued as a Jig Dev Proxy Local CA"));
}

#[test]
fn ca_provenance_rejects_missing_key_usage() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let mut params = ca_params(&ProxySettings::default()).unwrap();
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = params.self_signed(&key).unwrap();
    fs::write(store.ca_path(), cert.pem()).unwrap();
    fs::write(store.ca_key_path(), key.serialize_pem()).unwrap();

    let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

    assert!(error.contains("missing expected key usage"));
}

#[test]
fn ca_provenance_rejects_expired_ca_certificate() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let mut params = ca_params(&ProxySettings::default()).unwrap();
    params.not_before = OffsetDateTime::now_utc() - TimeDuration::days(10);
    params.not_after = OffsetDateTime::now_utc() - TimeDuration::days(1);
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = params.self_signed(&key).unwrap();
    fs::write(store.ca_path(), cert.pem()).unwrap();
    fs::write(store.ca_key_path(), key.serialize_pem()).unwrap();

    let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

    assert!(error.contains("outside its validity period"));
}

#[cfg(unix)]
#[test]
fn ca_provenance_rejects_multiple_pem_certificates() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let pem = fs::read_to_string(store.ca_path()).unwrap();
    fs::write(store.ca_path(), format!("{pem}\n{pem}")).unwrap();

    let error = ensure_jig_ca_certificate(&store).unwrap_err().to_string();

    assert!(error.contains("more than one PEM certificate"));
}

#[cfg(unix)]
#[test]
fn trust_rejects_ca_key_mismatch() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    fs::write(store.ca_key_path(), other_key.serialize_pem()).unwrap();

    let error = trust(&settings, false).unwrap_err().to_string();

    assert!(error.contains("does not match the CA certificate"));
}

#[cfg(unix)]
#[test]
fn trust_requires_accept_trust_scope() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();

    let error = trust(&settings, false).unwrap_err().to_string();

    assert!(error.contains("--accept-trust-scope"));
}

#[test]
fn untrust_requires_accept_trust_scope() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let error = untrust(&settings, false).unwrap_err().to_string();

    assert!(error.contains("--accept-trust-scope"));
}

#[test]
fn security_fingerprint_parser_pairs_sha1_with_pem_sha256() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let pem = fs::read_to_string(store.ca_path()).unwrap();
    let expected_sha256 = pem_sha256_hex(&pem).unwrap();
    let output = format!("SHA-1 hash: 00112233445566778899AABBCCDDEEFF00112233\n{pem}");

    assert_eq!(
        security_find_certificate_fingerprints(output.as_bytes()),
        vec![CertificateFingerprints {
            sha1: "00112233445566778899AABBCCDDEEFF00112233".to_string(),
            sha256: expected_sha256,
        }]
    );
}

#[test]
fn security_fingerprint_parser_rejects_malformed_sha1() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let pem = fs::read_to_string(store.ca_path()).unwrap();
    let output = format!("SHA-1 hash: not-a-fingerprint\n{pem}");

    assert!(security_find_certificate_fingerprints(output.as_bytes()).is_empty());
}

#[test]
fn certificate_hosts_include_existing_routes_and_new_hosts() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "web.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: None,
            owner_start_token: None,
            mode: RouteMode::Alias,
            created_at_ms: crate::state::now_ms(),
        })
        .unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["api.demo.localhost".into()],
        ..ProxySettings::default()
    };

    let hosts = certificate_hosts(&settings, &store, &["admin.demo.localhost".into()]).unwrap();

    assert!(hosts.contains(&"localhost".to_string()));
    assert!(!hosts.contains(&"*.localhost".to_string()));
    assert!(hosts.contains(&"api.demo.localhost".to_string()));
    assert!(hosts.contains(&"web.demo.localhost".to_string()));
    assert!(hosts.contains(&"admin.demo.localhost".to_string()));
}

#[test]
fn certificate_hosts_do_not_add_bare_wildcard_for_custom_tld() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        tld: "corp.example".into(),
        additional_dns_names: vec!["*.demo.corp.example".into()],
        ..ProxySettings::default()
    };

    let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

    assert!(hosts.contains(&"*.demo.corp.example".to_string()));
    assert!(!hosts.contains(&"corp.example".to_string()));
    assert!(!hosts.contains(&"*.corp.example".to_string()));
}

#[test]
fn certificate_hosts_do_not_add_bare_local_wildcard_in_lan_mode() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        tld: "local".into(),
        lan: true,
        additional_dns_names: vec!["*.demo.local".into()],
        ..ProxySettings::default()
    };

    let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

    assert!(hosts.contains(&"*.demo.local".to_string()));
    assert!(!hosts.contains(&"local".to_string()));
    assert!(!hosts.contains(&"*.local".to_string()));
}

#[test]
fn certificate_hosts_reject_invalid_additional_dns_name() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["bad,name.localhost".into()],
        ..ProxySettings::default()
    };

    let error = certificate_hosts(&settings, &store, &[])
        .unwrap_err()
        .to_string();

    assert!(error.contains("invalid hostname"));
}

#[test]
fn certificate_hosts_normalize_dns_names_to_lowercase() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        tld: "LOCALHOST".into(),
        additional_dns_names: vec!["API.DEMO.LOCALHOST".into()],
        ..ProxySettings::default()
    };

    let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

    assert!(hosts.contains(&"api.demo.localhost".to_string()));
    assert!(hosts.contains(&"localhost".to_string()));
    assert!(!hosts.contains(&"*.localhost".to_string()));
}

#[test]
fn certificate_hosts_reject_excessive_sans() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: (0..MAX_CERTIFICATE_HOSTS)
            .map(|index| format!("host-{index}.localhost"))
            .collect(),
        ..ProxySettings::default()
    };

    let error = certificate_hosts(&settings, &store, &[])
        .unwrap_err()
        .to_string();

    assert!(error.contains("SAN entries"));
}

#[test]
fn cert_temp_names_require_generated_suffix() {
    assert!(is_cert_temp_name("ca.pem.123.456.0.tmp"));
    assert!(is_cert_temp_name("leaf-hosts.json.123.456.0.tmp"));
    assert!(is_cert_temp_name("trusted-ca.json.123.456.0.tmp"));
    assert!(is_cert_temp_name("trusted-anchors.pem.123.456.0.tmp"));
    assert!(!is_cert_temp_name("ca.pem.attacker.tmp"));
    assert!(!is_cert_temp_name("leaf.pem.123.tmp"));
}

#[test]
fn linux_trust_list_parser_finds_jig_ca_uris() {
    let output = br"
pkcs11:id=%01;type=cert
    type: certificate
    label: Jig Dev Proxy Local CA
pkcs11:id=%02;type=cert
    type: certificate
    label: Other CA
pkcs11:id=%03;type=cert
    type: certificate
    label: Jig Dev Proxy Local CA
";

    assert_eq!(
        trust_list_jig_ca_uris(output),
        vec![
            "pkcs11:id=%01;type=cert".to_string(),
            "pkcs11:id=%03;type=cert".to_string()
        ]
    );
    assert!(trust_list_jig_ca_uris(b"label: Other CA\n").is_empty());
}

#[test]
fn macos_untrust_warning_only_reports_limit() {
    assert!(macos_untrust_warning(0).is_some());
    assert_eq!(macos_untrust_warning(10), None);
    assert!(macos_untrust_warning(MACOS_UNTRUST_REMOVAL_LIMIT).is_some());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn command_path_arg_disambiguates_dash_prefixed_relative_paths() {
    assert_eq!(
        command_path_arg(Path::new("-state/ca.pem")),
        Path::new(".").join("-state/ca.pem").into_os_string()
    );
    assert_eq!(
        command_path_arg(Path::new("state/ca.pem")),
        Path::new("state/ca.pem").as_os_str().to_owned()
    );
}

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
#[test]
fn certificate_hosts_prune_dead_process_routes() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    store
        .add_route(Route {
            hostname: "stale.demo.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(999_999),
            owner_start_token: Some("dead-process".into()),
            mode: RouteMode::Process,
            created_at_ms: crate::state::now_ms(),
        })
        .unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

    assert!(!hosts.contains(&"stale.demo.localhost".to_string()));
}

#[cfg(unix)]
#[test]
fn generated_private_keys_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let ca_mode = fs::metadata(store.ca_key_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let leaf_mode = fs::metadata(store.leaf_key_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(ca_mode, 0o600);
    assert_eq!(leaf_mode, 0o600);
}

#[cfg(unix)]
#[test]
fn private_key_reads_reject_group_or_world_readable_files() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let key = temp.path().join("key.pem");
    fs::write(&key, "not a key").unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();

    let error = read_optional_private_key(&key).unwrap_err().to_string();

    assert!(error.contains("permissions 644"));
}

#[cfg(unix)]
#[test]
fn private_key_reads_reject_symlink_files() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let key = temp.path().join("key.pem");
    let target = temp.path().join("target.pem");
    fs::write(&target, "not a key").unwrap();
    symlink(&target, &key).unwrap();

    let error = read_optional_private_key(&key).unwrap_err().to_string();

    assert!(
        error.contains("Too many levels of symbolic links")
            || error.contains("symbolic link")
            || error.contains("os error 40")
    );
}

#[cfg(unix)]
#[test]
fn ensure_keeps_existing_leaf_when_hosts_are_unchanged() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["api.demo.localhost".into()],
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_key = fs::read_to_string(store.leaf_key_path()).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();

    ensure_for_hosts(&settings, &[]).unwrap();

    assert_eq!(
        fs::read_to_string(store.leaf_key_path()).unwrap(),
        first_key
    );
    assert_eq!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    assert!(store.leaf_hosts_path().exists());
}

#[cfg(unix)]
#[test]
fn ensure_keeps_existing_leaf_when_hosts_are_a_superset() {
    let temp = tempdir().unwrap();
    let prepared_settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["web.demo.localhost".into()],
        ..ProxySettings::default()
    };

    generate(&prepared_settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
    let contextless_settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    ensure(&contextless_settings).unwrap();

    assert_eq!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    assert!(
        fs::read_to_string(store.leaf_hosts_path())
            .unwrap()
            .contains("web.demo.localhost")
    );
}

#[cfg(unix)]
#[test]
fn generated_leaf_verifies_against_generated_ca() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["web.demo.localhost".into()],
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let ca_der = first_certificate_der(&store.ca_path()).unwrap();
    let leaf_der = first_certificate_der(&store.leaf_path()).unwrap();
    let (_, ca) = parse_x509_certificate(&ca_der).unwrap();
    let (_, leaf) = parse_x509_certificate(&leaf_der).unwrap();

    let constraints = ca.name_constraints().unwrap().unwrap().value;
    assert!(constraints.permitted_subtrees.is_some());
    assert!(constraints.excluded_subtrees.is_none());
    leaf.verify_signature(Some(ca.public_key())).unwrap();
}

#[cfg(unix)]
#[test]
fn ca_constraints_must_cover_additional_dns_and_ip_settings() {
    let temp = tempdir().unwrap();
    let base_settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&base_settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let same_tld_settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["*.demo.localhost".into()],
        ..ProxySettings::default()
    };
    assert!(ca_name_constraints_cover_settings(&store.ca_path(), &same_tld_settings).unwrap());

    let extended_settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        additional_dns_names: vec!["*.other.local".into(), "192.0.2.10".into()],
        ..ProxySettings::default()
    };

    assert!(!ca_name_constraints_cover_settings(&store.ca_path(), &extended_settings).unwrap());

    generate(&extended_settings, true).unwrap();
    assert!(ca_name_constraints_cover_settings(&store.ca_path(), &extended_settings).unwrap());
}

#[test]
fn ca_constraints_reject_public_additional_dns_name() {
    let settings = ProxySettings {
        additional_dns_names: vec!["com".into()],
        ..ProxySettings::default()
    };

    let error = ca_dns_name_constraints(&settings).unwrap_err().to_string();

    assert!(error.contains("configured Jig development DNS name"));
}

#[test]
fn ca_constraints_do_not_permit_bare_non_localhost_tld() {
    let settings = ProxySettings {
        tld: "internal".into(),
        additional_dns_names: vec!["*.demo.internal".into()],
        ..ProxySettings::default()
    };

    let constraints = ca_dns_name_constraints(&settings).unwrap();

    assert!(constraints.contains(&"demo.internal".to_string()));
    assert!(!constraints.contains(&"internal".to_string()));
}

#[test]
fn empty_dns_constraint_does_not_cover_hosts() {
    assert!(!dns_constraint_covers("", "web.demo.localhost"));
}

#[cfg(unix)]
#[test]
fn write_leaf_rejects_hosts_outside_ca_name_constraints() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let error = write_leaf(&store, &["example.com".into()])
        .unwrap_err()
        .to_string();

    assert!(error.contains("outside the CA name constraints"));
}

#[cfg(unix)]
#[test]
fn ensure_reuses_existing_leaf_key_when_hosts_change() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_key = fs::read_to_string(store.leaf_key_path()).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();

    ensure_for_hosts(&settings, &["api.demo.localhost".into()]).unwrap();

    assert_eq!(
        fs::read_to_string(store.leaf_key_path()).unwrap(),
        first_key
    );
    assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    assert!(
        fs::read_to_string(store.leaf_hosts_path())
            .unwrap()
            .contains("api.demo.localhost")
    );
}

#[cfg(unix)]
#[test]
fn leaf_sidecar_records_full_sha256_hashes() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let text = fs::read_to_string(store.leaf_hosts_path()).unwrap();
    let record: LeafHostsRecord = serde_json::from_str(&text).unwrap();

    assert_eq!(record.ca_hash.len(), 64);
    assert_eq!(record.certificate_hash.len(), 64);
    assert!(
        record
            .ca_hash
            .chars()
            .all(|value| value.is_ascii_hexdigit())
    );
    assert!(
        record
            .certificate_hash
            .chars()
            .all(|value| value.is_ascii_hexdigit())
    );
    assert_eq!(Some(record.ca_hash), file_hash(&store.ca_path()));
    assert_eq!(Some(record.certificate_hash), file_hash(&store.leaf_path()));
}

#[cfg(unix)]
#[test]
fn ensure_regenerates_when_leaf_sidecar_hash_is_stale() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
    let text = fs::read_to_string(store.leaf_hosts_path()).unwrap();
    let mut record: LeafHostsRecord = serde_json::from_str(&text).unwrap();
    record.certificate_hash = "stale-certificate-hash".into();
    fs::write(
        store.leaf_hosts_path(),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();

    ensure_for_hosts(&settings, &[]).unwrap();

    assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
}

#[cfg(unix)]
#[test]
fn ensure_regenerates_when_leaf_sidecar_ca_hash_is_stale() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
    let text = fs::read_to_string(store.leaf_hosts_path()).unwrap();
    let mut record: LeafHostsRecord = serde_json::from_str(&text).unwrap();
    record.ca_hash = "stale-ca-hash".into();
    fs::write(
        store.leaf_hosts_path(),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();

    ensure_for_hosts(&settings, &[]).unwrap();

    assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
}

#[cfg(unix)]
#[test]
fn ensure_regenerates_expired_leaf_even_when_sidecar_matches() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        https: true,
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let hosts = certificate_hosts(&settings, &store, &[]).unwrap();

    let ca_cert_pem = fs::read_to_string(store.ca_path()).unwrap();
    let ca_key_pem = read_private_key(&store.ca_key_path()).unwrap();
    let ca_key = KeyPair::from_pem(&ca_key_pem).unwrap();
    let ca = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key).unwrap();
    let leaf_key_pem = read_private_key(&store.leaf_key_path()).unwrap();
    let leaf_key = KeyPair::from_pem(&leaf_key_pem).unwrap();
    let mut leaf_params = CertificateParams::new(hosts.clone()).unwrap();
    leaf_params.not_before = OffsetDateTime::now_utc() - TimeDuration::days(3);
    leaf_params.not_after = OffsetDateTime::now_utc() - TimeDuration::days(2);
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let expired = leaf_params.signed_by(&leaf_key, &ca).unwrap();
    write_public_pem(store.leaf_path(), &expired.pem()).unwrap();
    write_leaf_hosts(&store, &hosts).unwrap();

    ensure_for_hosts(&settings, &[]).unwrap();

    assert!(certificate_is_current(&store.leaf_path()).unwrap());
    assert_ne!(
        fs::read_to_string(store.leaf_path()).unwrap(),
        expired.pem()
    );
}

#[cfg(unix)]
#[test]
fn ensure_regenerates_mismatched_ca_key_pair() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_ca = fs::read_to_string(store.ca_path()).unwrap();
    let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    fs::write(store.ca_key_path(), other_key.serialize_pem()).unwrap();

    ensure_for_hosts(&settings, &[]).unwrap();

    assert_ne!(fs::read_to_string(store.ca_path()).unwrap(), first_ca);
    assert!(private_key_matches_certificate(&store.ca_key_path(), &store.ca_path()).unwrap());
}

#[cfg(unix)]
#[test]
fn ensure_regenerates_mismatched_leaf_key_pair() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
    let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    fs::write(store.leaf_key_path(), other_key.serialize_pem()).unwrap();

    ensure_for_hosts(&settings, &[]).unwrap();

    assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    assert!(private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()).unwrap());
}

#[cfg(unix)]
#[test]
fn generate_repairs_mismatched_leaf_key_pair_without_force() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_cert = fs::read_to_string(store.leaf_path()).unwrap();
    let other_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    fs::write(store.leaf_key_path(), other_key.serialize_pem()).unwrap();

    generate(&settings, false).unwrap();

    assert_ne!(fs::read_to_string(store.leaf_path()).unwrap(), first_cert);
    assert!(private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()).unwrap());
}

#[cfg(unix)]
#[test]
fn force_generate_refuses_jig_trusted_ca_marker() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_trusted_ca_marker(&store).unwrap();

    let error = generate(&settings, true).unwrap_err().to_string();

    assert!(error.contains("was trusted by Jig"));
}

#[cfg(unix)]
#[test]
fn generate_refuses_partial_trusted_ca_pair() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_trusted_ca_marker(&store).unwrap();
    fs::remove_file(store.ca_key_path()).unwrap();

    let error = generate(&settings, false).unwrap_err().to_string();

    assert!(error.contains("trusted"));
    assert!(!store.ca_key_path().exists());
}

#[cfg(unix)]
#[test]
fn ensure_refuses_partial_trusted_ca_pair() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    write_trusted_ca_marker(&store).unwrap();
    fs::remove_file(store.ca_key_path()).unwrap();

    let error = ensure_for_hosts(&settings, &[]).unwrap_err().to_string();

    assert!(error.contains("trusted"));
    assert!(!store.ca_key_path().exists());
}

#[test]
fn leaf_validity_stays_within_browser_limit() {
    let mut params = CertificateParams::default();
    set_validity(&mut params, LEAF_VALIDITY_DAYS);

    assert!((params.not_after - params.not_before) <= TimeDuration::days(200));
}

#[test]
fn ca_validity_is_bounded_for_local_development_roots() {
    let mut params = CertificateParams::default();
    set_validity(&mut params, CA_VALIDITY_DAYS);

    assert!((params.not_after - params.not_before) <= TimeDuration::days(730));
}

#[cfg(unix)]
#[test]
fn generated_ca_serials_are_random_128_bit_values() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first = ca_serial_bytes(&store).unwrap();

    generate(&settings, true).unwrap();
    let second = ca_serial_bytes(&store).unwrap();

    assert_eq!(first.len(), CA_SERIAL_NUMBER_BYTES);
    assert_eq!(second.len(), CA_SERIAL_NUMBER_BYTES);
    assert!(first.iter().any(|byte| *byte != 0));
    assert!(second.iter().any(|byte| *byte != 0));
    assert_ne!(first, second);
}

#[cfg(unix)]
#[test]
fn force_generate_rotates_leaf_private_key_with_ca() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let first_leaf_key = fs::read_to_string(store.leaf_key_path()).unwrap();

    generate(&settings, true).unwrap();

    assert_ne!(
        fs::read_to_string(store.leaf_key_path()).unwrap(),
        first_leaf_key
    );
    assert!(private_key_matches_certificate(&store.leaf_key_path(), &store.leaf_path()).unwrap());
}

fn ca_serial_bytes(store: &StateStore) -> Result<Vec<u8>> {
    let der = first_certificate_der(&store.ca_path())?;
    let (_, cert) = parse_x509_certificate(&der)
        .map_err(|error| anyhow!("Failed to parse test CA certificate DER: {error}"))?;
    Ok(cert.tbs_certificate.raw_serial().to_vec())
}

#[cfg(unix)]
#[test]
fn generate_removes_stale_certificate_temp_files() {
    let temp = tempdir().unwrap();
    let state_dir = temp.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let stale = state_dir.join("leaf.pem.123.456.0.tmp");
    let stale_trust_extract = state_dir.join("trusted-anchors.pem.123.456.1.tmp");
    fs::write(&stale, "stale").unwrap();
    fs::write(&stale_trust_extract, "stale").unwrap();
    let settings = ProxySettings {
        state_dir: Some(state_dir),
        ..ProxySettings::default()
    };

    generate(&settings, false).unwrap();

    assert!(!stale.exists());
    assert!(!stale_trust_extract.exists());
}
