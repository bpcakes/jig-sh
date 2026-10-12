#![cfg(any(target_os = "linux", target_os = "macos"))]
//! The format 3 release rehearsal through the current-source `jig` binary:
//! the frozen version 2 vault is migrated, edited, rotated, backed up, and
//! restored into an absent home, which then unlocks and verifies its audit
//! chain, while older copies of the vault are refused.
//!
//! Every home and the witness beside them are disposable, and every
//! credential is test-only. The child also gets a disposable `HOME`, so even
//! a build without test support could not reach the operator's witness.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const FIXTURE_VAULT: &str = include_str!("../../jig-vault/tests/fixtures/generated-v2/vault.json");
const FIXTURE_AUDIT: &str = include_str!("../../jig-vault/tests/fixtures/generated-v2/audit.jsonl");
/// The frozen fixture's test-only credential.
const FIXTURE_PASSPHRASE: &str = "fixture-v2-pass";
const ROTATED_PASSPHRASE: &str = "rehearsal replacement passphrase after migration";
const TOKEN: &str = "jig://ExampleProject/API_TOKEN";
const EDITED: &str = "rehearsal-edited-token";

struct Rehearsal {
    _temp: tempfile::TempDir,
    base: PathBuf,
    profile: PathBuf,
}

impl Rehearsal {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap();
        let temp = tempfile::Builder::new()
            .prefix("jig-vault-rehearsal-")
            .tempdir_in(root)
            .unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let base = temp.path().to_path_buf();
        let profile = base.join("profile");
        std::fs::create_dir(&profile).unwrap();
        Self {
            _temp: temp,
            base,
            profile,
        }
    }

    fn home(&self, name: &str, vault_json: &[u8], audit_jsonl: &[u8]) -> PathBuf {
        let home = self.base.join(name);
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        for (file, contents) in [("vault.json", vault_json), ("audit.jsonl", audit_jsonl)] {
            std::fs::write(home.join(file), contents).unwrap();
            std::fs::set_permissions(home.join(file), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        home
    }

    /// Runs `jig vault ARGS --home HOME` with the given credentials.
    fn jig(&self, args: &[&str], home: &Path, credentials: &[&str], stdin: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
        if args[0] != "read" {
            command.arg("--json");
        }
        command
            .arg("vault")
            .args(args)
            .arg("--home")
            .arg(home)
            .env("HOME", &self.profile)
            .env_remove("JIG_VAULT_HOME")
            .env_remove("JIG_VAULT_WITNESS_ROOT")
            .env_remove("JIG_VAULT_PASSPHRASE")
            .env_remove("JIG_VAULT_NEW_PASSPHRASE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in ["JIG_VAULT_PASSPHRASE", "JIG_VAULT_NEW_PASSPHRASE"]
            .into_iter()
            .zip(credentials)
        {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        child.wait_with_output().unwrap()
    }
}

fn json(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The combined value-free failure report; `--json` errors go to stdout.
fn failure(output: &Output) -> String {
    assert!(!output.status.success());
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for secret in [EDITED, FIXTURE_PASSPHRASE, ROTATED_PASSPHRASE] {
        assert!(!report.contains(secret), "{report}");
    }
    report
}

#[test]
fn a_v2_vault_is_migrated_rotated_backed_up_and_restored_through_the_cli() {
    let rehearsal = Rehearsal::new();
    let home = rehearsal.home("vault", FIXTURE_VAULT.as_bytes(), FIXTURE_AUDIT.as_bytes());
    let fixture = [FIXTURE_PASSPHRASE];
    let rotated = [ROTATED_PASSPHRASE];

    let status = json(&rehearsal.jig(&["status"], &home, &[], b""));
    assert_eq!(status["format_version"], 2);
    let migrated = json(&rehearsal.jig(&["migrate", "--to", "3"], &home, &fixture, b""));
    assert_eq!(migrated["to_version"], 3);
    assert_eq!(migrated["changed"], true);
    let edit = ["field", "set", TOKEN, "--value-stdin"];
    json(&rehearsal.jig(&edit, &home, &fixture, EDITED.as_bytes()));
    let earlier_vault = std::fs::read(home.join("vault.json")).unwrap();
    let earlier_audit = std::fs::read(home.join("audit.jsonl")).unwrap();

    let change = [FIXTURE_PASSPHRASE, ROTATED_PASSPHRASE];
    let changed = json(&rehearsal.jig(&["passphrase", "change"], &home, &change, b""));
    assert_eq!(changed["changed"], true);
    let refused = failure(&rehearsal.jig(&["field", "list"], &home, &fixture, b""));
    assert!(refused.contains("failed to unlock vault key"), "{refused}");

    let archive = rehearsal.base.join("vault.backup");
    let archive = archive.to_str().unwrap();
    json(&rehearsal.jig(
        &["backup", "create", "--out", archive],
        &home,
        &rotated,
        b"",
    ));
    let target = rehearsal.base.join("restored");
    let restore = ["backup", "restore", "--in", archive];
    let restored = json(&rehearsal.jig(&restore, &target, &rotated, b""));
    assert_eq!(restored["format_version"], 3);
    assert_eq!(restored["source_format_version"], 3);
    // Migration, the edit, and the rotation committed generations 1 to 3.
    assert_eq!(restored["generation"], 4);
    assert_eq!(restored["other_copies_stale"], true);

    let read = rehearsal.jig(&["read", TOKEN], &target, &rotated, b"");
    assert!(read.status.success());
    assert_eq!(read.stdout, EDITED.as_bytes());
    let verified = json(&rehearsal.jig(&["audit", "verify"], &target, &rotated, b""));
    assert_eq!(verified["ok"], true);

    // The restore fenced the live home, and a copy from before the rotation
    // is an older state of the same vault.
    let stale = failure(&rehearsal.jig(&["field", "list"], &home, &rotated, b""));
    assert!(
        stale.contains("older than its witnessed generation"),
        "{stale}"
    );
    let earlier = rehearsal.home("earlier", &earlier_vault, &earlier_audit);
    let rolled_back = failure(&rehearsal.jig(&["field", "list"], &earlier, &fixture, b""));
    assert!(
        rolled_back.contains("older than its witnessed generation"),
        "{rolled_back}"
    );

    // The test build kept the witness beside the disposable homes.
    let records = std::fs::read_dir(rehearsal.base.join(".jig-vault-witness/ids")).unwrap();
    assert_eq!(records.count(), 1);
    assert!(!rehearsal.profile.join(".jig").exists());
}
