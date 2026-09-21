use super::fixture::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

pub enum WaveKind {
    Mixed,
    Disjoint,
    Mutating,
    OrdinaryTimeout,
}

pub fn wave_fixture(kind: WaveKind) -> Fixture {
    let fixture = Fixture::new(true, 60);
    let manifest_path = fixture.root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let names: &[&str] = match kind {
        WaveKind::Mixed => &["ordinary-a", "ordinary-b", "cargo-a", "cargo-b"],
        WaveKind::OrdinaryTimeout => &["ordinary-a", "cargo-a"],
        WaveKind::Disjoint | WaveKind::Mutating => &["cargo-a", "cargo-b"],
    };
    let actions = names
        .iter()
        .map(|name| {
            let mut action = manifest["actions"][0].clone();
            action["target"]["action"] = json!(name);
            action["runner"]["environment"]["EXAMPLE_RUN_ID"] = json!(name);
            action["runner"]["environment"]["EXAMPLE_MUTATE_SOURCE"] =
                json!(
                    if matches!(kind, WaveKind::Mutating) && *name == "cargo-a" {
                        "1"
                    } else {
                        "0"
                    }
                );
            if name.starts_with("ordinary") {
                action.as_object_mut().unwrap().remove("resources");
                action["runner"]["environment"]["EXAMPLE_ASSERT_EXCLUSIVE"] = json!("0");
                if matches!(kind, WaveKind::OrdinaryTimeout) {
                    action["timeout_seconds"] = json!(2);
                }
            } else if !matches!(kind, WaveKind::Mixed) {
                let artifacts = fixture.signals.join(format!("artifacts-{name}"));
                fs::create_dir(&artifacts).unwrap();
                action["runner"]["environment"]["CARGO_TARGET_DIR"] = json!(artifacts);
                action["runner"]["environment"]["CARGO_BUILD_BUILD_DIR"] = json!(artifacts);
                action["runner"]["environment"]["EXAMPLE_RESOURCE_GROUP"] = json!(name);
            }
            action
        })
        .collect::<Vec<_>>();
    manifest["profiles"][0]["targets"] = json!(
        actions
            .iter()
            .map(|action| action["target"].clone())
            .collect::<Vec<_>>()
    );
    manifest["actions"] = json!(actions);
    write_contract(&fixture, &manifest);
    fixture
}

fn write_contract(fixture: &Fixture, manifest: &Value) {
    let config_path = fixture.root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    config["repository"]["profiles"] = toml::Value::try_from(&manifest["profiles"]).unwrap();
    let command = config["commands"]["example_check_command"]
        .as_str()
        .unwrap();
    let ending = r#"
if [ "$EXAMPLE_MUTATE_SOURCE" = 1 ]; then
  printf 'pub fn changed_example() {}\n' > src/lib.rs
  touch "$EXAMPLE_BARRIER_ROOT/mutated-$EXAMPLE_RUN_ID"
fi
if [ -f "$EXAMPLE_BARRIER_ROOT/fail-$EXAMPLE_RUN_ID" ]; then exit 7; fi
touch "$EXAMPLE_BARRIER_ROOT/completed-$EXAMPLE_RUN_ID"
"#;
    config["commands"]["example_check_command"] =
        toml::Value::String(format!("{command}\n{ending}"));
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(
        fixture.root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
    commit_fixture(&fixture.root);
}

pub fn blocking_owner_repository(fixture: &Fixture) -> std::path::PathBuf {
    let root = fixture.other_repository("example-resource-owner", 60, false);
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let environment = &mut manifest["actions"][0]["runner"]["environment"];
    environment["CARGO_TARGET_DIR"] = json!(fixture.signals.join("artifacts-cargo-a"));
    environment["CARGO_BUILD_BUILD_DIR"] = json!(fixture.signals.join("artifacts-cargo-a"));
    environment["EXAMPLE_RESOURCE_GROUP"] = json!("cargo-a");
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    commit_fixture(&root);
    root
}

fn commit_fixture(root: &Path) {
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
            "Example resource wave",
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

pub fn signal(fixture: &Fixture, name: &str) {
    fs::write(fixture.signals.join(name), "signal\n").unwrap();
}

pub fn release(fixture: &Fixture, id: &str) {
    signal(fixture, &format!("release-{id}"));
}

pub fn records(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn receipt_for<'a>(records: &'a [Value], action: &str) -> &'a Value {
    let selected = records
        .iter()
        .filter(|record| record["target"]["action"] == action)
        .collect::<Vec<_>>();
    assert_eq!(
        selected.len(),
        1,
        "one original target receipt for {action}: {records:#?}"
    );
    selected[0]
}
