use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;

use crate::execution::{ExecutionCancellation, ExecutionEvent, ExecutionObserver};
use crate::state::ResourceLease;

use super::*;

const REQUESTED_ALIAS: &str = "jig.resource_second";
const ARGUMENT: &str = " literal ; $(not-a-command) = value ";

#[test]
fn resource_alias_waits_and_preserves_dependencies_arguments_and_original_alias_receipt() {
    let fixture = Fixture::new(0, 0);
    let ctx = fixture.context();
    let claims = fixture.claims(&ctx);
    let owner = ResourceLease::try_acquire(&claims).unwrap().unwrap();
    let (notice, waiting) = mpsc::sync_channel(1);
    let output = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let mut observer = WaitObserver {
                notice: Some(notice),
                ..WaitObserver::default()
            };
            invoke(&ctx, &mut observer)
        });
        waiting.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(
            !fixture.marker().exists(),
            "alias bypassed the held Cargo claim"
        );
        assert!(
            fixture.dependency_marker().exists(),
            "declared prerequisite was omitted"
        );
        drop(owner);
        worker.join().unwrap().unwrap().into_value().unwrap()
    });
    assert_eq!(output["tool"], REQUESTED_ALIAS);
    assert_eq!(output["args"], json!({"value": ARGUMENT}));
    assert_eq!(output["result"]["stdout"], ARGUMENT);
    assert_eq!(output["result"]["exit_status"], 0);
    assert!(fixture.marker().exists());
    let receipts = fixture.receipts();
    let target_receipts = receipts
        .iter()
        .filter(|receipt| receipt["target"] == json!({"component":"repo","action":"check"}))
        .collect::<Vec<_>>();
    assert_eq!(
        target_receipts.len(),
        1,
        "adapter must not mint an extra receipt"
    );
    assert_eq!(target_receipts[0]["tool_name"], REQUESTED_ALIAS);
    assert_eq!(target_receipts[0]["args"], json!({"value": ARGUMENT}));
    assert_eq!(target_receipts[0]["id"], output["receipt_id"]);
    assert_eq!(
        receipts
            .iter()
            .filter(|receipt| receipt["target"]["action"] == "prepare")
            .count(),
        1
    );
}

#[test]
fn cancelling_a_waiting_alias_never_starts_the_child_or_releases_the_owner() {
    let fixture = Fixture::new(0, 0);
    let ctx = fixture.context();
    let claims = fixture.claims(&ctx);
    let owner = ResourceLease::try_acquire(&claims).unwrap().unwrap();
    let mut observer = WaitObserver {
        cancel_on_wait: true,
        ..WaitObserver::default()
    };
    assert!(matches!(
        invoke(&ctx, &mut observer).unwrap(),
        ManifestToolExecutionOutcome::Cancelled(_)
    ));
    assert!(observer.saw_wait);
    assert!(!fixture.marker().exists());
    assert!(ResourceLease::try_acquire(&claims).unwrap().is_none());
    drop(owner);
}

#[test]
fn failed_alias_keeps_collect_result_and_fail_fast_semantics() {
    let fixture = Fixture::new(7, 0);
    let ctx = fixture.context();
    let output = invoke(&ctx, &mut NoopExecutionObserver)
        .unwrap()
        .into_value()
        .unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["result"]["exit_status"], 7);
    let error = execute_manifest_tool_with_observer(
        &ctx,
        REQUESTED_ALIAS,
        json!({"value": ARGUMENT}),
        None,
        true,
        &mut NoopExecutionObserver,
    )
    .unwrap_err();
    assert!(error.to_string().contains("failed with status 7"));
    assert!(error.to_string().contains("receipt: "));
}

#[test]
fn failed_declared_dependency_prevents_alias_child_execution() {
    let fixture = Fixture::new(0, 9);
    let ctx = fixture.context();
    let output = invoke(&ctx, &mut NoopExecutionObserver)
        .unwrap()
        .into_value()
        .unwrap();
    assert_eq!(output["tool"], REQUESTED_ALIAS);
    assert_eq!(output["args"], json!({"value": ARGUMENT}));
    assert_ne!(output["result"]["exit_status"], 0);
    assert!(!fixture.marker().exists());
}

#[test]
fn effectful_dependency_requires_explicit_canonical_run_approval() {
    let fixture = Fixture::new(0, 0);
    let config_path = fixture.repo.path().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"][0]["effects"] = toml::Value::Array(
        ["worktree", "process"]
            .into_iter()
            .map(|effect| toml::Value::String(effect.into()))
            .collect(),
    );
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let manifest_path = fixture.repo.path().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["actions"][0]["effects"] = json!(["worktree", "process"]);
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let ctx = fixture.context();
    let error = invoke(&ctx, &mut NoopExecutionObserver)
        .err()
        .expect("effectful graph must be rejected");
    assert!(
        error.to_string().contains("explicitly authorize"),
        "{error:#}"
    );
    assert!(!fixture.marker().exists());
    assert!(!fixture.dependency_marker().exists());
    assert!(!fixture.repo.path().join(".agent/state/runs.jsonl").exists());
}

fn invoke(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<ManifestToolExecutionOutcome> {
    execute_manifest_tool_with_options(
        ctx,
        REQUESTED_ALIAS,
        json!({"value": ARGUMENT}),
        None,
        ManifestToolExecutionOptions::collect_result(true, true, true),
        ManifestToolExecutionBoundary::single(),
        observer,
    )
}

#[derive(Default)]
struct WaitObserver {
    notice: Option<mpsc::SyncSender<()>>,
    saw_wait: bool,
    cancel_on_wait: bool,
}

impl ExecutionObserver for WaitObserver {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        if let ExecutionEvent::Output { bytes, .. } = event
            && String::from_utf8_lossy(bytes).contains("Waiting for a Cargo build resource")
        {
            self.saw_wait = true;
            if let Some(notice) = self.notice.take() {
                notice.send(()).unwrap();
            }
        }
    }
}

