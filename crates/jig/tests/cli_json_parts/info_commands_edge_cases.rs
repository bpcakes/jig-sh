use super::*;

fn assert_invalid_override_info(output: &Value, adopted: bool) {
    assert_eq!(output["repo"]["name"], Value::Null);
    assert_eq!(
        output["repo"]["context_status"],
        if adopted { "recovered" } else { "invalid" }
    );
    assert_eq!(command_status(output, "info"), "needs_setup");
    assert_eq!(command_status(output, "prompt"), "ready");
    assert_dev_proxy_status(output, "proxy", "needs_setup", "repo_context_unavailable");
    if adopted {
        assert_dev_proxy_status(output, "dev", "not_configured", "dev_apps_not_configured");
    } else {
        assert_dev_proxy_status(output, "dev", "needs_setup", "repo_context_unavailable");
        #[cfg(feature = "dev-proxy")]
        assert!(
            command_by_name(output, "dev")["next_step"]
                .as_str()
                .unwrap()
                .contains("JIG_REPO_ROOT")
        );
    }
    assert_eq!(command_status(output, "vault"), "needs_setup");
    assert_eq!(
        command_by_name(output, "vault")["reason_code"],
        "vault_not_initialized"
    );
}

fn assert_invalid_override_next_steps(output: &Value, adopted: bool) {
    assert!(
        command_by_name(output, "info")["next_step"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT"),
        "adopted={adopted}: info"
    );
    #[cfg(feature = "dev-proxy")]
    assert!(
        command_by_name(output, "proxy")["next_step"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT"),
        "adopted={adopted}: proxy"
    );
}

fn assert_contextless_commands_run(
    repo: &Path,
    vault: &Path,
    invalid_root: &Path,
    proxy_state: &Path,
    adopted: bool,
) {
    for args in [
        &["vault", "status", "--json"][..],
        &["prompt", "list", "--json"][..],
    ] {
        let command = jig()
            .current_dir(repo)
            .env("JIG_REPO_ROOT", invalid_root)
            .env("JIG_VAULT_HOME", vault)
            .env("JIG_PROXY_STATE_DIR", proxy_state)
            .args(args)
            .output()
            .unwrap();
        assert!(
            command.status.success(),
            "adopted={adopted}: {}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&command.stderr)
        );
    }
}

fn assert_proxy_and_dev_contextless_commands(
    repo: &Path,
    invalid_root: &Path,
    vault: &Path,
    proxy_state: &Path,
    adopted: bool,
) {
    let proxy_list = jig()
        .current_dir(repo)
        .env("JIG_REPO_ROOT", invalid_root)
        .env("JIG_VAULT_HOME", vault)
        .env("JIG_PROXY_STATE_DIR", proxy_state)
        .args(["proxy", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        proxy_list.status.success(),
        cfg!(feature = "dev-proxy"),
        "adopted={adopted}: proxy list --json\n{}",
        String::from_utf8_lossy(&proxy_list.stderr)
    );

    if adopted && cfg!(feature = "dev-proxy") {
        let dev_status = jig()
            .current_dir(repo)
            .env("JIG_REPO_ROOT", invalid_root)
            .env("JIG_PROXY_STATE_DIR", proxy_state)
            .args(["dev", "status", "--json"])
            .output()
            .unwrap();
        assert!(
            dev_status.status.success(),
            "{}",
            String::from_utf8_lossy(&dev_status.stderr)
        );
    }
}

fn assert_invalid_override_case(adopted: bool) {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    if adopted {
        write_info_commands_repo(repo.path());
    }
    let invalid_root = repo.path().join("missing-override");
    let proxy_state = repo.path().join("proxy-state");
    let output = jig()
        .current_dir(repo.path())
        .env("JIG_REPO_ROOT", &invalid_root)
        .env("JIG_VAULT_HOME", vault.path())
        .env("JIG_PROXY_STATE_DIR", &proxy_state)
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "adopted={adopted}");
    assert!(output.stderr.is_empty(), "adopted={adopted}");
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_invalid_override_info(&output, adopted);
    assert_invalid_override_next_steps(&output, adopted);
    assert_contextless_commands_run(
        repo.path(),
        vault.path(),
        &invalid_root,
        &proxy_state,
        adopted,
    );
    assert_proxy_and_dev_contextless_commands(
        repo.path(),
        &invalid_root,
        vault.path(),
        &proxy_state,
        adopted,
    );
}

#[test]
fn info_commands_matches_contextless_commands_with_an_invalid_override() {
    for adopted in [false, true] {
        assert_invalid_override_case(adopted);
    }
}

#[test]
fn info_commands_prioritizes_invalid_override_recovery_when_the_local_repo_is_also_broken() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    std::fs::write(repo.path().join(".jig.toml"), "not valid toml = [").unwrap();
    let invalid_root = repo.path().join("missing-override");

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_REPO_ROOT", &invalid_root)
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["repo"]["context_status"], "invalid");
    for name in ["info", "check", "agent", "vault", "prompt"] {
        assert!(
            command_by_name(&output, name)["next_step"]
                .as_str()
                .unwrap()
                .contains("JIG_REPO_ROOT"),
            "{name}"
        );
    }
    #[cfg(feature = "dev-proxy")]
    assert!(
        command_by_name(&output, "dev")["next_step"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT"),
        "dev"
    );
}

#[cfg(unix)]
#[test]
fn info_commands_emits_no_progress_when_the_codex_probe_is_skipped() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let (stderr, terminal_output) = terminal_stderr();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .args(["info", "--commands"])
        .stderr(stderr)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(read_terminal(terminal_output), b"");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Jig command availability: phase-two")
    );
}

