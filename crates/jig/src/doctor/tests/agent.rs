#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;

use serde_json::json;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use tempfile::tempdir;
#[cfg(unix)]
use wait_timeout::ChildExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::support::{shell_quote_test_path, write_test_executable};
use crate::doctor::agent::agent_next_step;
#[cfg(unix)]
use crate::doctor::agent::standalone_codex_support_probe_with_signal_session;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{assert_test_process_stopped, read_test_process_identity};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn standalone_codex_sigint_sequence_helper() {
    let Some(codex) = std::env::var_os("JIG_STANDALONE_CODEX_SIGINT_BIN") else {
        return;
    };
    let result = standalone_codex_support_probe_with_signal_session(
        codex.as_os_str(),
        Duration::from_secs(30),
    );
    panic!("SIGINT was not re-delivered after standalone Codex cleanup: {result:?}");
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn standalone_codex_sigint_reaps_its_exact_descendant_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::context_checks::production_codex_probe_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let descendant_marker = temp.path().join("standalone-codex-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::agent::standalone_codex_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_STANDALONE_CODEX_SIGINT_BIN", &codex)
        .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the isolated standalone doctor helper.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("standalone doctor helper did not terminate after Codex cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}
#[test]
fn agent_next_step_prefers_command_shaped_steps() {
    let steps = vec![
        json!("Codex CLI is not available on PATH."),
        json!("Run `scripts/jig agent bootstrap` to register skills."),
    ];

    assert_eq!(
        agent_next_step(&steps),
        Some("Run `scripts/jig agent bootstrap` to register skills.")
    );
}
