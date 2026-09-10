use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use super::*;
use crate::agent_provider::{Choice, Metadata};

#[derive(Debug, Eq, PartialEq)]
enum ExampleConfig {
    Native,
    Explicit(PathBuf),
}

#[derive(Default)]
struct ExampleProvider {
    calls: RefCell<Vec<&'static str>>,
}

impl AgentProvider for ExampleProvider {
    type Home = ExampleConfig;
    const METADATA: Metadata = Metadata {
        command: "example-agent",
        homes_command: "example-agent homes",
        name: "Example Agent",
        executable: "example-agent",
        executable_env: "JIG_EXAMPLE_AGENT_BIN",
        usage: false,
        inspect_on_list: false,
        subscription_bucket: None,
    };

    fn resolve(&self, input: &Path) -> Result<Self::Home> {
        self.calls.borrow_mut().push("resolve");
        Ok(ExampleConfig::Explicit(input.to_owned()))
    }

    fn revalidate(&self, home: &Self::Home) -> Result<Self::Home> {
        self.calls.borrow_mut().push("revalidate");
        match home {
            ExampleConfig::Native => Ok(ExampleConfig::Native),
            ExampleConfig::Explicit(path) => Ok(ExampleConfig::Explicit(path.clone())),
        }
    }

    fn discover(&self) -> Result<Discovery<Self::Home>> {
        self.calls.borrow_mut().push("discover");
        let path = PathBuf::from("/tmp/ExampleAgent");
        Ok(Discovery {
            choices: vec![
                Choice {
                    selection: ExampleConfig::Native,
                    path: path.clone(),
                    name: "native".into(),
                    current: true,
                    details: vec![("Mode".into(), "native".into())],
                },
                Choice {
                    selection: ExampleConfig::Explicit(path.clone()),
                    path,
                    name: "explicit".into(),
                    current: false,
                    details: vec![],
                },
            ],
            warnings: vec!["example warning".into()],
            inspection: None,
        })
    }

    fn prepare(&self, home: &Self::Home, args: &[OsString]) -> Result<PreparedLaunch> {
        self.calls.borrow_mut().push("prepare");
        let mut command = Command::new("example-agent-must-not-be-spawned");
        command.args(args);
        if let ExampleConfig::Explicit(path) = home {
            command.env("EXAMPLE_CONFIG_DIR", path);
        }
        Ok(PreparedLaunch {
            command,
            report: json!({"ok":true, "command":"example-agent launch", "args":args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>()}),
            error_context: "example launch failed".into(),
        })
    }

    fn homes_report(
        &self,
        _usage: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Value> {
        assert!(!cancelled());
        self.calls.borrow_mut().push("report");
        progress(1, 1);
        Ok(json!({"ok":true,"homes":[]}))
    }
}

#[test]
fn third_provider_dry_run_resolves_and_prepares_without_discovery_or_execution() {
    let provider = ExampleProvider::default();
    let args = ["--flag", "argument with spaces", ""].map(OsString::from);
    launch(
        &provider,
        Some(Path::new("ExampleAgent")),
        &args,
        true,
        true,
        HumanOutput::ClaudeLaunch,
    )
    .unwrap();
    assert_eq!(*provider.calls.borrow(), ["resolve", "prepare"]);
}

#[test]
fn capabilities_and_json_preflight_prevent_provider_work() {
    let provider = ExampleProvider::default();
    assert!(
        homes(&provider, true, true, HumanOutput::ClaudeHomes)
            .unwrap_err()
            .to_string()
            .contains("does not support")
    );
    assert!(
        launch(
            &provider,
            Some(Path::new("ExampleAgent")),
            &[],
            false,
            true,
            HumanOutput::ClaudeLaunch
        )
        .is_err()
    );
    assert!(
        launch(&provider, None, &[], true, true, HumanOutput::ClaudeLaunch)
            .unwrap_err()
            .to_string()
            .contains("Pass a Example Agent HOME")
    );
    assert!(provider.calls.borrow().is_empty());
    homes(&provider, false, true, HumanOutput::ClaudeHomes).unwrap();
    assert_eq!(*provider.calls.borrow(), ["report"]);
}

#[test]
fn third_provider_selection_preserves_same_path_modes_and_revalidates_selected_identity() {
    let provider = ExampleProvider::default();
    for index in [0, 1] {
        let home = select_with(&provider, |entries, source, warnings| {
            assert!(source.is_none());
            assert_eq!(entries[0].home.path, entries[1].home.path);
            assert_ne!(entries[0].home.name, entries[1].home.name);
            assert_eq!(warnings, ["example warning"]);
            Ok(Some(index))
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            home,
            if index == 0 {
                ExampleConfig::Native
            } else {
                ExampleConfig::Explicit("/tmp/ExampleAgent".into())
            }
        );
    }
    assert_eq!(
        *provider.calls.borrow(),
        ["discover", "revalidate", "discover", "revalidate"]
    );
}

#[test]
fn cancellation_and_invalid_picker_indices_never_prepare_a_launch() {
    let provider = ExampleProvider::default();
    assert!(
        select_with(&provider, |_, _, _| Ok(None))
            .unwrap()
            .is_none()
    );
    assert!(select_with(&provider, |_, _, _| Ok(Some(42))).is_err());
    assert_eq!(*provider.calls.borrow(), ["discover", "discover"]);
}

struct ExampleInspection;
impl HomeInspection for ExampleInspection {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        if !cancelled() {
            emit(1, json!({"account":null,"status":"unknown"}))?;
        }
        Ok(())
    }
}

#[test]
fn inspection_adapter_preserves_index_cancellation_and_receiver_errors() {
    let source = PickerInspection {
        source: Box::new(ExampleInspection),
    };
    let mut seen = Vec::new();
    source
        .inspect(
            &mut |update| {
                seen.push(update.index);
                Ok(())
            },
            &|| false,
        )
        .unwrap();
    assert_eq!(seen, [1]);
    source
        .inspect(
            &mut |_| panic!("cancelled source emitted an update"),
            &|| true,
        )
        .unwrap();
    assert_eq!(
        source
            .inspect(&mut |_| Err("receiver stopped".into()), &|| false)
            .unwrap_err(),
        "receiver stopped"
    );
}
