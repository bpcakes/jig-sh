use super::{
    AEAD_ALGORITHM, AeadRole, AuditRoot, MAGIC, V1_FORMAT_VERSION, V2_FORMAT_VERSION,
    V3_FORMAT_VERSION, V3StateFields, VaultHeader, VaultState, payload_aad, validate_header,
    validate_v1_header_compat, validate_v2_reader_header_compat,
};
use crate::crypto::KdfParams;
use crate::types::FieldKind;

const FIXTURE_MAC: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn fixture_header(version: u32) -> VaultHeader {
    VaultHeader {
        magic: MAGIC.into(),
        version,
        vault_id: "fixture-vault".into(),
        created_at_ms: 7,
        kdf: KdfParams::default(),
        salt_b64: "c2FsdA==".into(),
        aead: AEAD_ALGORITHM.into(),
        generation: (version == V3_FORMAT_VERSION).then_some(12),
    }
}

const COMMON_AAD_FIELDS: &str = "\
magic:9:jig-vault\n\
version:1:VERSION\n\
vault_id:13:fixture-vault\n\
created_at_ms:1:7\n\
kdf.algorithm:8:argon2id\n\
kdf.memory_kib:6:131072\n\
kdf.iterations:1:3\n\
kdf.parallelism:1:4\n\
kdf.output_len:2:32\n\
salt_b64:8:c2FsdA==\n\
aead:17:xchacha20poly1305\n";

fn expected_aad(domain: &str, version: u32, extra: &str, role: &str) -> Vec<u8> {
    let common = COMMON_AAD_FIELDS.replace("VERSION", &version.to_string());
    format!(
        "{domain}\n{common}{extra}payload_role:{}:{role}\n",
        role.len()
    )
    .into_bytes()
}

#[test]
fn v1_payload_aad_remains_byte_for_byte_stable() {
    let header = fixture_header(V1_FORMAT_VERSION);
    let aad = payload_aad(&header, AeadRole::WrappedDek);
    assert_eq!(
        aad,
        b"jig-vault-header-v1\n\
magic:9:jig-vault\n\
version:1:1\n\
vault_id:13:fixture-vault\n\
created_at_ms:1:7\n\
kdf.algorithm:8:argon2id\n\
kdf.memory_kib:6:131072\n\
kdf.iterations:1:3\n\
kdf.parallelism:1:4\n\
kdf.output_len:2:32\n\
salt_b64:8:c2FsdA==\n\
aead:17:xchacha20poly1305\n\
payload_role:11:wrapped_dek\n"
    );
    assert_eq!(
        payload_aad(&header, AeadRole::State),
        expected_aad("jig-vault-header-v1", 1, "", "state")
    );
}

#[test]
fn v2_payload_aad_is_frozen_for_both_roles() {
    let header = fixture_header(V2_FORMAT_VERSION);
    assert_eq!(
        payload_aad(&header, AeadRole::WrappedDek),
        expected_aad("jig-vault-header-v2", 2, "", "wrapped_dek")
    );
    assert_eq!(
        payload_aad(&header, AeadRole::State),
        expected_aad("jig-vault-header-v2", 2, "", "state")
    );
}

#[test]
fn v3_generation_authenticates_only_the_state_role() {
    let header = fixture_header(V3_FORMAT_VERSION);
    assert_eq!(
        payload_aad(&header, AeadRole::WrappedDek),
        expected_aad("jig-vault-header-v3", 3, "", "wrapped_dek")
    );
    assert_eq!(
        payload_aad(&header, AeadRole::State),
        expected_aad("jig-vault-header-v3", 3, "generation:2:12\n", "state")
    );

    let mut advanced = header.clone();
    advanced.generation = Some(13);
    assert_eq!(
        payload_aad(&header, AeadRole::WrappedDek),
        payload_aad(&advanced, AeadRole::WrappedDek)
    );
    assert_ne!(
        payload_aad(&header, AeadRole::State),
        payload_aad(&advanced, AeadRole::State)
    );
}

#[test]
fn version_two_has_a_distinct_aad_domain_and_old_validator_rejects_it() {
    let header = fixture_header(V2_FORMAT_VERSION);
    validate_header(&header).unwrap();
    assert!(payload_aad(&header, AeadRole::State).starts_with(b"jig-vault-header-v2\n"));
    let error = validate_v1_header_compat(&header).unwrap_err().to_string();
    assert_eq!(error, "unsupported vault version 2");
}

#[test]
fn version_two_readers_reject_version_three_headers() {
    let header = fixture_header(V3_FORMAT_VERSION);
    validate_header(&header).unwrap();
    let error = validate_v2_reader_header_compat(&header)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "unsupported vault version 3");
}

