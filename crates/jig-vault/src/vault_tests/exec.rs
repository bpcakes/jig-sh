use super::*;

#[test]
fn exec_preparation_resolves_fields_and_builds_concealed_only_redaction() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let concealed = VaultReference::parse("jig://Production/TOKEN").unwrap();
    let text = VaultReference::parse("jig://Production/FEATURE_FLAG").unwrap();
    vault
        .apply_field_batch(
            &passphrase(),
            vec![
                FieldMutation::set(
                    concealed.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"secret-value".to_vec()),
                ),
                FieldMutation::set(
                    text.clone(),
                    FieldKind::Text,
                    SecretBytes::new(b"false".to_vec()),
                ),
            ],
        )
        .unwrap();
    let request = VaultExec::new(
        vec![
            "argv-secret-sentinel".into(),
            "argument-value-sentinel".into(),
        ],
        vec![
            ExecEnvBinding::literal(
                exec_var("LITERAL"),
                SecretBytes::new(b"literal-value-sentinel".to_vec()),
            )
            .unwrap(),
            ExecEnvBinding::field(exec_var("TOKEN"), concealed),
            ExecEnvBinding::field(exec_var("FEATURE_FLAG"), text),
        ],
    )
    .unwrap();

    let prepared = vault.store.prepare_exec(&passphrase(), request).unwrap();
    assert_prepared_bindings(&prepared);
    assert_prepared_redaction(&prepared);
    let operation_id = assert_exec_start_audit(&vault, &prepared);

    prepared.record_finish(0, None).unwrap();
    assert_exec_finish_audit(&vault, &operation_id);
}

fn assert_prepared_bindings(prepared: &PreparedExec) {
    assert_eq!(prepared.command.len(), 2);
    assert_eq!(prepared.env.len(), 3);
    assert_eq!(prepared.env[0].field_kind, None);
    assert_eq!(prepared.env[0].value.as_str(), "literal-value-sentinel");
    assert_eq!(prepared.env[1].field_kind, Some(FieldKind::Concealed));
    assert_eq!(prepared.env[1].value.as_str(), "secret-value");
    assert_eq!(prepared.env[2].field_kind, Some(FieldKind::Text));
    assert_eq!(prepared.env[2].value.as_str(), "false");
}

fn assert_prepared_redaction(prepared: &PreparedExec) {
    let mut redactor = prepared.redactor.independent_stream();
    let mut output = Vec::new();
    redactor
        .push_chunk(
            b"raw=secret-value b64=c2VjcmV0LXZhbHVl text=false literal=literal-value-sentinel",
            &mut output,
        )
        .unwrap();
    redactor.finish(&mut output).unwrap();
    assert_eq!(
        output,
        b"raw=[REDACTED] b64=[REDACTED] text=false literal=literal-value-sentinel"
    );
}

fn assert_exec_start_audit(vault: &Vault, prepared: &PreparedExec) -> String {
    let events = audit_events(&vault.store);
    let start = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(start.details["operation_id"], prepared.operation_id);
    assert_eq!(start.details["argument_count"], 2);
    assert_eq!(start.details["binding_count"], 3);
    assert_eq!(start.details["literal_binding_count"], 1);
    assert_eq!(start.details["field_binding_count"], 2);
    assert_eq!(start.details["field_bindings"][0]["var"], "TOKEN");
    assert_eq!(
        start.details["field_bindings"][0]["reference"],
        "jig://Production/TOKEN"
    );
    assert_exec_audit_has_no_values(vault);
    prepared.operation_id.clone()
}

fn assert_exec_audit_has_no_values(vault: &Vault) {
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    for forbidden in [
        "argv-secret-sentinel",
        "argument-value-sentinel",
        "literal-value-sentinel",
        "secret-value",
        "c2VjcmV0LXZhbHVl",
    ] {
        assert!(!audit.contains(forbidden), "audit leaked {forbidden}");
    }
}

fn assert_exec_finish_audit(vault: &Vault, operation_id: &str) {
    let events = audit_events(&vault.store);
    let finish = events.last().unwrap();
    assert_eq!(finish.action, "exec_finish");
    assert_eq!(finish.details["operation_id"], operation_id);
    assert_eq!(finish.details["exit_status"], 0);
    assert!(finish.details["exit_signal"].is_null());
}

#[test]
fn exec_preparation_missing_field_records_value_free_failed_event() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let request = VaultExec::new(
        vec!["missing-field-command-sentinel".into()],
        vec![ExecEnvBinding::field(
            exec_var("TOKEN"),
            VaultReference::parse("jig://Production/MISSING").unwrap(),
        )],
    )
    .unwrap();

    let error = vault
        .store
        .prepare_exec(&passphrase(), request)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::NotFound);
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(failed.action, "exec_failed");
    assert_eq!(failed.details["stage"], "resolve");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
    assert_eq!(failed.details["error"], "vault exec failed");
    assert!(
        !vault
            .store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains("missing-field-command-sentinel")
    );
}

