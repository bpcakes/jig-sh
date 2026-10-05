use super::*;

#[test]
fn inventory_commands_bound_recursive_syntax_and_preserve_failed_output() {
    let temp = inventory_root();
    let root = temp.path();
    let todo_path = root.join("docs/sqlx-unchecked-queries-todo.md");
    let sentinel = "existing inventory must survive a failed scan\n";
    fs::write(&todo_path, sentinel).unwrap();
    let commands: [&[&str]; 2] = [
        &["check", "sqlx-unchecked-non-test", "--json"],
        &["generate-sqlx-unchecked-queries-todo", "--json"],
    ];
    for expression in [
        format!("{}0{}", "(".repeat(5_000), ")".repeat(5_000)),
        format!("{}0{}", "(".repeat(200_000), ")".repeat(200_000)),
        format!("{}0", "!".repeat(5_000)),
        format!("0{}", "+0".repeat(5_000)),
        format!("value{}", ".method()".repeat(5_000)),
        format!("None::<{}u8{}>", "Vec<".repeat(5_000), ">".repeat(5_000)),
        // Separators cannot erase enclosing block or generic-type depth.
        format!("{}0{}", "{ let _ = 0; ".repeat(5_000), "}".repeat(5_000)),
        format!(
            "None::<{}u8{}>",
            "Pair<u8,".repeat(5_000),
            ">".repeat(5_000)
        ),
        // Tokens after a group still contribute to its recursive AST path.
        format!(
            "{}0{}{}",
            "(".repeat(1_000),
            ")".repeat(1_000),
            ".method()".repeat(400)
        ),
    ] {
        // Full files and include! expression fragments share the resource
        // boundary, as do expression macros visited inside their ASTs.
        for source in [
            format!("fn example() {{ sqlx::query(\"SELECT 1\"); let _ = {expression}; }}"),
            format!("{{ sqlx::query(\"SELECT 1\"); {expression} }}"),
            format!("fn example() {{ let _ = vec![sqlx::query(\"SELECT 1\"), {expression}]; }}"),
        ] {
            fs::write(root.join("src/resource.rs"), source).unwrap();
            for args in commands {
                let output = jig(root, args);
                assert!(matches!(output.status.code(), Some(1 | 2)), "{output:?}");
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                let error = value["error"]["message"].as_str().unwrap();
                assert!(error.contains("src/resource.rs"), "{error}");
                assert!(error.contains("token complexity limit"), "{error}");
                assert_eq!(fs::read_to_string(&todo_path).unwrap(), sentinel);
            }
        }
    }
    // The original aborting input is valid and near the supported budget:
    // the worker must parse, visit, and destroy its AST successfully.
    let source = format!(
        "fn example() {{ let _ = {}sqlx::query(\"SELECT 1\"){}; }}",
        "(".repeat(2_000),
        ")".repeat(2_000)
    );
    fs::write(root.join("src/resource.rs"), source).unwrap();
    check(root, 1);
    let output = jig(root, commands[1]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], 1);
    assert!(
        fs::read_to_string(&todo_path)
            .unwrap()
            .contains("src/resource.rs:1")
    );
}

#[test]
fn inventory_accepts_large_flat_definitions_and_still_finds_queries() {
    let temp = inventory_root();
    let root = temp.path();
    let mut definitions =
        String::from("struct Contract { name: &'static str, schedule: &'static str }\n");
    for index in 0..400 {
        definitions.push_str(&format!(
            "pub const JOB_{index}: &str = \"example-job-{index}\";\n\
             pub const SCHEDULE_{index}: &str = \"0 * * * *\";\n\
             pub const CONTRACT_{index}: Contract = Contract {{\n\
                 name: JOB_{index}, schedule: SCHEDULE_{index},\n\
             }};\n"
        ));
    }
    // Thousands of tokens, but each declaration has a shallow AST.
    assert!(definitions.lines().count() > 1_091);
    fs::write(root.join("src/definitions.rs"), &definitions).unwrap();
    check(root, 0);
    let args = ["generate-sqlx-unchecked-queries-todo", "--json"];
    let output = jig(root, &args);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], 0);

    // The same breadth is valid inside modules and function blocks.
    for source in [
        format!("mod definitions {{ {definitions} }}"),
        format!("fn definitions() {{ {definitions} }}"),
    ] {
        fs::write(root.join("src/definitions.rs"), source).unwrap();
        check(root, 0);
    }

    // Broad files must be scanned completely, including calls near the end.
    definitions.push_str("fn example() { sqlx::query(\"SELECT 1\"); }\n");
    fs::write(root.join("src/definitions.rs"), definitions).unwrap();
    check(root, 1);
    let output = jig(root, &args);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], 1);
    assert!(
        fs::read_to_string(root.join("docs/sqlx-unchecked-queries-todo.md"))
            .unwrap()
            .contains("src/definitions.rs:2002")
    );
}
