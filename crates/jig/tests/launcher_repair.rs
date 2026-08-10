use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::tempdir;

fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn binary_file_identity(path: &Path) -> String {
    let mut command = Command::new("/usr/bin/stat");
    if cfg!(target_os = "macos") {
        command.args(["-f", "%z:%Fm:%Fc:%i"]);
    } else {
        command.args(["-c", "%s:%Y:%Z:%i:%y:%z"]);
    }
    let output = command.arg(path).output().unwrap();
    assert_success(&output, "inspect published repair binary identity");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .replace(' ', "T")
}

#[test]
fn launcher_only_update_uses_trusted_bash_and_runs_the_real_seed_subprocess_path() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repaired");
    let jig = env!("CARGO_BIN_EXE_jig");

    initialize_contract_drift_repo(jig, &destination, "repair-integration");

    let fake_bin = temp.path().join("recording-bin");
    let bash_log = temp.path().join("bash-invocations");
    let helper_log = temp.path().join("helper-invocations");
    fs::create_dir(&fake_bin).unwrap();
    let recording_bash = fake_bin.join("bash");
    fs::write(
        &recording_bash,
        r#"#!/bin/sh
{
  printf 'call\0'
  for arg in "$@"; do printf '%s\0' "$arg"; done
} >>"$JIG_TEST_BASH_LOG"
exit 79
"#,
    )
    .unwrap();
    fs::set_permissions(&recording_bash, fs::Permissions::from_mode(0o755)).unwrap();
    let recording_cp = fake_bin.join("cp");
    fs::write(
        &recording_cp,
        "#!/bin/sh\nprintf cp >\"$JIG_TEST_HELPER_LOG\"\nexit 79\n",
    )
    .unwrap();
    fs::set_permissions(&recording_cp, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let repair = Command::new(jig)
        .arg("update")
        .arg(&destination)
        .args(["--launcher-only", "--force"])
        .env("PATH", path)
        .env("JIG_TEST_BASH_LOG", &bash_log)
        .env("JIG_TEST_HELPER_LOG", &helper_log)
        .output()
        .unwrap();
    assert_success(&repair, "launcher-only repair");

    let install_base = if destination.join(".git").is_dir() {
        destination.join(".git/jig-tools")
    } else {
        destination.join(".agent/.cache/jig")
    };
    let canonical_install_base = fs::canonicalize(&install_base).unwrap();
    assert!(
        !bash_log.exists(),
        "launcher repair executed an ambient PATH bash"
    );
    assert!(
        !helper_log.exists(),
        "launcher repair executed an ambient PATH helper command"
    );
    let expected_profiles = if cfg!(feature = "dev-proxy") {
        vec!["runtime", "default"]
    } else {
        vec!["runtime"]
    };
    for profile in expected_profiles {
        let cache_name = if profile == "default" {
            "contract-3".to_string()
        } else {
            format!("contract-3-{profile}")
        };
        assert!(
            canonical_install_base.join(cache_name).is_dir(),
            "missing published {profile} repair cache"
        );
    }
    let runtime_stamp = install_base.join("contract-3-runtime/.jig-source-stamp");
    let stamp = fs::read_to_string(&runtime_stamp).unwrap();
    assert!(stamp.starts_with("jig-seeded-runtime-v1\nbinary:sha256:"));
    let identity_lines = stamp
        .lines()
        .filter(|line| line.starts_with("binary-identity:"))
        .collect::<Vec<_>>();
    assert_eq!(identity_lines.len(), 1, "{stamp}");
    assert!(
        !identity_lines[0].chars().any(char::is_whitespace),
        "{stamp}"
    );
    assert_eq!(
        identity_lines[0].strip_prefix("binary-identity:").unwrap(),
        binary_file_identity(&install_base.join("contract-3-runtime/bin/jig")),
        "seeded provenance must describe the published binary"
    );
    assert!(stamp.contains("\nsource:sha256:"));

    let launcher = destination.join("scripts/jig");
    assert!(launcher_text(&launcher).contains("CONTRACT_VERSION=\"3\""));
    let version = Command::new(&launcher)
        .arg("--version")
        .env_remove("JIG_DEV_BIN")
        .current_dir(&destination)
        .output()
        .unwrap();
    assert_success(&version, "repaired launcher execution");
}

#[test]
fn launcher_only_update_rolls_back_real_scripts_when_seed_process_fails() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("rollback");
    let jig = env!("CARGO_BIN_EXE_jig");
    initialize_contract_drift_repo(jig, &destination, "repair-rollback");
    configure_unfingerprintable_local_source(&destination, &temp.path().join("deep-source"));

    let launcher = destination.join("scripts/jig");
    let installer = destination.join("scripts/install-jig.sh");
    let launcher_before = fs::read(&launcher).unwrap();
    let installer_before = fs::read(&installer).unwrap();

    let repair = Command::new(jig)
        .arg("update")
        .arg(&destination)
        .args(["--launcher-only", "--force"])
        .output()
        .unwrap();

    assert!(
        !repair.status.success(),
        "seed failure unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&repair.stderr);
    assert!(
        stderr.contains("Local Jig source stamp limit exceeded")
            && stderr.contains("Failed to seed the repaired launcher runtime for profile runtime"),
        "{stderr}"
    );
    assert_eq!(fs::read(&launcher).unwrap(), launcher_before);
    assert_eq!(fs::read(&installer).unwrap(), installer_before);
}

