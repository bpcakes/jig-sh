use super::*;

#[test]
fn direct_field_read_writes_exact_binary_bytes_and_terminalizes_success() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/BINARY").unwrap();
    let value = b"\xff\0binary-value";
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();

    let before_count = audit_events(&vault.store).len();
    let mut output = Vec::new();
    let result = vault
        .read_field_to(&passphrase(), reference.clone(), &mut output)
        .unwrap();
    assert_eq!(output, value);
    assert_eq!(result.bytes_written, value.len());

    let events = audit_events(&vault.store);
    assert_eq!(events.len(), before_count + 2);
    let start = &events[events.len() - 2];
    let finish = &events[events.len() - 1];
    assert_eq!(start.action, "field_read_start");
    assert_eq!(finish.action, "field_read_finish");
    assert_eq!(
        start.details["operation_id"],
        finish.details["operation_id"]
    );
    assert_eq!(start.details["reference"], reference.to_string());
    assert_eq!(finish.details["sink"], "stream");
    assert_eq!(finish.details["bytes_written"], value.len());
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(!audit.contains("binary-value"));
}

#[test]
fn template_injection_resolves_all_references_deduplicates_and_preserves_bytes() {
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
                    SecretBytes::new(b"hidden-value".to_vec()),
                ),
                FieldMutation::set(
                    VaultReference::parse("jig://Production/FLAG").unwrap(),
                    FieldKind::Text,
                    SecretBytes::new(vec![0xff, 0x00, b'0']),
                ),
            ],
        )
        .unwrap();

    let template = InjectionTemplate::parse(SecretBytes::new(
        b"A={{ jig://Production/TOKEN }}\0B={{jig://Production/FLAG}} C={{ jig://Production/TOKEN }}"
            .to_vec(),
    ))
    .unwrap();
    let mut output = Vec::new();
    vault
        .inject_template_to(&passphrase(), template, &mut output)
        .unwrap();
    assert_eq!(output, b"A=hidden-value\0B=\xff\x000 C=hidden-value");

    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let finish = &events[events.len() - 1];
    assert_eq!(start.action, "template_inject_start");
    assert_eq!(start.details["reference_count"], 2);
    assert_eq!(start.details["references"].as_array().unwrap().len(), 2);
    assert_eq!(finish.action, "template_inject_finish");
    assert_eq!(
        start.details["operation_id"],
        finish.details["operation_id"]
    );
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    assert!(!audit.contains("hidden-value"));
}

#[test]
fn missing_late_template_reference_records_failure_without_output() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let present = VaultReference::parse("jig://Production/PRESENT").unwrap();
    vault
        .set_field(
            &passphrase(),
            present,
            FieldKind::Concealed,
            SecretBytes::new(b"present-value".to_vec()),
        )
        .unwrap();
    let template = InjectionTemplate::parse(SecretBytes::new(
        b"{{jig://Production/PRESENT}}/{{jig://Production/MISSING}}".to_vec(),
    ))
    .unwrap();

    let mut output = Vec::new();
    let error = vault
        .inject_template_to(&passphrase(), template, &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::NotFound);
    assert!(output.is_empty());
    assert!(!error.to_string().contains("present-value"));
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = &events[events.len() - 1];
    assert_eq!(start.action, "template_inject_start");
    assert_eq!(failed.action, "template_inject_failed");
    assert_eq!(failed.details["stage"], "resolve");
    assert_eq!(
        start.details["operation_id"],
        failed.details["operation_id"]
    );
}

struct PrefixThenFailWriter {
    output: Vec<u8>,
    remaining: usize,
}

impl Write for PrefixThenFailWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "writer-failure-sentinel",
            ));
        }
        let len = bytes.len().min(self.remaining);
        self.output.extend_from_slice(&bytes[..len]);
        self.remaining -= len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn partial_stream_failure_records_failed_terminal_event_without_value_leak() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/TOKEN").unwrap();
    let value = b"writer-failure-sentinel";
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(value.to_vec()),
        )
        .unwrap();
    let mut writer = PrefixThenFailWriter {
        output: Vec::new(),
        remaining: 3,
    };

    let error = vault
        .read_field_to(&passphrase(), reference, &mut writer)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::Io);
    assert_eq!(writer.output, &value[..3]);
    assert!(!format!("{error:#}").contains("writer-failure-sentinel"));
    let events = audit_events(&vault.store);
    let start = &events[events.len() - 2];
    let failed = &events[events.len() - 1];
    assert_eq!(start.action, "field_read_start");
    assert_eq!(failed.action, "field_read_failed");
    assert_eq!(failed.details["stage"], "sink");
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
            .contains("writer-failure-sentinel")
    );
}

