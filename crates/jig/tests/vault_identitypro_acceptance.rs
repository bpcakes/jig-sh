#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const INITIAL_PASSPHRASE: &str = "test-only-initial-passphrase";
const ROTATED_PASSPHRASE: &str = "test-only-rotated-passphrase";
const CONCEALED_VALUE: &str = "test-only-mask-value";
const REPOSITORY_VALUE: &str = "fixture:local-store";
const COMPRESSION_VALUE: &str = "false";
const OP_REFERENCE: &str = "op://IdentityPro/Production/RESTIC_PASSWORD";

fn private_dir(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn write_synthetic_repo(repo: &Path) {
    private_dir(repo);
    private_dir(&repo.join(".agent"));
    std::fs::write(
        repo.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/test-only-template"
_commit = "test-only"
repo_name = "identitypro-synthetic"
default_branch = "main"
jig_version = "{}"
contract_check_command = "true"

[vault]
scope = "repo"
scope_id = "scope_identitypro_acceptance"
allow_global = false
"#,
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::write(
        repo.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": env!("CARGO_PKG_VERSION"),
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        repo.join("config.template"),
        b"repository={{ jig://Production/RESTIC_REPOSITORY }}\ncompression={{ jig://Production/RESTIC_COMPRESSION }}\npassword={{ jig://Production/RESTIC_PASSWORD }}\n",
    )
    .unwrap();

    git(repo, ["init", "-q", "--template="]);
    git(repo, ["add", "."]);
    let status = Command::new("git")
        .current_dir(repo)
        .args([
            "-c",
            "user.name=Jig Test",
            "-c",
            "user.email=jig-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "synthetic baseline",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "failed to commit synthetic baseline");
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "synthetic git command failed");
}

fn install_fake_op(bin: &Path) -> PathBuf {
    private_dir(bin);
    let op = bin.join("op");
    std::fs::write(
        &op,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{JIG_VAULT_PASSPHRASE+set}}" = set ] || [ "${{JIG_VAULT_NEW_PASSPHRASE+set}}" = set ]; then
  exit 86
fi
if [ "$#" -ne 3 ] || [ "$1" != read ] || [ "$2" != --no-newline ] || [ "$3" != '{}' ]; then
  exit 87
fi
printf 'arg=<%s>\n' "$1" "$2" "$3" >> "$OP_TEST_LOG"
printf '%s' '{}'
"#,
            OP_REFERENCE, CONCEALED_VALUE
        ),
    )
    .unwrap();
    std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o700)).unwrap();
    op
}

fn jig_command(
    repo: &Path,
    vault_base: &Path,
    current_passphrase: &str,
    new_passphrase: Option<&str>,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(repo)
        .env("JIG_VAULT_HOME", vault_base)
        .env("JIG_VAULT_PASSPHRASE", current_passphrase)
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD");
    if let Some(new_passphrase) = new_passphrase {
        command.env("JIG_VAULT_NEW_PASSPHRASE", new_passphrase);
    }
    command
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed with status {:?}",
        output.status.code()
    );
}

fn output_json(label: &str, output: &Output) -> serde_json::Value {
    assert_success(label, output);
    assert!(output.stderr.is_empty(), "{label} wrote a diagnostic");
    serde_json::from_slice(&output.stdout).expect("vault command returned malformed JSON")
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn assert_no_sensitive_bytes(bytes: &[u8], context: &str) {
    for value in [
        CONCEALED_VALUE.as_bytes(),
        INITIAL_PASSPHRASE.as_bytes(),
        ROTATED_PASSPHRASE.as_bytes(),
    ] {
        assert!(
            !contains_bytes(bytes, value),
            "{context} contained protected test bytes"
        );
    }
}

fn assert_no_encrypted_payload_plaintext(bytes: &[u8], context: &str) {
    assert_no_sensitive_bytes(bytes, context);
    assert!(
        !contains_bytes(bytes, REPOSITORY_VALUE.as_bytes()),
        "{context} contained imported text bytes"
    );
}

fn assert_json_has_no_field_values(value: &serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                assert_json_has_no_field_values(value);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                assert_json_has_no_field_values(value);
            }
        }
        serde_json::Value::String(value) => {
            assert!(
                ![
                    CONCEALED_VALUE,
                    REPOSITORY_VALUE,
                    COMPRESSION_VALUE,
                    INITIAL_PASSPHRASE,
                    ROTATED_PASSPHRASE,
                ]
                .contains(&value.as_str()),
                "structured vault output contained a field value"
            );
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn structured_output(label: &str, output: &Output) -> serde_json::Value {
    assert_no_sensitive_bytes(&output.stdout, label);
    assert_no_sensitive_bytes(&output.stderr, label);
    let value = output_json(label, output);
    assert_json_has_no_field_values(&value);
    value
}

fn assert_audit_has_no_values(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    assert_no_sensitive_bytes(&bytes, "vault audit");
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: serde_json::Value =
            serde_json::from_slice(line).expect("audit contained malformed JSON");
        assert_json_has_no_field_values(&value);
    }
}

