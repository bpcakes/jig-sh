use std::ffi::{OsStr, OsString};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{env, fs};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use jig_context::RepoContext;
#[cfg(any(target_os = "linux", target_os = "macos", feature = "dev-proxy"))]
use jig_owned_process::ProcessOutputLimits;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use serde_json::json;
use tempfile::tempdir;
#[cfg(unix)]
use wait_timeout::ChildExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::support::{
    OWNED_PROCESS_DESCENDANT_MARKER_ENV, shell_quote_test_path, write_doctor_fixture,
    write_test_executable,
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::doctor::context_checks::doctor_context_checks;
use crate::doctor::proxy::proxy_list_command;
#[cfg(feature = "dev-proxy")]
use crate::doctor::proxy::proxy_list_output_limits;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::doctor::proxy::proxy_list_output_with_timeout;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::doctor::proxy::proxy_list_output_with_timeout_and_limits_and_cancellation;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{assert_test_process_stopped, read_test_process_identity};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_helper() {
    let Some(mode) = std::env::var_os("JIG_DOCTOR_PROXY_LIST_HELPER") else {
        return;
    };
    if mode == "valid" {
        println!(r#"{{"ok":true,"running":false,"routes":[]}}"#);
        return;
    }

    let marker = PathBuf::from(
        std::env::var_os("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER")
            .expect("hanging proxy-list helper has a descendant marker"),
    );
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::support::owned_process_descendant_helper",
            "--nocapture",
        ])
        .env(OWNED_PROCESS_DESCENDANT_MARKER_ENV, &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    std::mem::forget(child);
    let _ = read_test_process_identity(&marker);
    std::thread::sleep(Duration::from_secs(30));
}
#[test]
fn proxy_list_command_preserves_the_portable_launcher_plan() {
    let temp = tempdir().unwrap();
    let (launcher, command) = proxy_list_command(temp.path()).unwrap();
    let args = command
        .get_args()
        .map(OsStr::to_os_string)
        .collect::<Vec<_>>();

    assert!(launcher.is_absolute());
    assert_eq!(command.get_current_dir(), Some(temp.path()));
    for key in jig_owned_process::BASH_CONTROL_ENVIRONMENT_KEYS {
        assert!(
            command
                .get_envs()
                .any(|(candidate, value)| candidate == OsStr::new(key) && value.is_none()),
            "{key} was not removed from the launcher-backed proxy diagnostic"
        );
    }
    assert_eq!(command.get_program(), launcher.as_os_str());
    assert_eq!(
        args,
        vec![
            OsString::from("proxy"),
            OsString::from("list"),
            OsString::from("--json"),
        ]
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_executes_the_launcher_through_a_clean_bash_environment() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let poison_marker = temp.path().join("proxy-poison-ran");
    let trace_marker = temp.path().join("proxy-trace-poison-ran");
    fs::write(
        temp.path().join("scripts/proxy-startup-poison.sh"),
        "printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"\nexit 91\n",
    )
    .unwrap();
    // Execute an existing inode: a concurrent fork can retain a freshly written
    // fixture's writable descriptor and make exec fail with ETXTBSY.
    let launcher = temp.path().join("scripts/jig");
    fs::remove_file(&launcher).unwrap();
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/doctor/tests/fixtures/proxy-clean-environment.sh"),
        &launcher,
    )
    .unwrap();
    let (_, mut command) = proxy_list_command(temp.path()).unwrap();
    command
        .env(
            "BASH_ENV",
            temp.path().join("scripts/proxy-startup-poison.sh"),
        )
        .env("ENV", temp.path().join("scripts/proxy-startup-poison.sh"))
        .env("CDPATH", ".")
        .env(
            "BASH_FUNC_jig_doctor_proxy_poison%%",
            "() { printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"; }",
        )
        .env("SHELLOPTS", "xtrace:verbose")
        .env("BASHOPTS", "extglob")
        .env(
            "PS4",
            "JIG_DOCTOR_PROXY_PS4_POISON$(printf poison > \"$JIG_DOCTOR_PROXY_TRACE_MARKER\")",
        )
        .env("BASH_XTRACEFD", "2")
        .env("JIG_DOCTOR_PROXY_POISON_MARKER", &poison_marker)
        .env("JIG_DOCTOR_PROXY_TRACE_MARKER", &trace_marker)
        .env("JIG_DOCTOR_PROXY_ORDINARY", "preserved");
    jig_owned_process::sanitize_bash_environment(&mut command);

    let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["running"], false);
    assert_eq!(output["routes"], json!([]));
    assert!(
        !poison_marker.exists(),
        "Bash startup control environment executed during proxy diagnostics"
    );
    assert!(
        !trace_marker.exists(),
        "Bash trace environment executed during proxy diagnostics"
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_accepts_valid_json_larger_than_the_diagnostic_default() {
    let temp = tempdir().unwrap();
    let launcher = temp.path().join("proxy-list-valid");
    write_test_executable(
        &launcher,
        "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 1100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
    );
    let mut command = Command::new(launcher);

    let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["running"], false);
    assert_eq!(output["routes"], json!([]));
    assert!(
        output["padding"].as_str().unwrap().len() > ProcessOutputLimits::default().stdout,
        "proxy functional JSON must not share the 16 KiB diagnostic cap"
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_reports_injected_stdout_truncation() {
    let temp = tempdir().unwrap();
    let launcher = temp.path().join("proxy-list-truncated");
    write_test_executable(
        &launcher,
        "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
    );
    let mut command = Command::new(launcher);

    let error = proxy_list_output_with_timeout_and_limits_and_cancellation(
        &mut command,
        Duration::from_secs(2),
        ProcessOutputLimits {
            stdout: 128,
            stderr: ProcessOutputLimits::default().stderr,
        },
        || false,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("exceeded the diagnostic capture limit"),
        "{error}"
    );
}
#[cfg(feature = "dev-proxy")]
#[test]
fn proxy_list_capture_limit_exceeds_the_unchanged_routes_file_limit() {
    assert_eq!(jig_dev_proxy::MAX_ROUTES_FILE_BYTES, 4 * 1024 * 1024);
    let limits = proxy_list_output_limits();
    assert!(limits.stdout > jig_dev_proxy::MAX_ROUTES_FILE_BYTES as usize);
    assert_eq!(limits.stderr, ProcessOutputLimits::default().stderr);
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_timeout_reaps_its_exact_descendant() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("proxy-list-descendant");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "doctor::tests::proxy::proxy_list_output_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
        .env("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER", &marker);

    let error = proxy_list_output_with_timeout(&mut command, Duration::from_millis(100))
        .unwrap_err()
        .to_string();
    let descendant = read_test_process_identity(&marker);

    assert!(error.contains("timed out"), "{error}");
    assert_test_process_stopped(&descendant);
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_check_sigint_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_PROXY_SIGINT_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after proxy cleanup: {result:?}");
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_check_sigint_reaps_its_exact_descendant_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
        ),
    )
    .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::proxy::proxy_list_output_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let descendant_marker = temp.path().join("proxy-sigint-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::proxy::proxy_check_sigint_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_PROXY_SIGINT_ROOT", temp.path())
        .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
        .env(
            "JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER",
            &descendant_marker,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the live isolated helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("SIGINT helper did not terminate after proxy cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}
