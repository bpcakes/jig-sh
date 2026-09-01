use super::*;

pub(super) fn assert_initialized(output: &Output) -> PathBuf {
    let payload = structured_output("vault initialization", output);
    assert_eq!(payload["vault_scope"], "repo");
    assert_eq!(payload["vault_scope_id"], "scope_vault_consumer_acceptance");
    PathBuf::from(
        payload["vault_home"]
            .as_str()
            .expect("init omitted vault home"),
    )
}

pub(super) fn assert_imported(output: &Output, generated_env: &Path, op_log: &Path) {
    let payload = structured_output("vault import", output);
    assert_eq!(payload["dry_run"], false);
    assert_eq!(payload["fields"][0]["kind"], "concealed");
    assert_eq!(payload["fields"][1]["kind"], "text");
    assert_eq!(payload["fields"][2]["kind"], "text");
    assert_eq!(
        std::fs::read(generated_env).unwrap(),
        b"RESTIC_PASSWORD=jig://Production/RESTIC_PASSWORD\nRESTIC_REPOSITORY=jig://Production/RESTIC_REPOSITORY\nRESTIC_COMPRESSION=jig://Production/RESTIC_COMPRESSION\n"
    );
    assert_eq!(
        std::fs::read_to_string(op_log).unwrap(),
        format!("arg=<read>\narg=<--no-newline>\narg=<{OP_REFERENCE}>\n")
    );
}

pub(super) fn assert_source_exec(output: &Output) {
    assert_success("source vault exec", output);
    assert_no_sensitive_bytes(&output.stdout, "source exec stdout");
    assert_no_sensitive_bytes(&output.stderr, "source exec stderr");
    assert_eq!(
        output.stdout,
        format!(
            "password=[REDACTED] repository={REPOSITORY_VALUE} compression={COMPRESSION_VALUE}\n"
        )
        .as_bytes(),
        "source exec output did not preserve text and redact concealed data"
    );
    assert_eq!(
        output.stderr, b"error-password=[REDACTED]\n",
        "source exec stderr was not independently redacted"
    );
}

pub(super) fn assert_injected(output: &Output, rendered_config: &Path) {
    assert_success("vault injection", output);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(
        std::fs::read(rendered_config).unwrap(),
        format!(
            "repository={REPOSITORY_VALUE}\ncompression={COMPRESSION_VALUE}\npassword={CONCEALED_VALUE}\n"
        )
        .as_bytes(),
        "injected fixture did not contain the exact field values"
    );
    assert_eq!(
        std::fs::metadata(rendered_config)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

pub(super) fn assert_read_pipe(read: &Output, pipe: &Output) {
    assert_success("vault read", read);
    assert!(read.stderr.is_empty());
    assert_success("vault read pipe", pipe);
    assert_eq!(pipe.stdout, b"read-pipe-ok");
    assert!(pipe.stderr.is_empty());
}

pub(super) fn assert_backup_created(output: &Output, backup: &Path, destination_base: &Path) {
    let payload = structured_output("vault backup creation", output);
    assert_eq!(payload["backup_version"], 1);
    assert!(payload["bytes_written"].as_u64().unwrap() > 0);
    assert_no_encrypted_payload_plaintext(&std::fs::read(backup).unwrap(), "encrypted backup");
    assert!(!destination_base.exists());
}

pub(super) fn assert_restored(
    output: &Output,
    destination_base: &Path,
    source_home: &Path,
) -> PathBuf {
    let payload = structured_output("vault restore", output);
    assert_eq!(payload["restored"], true);
    assert_eq!(payload["format_version"], 2);
    assert_eq!(payload["vault_scope"], "repo");
    let restored_home = PathBuf::from(
        payload["vault_home"]
            .as_str()
            .expect("restore omitted destination vault home"),
    );
    assert!(restored_home.starts_with(destination_base.join("scopes")));
    assert_ne!(restored_home, source_home);
    assert!(restored_home.join("vault.json").is_file());
    restored_home
}

pub(super) fn assert_restored_exec(output: &Output) {
    assert_eq!(output.status.code(), Some(23));
    assert_no_sensitive_bytes(&output.stdout, "restored exec stdout");
    assert_no_sensitive_bytes(&output.stderr, "restored exec stderr");
    assert_eq!(
        output.stdout,
        format!(
            "restored-password=[REDACTED] repository={REPOSITORY_VALUE} compression={COMPRESSION_VALUE}\n"
        )
        .as_bytes(),
        "restored exec output did not preserve text and redact concealed data"
    );
    assert_eq!(
        output.stderr, b"restored-error=[REDACTED]\n",
        "restored exec emitted a second Jig error or unredacted bytes"
    );
}

pub(super) fn assert_persisted_state_hides_values(
    repo: &Path,
    source_home: &Path,
    restored_home: &Path,
) {
    assert_audit_has_no_values(&source_home.join("audit.jsonl"));
    assert_audit_has_no_values(&restored_home.join("audit.jsonl"));
    assert_no_encrypted_payload_plaintext(
        &std::fs::read(source_home.join("vault.json")).unwrap(),
        "source encrypted vault",
    );
    assert_no_encrypted_payload_plaintext(
        &std::fs::read(restored_home.join("vault.json")).unwrap(),
        "restored encrypted vault",
    );
    assert!(!repo.join(".agent/state/receipts.jsonl").exists());
}

pub(super) fn assert_git_hides_values(repo: &Path, diff: &Output) {
    assert_success("synthetic Git diff", diff);
    assert_no_sensitive_bytes(&diff.stdout, "synthetic Git diff");
    for value in [REPOSITORY_VALUE, COMPRESSION_VALUE] {
        assert!(!contains_bytes(&diff.stdout, value.as_bytes()));
    }
    assert_repo_has_no_imported_values(repo);
}
