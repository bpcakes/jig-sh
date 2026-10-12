#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use std::fs;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use std::path::PathBuf;
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use std::process::Command;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Stdio;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use jig_context::RepoContext;
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use tempfile::tempdir;
#[cfg(unix)]
use wait_timeout::ChildExt;

#[cfg(unix)]
use super::support::cargo_sqlx_program;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::support::{
    OWNED_PROCESS_DESCENDANT_MARKER_ENV, shell_quote_test_path, write_doctor_fixture,
};
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use super::support::{write_sqlx_doctor_fixture_with_command, write_test_executable};
#[cfg(unix)]
use crate::doctor::check::check;
#[cfg(any(target_os = "linux", target_os = "macos", unix))]
use crate::doctor::context_checks::doctor_context_checks;
#[cfg(unix)]
use crate::doctor::context_checks::{
    DoctorContextChecks, go_runtime_probe_required, mark_doctor_signal_retirement_failure,
};
#[cfg(unix)]
use crate::test_env::TestRepoBuilder;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{
    TestProcessIdentity, assert_test_process_stopped, publish_test_process_identity,
    read_test_process_identity,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_sqlx_probe_sigint_helper() {
    let Some(marker) = std::env::var_os("JIG_DOCTOR_SQLX_PRODUCTION_MARKER") else {
        return;
    };
    let identity = TestProcessIdentity::capture_current().unwrap();
    publish_test_process_identity(Path::new(&marker), &identity);
    std::thread::sleep(Duration::from_secs(30));
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_sqlx_sigint_sequence_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_SQLX_SEQUENCE_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after SQLx cleanup: {result:?}");
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_during_production_sqlx_prevents_codex_and_proxy_spawns() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "sqlx prepare -D sqlite:production-signal.db",
    );
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();

    let probe_marker = temp.path().join("sqlx-production-probe");
    let tools = tempdir().unwrap();
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nJIG_DOCTOR_SQLX_PRODUCTION_MARKER={} exec {} --exact doctor::tests::context_checks::production_sqlx_probe_sigint_helper --nocapture\n",
            shell_quote_test_path(&probe_marker),
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let codex_marker = temp.path().join("codex-started");
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nprintf c > '{}'\nexit 0\n",
            codex_marker.display()
        ),
    );
    let proxy_marker = temp.path().join("proxy-started");
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            proxy_marker.display()
        ),
    );

    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::context_checks::doctor_sqlx_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_SQLX_SEQUENCE_ROOT", temp.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env("PATH", fs::canonicalize(tools.path()).unwrap())
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let probe = read_test_process_identity(&probe_marker);
    // SAFETY: this test owns the isolated doctor helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("doctor helper did not terminate after SQLx cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&probe);
    assert!(
        !codex_marker.exists(),
        "Codex started after SQLx cancellation"
    );
    assert!(
        !proxy_marker.exists(),
        "proxy started after SQLx cancellation"
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_codex_probe_helper() {
    let Some(marker) = std::env::var_os("JIG_DOCTOR_CODEX_DESCENDANT_MARKER") else {
        return;
    };
    for _ in 0..2_000 {
        println!("codex-probe-secret-that-must-not-leak");
        eprintln!("codex-probe-secret-that-must-not-leak");
    }
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
    let _ = read_test_process_identity(Path::new(&marker));
    std::thread::sleep(Duration::from_secs(30));
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_codex_sigint_sequence_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_CODEX_SEQUENCE_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after Codex cleanup: {result:?}");
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_during_noisy_codex_reaps_descendant_and_prevents_proxy_spawn() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::context_checks::production_codex_probe_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let proxy_marker = temp.path().join("proxy-started");
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            proxy_marker.display()
        ),
    );
    let descendant_marker = temp.path().join("codex-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::context_checks::doctor_codex_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_CODEX_SEQUENCE_ROOT", temp.path())
        .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the isolated doctor helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("doctor helper did not terminate after Codex cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
    assert!(
        !proxy_marker.exists(),
        "proxy started after Codex cancellation"
    );
}
#[cfg(unix)]
#[test]
fn doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches() {
    const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BATCH_HELPER";
    if let Some(root) = std::env::var_os(HELPER) {
        let root = PathBuf::from(root);
        let ctx = RepoContext::load_from_root(root.clone()).unwrap();
        let first = doctor_context_checks(&ctx);
        assert!(first.required_tools.ok, "{}", first.required_tools.detail);
        assert_eq!(
            cargo_sqlx_program(&first.required_tools)["driver_probe"]["status"],
            "compatible"
        );
        assert_eq!(first.agent.status, "missing", "{}", first.agent.detail);
        assert_eq!(first.agent.data["codex"]["available"], true);
        assert_eq!(first.proxy.status, "not running", "{}", first.proxy.detail);
        assert_eq!(
            fs::read_to_string(root.join("probe-count")).unwrap(),
            "dckp"
        );

        let second = doctor_context_checks(&ctx);
        assert!(second.required_tools.ok, "{}", second.required_tools.detail);
        assert_eq!(second.required_tools.status, "present");
        assert_eq!(
            cargo_sqlx_program(&second.required_tools)["driver_probe"]["status"],
            "compatible"
        );
        assert_eq!(second.agent.status, "missing", "{}", second.agent.detail);
        assert_eq!(second.agent.data["codex"]["available"], true);
        assert_eq!(
            second.proxy.status, "not running",
            "{}",
            second.proxy.detail
        );
        assert_eq!(
            fs::read_to_string(root.join("probe-count")).unwrap(),
            "dckpdckp"
        );
        return;
    }

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "sqlx prepare -D sqlite:reusable.db && cargo-sqlx sqlx prepare -D sqlite:reusable.db",
    );
    let tools = tempdir().unwrap();
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nprintf d >> '{}'\nexit 0\n",
            temp.path().join("probe-count").display()
        ),
    );
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf c >> '{}'\nexit 0\n",
            temp.path().join("probe-count").display()
        ),
    );
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
        ),
    )
    .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    fs::write(
            temp.path().join(".jig.toml"),
            fs::read_to_string(temp.path().join(".jig.toml"))
                .unwrap()
                .replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                ),
        )
        .unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nprintf k >> '{}'\n[ \"$*\" = \"plugin marketplace add --help\" ]\n",
            temp.path().join("probe-count").display()
        ),
    );
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p >> '{}'\nprintf '%s\\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            temp.path().join("probe-count").display()
        ),
    );
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::context_checks::doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches",
            "--nocapture",
        ])
        .env(HELPER, temp.path())
        .env("PATH", fs::canonicalize(tools.path()).unwrap())
        .env("JIG_CODEX_BIN", codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "reusable batch helper exited with {status}"
    );
}
#[cfg(unix)]
#[test]
fn signal_retirement_failure_marks_process_dependent_checks_unverified() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:retirement.db");
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let mut checks = DoctorContextChecks {
        required_tools: check(
            "required_tools",
            "Required tools",
            true,
            true,
            "present",
            "present",
        ),
        rust_runtime: Some(check(
            "rust_runtime",
            "Rust runtime",
            true,
            true,
            "compatible",
            "compatible",
        )),
        go_runtime: None,
        node_runtime: Some(check(
            "node_runtime",
            "Node runtime",
            true,
            true,
            "compatible",
            "compatible",
        )),
        sqlx_cli: Some(check(
            "sqlx_cli",
            "SQLx CLI",
            true,
            true,
            "compatible",
            "compatible",
        )),
        agent: check(
            "agent_skills",
            "Agent skills",
            false,
            true,
            "installed",
            "installed",
        ),
        proxy: check("proxy", "Dev proxy", false, true, "running", "running"),
    };

    mark_doctor_signal_retirement_failure(&ctx, &mut checks);

    assert_eq!(checks.required_tools.status, "present_unverified");
    assert!(
        checks
            .required_tools
            .detail
            .contains("could not retire safely")
    );
    let node_runtime = checks.node_runtime.as_ref().unwrap();
    assert!(!node_runtime.ok);
    assert_eq!(node_runtime.status, "unverified");
    assert!(node_runtime.detail.contains("could not retire safely"));
    for runtime in [
        checks.rust_runtime.as_ref().unwrap(),
        checks.sqlx_cli.as_ref().unwrap(),
    ] {
        assert!(!runtime.ok);
        assert_eq!(runtime.status, "unverified");
        assert!(runtime.detail.contains("could not retire safely"));
    }
    for process_check in [&checks.agent, &checks.proxy] {
        assert!(!process_check.ok);
        assert_eq!(process_check.status, "error");
        assert!(process_check.detail.contains("could not retire safely"));
    }
}
#[cfg(unix)]
#[test]
fn go_module_requires_a_supervised_doctor_process_session() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config("backend_language = \"go\"")
        .write();
    fs::write(
        temp.path().join("go.mod"),
        "module example.com/doctor-fixture\n\ngo 1.24\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert!(go_runtime_probe_required(&ctx));
}