#[test]
fn exec_preparation_invalid_field_bytes_record_resolve_failure() {
    for (field, value, requirement) in [
        ("BINARY", vec![b's', b'e', b'c', 0xff], "UTF-8"),
        ("NUL", b"sec\0ret".to_vec(), "NUL"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        vault.init(&passphrase()).unwrap();
        let reference = VaultReference::parse(&format!("jig://Production/{field}")).unwrap();
        vault
            .set_field(
                &passphrase(),
                reference.clone(),
                FieldKind::Concealed,
                SecretBytes::new(value),
            )
            .unwrap();
        let request = VaultExec::new(
            vec!["command".into()],
            vec![ExecEnvBinding::field(exec_var("VALUE"), reference)],
        )
        .unwrap();

        let error = vault
            .store
            .prepare_exec(&passphrase(), request)
            .unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert!(error.to_string().contains(requirement));
        let events = audit_events(&vault.store);
        let start = &events[events.len() - 2];
        let failed = events.last().unwrap();
        assert_eq!(start.action, "exec_start");
        assert_eq!(failed.action, "exec_failed");
        assert_eq!(failed.details["stage"], "resolve");
        assert_eq!(
            start.details["operation_id"],
            failed.details["operation_id"]
        );
    }
}

#[test]
fn exec_preparation_redaction_bound_records_redaction_failure() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/LARGE_TOKEN").unwrap();
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(vec![b'x'; crate::exec::MAX_EXEC_CONCEALED_VALUE_LEN + 1]),
        )
        .unwrap();
    let request = VaultExec::new(
        vec!["command".into()],
        vec![ExecEnvBinding::field(exec_var("TOKEN"), reference)],
    )
    .unwrap();

    let error = vault
        .store
        .prepare_exec(&passphrase(), request)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(failed.action, "exec_failed");
    assert_eq!(failed.details["stage"], "redaction");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
}

#[test]
fn exec_spawn_failure_records_value_free_terminal_event() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let command_sentinel = "jig-vault-missing-command-secret-sentinel";
    let request = VaultExec::new(vec![command_sentinel.into()], Vec::new()).unwrap();

    let error = vault.exec(&passphrase(), request).unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Process);
    assert!(!error.to_string().contains(command_sentinel));
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = events.last().unwrap();
    assert_eq!(start.action, "exec_start");
    assert_eq!(failed.action, "exec_failed");
    assert_eq!(failed.details["stage"], "spawn");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
    assert!(
        !vault
            .store
            .read_audit_text()
            .unwrap()
            .unwrap()
            .contains(command_sentinel)
    );
}

#[cfg(unix)]
#[test]
fn brokered_run_resolves_canonical_references_in_version_two() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    vault
        .apply_field_batch(
            &passphrase(),
            vec![
                FieldMutation::set(
                    VaultReference::parse("jig://Production/TOKEN").unwrap(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"canonical-token-value".to_vec()),
                ),
                FieldMutation::set(
                    VaultReference::parse("jig://Production/FLAG").unwrap(),
                    FieldKind::Text,
                    SecretBytes::new(b"text-flag-value".to_vec()),
                ),
            ],
        )
        .unwrap();
    let request = BrokeredRun::new(
        vec![
            "sh".into(),
            "-c".into(),
            "test \"$TOKEN\" = canonical-token-value && test \"$LEGACY\" = \"$TOKEN\" && test \"$FLAG\" = text-flag-value && printf '%s %s %s' \"$TOKEN\" \"$FLAG\" \"$LEGACY\"".into(),
        ],
        vec![
            BrokeredEnv::parse("TOKEN=jig://Production/TOKEN").unwrap(),
            BrokeredEnv::parse("FLAG=jig://Production/FLAG").unwrap(),
            BrokeredEnv::parse("LEGACY=Production/TOKEN").unwrap(),
        ],
    )
    .unwrap();

    let output = vault.run_brokered(&passphrase(), request).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    // The compatible broker redacts every injected value of at least 4 bytes,
    // including text fields, unlike exec's concealed-only redaction.
    assert_eq!(output.stdout, "[REDACTED] [REDACTED] [REDACTED]");
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(audit.contains("\"secret_name\":\"Production/TOKEN\""));
    assert!(!audit.contains("canonical-token-value"));
    assert!(!audit.contains("text-flag-value"));
}

#[cfg(unix)]
#[test]
fn brokered_run_reports_missing_canonical_reference_in_reference_form() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let missing = |mapping: &str| {
        let request = BrokeredRun::new(
            vec!["true".into()],
            vec![BrokeredEnv::parse(mapping).unwrap()],
        )
        .unwrap();
        vault.run_brokered(&passphrase(), request).unwrap_err()
    };

    let canonical = missing("TOKEN=jig://Production/MISSING");
    assert_eq!(canonical.kind(), VaultErrorKind::NotFound);
    assert_eq!(
        canonical.to_string(),
        "vault field 'jig://Production/MISSING' does not exist"
    );

    let legacy = missing("TOKEN=Production/MISSING");
    assert_eq!(legacy.kind(), VaultErrorKind::NotFound);
    assert_eq!(
        legacy.to_string(),
        "vault secret 'Production/MISSING' does not exist"
    );
}

#[test]
fn failed_exec_and_retained_audit_refusal_keep_integrity_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("ExampleVault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let missing = temp
        .path()
        .join("ExampleMissingExecutable")
        .to_string_lossy()
        .into_owned();
    let request = VaultExec::new(vec![missing], vec![]).unwrap();
    let prepared = vault.store.prepare_exec(&passphrase(), request).unwrap();
    let envelope = std::fs::read(vault.store.vault_path()).unwrap();
    std::fs::write(vault.store.audit_path(), b"").unwrap();
    let error = prepared.execute().unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Process);
    assert_eq!(error.recovery(), Some(crate::VaultRecovery::Integrity));
    assert!(error.message().contains("not anchored"));
    assert_eq!(std::fs::read(vault.store.vault_path()).unwrap(), envelope);
    assert!(std::fs::read(vault.store.audit_path()).unwrap().is_empty());
}
