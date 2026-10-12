use super::scan_sqlx_calls;

#[test]
fn expression_fragments_keep_calls_spans_and_test_paths() {
    for path in [
        "src/value.rs",
        "tests/value.rs",
        "crates/example/tests/value.rs",
    ] {
        assert!(scan_sqlx_calls(path, "42\n").unwrap().is_empty());
        for text in [
            "// included expression\nsqlx::query // comment\n(\"SELECT 1\")\n",
            "{\nsqlx::query(\"SELECT 1\")\n}\n",
        ] {
            let calls = scan_sqlx_calls(path, text).unwrap();
            assert_eq!(calls.len(), 1, "{path}: {text}");
            assert_eq!(calls[0].line, 2);
            assert_eq!(calls[0].function, "sqlx::query");
            assert!(!calls[0].checked);
            assert_eq!(calls[0].is_test, path != "src/value.rs");
        }
    }
}

#[test]
fn expression_blocks_reuse_module_and_macro_visitors() {
    let text = r#"{
    #[cfg(test)] mod unit { fn sample() { sqlx::query("test"); } }
    let _ = sqlx::query!("checked");
    matches!(load(sqlx::query("production")), Ok(ref row))
}"#;
    let calls = scan_sqlx_calls("src/value.rs", text).unwrap();
    let actual = calls
        .iter()
        .map(|call| (call.line, call.checked, call.is_test))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [(2, false, true), (3, true, false), (4, false, false)]
    );
}

#[test]
fn malformed_fragment_retains_both_parser_diagnostics() {
    let text = "{\n    let value = 42;\n    let broken = ;\n    value\n}\n";
    let error = scan_sqlx_calls("src/value.rs", text)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.starts_with("cannot parse SQLx inventory source src/value.rs:1:1:"),
        "{error}"
    );
    assert!(error.contains("(file parse)"), "{error}");
    assert!(
        error.contains("expression parse at src/value.rs:3:18: expected an expression"),
        "{error}"
    );
}

#[test]
fn fragments_must_parse_completely() {
    for text in [
        "42 trailing",
        "sqlx::query(\"SELECT 1\") trailing",
        "{ sqlx::query(\"SELECT 1\"); } trailing",
        "{ sqlx::query(\"SELECT 1\");",
        "fn broken(",
    ] {
        let error = scan_sqlx_calls("src/value.rs", text).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("cannot parse SQLx inventory source src/value.rs:"),
            "{text}: {error}"
        );
    }
}