fn assert_repo_has_no_imported_values(path: &Path) {
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            assert_repo_has_no_imported_values(&entry.path());
        } else if file_type.is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert_no_encrypted_payload_plaintext(&bytes, "synthetic repository");
            assert!(
                !contains_bytes(&bytes, b"compression=false"),
                "synthetic repository contained rendered compression text"
            );
        }
    }
}

fn run_exec(
    repo: &Path,
    vault_base: &Path,
    passphrase: &str,
    env_file: &Path,
    home: Option<&Path>,
    script: &str,
) -> Output {
    let mut command = jig_command(repo, vault_base, passphrase, None);
    command.args(["vault", "exec", "--env-file"]).arg(env_file);
    if let Some(home) = home {
        command.arg("--home").arg(home);
    }
    command.args(["--", "/bin/sh", "-c", script]);
    command.output().unwrap()
}

#[test]
fn synthetic_identitypro_cutover_covers_the_general_project_vault_workflow() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = temp.path().join("identitypro-synthetic");
    let vault_base = temp.path().join("vault-base");
    let fake_bin = temp.path().join("fake-bin");
    let fixture_dir = temp.path().join("fixture-input");
    let runtime_dir = temp.path().join("rendered-output");
    private_dir(&vault_base);
    private_dir(&fixture_dir);
    private_dir(&runtime_dir);
    write_synthetic_repo(&repo);
    install_fake_op(&fake_bin);

    let source_env = fixture_dir.join("identitypro.env");
    let generated_env = repo.join(".env.jig");
    let op_log = fixture_dir.join("op.log");
    let rendered_config = runtime_dir.join("config");
    let backup = temp.path().join("project-vault.backup");
    let restored_home = temp.path().join("restored-vault");
    std::fs::write(
        &source_env,
        format!(
            "RESTIC_PASSWORD={OP_REFERENCE}\nRESTIC_REPOSITORY={REPOSITORY_VALUE}\nRESTIC_COMPRESSION={COMPRESSION_VALUE}\n"
        ),
    )
    .unwrap();

    let initialized = jig_command(&repo, &vault_base, INITIAL_PASSPHRASE, None)
        .args(["--json", "vault", "init"])
        .output()
        .unwrap();
    let initialized_json = structured_output("vault initialization", &initialized);
    assert_eq!(initialized_json["vault_scope"], "repo");
    assert_eq!(
        initialized_json["vault_scope_id"],
        "scope_identitypro_acceptance"
    );
    let source_home = PathBuf::from(
        initialized_json["vault_home"]
            .as_str()
            .expect("init omitted vault home"),
    );

    let migrated = jig_command(&repo, &vault_base, INITIAL_PASSPHRASE, None)
        .args(["--json", "vault", "migrate", "--to", "2"])
        .output()
        .unwrap();
    structured_output("vault migration", &migrated);

    let mut path_parts = vec![fake_bin];
    if let Some(path) = std::env::var_os("PATH") {
        path_parts.extend(std::env::split_paths(&path));
    }
    let fake_path = std::env::join_paths(path_parts).unwrap();
    let imported = jig_command(&repo, &vault_base, INITIAL_PASSPHRASE, None)
        .args(["--json", "vault", "import", "onepassword", "--env-file"])
        .arg(&source_env)
        .args(["--item", "Production", "--out-env"])
        .arg(&generated_env)
        .env("PATH", fake_path)
        .env("OP_TEST_LOG", &op_log)
        .output()
        .unwrap();
    let imported_json = structured_output("vault import", &imported);
    assert_eq!(imported_json["dry_run"], false);
    assert_eq!(imported_json["fields"][0]["kind"], "concealed");
    assert_eq!(imported_json["fields"][1]["kind"], "text");
    assert_eq!(imported_json["fields"][2]["kind"], "text");
    assert_eq!(
        std::fs::read(&generated_env).unwrap(),
        b"RESTIC_PASSWORD=jig://Production/RESTIC_PASSWORD\nRESTIC_REPOSITORY=jig://Production/RESTIC_REPOSITORY\nRESTIC_COMPRESSION=jig://Production/RESTIC_COMPRESSION\n"
    );
    assert_eq!(
        std::fs::read_to_string(&op_log).unwrap(),
        format!("arg=<read>\narg=<--no-newline>\narg=<{OP_REFERENCE}>\n")
    );

    let source_exec = run_exec(
        &repo,
        &vault_base,
        INITIAL_PASSPHRASE,
        &generated_env,
        None,
        "printf 'password=%s repository=%s compression=%s\\n' \"$RESTIC_PASSWORD\" \"$RESTIC_REPOSITORY\" \"$RESTIC_COMPRESSION\"; printf 'error-password=%s\\n' \"$RESTIC_PASSWORD\" >&2",
    );
    assert_success("source vault exec", &source_exec);
    assert_no_sensitive_bytes(&source_exec.stdout, "source exec stdout");
    assert_no_sensitive_bytes(&source_exec.stderr, "source exec stderr");
    assert!(
        source_exec.stdout
            == format!(
                "password=[REDACTED] repository={REPOSITORY_VALUE} compression={COMPRESSION_VALUE}\n"
            )
            .as_bytes(),
        "source exec output did not preserve text and redact concealed data"
    );
    assert!(
        source_exec.stderr == b"error-password=[REDACTED]\n",
        "source exec stderr was not independently redacted"
    );

    let injected = jig_command(&repo, &vault_base, INITIAL_PASSPHRASE, None)
        .args(["vault", "inject", "--in"])
        .arg(repo.join("config.template"))
        .arg("--out-file")
        .arg(&rendered_config)
        .output()
        .unwrap();
    assert_success("vault injection", &injected);
    assert!(injected.stdout.is_empty());
    assert!(injected.stderr.is_empty());
    assert!(
        std::fs::read(&rendered_config).unwrap()
            == format!(
                "repository={REPOSITORY_VALUE}\ncompression={COMPRESSION_VALUE}\npassword={CONCEALED_VALUE}\n"
            )
            .as_bytes(),
        "injected fixture did not contain the exact field values"
    );
    assert_eq!(
        std::fs::metadata(&rendered_config)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let mut read_command = jig_command(&repo, &vault_base, INITIAL_PASSPHRASE, None);
    let mut read = read_command
        .args(["vault", "read", "jig://Production/RESTIC_PASSWORD"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let read_stdout = read.stdout.take().expect("vault read did not open stdout");
    let pipe_output = Command::new("/bin/sh")
        .args([
            "-c",
            "IFS= read -r value || :; [ \"$value\" = \"$EXPECTED_READ\" ]; printf read-pipe-ok",
        ])
        .env("EXPECTED_READ", CONCEALED_VALUE)
        .stdin(Stdio::from(read_stdout))
        .output()
        .unwrap();
    let read_output = read.wait_with_output().unwrap();
    assert_success("vault read", &read_output);
    assert!(read_output.stderr.is_empty());
    assert_success("vault read pipe", &pipe_output);
    assert_eq!(pipe_output.stdout, b"read-pipe-ok");
    assert!(pipe_output.stderr.is_empty());

    let changed = jig_command(
        &repo,
        &vault_base,
        INITIAL_PASSPHRASE,
        Some(ROTATED_PASSPHRASE),
    )
    .args(["--json", "vault", "passphrase", "change"])
    .output()
    .unwrap();
    let changed_json = structured_output("passphrase change", &changed);
    assert_eq!(changed_json["changed"], true);

    let created_backup = jig_command(&repo, &vault_base, ROTATED_PASSPHRASE, None)
        .args(["--json", "vault", "backup", "create", "--out"])
        .arg(&backup)
        .output()
        .unwrap();
    let backup_json = structured_output("vault backup creation", &created_backup);
    assert_eq!(backup_json["backup_version"], 1);
    assert!(backup_json["bytes_written"].as_u64().unwrap() > 0);
    assert_no_encrypted_payload_plaintext(&std::fs::read(&backup).unwrap(), "encrypted backup");

    let restored = jig_command(&repo, &vault_base, ROTATED_PASSPHRASE, None)
        .args(["--json", "vault", "backup", "restore", "--in"])
        .arg(&backup)
        .arg("--home")
        .arg(&restored_home)
        .output()
        .unwrap();
    let restored_json = structured_output("vault restore", &restored);
    assert_eq!(restored_json["restored"], true);
    assert_eq!(restored_json["format_version"], 2);
    assert_eq!(
        restored_json["vault_home"],
        restored_home.display().to_string()
    );

    let restored_exec = run_exec(
        &repo,
        &vault_base,
        ROTATED_PASSPHRASE,
        &generated_env,
        Some(&restored_home),
        "printf 'restored-password=%s repository=%s compression=%s\\n' \"$RESTIC_PASSWORD\" \"$RESTIC_REPOSITORY\" \"$RESTIC_COMPRESSION\"; printf 'restored-error=%s\\n' \"$RESTIC_PASSWORD\" >&2; exit 23",
    );
    assert_eq!(restored_exec.status.code(), Some(23));
    assert_no_sensitive_bytes(&restored_exec.stdout, "restored exec stdout");
    assert_no_sensitive_bytes(&restored_exec.stderr, "restored exec stderr");
    assert!(
        restored_exec.stdout
            == format!(
                "restored-password=[REDACTED] repository={REPOSITORY_VALUE} compression={COMPRESSION_VALUE}\n"
            )
            .as_bytes(),
        "restored exec output did not preserve text and redact concealed data"
    );
    assert!(
        restored_exec.stderr == b"restored-error=[REDACTED]\n",
        "restored exec emitted a second Jig error or unredacted bytes"
    );

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

    git(&repo, ["add", ".env.jig"]);
    let cached_diff = Command::new("git")
        .current_dir(&repo)
        .args(["diff", "--cached", "--binary", "--no-ext-diff"])
        .output()
        .unwrap();
    assert_success("synthetic Git diff", &cached_diff);
    assert_no_sensitive_bytes(&cached_diff.stdout, "synthetic Git diff");
    for value in [REPOSITORY_VALUE, COMPRESSION_VALUE] {
        assert!(!contains_bytes(&cached_diff.stdout, value.as_bytes()));
    }
    assert_repo_has_no_imported_values(&repo);
}
