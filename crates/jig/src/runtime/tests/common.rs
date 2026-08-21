use super::*;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::state::{ReceiptInput, record_receipt};
use crate::test_env::TestRepoBuilder;

pub(super) fn write_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
[commands]
custom_check_command = "printf 'manifest target ran\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "custom_check_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_command_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
rust_migration_dir = "migrations"
rust_sqlx_metadata_dir = ".sqlx"
schema_dump_command = "printf 'schema dump\n'"
rust_test_command = "printf 'command tool ran\n'"
contract_check_command = "printf 'contract ok\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "rust_test_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_mutating_check_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
[commands]
first_check_command = "printf 'first ran\n'"
mutating_check_command = "printf 'generated\n' > generated.txt"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.first_check"

[[work.gates]]
id = "mutating"
kind = "check"
tool = "jig.mutating_check"
"#,
        )
        .required_commands(["first_check_command", "mutating_check_command"])
        .tool(json!({
            "name": "jig.first_check",
            "kind": "command",
            "description": "Run configured first check.",
            "command": "first_check_command"
        }))
        .tool(json!({
            "name": "jig.mutating_check",
            "kind": "command",
            "description": "Run configured mutating check.",
            "command": "mutating_check_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_failing_check_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
[commands]
custom_check_command = "printf 'check failed\n' >&2; exit 7"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "custom_check_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_timeout_check_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
[commands]
timeout_check_command = "sleep 30"

[execution]
command_timeout_seconds = 1

[[work.gates]]
id = "timeout"
kind = "check"
tool = "jig.timeout_check"
"#,
        )
        .required_commands(["timeout_check_command"])
        .tool(json!({
            "name": "jig.timeout_check",
            "kind": "command",
            "description": "Run configured timeout check.",
            "command": "timeout_check_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_fail_fast_check_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
[commands]
failing_check_command = "printf 'check failed\n' >&2; exit 7"
later_check_command = "printf 'later check ran\n' > later-check-ran.txt"

[[work.gates]]
id = "failing"
kind = "check"
tool = "jig.failing_check"

[[work.gates]]
id = "later"
kind = "check"
tool = "jig.later_check"
"#,
        )
        .required_commands(["failing_check_command", "later_check_command"])
        .tool(json!({
            "name": "jig.failing_check",
            "kind": "command",
            "description": "Run configured failing check.",
            "command": "failing_check_command"
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Run configured later check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(root);
}

pub(super) fn write_review_fixture_repo(root: &Path) {
    write_review_fixture_repo_with_check(root, "printf 'check ok\\n'");
}

pub(super) fn write_review_fixture_repo_with_check(root: &Path, check_command: &str) {
    write_review_fixture_repo_with_options(root, check_command, true);
}

pub(super) fn write_review_fixture_repo_without_refinement(root: &Path) {
    write_review_fixture_repo_with_options(root, "printf 'check ok\\n'", false);
}

fn write_review_fixture_repo_with_options(root: &Path, check_command: &str, refinement: bool) {
    let refinement_config = if refinement {
        r#"
[[work.refinements]]
id = "test-refinement"
skill = "jig-rust:rust-simplify"
"#
    } else {
        ""
    };
    TestRepoBuilder::new(root)
        .config(format!(
            r#"
[commands]
custom_check_command = "{check_command}"

[[work.gates]]
id = "rust-error-handling"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
severity = "high"
required = true

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
{refinement_config}
"#
        ))
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run configured custom check.",
            "command": "custom_check_command"
        }))
        .write();
    write_open_plan(root);
}

fn write_open_plan(root: &Path) {
    let ctx = RepoContext::load_from(root).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Test plan", "# Test plan\n").unwrap();
}

pub(super) fn open_test_plan(ctx: &RepoContext) -> String {
    // Most runtime fixtures seed plan_1 because work-check tests exercise that
    // stable id directly. Reuse it while it remains open; otherwise fall back to
    // opening a fresh plan for tests that deliberately closed the seeded one.
    if crate::state::ensure_plan_is_open(ctx, "plan_1").is_ok() {
        return "plan_1".into();
    }

    let plan = crate::state::plans_open(
        ctx,
        crate::state::PlanOpenRequest {
            title: "Test plan".into(),
            body: Some("Test body".into()),
            body_file: None,
        },
    )
    .unwrap();

    plan["plan_id"].as_str().unwrap().to_string()
}

pub(super) struct TestReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) plan_id: &'a str,
    pub(super) started_at_ms: u64,
    pub(super) ended_at_ms: u64,
    pub(super) worktree_fingerprint: Option<String>,
}

pub(super) fn record_test_receipt(ctx: &RepoContext, receipt: TestReceipt<'_>) -> String {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: receipt.tool_name,
            args: receipt.args,
            invoked_command_key: None,
            plan_id: Some(receipt.plan_id.to_string()),
            started_at_ms: receipt.started_at_ms,
            ended_at_ms: receipt.ended_at_ms,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: receipt.worktree_fingerprint.map(Ok),
        },
    )
    .unwrap()
}

pub(super) fn init_git_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "fixture@example.com"]);
    run_git(root, &["config", "user.name", "Fixture"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-m", "initial fixture"]);
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

pub(super) fn write_codex_stub(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
