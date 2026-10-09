use super::*;
use jig_vault::test_support::with_passphrase_estimate_for_test;

const RECORDED: &str = "passwordpasswordpassword";

fn init(home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["--json", "vault", "init", "--home"])
        .arg(home)
        .env("JIG_VAULT_PASSPHRASE", RECORDED)
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .env_remove("JIG_VAULT_WITNESS_ROOT")
        .output()
        .unwrap()
}

#[test]
fn init_retry_accepts_a_recorded_credential_that_now_fails_policy() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let recorded = SecretString::from(RECORDED.to_owned());
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    with_passphrase_estimate_for_test(u64::MAX, || {
        assert!(vault.init(&recorded).is_err());
    });
    assert!(jig_vault::validate_new_vault_passphrase(&recorded).is_err());
    assert_eq!(json(&jig(&["status"], &home))["pending_transaction"], true);

    json(&init(&home));
    assert_eq!(json(&jig(&["status"], &home))["pending_transaction"], false);
    vault.snapshot(&recorded).unwrap();
    let events = std::fs::read_to_string(home.join("audit.jsonl")).unwrap();
    assert_eq!(
        events
            .lines()
            .filter(|line| line.contains("\"action\":\"vault_initialized\""))
            .count(),
        1
    );
}

#[test]
fn fresh_init_rejects_the_same_weak_candidate_without_state_or_value_disclosure() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let error = failure(&init(&home));
    assert!(error.contains(jig_vault::NEW_VAULT_PASSPHRASE_POLICY));
    assert!(error.contains("This step needs the operator"));
    assert!(!error.contains(RECORDED));
    assert!(!home.join("vault.json").exists());
    assert!(!home.join("audit.jsonl").exists());
    assert_eq!(json(&jig(&["status"], &home))["pending_transaction"], false);
}
