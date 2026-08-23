use std::net::TcpListener;
use std::path::PathBuf;

use super::*;
use crate::test_tempdir as tempdir;

fn write_process_test_marker(path: &Path, contents: &str) {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, contents).unwrap();
    fs::rename(temporary, path).unwrap();
}

fn wait_for_process_test_marker(path: &Path, child: &mut Child, label: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                return contents;
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("{label} exited before publishing its marker: {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{label} did not publish its marker");
}

#[test]
fn startup_output_disposition_prints_failures_and_discards_interruptions() {
    const HELPER_ENV: &str = "JIG_STARTUP_OUTPUT_DISPOSITION_HELPER";
    const TEST_NAME: &str = "processes::startup_failure_tests::startup_output_disposition_prints_failures_and_discards_interruptions";
    const CAPTURED_TEXT: &str = "jig-unique-startup-failure-output";

    if let Some(disposition) = std::env::var_os(HELPER_ENV) {
        let temp = crate::test_tempdir().unwrap();
        let settings = ProxySettings::default();
        let spec = AppRunSpec {
            name: "startup-output".into(),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(Vec::new()),
            kind: AppKind::EnvPort,
            hostname: "startup-output.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: false,
        };
        let argv = [
            "sh".to_string(),
            "-c".to_string(),
            format!("printf '{CAPTURED_TEXT}\\n'"),
        ];
        let mut spawned = spawn_child(&spec, &argv, 4321, &settings, &[]).unwrap();
        assert!(spawned.child.wait().unwrap().success());
        let disposition = if disposition == "failure" {
            StartupOutputDisposition::Failure
        } else {
            StartupOutputDisposition::Interrupted
        };
        spawned.output.finish_start_failure(disposition);
        return;
    }

    let run_helper = |disposition: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(HELPER_ENV, disposition)
            .output()
            .unwrap()
    };

    let failure = run_helper("failure");
    assert!(failure.status.success());
    let failure_stderr = String::from_utf8_lossy(&failure.stderr);
    assert!(failure_stderr.contains(CAPTURED_TEXT));
    assert!(failure_stderr.contains("--- output from app 'startup-output' ---"));

    let interruption = run_helper("interrupted");
    assert!(interruption.status.success());
    assert!(!String::from_utf8_lossy(&interruption.stderr).contains(CAPTURED_TEXT));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn route_publication_prefers_child_exit_during_final_owner_verification() {
    let _guard = termination_test_guard();
    struct ChildCleanup(Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            terminate_and_reap_logged(&mut self.0, "test cleanup failed");
        }
    }

    let temp = tempdir().unwrap();
    let exit = temp.path().join("exit-app");
    let store = StateStore::resolve(Some(temp.path().join("state"))).unwrap();
    let spec = AppRunSpec::new(
        "post-bind-failure",
        temp.path().to_path_buf(),
        CommandSpec::Argv(Vec::new()),
        "post-bind-failure.localhost",
    );
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "while [ ! -e \"$1\" ]; do sleep 0.01; done; exit 29",
        "sh",
        exit.to_str().unwrap(),
    ]);
    configure_app_child_process_group(&mut command);
    let mut child = ChildCleanup(command.spawn().unwrap());
    let child_pid = child.0.id();
    let owner_start_token = process_start_token(child_pid).unwrap();
    let route = Route {
        hostname: RouteHostname::new("post-bind-failure.localhost").unwrap(),
        target_host: "127.0.0.1".into(),
        target_port: 43_211,
        owner_pid: Some(child_pid),
        owner_start_token: Some(owner_start_token),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };
    let mut verification_calls = 0;

    let error = publish_process_route_interruptible_with_verifier(
        &store,
        route,
        &spec.name,
        &mut child.0,
        &|| None,
        |_, _, child| {
            verification_calls += 1;
            if verification_calls == 1 {
                return Ok(());
            }
            fs::write(&exit, b"exit\n").unwrap();
            let status = child.wait().unwrap();
            assert_eq!(child_exit_status(&status), 29);
            Err(anyhow::anyhow!(
                "synthetic final owner verification failure"
            ))
        },
    )
    .unwrap_err();

    assert_eq!(verification_calls, 2);
    assert!(
        error
            .to_string()
            .contains("App 'post-bind-failure' exited with status 29 while listener ownership")
    );
    assert!(
        format!("{error:#}").contains("synthetic final owner verification failure"),
        "the final ownership failure should remain available as diagnostic context"
    );
    assert!(
        !format!("{error:#}").contains("route was not published"),
        "listener observation cannot claim durable rollback succeeded"
    );
    assert!(
        store.read_routes(false).unwrap().is_empty(),
        "the failed final verification must roll back the route"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn route_publication_rejects_child_exit_during_final_successful_owner_verification() {
    let _guard = termination_test_guard();
    struct ChildCleanup(Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            terminate_and_reap_logged(&mut self.0, "test cleanup failed");
        }
    }

    let temp = tempdir().unwrap();
    let exit = temp.path().join("exit-app");
    let store = StateStore::resolve(Some(temp.path().join("state"))).unwrap();
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "while [ ! -e \"$1\" ]; do sleep 0.01; done; exit 29",
        "sh",
        exit.to_str().unwrap(),
    ]);
    configure_app_child_process_group(&mut command);
    let mut child = ChildCleanup(command.spawn().unwrap());
    let child_pid = child.0.id();
    let owner_start_token = process_start_token(child_pid).unwrap();
    let route = Route {
        hostname: RouteHostname::new("post-bind-success.localhost").unwrap(),
        target_host: "127.0.0.1".into(),
        target_port: 43_215,
        owner_pid: Some(child_pid),
        owner_start_token: Some(owner_start_token),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };
    let mut verification_calls = 0;

    let error = publish_process_route_interruptible_with_verifier(
        &store,
        route,
        "post-bind-success",
        &mut child.0,
        &|| None,
        |_, _, child| {
            verification_calls += 1;
            if verification_calls == 1 {
                return Ok(());
            }
            fs::write(&exit, b"exit\n").unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            let status = loop {
                match try_wait_preserving_process_group(child).unwrap() {
                    Some(status) => break status,
                    None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                    None => {
                        panic!("child did not exit after final owner verification was triggered")
                    }
                }
            };
            assert_eq!(child_exit_status(&status), 29);
            Ok(())
        },
    )
    .expect_err("a child that exited during final verification cannot publish a route");

    assert_eq!(verification_calls, 2);
    assert!(
        error
            .to_string()
            .contains("App 'post-bind-success' exited with status 29 while listener ownership")
    );
    assert!(
        store.read_routes(false).unwrap().is_empty(),
        "the final live-child check must roll back the route"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn route_publication_preserves_interruption_from_final_owner_verification() {
    let _guard = termination_test_guard();
    struct ChildCleanup(Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            terminate_and_reap_logged(&mut self.0, "test cleanup failed");
        }
    }

    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("state"))).unwrap();
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    configure_app_child_process_group(&mut command);
    let mut child = ChildCleanup(command.spawn().unwrap());
    let child_pid = child.0.id();
    let owner_start_token = process_start_token(child_pid).unwrap();
    let route = Route {
        hostname: RouteHostname::new("interrupted-owner.localhost").unwrap(),
        target_host: "127.0.0.1".into(),
        target_port: 43_216,
        owner_pid: Some(child_pid),
        owner_start_token: Some(owner_start_token),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };
    let reason = TerminationReason::from_signal(libc::SIGINT);
    let mut verification_calls = 0;

    let error = publish_process_route_interruptible_with_verifier(
        &store,
        route,
        "interrupted-owner",
        &mut child.0,
        &|| None,
        |_, _, _| {
            verification_calls += 1;
            if verification_calls == 1 {
                Ok(())
            } else {
                Err(interruption_error(reason))
            }
        },
    )
    .expect_err("an interrupted final verification cannot publish a route");

    assert_eq!(verification_calls, 2);
    assert!(
        is_interruption(&error),
        "interruption was hidden: {error:#}"
    );
    assert_eq!(interruption_reason(&error), Some(reason));
    let interruption_message = format!("Interrupted by {}", reason.label());
    let rendered = format!("{error:#}");
    assert_eq!(
        rendered.matches(&interruption_message).count(),
        1,
        "interruption should render exactly once: {rendered}"
    );
    assert!(
        store.read_routes(false).unwrap().is_empty(),
        "the interrupted final verification must roll back the route"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn route_publication_rejects_an_owner_pid_different_from_the_supervised_child() {
    let _guard = termination_test_guard();
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("state"))).unwrap();
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let child_pid = child.id();
    let owner_start_token = process_start_token(child_pid).unwrap();
    let mismatched_pid = child_pid ^ 1;
    let route = Route {
        hostname: RouteHostname::new("pid-mismatch.localhost").unwrap(),
        target_host: "127.0.0.1".into(),
        target_port: 43_213,
        owner_pid: Some(mismatched_pid),
        owner_start_token: Some(owner_start_token),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };

    let error =
        publish_process_route_interruptible(&store, route, "pid-mismatch", &mut child, &|| None)
            .unwrap_err();

    assert!(error.to_string().contains("records owner PID"));
    assert!(error.to_string().contains(&child_pid.to_string()));
    assert!(store.read_routes(false).unwrap().is_empty());
    terminate_and_reap(&mut child).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn route_publication_verifier_receives_the_candidate_that_is_persisted() {
    let _guard = termination_test_guard();
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("state"))).unwrap();
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let child_pid = child.id();
    let owner_start_token = process_start_token(child_pid).unwrap();
    let route = Route {
        hostname: RouteHostname::new("candidate.localhost").unwrap(),
        target_host: "127.0.0.2".into(),
        target_port: 43_214,
        owner_pid: Some(child_pid),
        owner_start_token: Some(owner_start_token),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };
    let mut verification_calls = 0;

    publish_process_route_interruptible_with_verifier(
        &store,
        route,
        "candidate",
        &mut child,
        &|| None,
        |app_name, candidate, supervised_child| {
            verification_calls += 1;
            assert_eq!(app_name, "candidate");
            assert_eq!(candidate.target_host.as_str(), "127.0.0.2");
            assert_eq!(candidate.target_port, 43_214);
            assert_eq!(candidate.owner_pid, Some(supervised_child.id()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(verification_calls, 2);
    let routes = store.read_routes(false).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].target_host.as_str(), "127.0.0.2");
    assert_eq!(routes[0].target_port, 43_214);
    assert_eq!(routes[0].owner_pid, Some(child_pid));
    terminate_and_reap(&mut child).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn route_publication_does_not_relabel_store_errors_as_owner_failures() {
    let _guard = termination_test_guard();
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().join("state"))).unwrap();
    fs::write(store.root().join("routes.json"), b"{not json").unwrap();
    let spec = AppRunSpec::new(
        "store-error",
        temp.path().to_path_buf(),
        CommandSpec::Argv(Vec::new()),
        "store-error.localhost",
    );
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 5"]);
    configure_app_child_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let child_pid = child.id();
    let owner_start_token = process_start_token(child_pid).unwrap();
    let route = Route {
        hostname: RouteHostname::new("store-error.localhost").unwrap(),
        target_host: "127.0.0.1".into(),
        target_port: 43_212,
        owner_pid: Some(child_pid),
        owner_start_token: Some(owner_start_token),
        mode: RouteMode::Process,
        created_at_ms: now_ms(),
    };

    let error = publish_process_route_interruptible_with_verifier(
        &store,
        route,
        &spec.name,
        &mut child,
        &|| None,
        |_, _, _| panic!("owner verification should not run after a route-store read failure"),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Failed to parse Jig proxy routes")
    );
    assert!(!format!("{error:#}").contains("listener ownership"));
    terminate_and_reap(&mut child).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn app_readiness_prefers_child_exit_during_listener_verification() {
    const HELPER_ENV: &str = "JIG_APP_READINESS_EXIT_DURING_VERIFY_HELPER";
    const MARKER_ENV: &str = "JIG_APP_READINESS_EXIT_DURING_VERIFY_MARKER";
    const EXIT_ENV: &str = "JIG_APP_READINESS_EXIT_DURING_VERIFY_EXIT";
    const TEST_NAME: &str = "processes::startup_failure_tests::app_readiness_prefers_child_exit_during_listener_verification";

    if std::env::var_os(HELPER_ENV).is_some() {
        let marker = PathBuf::from(std::env::var_os(MARKER_ENV).expect("helper marker path"));
        let exit = PathBuf::from(std::env::var_os(EXIT_ENV).expect("helper exit path"));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        write_process_test_marker(
            &marker,
            &format!("{}\n", listener.local_addr().unwrap().port()),
        );
        while !exit.exists() {
            thread::sleep(Duration::from_millis(10));
        }
        drop(listener);
        std::process::exit(23);
    }

    let _guard = termination_test_guard();
    struct ChildCleanup(Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            terminate_and_reap_logged(&mut self.0, "test cleanup failed");
        }
    }

    let temp = tempdir().unwrap();
    let marker = temp.path().join("listener.marker");
    let exit = temp.path().join("exit-listener");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(HELPER_ENV, "1")
        .env(MARKER_ENV, &marker)
        .env(EXIT_ENV, &exit)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_app_child_process_group(&mut command);
    let mut child = ChildCleanup(command.spawn().unwrap());
    let port = wait_for_process_test_marker(&marker, &mut child.0, "listener helper")
        .trim()
        .parse()
        .unwrap();

    let error = wait_for_app_ready_with_timeout_and_test_verifier(
        "post-bind-failure",
        "127.0.0.1",
        port,
        &mut child.0,
        Duration::from_secs(2),
        |_, _, _, child, _| {
            fs::write(&exit, b"exit\n").unwrap();
            let status = child.wait().unwrap();
            assert_eq!(child_exit_status(&status), 23);
            Err(anyhow::anyhow!("synthetic listener verification failure"))
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("App 'post-bind-failure' exited with status 23 while listener ownership")
    );
    assert!(
        format!("{error:#}").contains("synthetic listener verification failure"),
        "the ownership failure should remain available as diagnostic context"
    );
}
