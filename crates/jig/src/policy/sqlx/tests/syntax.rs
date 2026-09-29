use super::*;
use crate::policy::sqlx::check_non_test;

#[test]
fn ast_recognizes_split_paths_parentheses_and_nested_calls() {
    let text = r#"fn production() {
    let _ = ::sqlx /* namespace */ ::
        query /* call */ ("SELECT 1");
    let _ = (sqlx::query_as::<_, Row>)("SELECT 2");
    let _ = sqlx::query_scalar!("SELECT 3");
    consume(sqlx::query("SELECT 4"), sqlx::query("SELECT 5"));
}
"#;
    let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
    let actual = calls
        .iter()
        .map(|call| {
            (
                call.line,
                call.function.as_str(),
                call.checked,
                call.is_test,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            (2, "sqlx::query", false, false),
            (4, "sqlx::query_as", false, false),
            (5, "sqlx::query_scalar", true, false),
            (6, "sqlx::query", false, false),
            (6, "sqlx::query", false, false),
        ]
    );
}

#[test]
fn ast_distinguishes_sibling_modules_on_the_same_line() {
    let text = r#"mod production { fn f() { sqlx::query("SELECT 1"); } } #[cfg(test)] mod unit { fn f() { sqlx::query("SELECT 2"); } } mod other { fn f() { sqlx::query("SELECT 3"); } }"#;
    let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
    assert_eq!(
        calls.iter().map(|call| call.is_test).collect::<Vec<_>>(),
        [false, true, false]
    );
}

#[test]
fn ast_handles_multiline_and_inner_test_attributes_without_evaluating_cfg() {
    for text in [
        "#[cfg(\n/* comment */ test\n)] mod unit { fn f() { sqlx::query(\"SELECT 1\"); } }",
        "mod unit { #![cfg(test)] fn f() { sqlx::query(\"SELECT 1\"); } }",
        "#![cfg(test)]\nfn f() { sqlx::query(\"SELECT 1\"); }",
    ] {
        let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].is_test, "{text}");
    }
    for attr in [
        "cfg(not(test))",
        "cfg(feature = \"test\")",
        "cfg(any(test, feature = \"example\"))",
    ] {
        let text = format!("#[{attr}] mod example {{ fn f() {{ sqlx::query(\"SELECT 1\"); }} }}");
        let calls = scan_sqlx_calls("src/lib.rs", &text).unwrap();
        assert_eq!(calls.len(), 1);
        assert!(!calls[0].is_test, "{attr}");
    }
}

#[test]
fn ast_does_not_invent_calls_from_paths_literals_or_macro_tokens() {
    let text = r##"
macro_rules! example { () => { sqlx::query("SELECT 1") }; }
fn production() {
    let _reference = sqlx::query_as::<_, Row>;
    let _qualified = <T as sqlx>::query("SELECT 1");
    let _other = other::sqlx::query("SELECT 1");
    let _literal = "multiline string
        sqlx::query(\"not code\")";
    let _raw = r#"sqlx::query("not code")"#;
    opaque! { sqlx::query("macro-specific input") }
}
"##;
    assert!(scan_sqlx_calls("src/lib.rs", text).unwrap().is_empty());
}

#[test]
fn ast_reports_source_lines_with_unicode_and_crlf() {
    let text = "// café\r\nfn production() {\r\n    let _ = sqlx::query\r\n        (\"SELECT 1\");\r\n}\r\n";
    let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].line, 3);
}

#[test]
fn invalid_source_fails_the_check_and_preserves_the_existing_todo() {
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
    let output = PathBuf::from("todo.md");
    fs::write(temp.path().join(&output), "existing inventory\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for (bytes, expected) in [
        (
            b"fn production() {\n".as_slice(),
            "cannot parse SQLx inventory source src/invalid.rs:1:",
        ),
        (
            b"\xff".as_slice(),
            "cannot read SQLx inventory source src/invalid.rs",
        ),
    ] {
        fs::write(temp.path().join("src/invalid.rs"), bytes).unwrap();
        let error = check_non_test(&ctx).unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
        let error = generate_todo(
            &ctx,
            &SqlxTodoInput {
                output: Some(output.clone()),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
        assert_eq!(
            fs::read_to_string(temp.path().join(&output)).unwrap(),
            "existing inventory\n"
        );
    }
}

#[test]
fn ast_reads_expression_macro_input_but_not_opaque_grammars() {
    let text = r#"fn production() {
    let _ = vec![sqlx::query("SELECT 1")];
    let _ = vec![sqlx::query("SELECT 2"); 2];
    println!("{:?}", sqlx::query_as::<_, Row>("SELECT 3"));
    assert!(matches!(sqlx::query_scalar("SELECT 4"), _));
    let _ = std::vec![sqlx::query!("SELECT 5")];
    opaque! { sqlx::query("macro-specific input") }
    custom_vec![sqlx::query("macro-specific input")];
}
"#;
    let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
    let actual = calls
        .iter()
        .map(|call| (call.line, call.function.as_str(), call.checked))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            (2, "sqlx::query", false),
            (3, "sqlx::query", false),
            (4, "sqlx::query_as", false),
            (5, "sqlx::query_scalar", false),
            (6, "sqlx::query", true),
        ]
    );
}

#[test]
fn ast_keeps_expressions_read_before_grammar_it_cannot_follow() {
    let text = r#"fn production() {
    let _ = matches!(sqlx::query("SELECT 1"), Some(_) if true);
    // A shadowing macro may claim a familiar name for an internal rule.
    assert!(@internal sqlx::query("SELECT 2"));
    println!("{}", @internal sqlx::query("SELECT 3"));
}
"#;
    let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].line, 2);
    assert!(!calls[0].checked);
}

#[test]
fn ast_keeps_macro_prefix_when_a_nested_pattern_cannot_parse_as_an_expression() {
    for pattern in ["Ok(ref row)", "Some([ref row])", "[ref row]"] {
        let text = format!(
            "fn production() {{\n    assert!(matches!(load(sqlx::query(\"SELECT 1\"),\n        sqlx::query!(\"SELECT 2\")), {pattern}));\n}}\n"
        );
        let calls = scan_sqlx_calls("src/lib.rs", &text).unwrap();
        let actual = calls
            .iter()
            .map(|call| {
                (
                    call.line,
                    call.function.as_str(),
                    call.checked,
                    call.is_test,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                (2, "sqlx::query", false, false),
                (3, "sqlx::query", true, false)
            ],
            "{pattern}"
        );
    }
}

#[test]
fn ast_normalizes_raw_identifiers_and_reports_canonical_names() {
    let text = r##"
#[r#cfg(r#test)]
mod unit {
    fn f() {
        let _ = r#sqlx::r#query("SELECT 1");
        let _ = r#sqlx::query_as!(Row, "SELECT 2");
    }
}
"##;
    let calls = scan_sqlx_calls("src/lib.rs", text).unwrap();
    let actual = calls
        .iter()
        .map(|call| {
            (
                call.line,
                call.function.as_str(),
                call.checked,
                call.is_test,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            (5, "sqlx::query", false, true),
            (6, "sqlx::query_as", true, true),
        ]
    );
}
