use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::tempdir;

fn jig(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .current_dir(root)
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .args(args)
        .output()
        .unwrap()
}

fn check(root: &Path, expected: usize) {
    let output = jig(root, &["check", "sqlx-unchecked-non-test", "--json"]);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], expected, "{value}");
    assert_eq!(value["ok"], expected == 0);
    assert_eq!(output.status.success(), expected == 0);
}

#[test]
fn native_cli_inventory_detects_new_and_replacement_calls_and_fails_on_parse_errors() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
default_branch = "main"
repo_name = "ExampleProject"
jig_version = "0.2.0-beta.1"
rust_crate_roots = ["src", "tests"]
rust_test_command = "cargo test"
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        json!({
            "contract_version": 2,
            "jig_version": "0.2.0-beta.1",
            "tool_namespace": "jig",
            "required_commands": ["rust_test_command"],
            "tools": []
        })
        .to_string(),
    )
    .unwrap();
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    let call = "fn example() {\n    let _ = sqlx::query // explanatory comment\n        (\"SELECT 1\");\n}\n";
    fs::write(root.join("tests/integration.rs"), call).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        format!("const EXAMPLE: u32 = include!(\"value.rs\");\n#[cfg(test)] mod unit {{ {call} }}"),
    )
    .unwrap();
    fs::write(root.join("src/value.rs"), "42\n").unwrap();
    fs::write(
        root.join("src/checked.rs"),
        "fn example() { sqlx::query!(\"SELECT 1\"); }",
    )
    .unwrap();
    check(root, 0);
    let args = ["generate-sqlx-unchecked-queries-todo", "--json"];
    let output = jig(root, &args);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], 0);

    let fragment = "// included expression\nsqlx::query // comment\n(\"SELECT 1\")\n";
    fs::write(root.join("src/value.rs"), fragment).unwrap();
    check(root, 1);
    let output = jig(root, &args);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], 1);
    let todo_path = root.join("docs/sqlx-unchecked-queries-todo.md");
    let todo = fs::read_to_string(&todo_path).unwrap();
    assert!(todo.contains("- [ ] `src/value.rs:2`: `sqlx::query`"));

    fs::write(root.join("src/value.rs"), "sqlx::query!(\"SELECT 1\")\n").unwrap();
    fs::write(root.join("tests/value.rs"), fragment).unwrap();
    check(root, 0);

    fs::write(
        root.join("src/ordinary.rs"),
        "fn example() { sqlx::query(\"SELECT 1\"); }",
    )
    .unwrap();
    check(root, 1);
    // A removal cannot hide the replacement call from either inventory surface.
    fs::remove_file(root.join("src/ordinary.rs")).unwrap();
    fs::write(
        root.join("src/replacement.rs"),
        call.replace("    let _ = ", ""),
    )
    .unwrap();
    check(root, 1);
    let output = jig(root, &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], 1);
    let todo = fs::read_to_string(&todo_path).unwrap();
    assert!(todo.contains("- [ ] `src/replacement.rs:2`: `sqlx::query`"));
    assert!(todo.contains("- Non-test call sites: 1\n"));
    assert!(todo.contains("- Test call sites: 3\n"));
    assert!(todo.contains("- Compile-checked macro call sites already present: 2\n"));
    let test_items = todo.split("## TODO Items (Test Code)").nth(1).unwrap();
    assert!(test_items.contains("- [ ] `tests/value.rs:2`: `sqlx::query`"));

    for (broken, expression_diagnostic) in [
        ("fn broken(\n", None),
        ("42 trailing\n", None),
        (
            "{\n    let value = 42;\n    let broken = ;\n    value\n}\n",
            Some("expression parse at src/broken.rs:3:18: expected an expression"),
        ),
    ] {
        fs::write(root.join("src/broken.rs"), broken).unwrap();
        for args in [&["check", "sqlx-unchecked-non-test", "--json"][..], &args] {
            let output = jig(root, args);
            assert!(!output.status.success());
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                combined.contains("cannot parse SQLx inventory source src/broken.rs:"),
                "{combined}"
            );
            if let Some(expected) = expression_diagnostic {
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                let message = value["error"]["message"].as_str().unwrap();
                assert!(message.contains(expected), "{message}");
            }
        }
        assert_eq!(fs::read_to_string(&todo_path).unwrap(), todo);
    }
}
