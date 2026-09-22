use super::*;

#[test]
fn custom_tld_alias_allows_fresh_https_proxy_run() {
    check_fresh_https_launch(false);
}

#[test]
fn custom_tld_alias_allows_fresh_https_dev() {
    check_fresh_https_launch(true);
}

fn check_fresh_https_launch(dev: bool) {
    let _guard = LIFECYCLE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let state = temp.path().join("proxy-state");
    fs::create_dir(&repo).unwrap();
    write_repo_fixture(&repo, "ExampleProject");
    // Reserve distinct ephemeral ports together, then release immediately before launch.
    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let https = TcpListener::bind("127.0.0.1:0").unwrap();
    let config_path = repo.join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "[dev]",
            &format!(
                "[dev]\ntld = \"test\"\nhttps = true\nproxy_port = {}\nhttps_port = {}",
                http.local_addr().unwrap().port(),
                https.local_addr().unwrap().port()
            ),
        )
        .replace("proxy = false", "proxy = true");
    fs::write(config_path, config).unwrap();
    // Alias creation deliberately leaves certificates absent. Startup must carry
    // this repository's narrow DNS scope into the contextless detached daemon.
    run_json(
        &repo,
        [
            "proxy",
            "alias",
            "existing",
            "--port",
            "19090",
            "--state-dir",
        ],
        Some(&state),
    );
    assert!(!state.join("ca.pem").exists());
    let proxy_cleanup = ProxyCleanup {
        repo: &repo,
        state: &state,
    };
    let ready = temp.path().join("app-ready");
    let stdout = repo.join("app.stdout");
    let stderr = repo.join("app.stderr");
    let mut command = base_command(&repo);
    if dev {
        command.arg("dev");
    } else {
        command.args(["proxy", "run", "web"]);
    }
    command.arg("--state-dir").arg(&state);
    if !dev {
        command
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "lifecycle_env_port_helper", "--nocapture"]);
    }
    drop((http, https));
    let child = command
        .process_group(0)
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready)
        .stdin(Stdio::null())
        .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
        .spawn()
        .unwrap();
    let mut app = ForegroundDev {
        child,
        stdout,
        stderr,
        armed: true,
    };
    app.wait_until_ready(&ready);
    let status = run_json(&repo, ["proxy", "list", "--state-dir"], Some(&state));
    assert_eq!(status["running"], true, "{status}");
    assert!(state.join("ca.pem").exists());
    assert!(state.join("leaf.pem").exists());
    if dev {
        wait_for_running_status(&repo, &state);
        run_json(&repo, ["dev", "stop", "--state-dir"], Some(&state));
        app.wait_for_success("HTTPS development session stop");
    }
    // Drop retains exact child ownership for ad-hoc run cleanup, before proxy shutdown.
    drop(app);
    drop(proxy_cleanup);
}

struct ProxyCleanup<'a> {
    repo: &'a Path,
    state: &'a Path,
}

impl Drop for ProxyCleanup<'_> {
    fn drop(&mut self) {
        let output = base_command(self.repo)
            .args(["proxy", "stop", "--state-dir"])
            .arg(self.state)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
