// Shared by separate integration-test binaries, each using a subset of helpers.
#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::json;
use tempfile::TempDir;

const WATCHDOG: Duration = Duration::from_secs(90);

pub struct Fixture {
    _temp: TempDir,
    pub root: PathBuf,
    pub signals: PathBuf,
    artifacts: PathBuf,
}

impl Fixture {
    pub fn new(coordinated: bool, timeout: u64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("example-owner");
        let signals = temp.path().join("signals");
        let artifacts = temp.path().join("artifacts");
        fs::create_dir(&signals).unwrap();
        fs::create_dir(&artifacts).unwrap();
        write_repo(&root, &signals, &artifacts, coordinated, timeout);
        Self {
            _temp: temp,
            root,
            signals,
            artifacts,
        }
    }

    pub fn other_repository(&self, name: &str, timeout: u64, alias: bool) -> PathBuf {
        let root = self._temp.path().join(name);
        let artifact_path = if alias {
            let alias_path = self._temp.path().join("artifact-alias");
            std::os::unix::fs::symlink(&self.artifacts, &alias_path).unwrap();
            alias_path
        } else {
            self.artifacts.clone()
        };
        write_repo(&root, &self.signals, &artifact_path, true, timeout);
        root
    }

    pub fn spawn(&self, id: &str, extra: &[&str]) -> Running {
        self.spawn_in(&self.root, id, extra)
    }

    pub fn spawn_in(&self, root: &Path, id: &str, extra: &[&str]) -> Running {
        let mut args = vec!["check", "example:test"];
        args.extend_from_slice(extra);
        self.spawn_args_in(root, id, &args)
    }

    pub fn spawn_args(&self, id: &str, args: &[&str]) -> Running {
        self.spawn_args_in(&self.root, id, args)
    }

    fn spawn_args_in(&self, root: &Path, id: &str, args: &[&str]) -> Running {
        self.spawn_command(root, id, jig(root).args(args))
    }

    pub fn spawn_command(&self, root: &Path, id: &str, command: &mut Command) -> Running {
        let output = self.signals.join(format!("{id}.json"));
        let stderr = self.signals.join(format!("{id}.stderr"));
        let child = command
            .env("EXAMPLE_RUN_ID", id)
            .stdout(fs::File::create(&output).unwrap())
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        Running {
            child,
            root: root.to_path_buf(),
            signals: self.signals.clone(),
            id: id.into(),
            output,
            stderr,
            finished: false,
        }
    }

    pub fn launches(&self) -> String {
        fs::read_to_string(self.signals.join("launches")).unwrap_or_default()
    }
}

pub fn jig(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .env("NO_COLOR", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD");
    command
}

fn write_repo(root: &Path, signals: &Path, artifacts: &Path, coordinated: bool, timeout: u64) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join(".fixture-cargo-home")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "compile_error!(\"resource metadata must not compile\");\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"example-resource\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"example-resource\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join(".gitignore"),
        ".agent/state/\n.agent/plans/\n.agent/.cache/\n.fixture-cargo-home/\n",
    )
    .unwrap();
    let mut action = json!({
        "target":{"component":"example","action":"test"}, "intent":"check",
        "effects":["read_only","process"], "inputs":["src/**"], "inputs_policy":"exhaustive",
        "timeout_seconds":timeout,
        "runner":{"kind":"shell","command":"example_check_command", "environment":{
            "CARGO_HOME":root.join(".fixture-cargo-home"), "CARGO_TARGET_DIR":artifacts,
            "CARGO_BUILD_BUILD_DIR":artifacts, "EXAMPLE_BARRIER_ROOT":signals,
            "EXAMPLE_ASSERT_EXCLUSIVE":if coordinated { "1" } else { "0" },
            "EXAMPLE_RESOURCE_GROUP":"shared"
        }}
    });
    if coordinated {
        action["resources"] = json!([{"kind":"cargo_v1","workspace_manifest":"Cargo.toml"}]);
    }
    let components = json!([{"id":"example","root":".","adapters":["rust"]}]);
    let profiles = json!([{"id":"verify","targets":[{"component":"example","action":"test"}]}]);
    let command = r#"set -eu
if [ "$EXAMPLE_ASSERT_EXCLUSIVE" = 1 ]; then
  if ! mkdir "$EXAMPLE_BARRIER_ROOT/critical-$EXAMPLE_RESOURCE_GROUP" 2>/dev/null; then
    touch "$EXAMPLE_BARRIER_ROOT/overlap"
    exit 90
  fi
  trap 'rmdir "$EXAMPLE_BARRIER_ROOT/critical-$EXAMPLE_RESOURCE_GROUP"' EXIT
