use super::cargo_fixture::{Fixture, Running, jig};
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

pub struct BrowserFixture {
    pub inner: Fixture,
    // Reserve unique endpoint numbers for the entire test. The instrumented
    // browser does not bind them; these reservations isolate machine-wide keys.
    ports: Vec<TcpListener>,
}

impl BrowserFixture {
    pub fn new() -> Self {
        let inner = Fixture::new(false, 60);
        let ports = (0..4)
            .map(|_| TcpListener::bind(("127.0.0.1", 0)).unwrap())
            .collect();
        fs::write(inner.signals.join("browser.cjs"), BROWSER).unwrap();
        let bin = inner.signals.join("bin");
        fs::create_dir(&bin).unwrap();
        let cargo = bin.join("cargo");
        fs::write(
            &cargo,
            "#!/bin/sh\nprintf 'unexpected Cargo probe\\n' >> \"$EXAMPLE_BARRIER_ROOT/cargo-probes\"\nexit 99\n",
        )
        .unwrap();
        fs::set_permissions(cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let fixture = Self { inner, ports };
        fixture.configure(&fixture.inner.root, &[fixture.action("test", [0, 1], "")]);
        fixture
    }

    pub fn port(&self, index: usize) -> u16 {
        self.ports[index].local_addr().unwrap().port()
    }

    pub fn action(&self, name: &str, ports: [usize; 2], base_url: &str) -> Value {
        let manifest = self.manifest(&self.inner.root);
        let mut action = manifest["actions"][0].clone();
        action["target"]["action"] = json!(name);
        action["resources"] = json!([{"kind":"playwright_servers_v1"}]);
        let environment = &mut action["runner"]["environment"];
        environment["E2E_WEB_PORT"] = json!(self.port(ports[0]).to_string());
        environment["E2E_API_PORT"] = json!(self.port(ports[1]).to_string());
        environment["E2E_BASE_URL"] = json!(base_url);
        environment["NODE"] = json!("node");
        environment["EXAMPLE_FIXTURE_SCRIPT"] = json!(self.inner.signals.join("browser.cjs"));
        let inherited = std::env::var_os("PATH").unwrap();
        let mut path = vec![self.inner.signals.join("bin")];
        path.extend(std::env::split_paths(&inherited));
        environment["PATH"] = json!(std::env::join_paths(path).unwrap().into_string().unwrap());
        action
    }

    pub fn other(&self, name: &str, action: Value) -> PathBuf {
        let root = self.inner.other_repository(name, 60, false);
        self.configure(&root, &[action]);
        root
    }

    pub fn configure(&self, root: &Path, actions: &[Value]) {
        let mut manifest = self.manifest(root);
        manifest["actions"] = json!(actions);
        manifest["profiles"][0]["targets"] = json!(
            actions
                .iter()
                .map(|action| &action["target"])
                .collect::<Vec<_>>()
        );
        let config_path = root.join(".jig.toml");
        let mut config: toml::Value =
            toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        config["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
        config["repository"]["profiles"] = toml::Value::try_from(&manifest["profiles"]).unwrap();
        config["commands"]["example_check_command"] =
            toml::Value::String("exec node \"$EXAMPLE_FIXTURE_SCRIPT\"".into());
        fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        commit(root);
    }

    fn manifest(&self, root: &Path) -> Value {
        serde_json::from_slice(&fs::read(root.join(".agent/jig-contract.json")).unwrap()).unwrap()
    }

    pub fn wait_notice(&self, run: &mut Running, id: &str) {
        let started = Instant::now();
        loop {
            let stderr = fs::read_to_string(self.inner.signals.join(format!("{id}.stderr")))
                .unwrap_or_default();
            if stderr.contains("Waiting for ") && stderr.contains("resource") {
                return;
            }
            assert!(
                run.running(),
                "waiter exited before resource admission: {stderr}"
            );
            assert!(started.elapsed() < Duration::from_secs(90), "{stderr}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn release(&self, id: &str) {
        fs::write(
            self.inner.signals.join(format!("release-{id}")),
            "release\n",
        )
        .unwrap();
    }

    pub fn assert_no_overlap_or_cargo(&self) {
        assert!(!self.inner.signals.join("overlap").exists());
        assert!(
            !self.inner.signals.join("cargo-probes").exists(),
            "browser endpoint ownership must not invoke Cargo metadata"
        );
    }

    pub fn open_plan(&self) -> String {
        let output = jig(&self.inner.root)
            .args([
                "work",
                "start",
                "--title",
                "Example browser readiness",
                "--body",
                "Verify the current browser validator executes after admission.",
                "--print-plan-id",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}

pub fn records(root: &Path, file: &str) -> Vec<Value> {
    fs::read_to_string(root.join(".agent/state").join(file))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn commit(root: &Path) {
    for args in [
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Example Agent",
            "-c",
            "user.email=example@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "Example browser fixture",
        ],
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

// A controlled stand-in for the generated wrapper, not a Playwright benchmark.
// The first write is deliberately before parsing or claiming anything: invalid
// admission must prevent even this instrumented target from starting.
const BROWSER: &str = r#"
const fs = require('node:fs');
const path = require('node:path');
const root = process.env.EXAMPLE_BARRIER_ROOT;
const id = process.env.EXAMPLE_RUN_ID;
const marker = name => path.join(root, name);
fs.appendFileSync(marker('launches'), `${id}\n`);
const port = (name, fallback) => {
  const value = Number(process.env[name]?.trim() || fallback);
  if (!Number.isInteger(value) || value < 1 || value > 65535) process.exit(91);
  return value;
};
const web = port('E2E_WEB_PORT', 4173);
const api = port('E2E_API_PORT', 4174);
if (web === api) process.exit(91);
// Model the owning wrapper's current prerequisite guard, not SQLx or a database.
if (fs.existsSync(marker('readiness-denied'))) process.exit(42);
fs.appendFileSync(marker('expensive-launches'), `${id}\n`);
const held = [];
if (!process.env.E2E_BASE_URL?.trim()) {
  for (const endpoint of [web, api].sort((a, b) => a - b)) {
    const claim = marker(`endpoint-${endpoint}`);
    try { fs.mkdirSync(claim); held.push(claim); }
    catch { fs.writeFileSync(marker('overlap'), id); process.exit(90); }
  }
}
fs.writeFileSync(marker(`entered-${id}`), 'entered\n');
const timer = setInterval(() => {
  if (!fs.existsSync(marker(`release-${id}`))) return;
  clearInterval(timer);
  for (const claim of held) fs.rmdirSync(claim);
  process.exit(0);
}, 20);
"#;
