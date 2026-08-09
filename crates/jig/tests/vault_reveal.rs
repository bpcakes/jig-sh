use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jig_vault::{FieldKind, FieldMutation, SecretBytes, Vault, VaultReference};
use secrecy::SecretString;

const PASSPHRASE: &str = "correct horse battery staple";
const SECRET: &[u8] = b"super-private-value";

fn jig(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(args)
        .arg("--home")
        .arg(home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .output()
        .unwrap()
}

fn reference(value: &str) -> VaultReference {
    value.parse().unwrap()
}

fn initialized_vault() -> (tempfile::TempDir, PathBuf, Vault) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault-home");
    let vault = Vault::resolve(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .apply_field_batch(
            &passphrase,
            vec![
                FieldMutation::set(
                    reference("jig://Production/SECRET"),
                    FieldKind::Concealed,
                    SecretBytes::new(SECRET.to_vec()),
                ),
                FieldMutation::set(
                    reference("jig://Production/FLAG"),
                    FieldKind::Text,
                    SecretBytes::new(b"false".to_vec()),
                ),
                FieldMutation::set(
                    reference("jig://Production/BINARY"),
                    FieldKind::Concealed,
                    SecretBytes::new(vec![0, 0xff, b'X', b'\n', b'Z']),
                ),
            ],
        )
        .unwrap();
    (temp, home, vault)
}

#[test]
fn read_and_inject_use_exact_raw_bytes_without_json_or_newlines() {
    let (temp, home, vault) = initialized_vault();

    let read = jig(&home, &["vault", "read", "jig://Production/BINARY"]);
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert_eq!(read.stdout, [0, 0xff, b'X', b'\n', b'Z']);
    assert!(read.stderr.is_empty());

    let template_path = temp.path().join("config.template");
    let template = b"\xffsecret={{ jig://Production/SECRET }}\nflag={{jig://Production/FLAG}}\nbinary={{ jig://Production/BINARY }};again={{ jig://Production/SECRET }}";
    std::fs::write(&template_path, template).unwrap();
    let injected = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "inject", "--in"])
        .arg(&template_path)
        .arg("--home")
        .arg(&home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .output()
        .unwrap();
    assert!(
        injected.status.success(),
        "{}",
        String::from_utf8_lossy(&injected.stderr)
    );
    let mut expected = b"\xffsecret=".to_vec();
    expected.extend_from_slice(SECRET);
    expected.extend_from_slice(b"\nflag=false\nbinary=");
    expected.extend_from_slice(&[0, 0xff, b'X', b'\n', b'Z']);
    expected.extend_from_slice(b";again=");
    expected.extend_from_slice(SECRET);
    assert_eq!(injected.stdout, expected);
    assert!(injected.stderr.is_empty());

    let audit_path = vault.root().join("audit.jsonl");
    let audit_before = std::fs::read(&audit_path).unwrap();
    let rejected_json = jig(
        &home,
        &["--json", "vault", "read", "jig://Production/SECRET"],
    );
    assert!(!rejected_json.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&rejected_json.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--json is not supported")
    );
    assert!(
        !rejected_json
            .stdout
            .windows(SECRET.len())
            .any(|part| part == SECRET)
    );
    assert!(
        !rejected_json
            .stderr
            .windows(SECRET.len())
            .any(|part| part == SECRET)
    );
    assert_eq!(std::fs::read(audit_path).unwrap(), audit_before);
}

#[test]
fn raw_file_output_is_private_and_obeys_no_overwrite() {
    let (temp, home, _vault) = initialized_vault();
    let destination = temp.path().join("secret-output");

    let written = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "read", "jig://Production/SECRET", "--out-file"])
        .arg(&destination)
        .arg("--home")
        .arg(&home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .output()
        .unwrap();
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    assert!(written.stdout.is_empty());
    assert!(written.stderr.is_empty());
    assert_eq!(std::fs::read(&destination).unwrap(), SECRET);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let refused = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "read", "jig://Production/FLAG", "--out-file"])
        .arg(&destination)
        .arg("--home")
        .arg(&home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert_eq!(std::fs::read(&destination).unwrap(), SECRET);
    assert!(
        !refused
            .stderr
            .windows(SECRET.len())
            .any(|part| part == SECRET)
    );

    let in_place_template = temp.path().join("in-place-template");
    std::fs::write(&in_place_template, b"value={{ jig://Production/SECRET }}").unwrap();
    let injected = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "inject", "--in"])
        .arg(&in_place_template)
        .arg("--out-file")
        .arg(&in_place_template)
        .args(["--overwrite", "--home"])
        .arg(&home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .output()
        .unwrap();
    assert!(
        injected.status.success(),
        "{}",
        String::from_utf8_lossy(&injected.stderr)
    );
    assert!(injected.stdout.is_empty());
    assert!(injected.stderr.is_empty());
    let mut expected = b"value=".to_vec();
    expected.extend_from_slice(SECRET);
    assert_eq!(std::fs::read(&in_place_template).unwrap(), expected);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::fs::metadata(in_place_template)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
