use secrecy::SecretString;
use tempfile::tempdir;

use crate::test_env::{EnvVarGuard, lock_env};

use super::*;

#[test]
fn parses_env_mappings() {
    let parsed = parse_env_mappings(&["TOKEN=api_token".into()]).unwrap();
    assert_eq!(parsed[0].var().as_str(), "TOKEN");
    assert_eq!(parsed[0].secret_name().as_str(), "api_token");
}

#[test]
fn rejects_invalid_env_mapping_shape() {
    let error = parse_env_mappings(&["TOKEN".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("VAR=SECRET_NAME"));
}

#[test]
fn rejects_invalid_env_mapping_secret_name_before_unlock() {
    let error = parse_env_mappings(&["TOKEN=bad secret".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported characters"));
}

#[cfg(unix)]
#[test]
fn parses_file_mappings() {
    let parsed = parse_file_mappings(&["TOKEN_FILE=api_token".into()]).unwrap();
    assert_eq!(parsed[0].var().as_str(), "TOKEN_FILE");
    assert_eq!(parsed[0].secret_name().as_str(), "api_token");
}

#[cfg(not(unix))]
#[test]
fn rejects_file_mappings_on_non_unix() {
    let error = parse_file_mappings(&["TOKEN_FILE=api_token".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("requires Unix-style owner-only temporary files"));
}

#[cfg(unix)]
#[test]
fn rejects_invalid_file_mapping_shape() {
    let error = parse_file_mappings(&["TOKEN_FILE".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("VAR=SECRET_NAME"));
}

#[test]
fn read_secret_value_rejects_oversized_input() {
    let value = vec![b'x'; MAX_SECRET_VALUE_LEN + 1];
    let error = read_secret_value(std::io::Cursor::new(value))
        .unwrap_err()
        .to_string();
    assert!(error.contains("larger than"));
}

#[test]
fn status_does_not_require_passphrase() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("vault");
    let output = status(VaultStatusRequest {
        vault: VaultRuntimeOptions {
            home: Some(home.clone()),
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(output["exists"], false);
    assert_eq!(output["vault_file_exists"], false);
    assert_eq!(output["format_version"], Value::Null);
    assert!(!home.exists());
}

#[test]
fn status_reports_existing_vault() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault
        .init(&SecretString::from(
            "correct horse battery staple".to_string(),
        ))
        .unwrap();
    let output = status(VaultStatusRequest {
        vault: VaultRuntimeOptions {
            home: Some(home),
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(output["exists"], true);
    assert_eq!(output["vault_file_exists"], true);
    assert_eq!(output["format_version"], 3);
}

#[test]
fn field_list_reports_only_metadata_and_filters_by_item() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let home = temp.path().join("vault");
    let passphrase = "correct horse battery staple";
    let vault = Vault::resolve(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(passphrase.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/RESTIC_COMPRESSION".parse().unwrap(),
            FieldKind::Text,
            SecretBytes::new(b"false".to_vec()),
        )
        .unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Staging/ARRAY_APP_KEY".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(b"test-key".to_vec()),
        )
        .unwrap();

    set_captured_passphrase(SecretString::from(
        "correct horse battery staple".to_owned(),
    ))
    .unwrap();
    let output = list_fields(VaultFieldListRequest {
        item: Some("jig://Production".parse().unwrap()),
        vault: VaultRuntimeOptions {
            home: Some(home),
            ..Default::default()
        },
    })
    .unwrap();

    assert_eq!(output["command"], "vault field list");
    assert_eq!(output["item"], "jig://Production");
    assert_eq!(output["fields"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["fields"][0]["reference"],
        "jig://Production/RESTIC_COMPRESSION"
    );
    assert_eq!(output["fields"][0]["kind"], "text");
    assert_eq!(output["fields"][0]["value_len"], 5);
    assert!(output["fields"][0].get("value").is_none());
}

#[test]
fn field_remove_reports_whether_a_field_existed() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let home = temp.path().join("vault");
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    let vault = Vault::resolve(Some(home.clone())).unwrap();
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/RESTIC_PASSWORD".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(b"test-password".to_vec()),
        )
        .unwrap();

    set_captured_passphrase(SecretString::from(
        "correct horse battery staple".to_owned(),
    ))
    .unwrap();
    let output = remove_field(VaultFieldRemoveRequest {
        reference: "jig://Production/RESTIC_PASSWORD".parse().unwrap(),
        vault: VaultRuntimeOptions {
            home: Some(home.clone()),
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(output["removed"], true);

    set_captured_passphrase(SecretString::from(
        "correct horse battery staple".to_owned(),
    ))
    .unwrap();
    let output = remove_field(VaultFieldRemoveRequest {
        reference: "jig://Production/RESTIC_PASSWORD".parse().unwrap(),
        vault: VaultRuntimeOptions {
            home: Some(home),
            ..Default::default()
        },
    })
    .unwrap();
    assert_eq!(output["removed"], false);
}

#[test]
fn migrate_reports_an_unchanged_current_vault_and_refuses_downgrade() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let home = temp.path().join("vault");
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    let vault = Vault::resolve(Some(home.clone())).unwrap();
    vault.init(&passphrase).unwrap();

    set_captured_passphrase(SecretString::from(
        "correct horse battery staple".to_owned(),
    ))
    .unwrap();
    let output = migrate(VaultMigrateRequest {
        target_version: 3,
        vault: VaultRuntimeOptions {
            home: Some(home.clone()),
            ..Default::default()
        },
    })
    .unwrap();

    assert_eq!(output["command"], "vault migrate");
    assert_eq!(output["from_version"], 3);
    assert_eq!(output["to_version"], 3);
    assert_eq!(output["changed"], false);

    set_captured_passphrase(SecretString::from(
        "correct horse battery staple".to_owned(),
    ))
    .unwrap();
    let error = migrate(VaultMigrateRequest {
        target_version: 2,
        vault: VaultRuntimeOptions {
            home: Some(home),
            ..Default::default()
        },
    })
    .unwrap_err();
    assert!(error.to_string().contains("downgrades are not supported"));
}

#[test]
fn repo_scope_resolves_under_vault_base_home() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap().join("vault-base");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let _home = EnvVarGuard::set(VAULT_HOME_ENV, temp.path().join("vault-base"));

    let output = status(VaultStatusRequest {
        vault: VaultRuntimeOptions::repo("scope_123", "demo", &repo),
    })
    .unwrap();

    assert_eq!(output["vault_scope"], "repo");
    assert_eq!(output["vault_scope_id"], "scope_123");
    assert_eq!(output["vault_repo_name"], "demo");
    let vault_home = output["vault_home"].as_str().unwrap();
    assert!(vault_home.starts_with(&base.join("scopes/repo-").display().to_string()));
    assert!(!vault_home.ends_with("scope_123"));
    assert!(!base.exists());
}

#[test]
fn legacy_repo_scope_vault_blocks_trusted_namespace_cutover_until_migrated() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let base = temp.path().join("vault-base");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let _home = EnvVarGuard::set(VAULT_HOME_ENV, &base);
    let legacy_home = base.join("scopes").join("legacy_scope");
    let legacy_vault = Vault::resolve_for_test(Some(legacy_home.clone())).unwrap();
    legacy_vault
        .init(&SecretString::from(
            "correct horse battery staple".to_string(),
        ))
        .unwrap();

    let error = status(VaultStatusRequest {
        vault: VaultRuntimeOptions::repo("legacy_scope", "demo", &repo),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("legacy repo-scoped vault data exists"));
    assert!(error.contains("trusted repo-local vault namespace"));
    assert!(error.contains(&legacy_home.display().to_string()));
}

#[test]
fn copied_scope_id_does_not_reuse_another_repo_physical_vault_home() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let base = temp.path().join("vault-base");
    let repo_a = temp.path().join("repo-a");
    let repo_b = temp.path().join("repo-b");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();
    let _home = EnvVarGuard::set(VAULT_HOME_ENV, &base);

    let first = status(VaultStatusRequest {
        vault: VaultRuntimeOptions::repo("copied_scope", "demo", &repo_a),
    })
    .unwrap();
    let second = status(VaultStatusRequest {
        vault: VaultRuntimeOptions::repo("copied_scope", "demo", &repo_b),
    })
    .unwrap();

    assert_ne!(first["vault_home"], second["vault_home"]);
    assert_eq!(first["vault_scope_id"], "copied_scope");
    assert_eq!(second["vault_scope_id"], "copied_scope");
}
