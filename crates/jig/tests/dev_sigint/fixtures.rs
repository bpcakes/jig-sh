use super::*;

pub(super) fn write_repo_fixture(root: &Path) {
    write_repo_fixture_with_proxy(root, false);
}

pub(super) fn write_preflight_signal_fixture(root: &Path) {
    write_repo_fixture(root);
    let test_exe = serde_json::to_string(&std::env::current_exe().expect("resolve test binary"))
        .expect("quote test binary path");
    fs::create_dir_all(root.join("scripts")).expect("create scripts directory");
    fs::create_dir_all(root.join("web")).expect("create frontend directory");
    fs::write(
        root.join("scripts/check-webapps.sh"),
        r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" != "dependencies-ready" ] || [ "$#" -ne 2 ]; then
  exit 2
fi
sh -c 'trap "" INT HUP TERM; while :; do sleep 1; done' &
descendant="$!"
marker_tmp="$JIG_PREFLIGHT_SIGNAL_READY.tmp.$$"
printf '%s %s\n' "$$" "$descendant" > "$marker_tmp"
mv "$marker_tmp" "$JIG_PREFLIGHT_SIGNAL_READY"
while :; do sleep 1; done
"#,
    )
    .expect("write hanging dependency checker");
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/jig-dev-signal-template"
_commit = "test"
repo_name = "signal-test"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "true"

[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80

[dev]

[[dev.apps]]
name = "web"
kind = "env-port"
dir = "web"
argv = [{test_exe}, "--exact", "env_port_helper", "--nocapture"]
host = "127.0.0.1"
proxy = false

[agent_tooling.codex]
marketplaces = []
"#,
            env!("CARGO_PKG_VERSION"),
        ),
    )
    .expect("write preflight Jig config");
}

pub(super) fn wait_for_route(path: &Path, child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if fs::read_to_string(path)
            .is_ok_and(|contents| contents.contains("signal-helper.signal-test.localhost"))
        {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect route publication") {
            panic!("jig dev exited with {status} before publishing its route");
        }
        let now = Instant::now();
        if now >= deadline {
            panic!("timed out waiting for route in {}", path.display());
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

pub(super) fn write_repo_fixture_with_proxy(root: &Path, proxy: bool) {
    let test_exe = serde_json::to_string(&std::env::current_exe().expect("resolve test binary"))
        .expect("quote test binary path");
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/jig-dev-signal-template"
_commit = "test"
repo_name = "signal-test"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "true"

[dev]

[[dev.apps]]
name = "signal-helper"
kind = "env-port"
dir = "."
argv = [{test_exe}, "--exact", "env_port_helper", "--nocapture"]
host = "127.0.0.1"
proxy = {proxy}

[agent_tooling.codex]
marketplaces = []
"#,
            env!("CARGO_PKG_VERSION"),
        ),
    )
    .expect("write Jig config");
    fs::create_dir(root.join(".agent")).expect("create agent directory");
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": env!("CARGO_PKG_VERSION"),
            "required_commands": ["bootstrap_command"],
            "tools": [{
                "name": "jig.bootstrap",
                "kind": "command",
                "description": "Run the configured project bootstrap command.",
                "command": "bootstrap_command",
            }],
        }))
        .expect("serialize Jig contract"),
    )
    .expect("write Jig contract");
}

pub(super) fn wait_for_file(path: &Path, child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect jig dev readiness") {
            panic!(
                "jig dev exited with {status} before publishing {}",
                path.display()
            );
        }
        let now = Instant::now();
        if now >= deadline {
            panic!("timed out waiting for {}", path.display());
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

pub(super) fn wait_for_file_with_output(
    path: &Path,
    child: &mut Child,
    timeout: Duration,
    stdout: &Path,
    stderr: &Path,
) {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect jig dev readiness") {
            panic!(
                "jig dev exited with {status} before publishing {}\nstdout:\n{}\nstderr:\n{}",
                path.display(),
                fs::read_to_string(stdout).unwrap_or_default(),
                fs::read_to_string(stderr).unwrap_or_default()
            );
        }
        let now = Instant::now();
        if now >= deadline {
            panic!(
                "timed out waiting for {}\nstdout:\n{}\nstderr:\n{}",
                path.display(),
                fs::read_to_string(stdout).unwrap_or_default(),
                fs::read_to_string(stderr).unwrap_or_default()
            );
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}
