#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use tempfile::tempdir;
#[cfg(unix)]
use wait_timeout::ChildExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::support::owned_test_descendant_script;
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use super::support::write_test_executable;
#[cfg(unix)]
use crate::doctor::environment::DoctorEnvironment;
use crate::doctor::sqlx_driver::SqlxDriver;
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use crate::doctor::sqlx_driver_probe::SqlxProbeStyle;
#[cfg(unix)]
use crate::doctor::sqlx_driver_probe::probe_sqlx_driver_with_timeout_and_environment_and_cancellation;
use crate::doctor::sqlx_driver_probe::{SqlxDriverProbe, classify_sqlx_driver_probe};
#[cfg(unix)]
use crate::doctor::sqlx_driver_probe::{
    probe_sqlx_driver_with_timeout, probe_sqlx_driver_with_timeout_and_environment,
};
#[cfg(unix)]
use crate::signal_supervision::SignalSession;
#[cfg(unix)]
use crate::signal_supervision::session::finish_signal_session;
#[cfg(unix)]
use crate::test_env::{EnvVarGuard, lock_env};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{assert_test_process_stopped, read_test_process_identity};

#[test]
fn sqlx_driver_probe_classifies_supported_and_missing_drivers() {
    assert_eq!(
        classify_sqlx_driver_probe(SqlxDriver::Sqlite, true, "", ""),
        SqlxDriverProbe::Compatible
    );
    assert_eq!(
        classify_sqlx_driver_probe(
            SqlxDriver::Postgres,
            false,
            "error: unknown value \"jig-doctor-invalid\" for `ssl_mode`",
            ""
        ),
        SqlxDriverProbe::Compatible
    );
    assert_eq!(
        classify_sqlx_driver_probe(
            SqlxDriver::Postgres,
            false,
            "error: invalid value 'jig-doctor-invalid' for sslmode",
            ""
        ),
        SqlxDriverProbe::Compatible
    );
    assert_eq!(
        classify_sqlx_driver_probe(
            SqlxDriver::Sqlite,
            false,
            "",
            "error: error with configuration: no driver found for URL scheme \"sqlite\""
        ),
        SqlxDriverProbe::Incompatible
    );
    assert!(matches!(
        classify_sqlx_driver_probe(SqlxDriver::Sqlite, false, "error: unexpected argument", ""),
        SqlxDriverProbe::Indeterminate(_)
    ));
    for (stdout, stderr) in [
        ("error: invalid value for ssl_mode", ""),
        ("error: invalid value 'jig-doctor-invalid'", ""),
        ("jig-doctor-invalid", "error: invalid value for ssl_mode"),
    ] {
        assert!(matches!(
            classify_sqlx_driver_probe(SqlxDriver::Postgres, false, stdout, stderr,),
            SqlxDriverProbe::Indeterminate(_)
        ));
    }
}
#[cfg(unix)]
#[test]
fn sqlx_driver_probe_invokes_shim_safely_and_times_out() {
    let _env = lock_env();
    let _secret = EnvVarGuard::set("JIG_DOCTOR_TEST_SECRET", "must-not-be-inherited");
    let _database_url = EnvVarGuard::set("DATABASE_URL", "postgres://must-not-be-inherited");
    let temp = tempdir().unwrap();
    let supported = temp.path().join("cargo-sqlx-supported");
    write_test_executable(
        &supported,
        "#!/bin/sh\n[ -z \"${JIG_DOCTOR_TEST_SECRET+x}\" ] || exit 8\n[ -z \"${DATABASE_URL+x}\" ] || exit 8\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 8\n[ \"$HOME\" = \"$TMP\" ] || exit 8\n[ \"$HOME\" = \"$TEMP\" ] || exit 8\n[ \"$LC_ALL\" = C ] || exit 8\n[ \"$NO_COLOR\" = 1 ] || exit 8\n[ \"$1\" = sqlx ] || exit 9\nprintf '%s\\n' 'error: unknown value \"jig-doctor-invalid\" for ssl_mode'\nexit 1\n",
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &supported,
            SqlxProbeStyle::CargoSubcommand,
            SqlxDriver::Postgres,
            Duration::from_secs(1)
        ),
        SqlxDriverProbe::Compatible
    );

    let direct = temp.path().join("sqlx-supported");
    write_test_executable(
        &direct,
        "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nexit 0\n",
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &direct,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(1),
        ),
        SqlxDriverProbe::Compatible
    );

    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let unrelated = tempdir().unwrap();
    let tools = fs::canonicalize(tools.path()).unwrap();
    let path_limited = tools.join("sqlx-path-limited");
    write_test_executable(
        &path_limited,
        &format!(
            "#!/bin/sh\n[ \"$PATH\" = '{}' ] || exit 8\nexit 0\n",
            tools.display()
        ),
    );
    let broad_path = env::join_paths([tools.as_path(), unrelated.path()]).unwrap();
    assert_eq!(
        probe_sqlx_driver_with_timeout_and_environment(
            &path_limited,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(1),
            repo.path(),
            &DoctorEnvironment {
                search_path: Some(broad_path),
                ..DoctorEnvironment::default()
            },
        ),
        SqlxDriverProbe::Compatible
    );

    let hanging = temp.path().join("cargo-sqlx-hanging");
    write_test_executable(&hanging, "#!/bin/sh\nwhile :; do :; done\n");
    assert!(matches!(
        probe_sqlx_driver_with_timeout(
            &hanging,
            SqlxProbeStyle::CargoSubcommand,
            SqlxDriver::Sqlite,
            Duration::from_millis(20)
        ),
        SqlxDriverProbe::Indeterminate(reason) if reason.contains("timed out")
    ));

    let noisy = temp.path().join("cargo-sqlx-noisy");
    write_test_executable(
        &noisy,
        "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nexit 0\n",
    );
    let noisy_probe = probe_sqlx_driver_with_timeout(
        &noisy,
        SqlxProbeStyle::CargoSubcommand,
        SqlxDriver::Sqlite,
        Duration::from_secs(2),
    );
    assert!(
        matches!(
            &noisy_probe,
            SqlxDriverProbe::Indeterminate(reason) if reason.contains("capture limit")
        ),
        "unexpected noisy probe result: {noisy_probe:?}"
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sqlx_driver_probe_reaps_descendants_on_completion_and_timeout() {
    let temp = tempdir().unwrap();

    let completed_marker = temp.path().join("completed-descendant");
    let completed = temp.path().join("sqlx-completed");
    write_test_executable(
        &completed,
        &owned_test_descendant_script(&completed_marker, "exit 0"),
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &completed,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(2),
        ),
        SqlxDriverProbe::Compatible
    );
    let completed_descendant = read_test_process_identity(&completed_marker);

    let timeout_marker = temp.path().join("timeout-descendant");
    let hanging = temp.path().join("sqlx-timeout-tree");
    write_test_executable(
        &hanging,
        &owned_test_descendant_script(&timeout_marker, "while :; do :; done"),
    );
    let timeout_probe = probe_sqlx_driver_with_timeout(
        &hanging,
        SqlxProbeStyle::Direct,
        SqlxDriver::Sqlite,
        Duration::from_millis(300),
    );
    assert!(
        matches!(
            &timeout_probe,
            SqlxDriverProbe::Indeterminate(reason) if reason == "the driver probe timed out"
        ),
        "unexpected timeout probe result: {timeout_probe:?}"
    );
    let timeout_descendant = read_test_process_identity(&timeout_marker);

    for descendant in [completed_descendant, timeout_descendant] {
        assert_test_process_stopped(&descendant);
    }
}
#[cfg(unix)]
// The scoped signal session must remain active until the explicit finish path
// restores handlers and re-delivers any recorded signal.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_driver_probe_sigint_helper() {
    let Some(executable) = std::env::var_os("JIG_SQLX_PROBE_SIGINT_HELPER") else {
        return;
    };
    let signal_session = SignalSession::start().unwrap();
    let cancelled = || signal_session.cancelled();
    let result = probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        Path::new(&executable),
        SqlxProbeStyle::Direct,
        SqlxDriver::Sqlite,
        Duration::from_secs(30),
        Path::new("/"),
        &DoctorEnvironment::default(),
        Some(&cancelled),
    );
    let _ = finish_signal_session(signal_session);
    panic!("SIGINT was not re-delivered after probe cleanup: {result:?}");
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sqlx_driver_probe_sigint_reaps_descendants_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    let descendant_marker = temp.path().join("probe-descendant");
    let probe = temp.path().join("sqlx-sigint-tree");
    write_test_executable(
        &probe,
        &owned_test_descendant_script(&descendant_marker, "while :; do :; done"),
    );
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::sqlx_driver_probe::sqlx_driver_probe_sigint_helper",
            "--nocapture",
        ])
        .env("JIG_SQLX_PROBE_SIGINT_HELPER", &probe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: the test owns this live helper PID and sends a standard
    // termination signal solely to that subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("SIGINT helper did not terminate after probe cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}