#[test]
fn header_generation_is_required_exactly_for_version_three() {
    let mut missing = fixture_header(V3_FORMAT_VERSION);
    missing.generation = None;
    assert!(
        validate_header(&missing)
            .unwrap_err()
            .to_string()
            .contains("missing its generation")
    );

    let mut zero = fixture_header(V3_FORMAT_VERSION);
    zero.generation = Some(0);
    assert!(
        validate_header(&zero)
            .unwrap_err()
            .to_string()
            .contains("at least 1")
    );

    for version in [V1_FORMAT_VERSION, V2_FORMAT_VERSION] {
        let mut unexpected = fixture_header(version);
        unexpected.generation = Some(1);
        assert!(
            validate_header(&unexpected)
                .unwrap_err()
                .to_string()
                .contains("must not contain a generation")
        );
    }
}

#[test]
fn legacy_headers_keep_their_wire_shape() {
    for version in [V1_FORMAT_VERSION, V2_FORMAT_VERSION] {
        let value = serde_json::to_value(fixture_header(version)).unwrap();
        assert!(value.get("generation").is_none(), "v{version}");
    }
    let value = serde_json::to_value(fixture_header(V3_FORMAT_VERSION)).unwrap();
    assert_eq!(value["generation"], 12);
}

#[test]
fn missing_v2_field_kind_defensively_defaults_to_concealed() {
    let state = VaultState::deserialize_for_version(
        V2_FORMAT_VERSION,
        br#"{
            "secrets": {
                "Production/RESTIC_PASSWORD": {
                    "value_b64": "c2VjcmV0",
                    "value_len": 6,
                    "created_at_ms": 1,
                    "updated_at_ms": 1
                }
            }
        }"#,
    )
    .unwrap();

    assert_eq!(
        state.secrets["Production/RESTIC_PASSWORD"].kind,
        FieldKind::Concealed
    );
    assert!(state.v3.is_none());
}

fn v3_state() -> VaultState {
    VaultState {
        secrets: Default::default(),
        v3: Some(V3StateFields {
            audit_root: AuditRoot::from_legacy_audit_key(&[9_u8; 32]),
            generation: 4,
            mutation_audit_mac: FIXTURE_MAC.into(),
        }),
    }
}

#[test]
fn v3_state_round_trips_and_legacy_serializers_never_carry_its_fields() {
    let state = v3_state();
    let bytes = state.serialize_for_version(V3_FORMAT_VERSION).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["generation"], 4);
    assert_eq!(value["mutation_audit_mac"], FIXTURE_MAC);
    assert_eq!(
        value["audit_root_b64"],
        "CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk="
    );

    let decoded = VaultState::deserialize_for_version(V3_FORMAT_VERSION, &bytes).unwrap();
    let fields = decoded.v3.as_ref().unwrap();
    assert_eq!(fields.audit_root.as_bytes(), &[9_u8; 32]);
    assert_eq!(fields.generation, 4);
    assert_eq!(fields.mutation_audit_mac, FIXTURE_MAC);

    for version in [V1_FORMAT_VERSION, V2_FORMAT_VERSION] {
        assert!(state.serialize_for_version(version).is_err(), "v{version}");
        let legacy = VaultState::default();
        let bytes = legacy.serialize_for_version(version).unwrap();
        assert_eq!(bytes, br#"{"secrets":{}}"#, "v{version}");
    }
    assert!(
        VaultState::default()
            .serialize_for_version(V3_FORMAT_VERSION)
            .is_err()
    );
}

#[test]
fn v3_state_security_fields_are_required_and_validated() {
    let valid: serde_json::Value =
        serde_json::from_slice(&v3_state().serialize_for_version(V3_FORMAT_VERSION).unwrap())
            .unwrap();
    let decode = |value: &serde_json::Value| {
        VaultState::deserialize_for_version(V3_FORMAT_VERSION, &serde_json::to_vec(value).unwrap())
            .map(drop)
    };
    decode(&valid).unwrap();

    for field in [
        "audit_root_b64",
        "generation",
        "mutation_audit_mac",
        "secrets",
    ] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(decode(&missing).is_err(), "missing {field} must fail");
    }
    let invalid_values = [
        ("audit_root_b64", serde_json::json!("c2hvcnQ=")),
        ("audit_root_b64", serde_json::json!("not base64!")),
        ("generation", serde_json::json!(0)),
        ("generation", serde_json::json!(-1)),
        ("mutation_audit_mac", serde_json::json!("ABCDEF")),
        (
            "mutation_audit_mac",
            serde_json::json!(FIXTURE_MAC.to_uppercase()),
        ),
        ("mutation_audit_mac", serde_json::json!(null)),
    ];
    for (field, invalid) in invalid_values {
        let mut value = valid.clone();
        value[field] = invalid.clone();
        assert!(decode(&value).is_err(), "{field}={invalid} must fail");
    }
    let mut unknown = valid;
    unknown["unexpected"] = serde_json::json!(true);
    assert!(decode(&unknown).is_err());
}

#[test]
fn audit_root_debug_output_is_redacted() {
    let state = v3_state();
    let debug = format!("{state:?}");
    assert!(debug.contains("AuditRoot([REDACTED])"));
    assert!(!debug.contains("CQkJ"));
    assert!(!debug.contains("[9, 9"));
}
