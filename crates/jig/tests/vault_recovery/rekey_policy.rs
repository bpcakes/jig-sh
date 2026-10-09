use super::*;
use jig_vault::test_support::with_passphrase_estimate_for_test;

const RECORDED: &str = "passwordpasswordpassword";

fn change(home: &Path, new: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["--json", "vault", "passphrase", "change", "--home"])
        .arg(home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .env("JIG_VAULT_NEW_PASSPHRASE", new)
        .env_remove("JIG_VAULT_WITNESS_ROOT")
        .output()
        .unwrap()
}

#[test]
fn cli_rekey_retry_accepts_a_recorded_credential_that_now_fails_policy() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&passphrase()).unwrap();
    let recorded = SecretString::from(RECORDED.to_owned());
    arm_transaction_fault(TransactionFaultPoint::AfterPending);
    with_passphrase_estimate_for_test(u64::MAX, || {
        assert!(vault.change_passphrase(&passphrase(), &recorded).is_err());
    });
    assert!(jig_vault::validate_new_vault_passphrase(&recorded).is_err());
    let output = json(&change(&home, RECORDED));
    assert_eq!(output["changed"], true);
    assert!(
        !Vault::status(Some(home.clone()))
            .unwrap()
            .pending_transaction
    );
    vault.snapshot(&recorded).unwrap();
    assert!(vault.snapshot(&passphrase()).is_err());
    let events = std::fs::read_to_string(home.join("audit.jsonl")).unwrap();
    assert_eq!(
        events
            .lines()
            .filter(|line| line.contains("\"action\":\"passphrase_change\""))
            .count(),
        1
    );
}

#[test]
fn cli_rejects_a_new_weak_credential_with_operator_guidance_and_no_mutation() {
    let temp = private_tempdir();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    vault.init(&passphrase()).unwrap();
    let before_vault = std::fs::read(home.join("vault.json")).unwrap();
    let before_audit = std::fs::read(home.join("audit.jsonl")).unwrap();
    let error = failure(&change(&home, RECORDED));
    assert!(error.contains(jig_vault::NEW_VAULT_PASSPHRASE_POLICY));
    assert!(error.contains("This step needs the operator"));
    assert!(!error.contains(RECORDED));
    assert_eq!(
        std::fs::read(home.join("vault.json")).unwrap(),
        before_vault
    );
    assert_eq!(
        std::fs::read(home.join("audit.jsonl")).unwrap(),
        before_audit
    );
    vault.snapshot(&passphrase()).unwrap();
}
