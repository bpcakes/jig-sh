use super::fixture::{Fixture, Running, jig};
use serde_json::{Value, json};
use std::{fs, process::Command};

pub fn fixture() -> Fixture {
    configured_fixture(false, 0, None)
}

pub fn resource_fixture() -> Fixture {
    configured_fixture(true, 0, None)
}

pub fn resource_timeout_fixture() -> Fixture {
    configured_fixture(true, 0, Some(4))
}

pub fn wide_fixture() -> Fixture {
    configured_fixture(true, 8, None)
}

pub fn wide_resource_timeout_fixture() -> Fixture {
    configured_fixture(true, 8, Some(4))
}

pub fn all_resource_fixture(prerequisite_timeout: u64) -> Fixture {
    let fixture = resource_fixture();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".agent/jig-contract.json")).unwrap())
            .unwrap();
    for action in manifest["actions"].as_array_mut().unwrap() {
        action["resources"] = json!([{"kind":"cargo_v1", "workspace_manifest":"Cargo.toml"}]);
        if action["target"]["action"] == "prerequisite" {
            action["timeout_seconds"] = json!(prerequisite_timeout);
        }
    }
    let config =
        toml::from_str(&fs::read_to_string(fixture.root.join(".jig.toml")).unwrap()).unwrap();
    write_contract(&fixture, &manifest, config);
    fixture
}

pub fn disjoint_resource_timeout_fixture() -> Fixture {
    let fixture = all_resource_fixture(2);
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".agent/jig-contract.json")).unwrap())
            .unwrap();
    let slow = manifest["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|action| action["target"]["action"] == "slow")
        .unwrap();
    slow["timeout_seconds"] = json!(2);
    let artifacts = fixture.signals.join("artifacts-slow");
    fs::create_dir(&artifacts).unwrap();
    slow["runner"]["environment"]["CARGO_TARGET_DIR"] = json!(artifacts);
    slow["runner"]["environment"]["CARGO_BUILD_BUILD_DIR"] = json!(artifacts);
    let config =
        toml::from_str(&fs::read_to_string(fixture.root.join(".jig.toml")).unwrap()).unwrap();
    write_contract(&fixture, &manifest, config);
    fixture
}

pub fn resource_batch_fixture(disjoint: bool) -> Fixture {
    let fixture = resource_fixture();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".agent/jig-contract.json")).unwrap())
            .unwrap();
    let original = manifest["actions"].as_array().unwrap();
    let resource = original
        .iter()
        .find(|action| action["target"]["action"] == "slow")
        .unwrap();
    let mut actions = (0..8)
        .map(|index| {
            let name = format!("cargo-{index}");
            let mut action = resource.clone();
            action["target"]["action"] = json!(name);
            let environment = &mut action["runner"]["environment"];
            environment["EXAMPLE_RUN_ID"] = json!(name);
            if disjoint {
                let artifacts = fixture.signals.join(format!("artifacts-{name}"));
                fs::create_dir(&artifacts).unwrap();
                environment["CARGO_TARGET_DIR"] = json!(artifacts);
                environment["CARGO_BUILD_BUILD_DIR"] = json!(artifacts);
            }
            action
        })
        .collect::<Vec<_>>();
    actions.extend(
        original
            .iter()
            .filter(|action| action["target"]["action"] != "slow")
            .cloned(),
    );
    manifest["profiles"][0]["targets"] = json!(
        actions
            .iter()
            .map(|action| &action["target"])
            .collect::<Vec<_>>()
    );
    manifest["actions"] = json!(actions);
    let config =
        toml::from_str(&fs::read_to_string(fixture.root.join(".jig.toml")).unwrap()).unwrap();
    write_contract(&fixture, &manifest, config);
    fixture
}

pub fn resource_batch_with_disjoint_ninth() -> Fixture {
    let fixture = resource_batch_fixture(false);
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".agent/jig-contract.json")).unwrap())
            .unwrap();
    let mut ninth = manifest["actions"].as_array().unwrap()[7].clone();
    ninth["target"]["action"] = json!("cargo-8");
    ninth["runner"]["environment"]["EXAMPLE_RUN_ID"] = json!("cargo-8");
    let artifacts = fixture.signals.join("artifacts-cargo-8");
    fs::create_dir(&artifacts).unwrap();
    ninth["runner"]["environment"]["CARGO_TARGET_DIR"] = json!(artifacts);
    ninth["runner"]["environment"]["CARGO_BUILD_BUILD_DIR"] = json!(artifacts);
    manifest["profiles"][0]["targets"]
        .as_array_mut()
        .unwrap()
        .push(ninth["target"].clone());
    manifest["actions"].as_array_mut().unwrap().push(ninth);
    let config =
        toml::from_str(&fs::read_to_string(fixture.root.join(".jig.toml")).unwrap()).unwrap();
    write_contract(&fixture, &manifest, config);
    fixture
}