fi
printf '%s\n' "$EXAMPLE_RUN_ID" >> "$EXAMPLE_BARRIER_ROOT/launches"
touch "$EXAMPLE_BARRIER_ROOT/entered-$EXAMPLE_RUN_ID"
while [ ! -f "$EXAMPLE_BARRIER_ROOT/release-$EXAMPLE_RUN_ID" ]; do sleep 0.02; done
"#;
    let config = json!({
        "_src_path":"/tmp/template", "_commit":"abc123", "repo_name":"ExampleResourceProject", "default_branch":"main",
        "commands":{"example_check_command":command},
        "repository":{"components":components,"actions":[action],"profiles":profiles,"default_check_profile":"verify"},
        "work":{"gates":[{"id":"full","kind":"evidence","profile":"verify"}]}
    });
    fs::write(
        root.join(".jig.toml"),
        toml::to_string(&toml::Value::try_from(&config).unwrap()).unwrap(),
    )
    .unwrap();
    let manifest = json!({
        "contract_version":8,"tool_namespace":"jig","required_commands":["example_check_command"],"tools":[],
        "components":components,"actions":[action],"profiles":profiles,"default_check_profile":"verify"
    });
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Example Agent",
            "-c",
            "user.email=example@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "Example fixture",
        ],
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub struct Running {
    child: Child,
    root: PathBuf,
    signals: PathBuf,
    id: String,
    output: PathBuf,
    stderr: PathBuf,
    finished: bool,
}

impl Running {
    pub fn entered(&self) -> bool {
        self.signals.join(format!("entered-{}", self.id)).exists()
    }
    pub fn release(&self) {
        fs::write(
            self.signals.join(format!("release-{}", self.id)),
            "release\n",
        )
        .unwrap();
    }
    pub fn running(&mut self) -> bool {
        let running = self.child.try_wait().unwrap().is_none();
        if !running {
            // try_wait reaps a completed process. Drop must never signal the
            // former PID once it can be reused by an unrelated process.
            self.finished = true;
        }
        running
    }
    pub fn output(&self) -> String {
        fs::read_to_string(&self.output).unwrap_or_default()
    }
    pub fn assert_single_completion(&self, conclusion: &str) {
        let events = fs::read_to_string(self.root.join(".agent/state/runs.jsonl")).unwrap();
        let completions = events
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|event| event["event"] == "completed")
            .collect::<Vec<_>>();
        assert_eq!(
            completions.len(),
            1,
            "this fixture repository must contain exactly this process's completed run: {events}"
        );
        assert_eq!(completions[0]["conclusion"], conclusion, "{events}");
    }
    fn diagnostics(&self) -> String {
        format!(
            "{}\n{}",
            self.output(),
            fs::read_to_string(&self.stderr).unwrap_or_default()
        )
    }
    pub fn wait_entered(&mut self) {
        self.wait_for(|this| this.entered(), "child barrier");
    }
    pub fn wait_named_entry(&mut self, id: &str) {
        self.wait_for(
            |this| this.signals.join(format!("entered-{id}")).exists(),
            "named child barrier",
        );
    }
    pub fn wait_resource_notice(&mut self) {
        self.wait_for(
            |this| {
                fs::read_to_string(&this.stderr)
                    .unwrap_or_default()
                    .contains("Waiting for a Cargo build resource")
            },
            "resource wait notice",
        );
    }
    pub fn wait_any_entry(&mut self, ids: &[&str]) -> String {
        self.wait_for(
            |this| {
                ids.iter()
                    .any(|id| this.signals.join(format!("entered-{id}")).exists())
            },
            "any named child barrier",
        );
        ids.iter()
            .find(|id| self.signals.join(format!("entered-{id}")).exists())
            .unwrap()
            .to_string()
    }
    pub fn wait_signal(&mut self, name: &str) {
        self.wait_for(|this| this.signals.join(name).exists(), "fixture signal");
    }
    pub fn wait_target_publication(&mut self, action: &str) {
        self.wait_for(
            |this| {
                fs::read_to_string(this.root.join(".agent/state/runs.jsonl"))
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .any(|event| {
                        event["event"] == "target_completed"
                            && event["target"]["action"] == action
                            && event["result"]["receipt_id"].is_string()
                    })
            },
            "target result publication",
        );
    }
    fn wait_for(&mut self, ready: impl Fn(&Self) -> bool, label: &str) {
        let start = Instant::now();
        while !ready(self) {
            assert!(
                self.running(),
                "{} exited before {label}: {}",
                self.id,
                self.diagnostics()
            );
            assert!(
                start.elapsed() < WATCHDOG,
                "{} did not reach {label}: {}",
                self.id,
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn cancel(&self) {
        self.send_signal(libc::SIGTERM);
    }
    pub fn interrupt(&self) {
        self.send_signal(libc::SIGINT);
    }
    fn send_signal(&self, signal: libc::c_int) {
        // SAFETY: the live Child retains ownership of this PID until reaped.
        assert_eq!(
            unsafe { libc::kill(self.child.id() as libc::pid_t, signal) },
            0
        );
    }
    fn finish(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.finished = true;
                return status;
            }
            assert!(
                start.elapsed() < WATCHDOG,
                "{} did not complete: {}",
                self.id,
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn finish_success(&mut self) {
        assert!(self.finish().success(), "{}", self.diagnostics());
    }
    pub fn finish_failure(&mut self) {
        assert!(!self.finish().success(), "{}", self.diagnostics());
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = fs::write(
            self.signals.join(format!("release-{}", self.id)),
            "cleanup\n",
        );
        // SAFETY: an unreaped Child still owns its PID. Graceful termination
        // lets Jig clean its owned tree even after a fixture assertion fails.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