#[cfg(unix)]
#[test]
fn info_commands_keeps_json_quiet_while_probing_configured_codex_marketplaces() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    let codex_home = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    configure_codex_marketplace(repo.path());
    let codex = write_codex_stub(repo.path(), 2);
    let (stderr, terminal_output) = terminal_stderr();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", codex_home.path())
        .args(["info", "--commands", "--json"])
        .stderr(stderr)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(read_terminal(terminal_output), b"");
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    let agent = output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == "agent")
        .unwrap();
    assert_eq!(agent["status"], "unavailable");
    assert_eq!(
        agent["reason_code"],
        "codex_marketplace_support_unavailable"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn info_commands_reports_terminal_progress_for_a_configured_codex_probe() {
    let repo = tempdir().unwrap();
    let vault = tempdir().unwrap();
    let codex_home = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    configure_codex_marketplace(repo.path());
    write_registered_codex_marketplace(codex_home.path());
    let codex = write_codex_stub(repo.path(), 0);
    let (stderr, terminal_output) = terminal_stderr();

    let output = jig()
        .current_dir(repo.path())
        .env("JIG_VAULT_HOME", vault.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", codex_home.path())
        .env("NO_COLOR", "1")
        .args(["info", "--commands"])
        .stderr(stderr)
        .output()
        .unwrap();

    assert!(output.status.success());
    let terminal_output = String::from_utf8(read_terminal(terminal_output)).unwrap();
    assert!(terminal_output.contains("jig info --commands | inspect local Codex tooling"));
    assert!(terminal_output.contains("probe codex"));
    assert!(terminal_output.contains("command inventory probe complete"));
    assert!(!terminal_output.contains("agent doctor"));
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("Jig command availability: phase-two"));
}

pub(super) fn write_info_commands_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
default_branch = "main"
repo_name = "phase-two"
jig_version = "0.2.0-beta.1"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": [{
                "name": "jig.bootstrap",
                "kind": "command",
                "description": "Bootstrap the repository.",
                "command": "bootstrap_command"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
}

pub(super) fn write_test_launcher(root: &Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    for path in [
        root.join("scripts/jig"),
        root.join("scripts/install-jig.sh"),
    ] {
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
        }
    }
}

#[cfg(unix)]
fn configure_codex_marketplace(root: &Path) {
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[agent_tooling.codex]\nmarketplaces = []",
        r#"[[agent_tooling.codex.marketplaces]]
id = "jig-skills"
source = "bpcakes/jig-skills"
plugins = []"#,
    );
    fs::write(config_path, config).unwrap();
}

#[cfg(target_os = "linux")]
fn write_registered_codex_marketplace(codex_home: &Path) {
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"
"#,
    )
    .unwrap();
}

#[cfg(unix)]
fn write_codex_stub(root: &Path, exit_code: u8) -> PathBuf {
    let codex = root.join("codex-stub.sh");
    fs::write(
        &codex,
        format!("#!/bin/sh\nprintf 'captured probe output' >&2\nexit {exit_code}\n"),
    )
    .unwrap();
    let mut permissions = fs::metadata(&codex).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&codex, permissions).unwrap();
    codex
}

pub(super) fn command_by_name<'a>(output: &'a Value, name: &str) -> &'a Value {
    output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("missing command {name}"))
}

pub(super) fn command_status<'a>(output: &'a Value, name: &str) -> &'a str {
    command_by_name(output, name)["status"].as_str().unwrap()
}

pub(super) fn assert_dev_proxy_status(
    output: &Value,
    name: &str,
    feature_status: &str,
    feature_reason_code: &str,
) {
    let (expected_status, expected_reason_code) = if cfg!(feature = "dev-proxy") {
        (feature_status, feature_reason_code)
    } else {
        ("unavailable", "dev_proxy_feature_not_built")
    };
    assert_eq!(command_status(output, name), expected_status, "{name}");
    assert_eq!(
        command_by_name(output, name)["reason_code"],
        expected_reason_code,
        "{name}"
    );
}

#[cfg(unix)]
fn terminal_stderr() -> (Stdio, File) {
    let mut controller = -1;
    let mut terminal = -1;
    // SAFETY: openpty initializes both owned file descriptors on success. Each
    // descriptor is immediately wrapped exactly once and closed by File/Stdio.
    let result = unsafe {
        libc::openpty(
            &mut controller,
            &mut terminal,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        result,
        0,
        "openpty failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: successful openpty returned distinct owned descriptors.
    let controller = unsafe { File::from_raw_fd(controller) };
    // SAFETY: successful openpty returned distinct owned descriptors.
    let terminal = unsafe { File::from_raw_fd(terminal) };
    (Stdio::from(terminal), controller)
}

#[cfg(unix)]
fn read_terminal(mut controller: File) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        match controller.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                break;
            }
            Err(error) => panic!("failed reading terminal stderr: {error}"),
        }
    }
    output
}