fn configured_fixture(
    resource_sibling: bool,
    extra_siblings: usize,
    resource_timeout_seconds: Option<u64>,
) -> Fixture {
    let mut fixture = Fixture::new(false, 30);
    fixture.signals = fixture.root.join(".agent/.cache/signals");
    fs::create_dir_all(&fixture.signals).unwrap();
    let manifest_path = fixture.root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(11);
    let names = ["prerequisite", "dependent", "slow"]
        .into_iter()
        .map(str::to_owned)
        .chain((0..extra_siblings).map(|index| format!("sibling-{index}")));
    let actions = names
        .map(|name| {
            let mut action = manifest["actions"][0].clone();
            action["target"]["action"] = json!(name);
            action["runner"]["environment"]["EXAMPLE_RUN_ID"] = json!(name);
            action["runner"]["environment"]["EXAMPLE_BARRIER_ROOT"] = json!(fixture.signals);
            if name == "dependent" {
                action["depends_on"] = json!([{"component":"example","action":"prerequisite"}]);
            }
            if name == "slow" && resource_sibling {
                action["resources"] =
                    json!([{"kind":"cargo_v1", "workspace_manifest":"Cargo.toml"}]);
                if let Some(timeout) = resource_timeout_seconds {
                    action["timeout_seconds"] = json!(timeout);
                }
            }
            action
        })
        .collect::<Vec<_>>();
    manifest["profiles"][0]["targets"] =
        json!(actions.iter().map(|a| &a["target"]).collect::<Vec<_>>());
    manifest["actions"] = json!(actions);
    let config_path = fixture.root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["example_check_command"] = toml::Value::String(
        r#"set -eu
touch "$EXAMPLE_BARRIER_ROOT/active-$EXAMPLE_RUN_ID"
trap 'rm -f "$EXAMPLE_BARRIER_ROOT/active-$EXAMPLE_RUN_ID"' EXIT
set -- "$EXAMPLE_BARRIER_ROOT"/active-*
if [ "$#" -gt 8 ]; then touch "$EXAMPLE_BARRIER_ROOT/capacity-exceeded"; fi
touch "$EXAMPLE_BARRIER_ROOT/entered-$EXAMPLE_RUN_ID"
remaining=1000
while [ ! -f "$EXAMPLE_BARRIER_ROOT/release-$EXAMPLE_RUN_ID" ]; do
  remaining=$((remaining - 1))
  if [ "$remaining" -eq 0 ]; then
    printf 'Timed out at %s fixture barrier\n' "$EXAMPLE_RUN_ID" >&2
    exit 91
  fi
  sleep 0.02
done
if [ -f "$EXAMPLE_BARRIER_ROOT/fail-$EXAMPLE_RUN_ID" ]; then exit 7; fi
touch "$EXAMPLE_BARRIER_ROOT/completed-$EXAMPLE_RUN_ID"
"#
        .into(),
    );
    write_contract(&fixture, &manifest, config);
    fixture
}

fn write_contract(fixture: &Fixture, manifest: &Value, mut config: toml::Value) {
    config["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    config["repository"]["profiles"] = toml::Value::try_from(&manifest["profiles"]).unwrap();
    fs::write(
        fixture.root.join(".jig.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
    for args in [
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Example Agent",
            "-c",
            "user.email=example@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "Example dependency barriers",
        ],
    ] {
        let output = Command::new("git")
            .current_dir(&fixture.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

pub fn signal(fixture: &Fixture, name: &str) {
    fs::write(fixture.signals.join(name), "signal\n").unwrap();
}

pub fn release(fixture: &Fixture, name: &str) {
    signal(fixture, &format!("release-{name}"));
}

pub fn start(fixture: &Fixture, extra: &[&str]) -> Running {
    let mut args = vec!["check", "--profile", "verify"];
    args.extend_from_slice(extra);
    let mut run = fixture.spawn_args("example-run", &args);
    run.wait_named_entry("prerequisite");
    run.wait_named_entry("slow");
    assert!(!fixture.signals.join("entered-dependent").exists());
    run
}

pub fn records(fixture: &Fixture, name: &str) -> Vec<Value> {
    fs::read_to_string(fixture.root.join(".agent/state").join(name))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn receipt<'a>(records: &'a [Value], name: &str) -> &'a Value {
    let matches = records
        .iter()
        .filter(|r| r["target"]["action"] == name)
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "one original receipt for {name}: {records:#?}"
    );
    matches[0]
}

pub fn assert_dependent_skipped(fixture: &Fixture) {
    assert!(!fixture.signals.join("entered-dependent").exists());
    let events = records(fixture, "runs.jsonl");
    let dependent = events
        .iter()
        .find(|event| {
            event["event"] == "target_completed" && event["target"]["action"] == "dependent"
        })
        .expect("unstarted dependent must still be accounted for");
    assert_eq!(
        dependent["result"]["conclusion"], "skipped",
        "{dependent:#}"
    );
}

pub fn mutate(fixture: &Fixture) {
    fs::write(
        fixture.root.join("src/lib.rs"),
        "pub fn changed_example() {}\n",
    )
    .unwrap();
}

pub fn open_plan(fixture: &Fixture) -> String {
    let output = jig(&fixture.root)
        .args([
            "work",
            "start",
            "--title",
            "Example dependency scheduling",
            "--body",
            "Validate original dependency evidence after a later source mutation.",
            "--print-plan-id",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap().trim().into()
}
