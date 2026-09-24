#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::*;
use crate::test_env::{EnvVarGuard, lock_env};

const PREPARE: &str = r#"#!/bin/sh
set -eu
test "$(cat package.json)" = '{"name":"ExampleProject","version":"1.0.0"}'
mkdir -p node_modules/.cache
printf '%s\n' prepared > node_modules/.cache/deps
printf '%s\n' prepared
"#;

fn fixture(root: &Path, prepare: Option<&str>, sandbox: &str, timeout: Option<u64>) {
    let mut config = String::new();
    if let Some(timeout) = timeout {
        config.push_str(&format!(
            "[execution]\ncommand_timeout_seconds = {timeout}\n"
        ));
    }
    config.push_str(&format!(
        "[[loop.workflows]]\nid = \"example-task\"\nkind = \"codex_task\"\nschedule = \"* * * * *\"\nprompt_file = \"tasks/task.md\"\ncheckout = \"worktree\"\nsandbox = \"{sandbox}\"\n"
    ));
    if let Some(prepare) = prepare {
        config.push_str(&format!("prepare_command = [\"{prepare}\"]\n"));
    }
    TestRepoBuilder::new(root)
        .repo_name("ExampleProject")
        .required_commands(Vec::<String>::new())
        .config(config)
        .write();
    fs::write(root.join(".gitignore"), ".agent/runtime/\nnode_modules/\n").unwrap();
    fs::create_dir_all(root.join("tasks")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("tasks/task.md"), "Inspect ExampleProject.\n").unwrap();
    fs::write(
        root.join("package.json"),
        "{\"name\":\"ExampleProject\",\"version\":\"1.0.0\"}",
    )
    .unwrap();
    fs::write(root.join("scripts/prepare.sh"), PREPARE).unwrap();
    fs::set_permissions(
        root.join("scripts/prepare.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "fixture@example.com"],
        vec!["config", "user.name", "Fixture"],
        vec!["add", "."],
        vec!["commit", "-m", "ExampleProject fixture"],
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

fn codex_stub(path: &Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
if [ "$1" = sandbox ]; then
  shift
  [ "$1" = --permission-profile ]
  profile="$2"
  shift 2
  [ "$1" = --include-managed-config ]
  shift
  [ "$1" = --cd ]
  [ "$2" = "$PWD" ]
  shift 2
  [ "$1" = -- ]
  shift
  [ "$profile" = "$JIG_TEST_PROFILE" ]
  printf 'prepare:%s\n' "$profile" >> "$JIG_TEST_LOG"
  exec "$@"
fi
printf 'worker\n' >> "$JIG_TEST_LOG"
if [ "$JIG_TEST_EXPECT_DEPS" = yes ]; then
  [ "$(cat node_modules/.cache/deps)" = prepared ]
  [ "$(cat package.json)" = '{"name":"ExampleProject","version":"1.0.0"}' ]
fi
out=
previous=
for argument in "$@"; do
  if [ "$previous" = -o ]; then out="$argument"; fi
  previous="$argument"
done
cat >/dev/null
printf 'ExampleProject worker completed\n' > "$out"
"#,
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn dispatch(
    ctx: &RepoContext,
    observer: &mut dyn crate::execution::ExecutionControl,
) -> (Value, OccurrenceStore) {
    let workflow = list_workflows(ctx)
        .unwrap()
        .into_iter()
        .find(|workflow| workflow.id == "example-task")
        .unwrap();
    let mut occurrences = OccurrenceStore::new(ctx);
    let result = dispatch_workflow(
        ctx,
        &mut occurrences,
        &workflow,
        super::timestamp("2026-08-21T08:42:30Z"),
        observer,
    );
    (result.action.unwrap(), occurrences)
}

#[test]
fn preparation_runs_in_the_checkout_before_worker_and_no_hook_still_runs() {
    let _lock = lock_env();
    let bin = tempdir().unwrap();
    let codex = bin.path().join("codex-stub.sh");
    codex_stub(&codex);
    let log = bin.path().join("calls.log");
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex.as_os_str());
    let _log = EnvVarGuard::set("JIG_TEST_LOG", log.as_os_str());
    let _profile = EnvVarGuard::set("JIG_TEST_PROFILE", std::ffi::OsStr::new(":workspace"));

    let prepared = tempdir().unwrap();
    fixture(
        prepared.path(),
        Some("./scripts/prepare.sh"),
        "workspace-write",
        None,
    );
    fs::write(
        prepared.path().join("package.json"),
        "uncommitted controller value",
    )
    .unwrap();
    let _deps = EnvVarGuard::set("JIG_TEST_EXPECT_DEPS", std::ffi::OsStr::new("yes"));
    let ctx = RepoContext::load_from(prepared.path()).unwrap();
    let (action, occurrences) = dispatch(&ctx, &mut NoopExecutionObserver);
    assert_eq!(action["status"], "succeeded", "{action:#}");
    assert_eq!(
        action["tick"]["actions"][0]["preparation"]["status"],
        "succeeded"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "prepare::workspace\nworker\n"
    );
    assert!(
        occurrences
            .snapshot()
            .unwrap()
            .iter()
            .all(|item| item.worktree.is_none())
    );

    fs::write(&log, "").unwrap();
    let plain = tempdir().unwrap();
    fixture(plain.path(), None, "workspace-write", None);
    let _deps = EnvVarGuard::set("JIG_TEST_EXPECT_DEPS", std::ffi::OsStr::new("no"));
    let ctx = RepoContext::load_from(plain.path()).unwrap();
    let (action, _) = dispatch(&ctx, &mut NoopExecutionObserver);
    assert_eq!(action["status"], "succeeded", "{action:#}");
    assert!(action["tick"]["actions"][0].get("preparation").is_none());
    assert_eq!(fs::read_to_string(&log).unwrap(), "worker\n");

    fs::write(&log, "").unwrap();
    let read_only = tempdir().unwrap();
    fixture(read_only.path(), Some("/bin/true"), "read-only", None);
    let _profile = EnvVarGuard::set("JIG_TEST_PROFILE", std::ffi::OsStr::new(":read-only"));
    let ctx = RepoContext::load_from(read_only.path()).unwrap();
    let (action, _) = dispatch(&ctx, &mut NoopExecutionObserver);
    assert_eq!(action["status"], "succeeded", "{action:#}");
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "prepare::read-only\nworker\n"
    );
}

#[test]
fn failed_or_missing_preparation_retains_evidence_and_does_not_replay() {
    let _lock = lock_env();
    let bin = tempdir().unwrap();
    let codex = bin.path().join("codex-stub.sh");
    codex_stub(&codex);
    let log = bin.path().join("calls.log");
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex.as_os_str());
    let _log = EnvVarGuard::set("JIG_TEST_LOG", log.as_os_str());
    let _profile = EnvVarGuard::set("JIG_TEST_PROFILE", std::ffi::OsStr::new(":workspace"));
    let _deps = EnvVarGuard::set("JIG_TEST_EXPECT_DEPS", std::ffi::OsStr::new("no"));

    for (command, script, expected) in [
        ("./scripts/missing.sh", None, "failed"),
        (
            "./scripts/prepare.sh",
            Some(
                "#!/bin/sh\nmkdir -p node_modules/.cache\necho partial > node_modules/.cache/deps\nexit 9\n",
            ),
            "failed",
        ),
    ] {
        fs::write(&log, "").unwrap();
        let repo = tempdir().unwrap();
        fixture(repo.path(), Some(command), "workspace-write", None);
        if let Some(script) = script {
            fs::write(repo.path().join("scripts/prepare.sh"), script).unwrap();
            commit_script(repo.path());
        }
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let workflow = list_workflows(&ctx)
            .unwrap()
            .into_iter()
            .find(|workflow| workflow.id == "example-task")
            .unwrap();
        let mut occurrences = OccurrenceStore::new(&ctx);
        let at = super::timestamp("2026-08-21T08:42:30Z");
        let first = dispatch_workflow(
            &ctx,
            &mut occurrences,
            &workflow,
            at,
            &mut NoopExecutionObserver,
        );
        let first_action = first.action.unwrap();
        let task = &first_action["tick"]["actions"][0];
        assert_eq!(
            first_action["status"], "needs_attention",
            "{first_action:#}"
        );
        assert_eq!(task["preparation"]["status"], expected);
        assert_eq!(task["worker_started"], false);
        let retained = task["checkout"]["path"].as_str().unwrap();
        assert!(Path::new(retained).exists());
        assert!(
            fs::read_to_string(repo.path().join(".agent/state/receipts.jsonl"))
                .unwrap()
                .contains("preparation")
        );
        assert_eq!(
            occurrences.snapshot().unwrap()[0].status,
            OccurrenceStatus::NeedsAttention
        );
        let second = dispatch_workflow(
            &ctx,
            &mut occurrences,
            &workflow,
            at + 60_000,
            &mut NoopExecutionObserver,
        );
        assert_ne!(second.action.unwrap()["status"], "succeeded");
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
        let status = super::super::super::engine::status_at_with_cancellation(
            &ctx,
            crate::command::LoopStatusRequest {
                workflow: Some("example-task".into()),
            },
            &|| false,
            u64::MAX,
        )
        .unwrap();
        assert!(
            status["leases"].as_array().unwrap().is_empty(),
            "{status:#}"
        );
    }
}

struct CancelAfterPreparation(PathBuf);

impl crate::execution::ExecutionObserver for CancelAfterPreparation {}

impl crate::execution::ExecutionCancellation for CancelAfterPreparation {
    fn cancelled(&self) -> bool {
        self.0.exists()
    }
}

#[test]
fn timed_out_and_cancelled_preparation_never_launch_worker() {
    let _lock = lock_env();
    let bin = tempdir().unwrap();
    let codex = bin.path().join("codex-stub.sh");
    codex_stub(&codex);
    let log = bin.path().join("calls.log");
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex.as_os_str());
    let _log = EnvVarGuard::set("JIG_TEST_LOG", log.as_os_str());
    let _profile = EnvVarGuard::set("JIG_TEST_PROFILE", std::ffi::OsStr::new(":workspace"));
    let _deps = EnvVarGuard::set("JIG_TEST_EXPECT_DEPS", std::ffi::OsStr::new("no"));

    for cancelled in [false, true] {
        fs::write(&log, "").unwrap();
        let repo = tempdir().unwrap();
        fixture(
            repo.path(),
            Some("./scripts/prepare.sh"),
            "workspace-write",
            if cancelled { None } else { Some(1) },
        );
        let marker = bin.path().join("started");
        let script = format!(
            "#!/bin/sh\nprintf 'partial stdout\\n'\nprintf 'partial stderr\\n' >&2\ntouch '{}'\nsleep 5\n",
            marker.display()
        );
        fs::write(repo.path().join("scripts/prepare.sh"), script).unwrap();
        commit_script(repo.path());
        let ctx = RepoContext::load_from(repo.path()).unwrap();
        let action = if cancelled {
            dispatch(&ctx, &mut CancelAfterPreparation(marker.clone())).0
        } else {
            dispatch(&ctx, &mut NoopExecutionObserver).0
        };
        let task = &action["tick"]["actions"][0];
        assert_eq!(action["status"], "needs_attention", "{action:#}");
        assert_eq!(
            task["preparation"]["status"],
            if cancelled { "cancelled" } else { "timed_out" }
        );
        assert_eq!(task["worker_started"], false);
        assert_eq!(task["preparation"]["stdout"], "partial stdout\n");
        assert_eq!(task["preparation"]["stderr"], "partial stderr\n");
        assert!(Path::new(task["checkout"]["path"].as_str().unwrap()).exists());
        assert_eq!(fs::read_to_string(&log).unwrap(), "prepare::workspace\n");
        fs::remove_file(&marker).unwrap();
    }
}

fn commit_script(root: &Path) {
    for args in [
        vec!["add", "scripts/prepare.sh"],
        vec!["commit", "-m", "Change ExampleProject preparation"],
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn installed_codex_read_only_profile_denies_preparation_writes() {
    let _lock = lock_env();
    let repo = tempdir().unwrap();
    let output = Command::new("codex")
        .args([
            "sandbox",
            "--permission-profile",
            ":read-only",
            "--include-managed-config",
            "--cd",
        ])
        .arg(repo.path())
        .args([
            "--",
            "/bin/sh",
            "-c",
            "printf 'sandbox-child-started\\n'; touch blocked",
        ])
        .current_dir(repo.path())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The unit fixture requires no Codex installation. Runtimes with
            // the executable exercise this actual sandbox boundary as well.
            return;
        }
        Err(error) => panic!("Failed to start Codex sandbox: {error}"),
    };
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"sandbox-child-started\n", "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("blocked"), "{output:?}");
    assert!(
        [
            "Read-only file system",
            "Permission denied",
            "Operation not permitted"
        ]
        .iter()
        .any(|denial| stderr.contains(denial)),
        "{output:?}"
    );
    assert!(!repo.path().join("blocked").exists());
}