#[test]
fn tampered_audit_rejects_reveal_start_before_any_value_is_prepared() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/TOKEN").unwrap();
    vault
        .set_field(
            &passphrase(),
            reference.clone(),
            FieldKind::Concealed,
            SecretBytes::new(b"audit-failure-sentinel".to_vec()),
        )
        .unwrap();
    let audit = vault.store.read_audit_text().unwrap().unwrap();
    let tampered = audit.replacen("field_batch_apply", "field_batch_tamper", 1);
    assert_ne!(tampered, audit);
    std::fs::write(vault.store.audit_path(), &tampered).unwrap();

    let mut output = Vec::new();
    let error = vault
        .read_field_to(&passphrase(), reference, &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::AuditTampered);
    assert!(output.is_empty());
    assert!(!format!("{error:#}").contains("audit-failure-sentinel"));
    assert_eq!(vault.store.read_audit_text().unwrap().unwrap(), tampered);
}

#[test]
fn static_version_one_fixture_supports_controlled_read_and_injection() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    install_cli_generated_v1_fixture(&vault.store);
    let passphrase = cli_generated_v1_fixture_passphrase();
    let reference = VaultReference::parse("jig://Production/RESTIC_PASSWORD").unwrap();

    let mut read_output = Vec::new();
    vault
        .read_field_to(&passphrase, reference, &mut read_output)
        .unwrap();
    assert_eq!(read_output, CLI_GENERATED_V1_SECRET_VALUE);

    let template = InjectionTemplate::parse(SecretBytes::new(
        b"before={{jig://Production/RESTIC_PASSWORD}}:after".to_vec(),
    ))
    .unwrap();
    let mut inject_output = Vec::new();
    vault
        .inject_template_to(&passphrase, template, &mut inject_output)
        .unwrap();
    let mut expected = b"before=".to_vec();
    expected.extend_from_slice(CLI_GENERATED_V1_SECRET_VALUE);
    expected.extend_from_slice(b":after");
    assert_eq!(inject_output, expected);
    assert_eq!(vault.verify_audit(&passphrase).unwrap().event_count, 6);
}

#[test]
fn rendered_output_bound_records_template_failure() {
    let temp = tempfile::tempdir().unwrap();
    let vault = Vault::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    vault.init(&passphrase()).unwrap();
    let reference = VaultReference::parse("jig://Production/LARGE").unwrap();
    vault
        .set_field(
            &passphrase(),
            reference,
            FieldKind::Concealed,
            SecretBytes::new(vec![b'x'; MAX_SECRET_VALUE_LEN]),
        )
        .unwrap();
    let template = InjectionTemplate::parse(SecretBytes::new(
        "{{jig://Production/LARGE}}".repeat(17).into_bytes(),
    ))
    .unwrap();

    let mut output = Vec::new();
    let error = vault
        .inject_template_to(&passphrase(), template, &mut output)
        .unwrap_err();
    assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
    assert!(output.is_empty());
    assert!(error.to_string().contains("rendered template exceeds"));
    let events = audit_events(&vault.store);
    assert_eq!(events[events.len() - 2].action, "template_inject_start");
    assert_eq!(events.last().unwrap().action, "template_inject_failed");
    assert_eq!(events.last().unwrap().details["stage"], "render");
}

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
    assert_eq!(prepared.command.len(), 2);
    assert_eq!(prepared.env.len(), 3);
    assert_eq!(prepared.env[0].field_kind, None);
    assert_eq!(prepared.env[0].value.as_str(), "literal-value-sentinel");
    assert_eq!(prepared.env[1].field_kind, Some(FieldKind::Concealed));
    assert_eq!(prepared.env[1].value.as_str(), "secret-value");
    assert_eq!(prepared.env[2].field_kind, Some(FieldKind::Text));
    assert_eq!(prepared.env[2].value.as_str(), "false");

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

    prepared.record_finish(0, None).unwrap();
    let events = audit_events(&vault.store);
    let finish = events.last().unwrap();
    assert_eq!(finish.action, "exec_finish");
    assert_eq!(
        finish.details["operation_id"],
        start.details["operation_id"]
    );
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
