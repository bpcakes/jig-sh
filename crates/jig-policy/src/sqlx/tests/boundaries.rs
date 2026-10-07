use super::*;
use crate::sqlx::check_non_test;

#[test]
fn call_delimiters_allow_comments_and_newlines() {
    for separator in [
        "",
        " ",
        "\n        ",
        " // explanatory comment\n        ",
        " /* explanatory comment */ ",
        " /* outer\n /* nested */\n */\n        ",
        "\n // sqlx::query(\"ignored\")\n\n        ",
    ] {
        for (name, delimiter, checked) in [
            ("sqlx::query", "(\"SELECT 1\")", false),
            ("sqlx::query_as", "::<_, Row>(\"SELECT 1\")", false),
            ("sqlx::query_scalar", "::\n<i64>(\"SELECT 1\")", false),
            ("sqlx::query", "!(\"SELECT 1\")", true),
            ("sqlx::query_as", "!(Row, \"SELECT 1\")", true),
            ("sqlx::query_file", "!(\"query.sql\")", true),
            ("sqlx::query_file_as", "!(Row, \"query.sql\")", true),
            ("sqlx::query_file_scalar", "!(\"query.sql\")", true),
        ] {
            let text = format!("fn production() {{\n{name}{separator}{delimiter};\n}}\n");
            for (path, is_test) in [
                ("crates/app/src/lib.rs", false),
                ("crates/app/tests/integration.rs", true),
                ("tests/integration.rs", true),
            ] {
                let calls = scan_sqlx_calls(path, &text).unwrap();
                assert_eq!(calls.len(), 1, "{path}: {text}");
                let call = &calls[0];
                assert_eq!(call.path, path);
                assert_eq!(call.line, 2, "{text}");
                assert_eq!(call.function, name);
                assert_eq!(call.checked, checked, "{text}");
                assert_eq!(call.is_test, is_test, "{path}: {text}");
            }
        }
    }
}

#[test]
fn multiline_calls_preserve_cfg_test_scope_and_production_locations() {
    let text = "#[cfg(test)]\nmod tests {\n    fn test() {\n        sqlx::query // test\n            (\"SELECT 1\");\n    }\n}\nfn production() {\nsqlx::query /* production\n */ (\"SELECT 2\");\n}\n";
    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text).unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].line, 4);
    assert!(calls[0].is_test);
    assert_eq!(calls[1].line, 9);
    assert!(!calls[1].is_test);
    assert!(calls.iter().all(|call| !call.checked));
}

#[test]
fn lookahead_stops_at_intervening_code_and_does_not_match_longer_names() {
    let text = r#"
fn production() {
    let query = sqlx::query; // a function reference is not a call
    (query)("SELECT 1");
    let _ = sqlx::query_with
        ("SELECT 1", arguments);
    let _ = my_sqlx::query // another crate
        ("SELECT 1");
}
"#;
    assert!(scan_sqlx_calls("src/lib.rs", text).unwrap().is_empty());
}

#[test]
fn native_check_and_todo_share_comment_separated_call_inventory() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config("rust_crate_roots = [\".\"]\nrust_test_command = \"cargo test\"\n")
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
    assert!(
        Command::new("git")
            .current_dir(temp.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::create_dir_all(temp.path().join("tests")).unwrap();
    let production = "fn production() {\n    let _ = sqlx::query // explanatory comment\n        (\"SELECT 1\");\n}\n";
    fs::write(temp.path().join("tests/integration.rs"), production).unwrap();
    fs::write(
        temp.path().join("src/lib.rs"),
        format!("#[cfg(test)]\nmod tests {{\n{production}}}\n"),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let before = check_non_test(&ctx).unwrap();
    assert_eq!(before["non_test_count"], 0);
    assert_eq!(before["ok"], true);

    fs::write(temp.path().join("src/production.rs"), production).unwrap();
    let after = check_non_test(&ctx).unwrap();
    assert_eq!(after["non_test_count"], 1);
    assert_eq!(after["ok"], false);
    fs::write(
        temp.path().join("src/checked.rs"),
        "fn checked() { sqlx::query // checked\n !(\"SELECT 1\"); }",
    )
    .unwrap();
    let generated = generate_todo(&ctx, &SqlxTodoInput { output: None }).unwrap();
    assert_eq!(generated["non_test_count"], 1);
    let body = fs::read_to_string(temp.path().join("docs/sqlx-unchecked-queries-todo.md")).unwrap();
    assert!(body.contains("- Unchecked call sites: 3\n"));
    assert!(body.contains("- Compile-checked macro call sites already present: 1\n"));
    assert!(body.contains("- Test call sites: 2\n"));
    assert!(body.contains("- [ ] `src/production.rs:2`: `sqlx::query`"));
    assert!(body.contains("not complete Rust syntax coverage"));
}

#[test]
fn native_check_and_todo_keep_calls_before_a_nested_macro_parse_failure() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config("rust_crate_roots = [\"src\"]\nrust_test_command = \"cargo test\"\n")
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
    assert!(
        Command::new("git")
            .current_dir(temp.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/lib.rs"),
        r#"fn production() {
    matches!(load(sqlx::query("SELECT 1")), Ok(ref row));
    matches!(load(sqlx::query!("SELECT 2")), Ok(ref row));
}
#[cfg(test)]
mod unit {
    fn test() {
        matches!(load(sqlx::query("SELECT 3")), Ok(ref row));
    }
}
"#,
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let checked = check_non_test(&ctx).unwrap();
    assert_eq!(checked["ok"], false);
    assert_eq!(checked["non_test_count"], 1);
    let generated = generate_todo(&ctx, &SqlxTodoInput { output: None }).unwrap();
    assert_eq!(generated["non_test_count"], 1);
    let body = fs::read_to_string(temp.path().join("docs/sqlx-unchecked-queries-todo.md")).unwrap();
    assert!(body.contains("- Unchecked call sites: 2\n"), "{body}");
    assert!(
        body.contains("- Compile-checked macro call sites already present: 1\n"),
        "{body}"
    );
    let (production, tests) = body.split_once("## TODO Items (Test Code)").unwrap();
    assert!(
        production.contains("- [ ] `src/lib.rs:2`: `sqlx::query`"),
        "{body}"
    );
    assert!(!production.contains("`src/lib.rs:8`"), "{body}");
    assert!(
        tests.contains("- [ ] `src/lib.rs:8`: `sqlx::query`"),
        "{body}"
    );
}
