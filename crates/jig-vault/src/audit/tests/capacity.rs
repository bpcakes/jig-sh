use super::*;

#[test]
fn audit_only_appends_admit_only_readable_successors() {
    // Exercise a normal prefix, a complete last line without a newline,
    // and a discarded torn suffix. Small per-store caps use the real read,
    // preparation and append paths without allocating 256 MiB per case.
    for suffix in ["\n", "", "\n{\"partial\""] {
        let temp = tempfile::tempdir().unwrap();
        let mut store =
            VaultStore::resolve_for_test(Some(temp.path().join("ExampleVault"))).unwrap();
        let key = [7_u8; 32];
        AuditEvent::append(
            &store,
            &retained(key),
            AuditAction::SecretSet,
            serde_json::json!({}),
        )
        .unwrap();
        let text = store.read_audit_text().unwrap().unwrap();
        let before = format!("{}{suffix}", text.trim_end_matches('\n'));
        std::fs::write(store.audit_path(), &before).unwrap();
        let prepared = store
            .with_lock(|| {
                AuditEvent::prepare_append_unlocked(
                    &store,
                    &key,
                    AuditAction::SecretRemove,
                    serde_json::json!({}),
                )
            })
            .unwrap();
        let transition = prepared.transition();
        let limit = transition.prefix_len + transition.append.len() as u64;

        store.set_audit_text_read_limit_for_test(limit - 1);
        let error = AuditEvent::append(
            &store,
            &retained(key),
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("audit read limit"), "{error}");
        assert_eq!(std::fs::read_to_string(store.audit_path()).unwrap(), before);

        store.set_audit_text_read_limit_for_test(limit);
        AuditEvent::append(
            &store,
            &retained(key),
            AuditAction::SecretRemove,
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(store.audit_len().unwrap(), Some(limit));
        let verified = AuditEvent::verify_chain(&store, &key).unwrap();
        assert_eq!(verified.event_count, 2);
        assert_eq!(verified.torn_tail_bytes, 0);
    }
}