impl ExecutionCancellation for WaitObserver {
    fn cancelled(&self) -> bool {
        self.cancel_on_wait && self.saw_wait
    }
}

struct Fixture {
    repo: tempfile::TempDir,
    barriers: tempfile::TempDir,
}

impl Fixture {
    fn new(exit: i32, dependency_exit: i32) -> Self {
        let fixture = Self {
            repo: tempfile::tempdir().unwrap(),
            barriers: tempfile::tempdir().unwrap(),
        };
        let root = fixture.repo.path();
        let environment = json!({
            "EXAMPLE_MARKER": fixture.marker(),
            "EXAMPLE_DEPENDENCY_MARKER": fixture.dependency_marker(),
            "CARGO_HOME": root.join("cargo-home"),
            "CARGO_TARGET_DIR": root.join("artifacts"),
            "CARGO_BUILD_BUILD_DIR": root.join("artifacts"),
        });
        let actions = json!([
            {
                "target":{"component":"repo","action":"prepare"},
                "intent":"check", "effects":["read_only","process"],
                "runner":{"kind":"argv","program":"/bin/sh","args":["-c",format!("printf ready > \"$EXAMPLE_DEPENDENCY_MARKER\"; exit {dependency_exit}")],"environment":environment},
                "inputs":["**"], "timeout_seconds":15
            },
            {
                "target":{"component":"repo","action":"check"},
                "intent":"check", "effects":["read_only","process"],
                "runner":{"kind":"argv","program":"/bin/sh","args":["-c",format!("test -f \"$EXAMPLE_DEPENDENCY_MARKER\" || exit 95; printf started > \"$EXAMPLE_MARKER\"; printf '%s' \"$1\"; exit {exit}"),"example",{"argument":"value"}],"environment":environment},
                "arguments":{"value":{"type":"string","required":true,"max_bytes":1024}},
                "resources":[{"kind":"cargo_v1","workspace_manifest":"Cargo.toml","context":jig_contract::CargoImpactContextV1::default()}],
                "depends_on":[{"component":"repo","action":"prepare"}],
                "legacy_aliases":["jig.resource_first",REQUESTED_ALIAS],
                "inputs":["**"], "timeout_seconds":15
            }
        ]);
        let repository = json!({
            "default_check_profile":"verify", "components":[{"id":"repo","root":"."}],
            "actions":actions, "profiles":[{"id":"verify","targets":[{"component":"repo","action":"check"}]}]
        });
        crate::test_env::TestRepoBuilder::new(root)
            .repo_name("ExampleResourceAlias")
            .contract_version(8)
            .required_commands(Vec::<String>::new())
            .config(toml::to_string(&json!({"repository":repository})).unwrap())
            .write();
        let manifest_path = root.join(".agent/jig-contract.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        for (key, value) in repository.as_object().unwrap() {
            manifest[key] = value.clone();
        }
        manifest["tools"] = json!([
            {"name":"jig.resource_first","kind":"command","description":"First generic alias.","command":"unused"},
            {"name":REQUESTED_ALIAS,"kind":"command","description":"Second generic alias.","command":"unused"}
        ]);
        fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("cargo-home")).unwrap();
        fs::create_dir(root.join("artifacts")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"example-resource-alias\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"example-resource-alias\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "// Metadata only.\n").unwrap();
        fs::write(
            root.join(".gitignore"),
            ".agent/state/\n.agent/.cache/\ncargo-home/\nartifacts/\n",
        )
        .unwrap();
        for args in [
            &["init"][..],
            &["config", "user.email", "fixture@example.com"],
            &["config", "user.name", "Example Fixture"],
            &["add", "."],
            &["commit", "-m", "Generic alias fixture"],
        ] {
            git(root, args);
        }
        fixture
    }

    fn context(&self) -> RepoContext {
        RepoContext::load_from(self.repo.path()).unwrap()
    }
    fn marker(&self) -> std::path::PathBuf {
        self.barriers.path().join("child-started")
    }
    fn dependency_marker(&self) -> std::path::PathBuf {
        self.barriers.path().join("dependency-started")
    }

    fn claims(&self, ctx: &RepoContext) -> Vec<crate::state::ResourceClaim> {
        let catalog = RepositoryCatalog::from_context(ctx).unwrap();
        let target: TargetId = "repo:check".parse().unwrap();
        let plan = plan_action_run_with_cancellation(
            ctx,
            &catalog,
            PlanRunRequest {
                selectors: vec![target.to_string()],
                ..PlanRunRequest::default()
            },
            BTreeMap::from([(
                target.clone(),
                BTreeMap::from([("value".into(), ARGUMENT.into())]),
            )]),
            &|| false,
        )
        .unwrap();
        let planned = plan
            .targets
            .iter()
            .find(|planned| planned.target == target)
            .unwrap();
        let resolved = crate::repository::cargo_resources::resolve(
            ctx,
            planned,
            Duration::from_secs(10),
            &|| false,
        )
        .unwrap();
        assert_eq!(resolved.partial_reason, None);
        resolved.claims
    }

    fn receipts(&self) -> Vec<Value> {
        fs::read_to_string(self.repo.path().join(".agent/state/receipts.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