#[cfg(feature = "dev-proxy")]
#[test]
fn launcher_only_update_does_not_publish_any_cache_when_seeding_fails() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("cache-rollback");
    let jig = env!("CARGO_BIN_EXE_jig");
    initialize_contract_drift_repo(jig, &destination, "repair-cache-rollback");
    configure_unfingerprintable_local_source(&destination, &temp.path().join("deep-source"));

    let launcher = destination.join("scripts/jig");
    let installer = destination.join("scripts/install-jig.sh");
    let launcher_before = fs::read(&launcher).unwrap();
    let installer_before = fs::read(&installer).unwrap();
    let cache_base = if destination.join(".git").is_dir() {
        destination.join(".git/jig-tools")
    } else {
        destination.join(".agent/.cache/jig")
    };
    for cache_name in ["contract-3-runtime", "contract-3"] {
        let cache = cache_base.join(cache_name);
        fs::create_dir_all(cache.join("bin")).unwrap();
        fs::write(cache.join("bin/jig"), format!("old-{cache_name}-binary")).unwrap();
        fs::write(
            cache.join(".jig-source-stamp"),
            format!("old-{cache_name}-stamp"),
        )
        .unwrap();
    }

    let repair = Command::new(jig)
        .arg("update")
        .arg(&destination)
        .args(["--launcher-only", "--force"])
        .output()
        .unwrap();

    assert!(!repair.status.success());
    assert_eq!(fs::read(&launcher).unwrap(), launcher_before);
    assert_eq!(fs::read(&installer).unwrap(), installer_before);
    for cache_name in ["contract-3-runtime", "contract-3"] {
        let cache = cache_base.join(cache_name);
        assert_eq!(
            fs::read_to_string(cache.join("bin/jig")).unwrap(),
            format!("old-{cache_name}-binary")
        );
        assert_eq!(
            fs::read_to_string(cache.join(".jig-source-stamp")).unwrap(),
            format!("old-{cache_name}-stamp")
        );
    }
    assert!(fs::read_dir(&cache_base).unwrap().flatten().all(|entry| {
        !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".jig-launcher-repair-")
    }));
}

fn initialize_contract_drift_repo(jig: &str, destination: &Path, repo_name: &str) {
    let init = Command::new(jig)
        .arg("init")
        .arg(destination)
        .args([
            "--preset",
            "harness-only",
            "--repo-name",
            repo_name,
            "--sqlx-enabled",
            "false",
            "--no-input",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert_success(&init, "fixture init");

    let manifest_path = destination.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = 3.into();
    manifest["jig_version"] = "0.2.0-beta.1".into();
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap() + "\n",
    )
    .unwrap();

    let answers_path = destination.join(".jig.toml");
    let answers = fs::read_to_string(&answers_path).unwrap();
    let answers = answers.replacen(
        "template_source_url =",
        "jig_version = \"0.2.0-beta.1\"\ntemplate_source_url =",
        1,
    );
    fs::write(&answers_path, answers).unwrap();
}

fn configure_unfingerprintable_local_source(destination: &Path, source: &Path) {
    fs::create_dir_all(source.join("crates/jig")).unwrap();
    fs::write(
        source.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/jig\"]\n",
    )
    .unwrap();
    fs::write(source.join("Cargo.lock"), "# fixture\n").unwrap();
    fs::write(
        source.join("crates/jig/Cargo.toml"),
        "[package]\nname = \"jig-sh\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    let mut nested = source.join("crates/jig");
    for _ in 0..130 {
        nested.push("d");
    }
    fs::create_dir_all(nested).unwrap();

    let answers_path = destination.join(".jig.toml");
    let mut answers: toml::Value =
        toml::from_str(&fs::read_to_string(&answers_path).unwrap()).unwrap();
    let answers = answers.as_table_mut().unwrap();
    answers.insert(
        "_src_path".into(),
        toml::Value::String(source.display().to_string()),
    );
    answers.insert("_commit".into(), toml::Value::String(String::new()));
    answers.insert(
        "template_source_url".into(),
        toml::Value::String(String::new()),
    );
    fs::write(&answers_path, toml::to_string(answers).unwrap()).unwrap();
}

#[test]
fn launcher_rejects_drift_from_the_repository_contract_before_dispatch() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("contract-drift");
    let jig = env!("CARGO_BIN_EXE_jig");

    let init = Command::new(jig)
        .arg("init")
        .arg(&destination)
        .args([
            "--preset",
            "harness-only",
            "--repo-name",
            "contract-drift",
            "--sqlx-enabled",
            "false",
            "--no-input",
            "--no-vault",
        ])
        .output()
        .unwrap();
    assert_success(&init, "fixture init");

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    let contract_version = manifest["contract_version"].as_u64().unwrap();
    let drift_version = contract_version + 1;
    let launcher = destination.join("scripts/jig");
    let launcher_source = launcher_text(&launcher);
    let drifted = launcher_source.replacen(
        &format!("CONTRACT_VERSION=\"{contract_version}\""),
        &format!("CONTRACT_VERSION=\"{drift_version}\""),
        1,
    );
    assert_ne!(drifted, launcher_source);
    fs::write(&launcher, drifted).unwrap();

    let output = Command::new(&launcher)
        .arg("info")
        .env("JIG_DEV_BIN", jig)
        .current_dir(&destination)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!(
        "Launcher contract version {drift_version} does not match repository contract version {contract_version}"
    )), "{stderr}");
}

fn launcher_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}
