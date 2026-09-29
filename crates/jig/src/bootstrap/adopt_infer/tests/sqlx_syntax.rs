use super::*;

#[test]
fn sqlx_adoption_detects_rust_macro_syntax() {
    for source in [
        "fn production() { let _ = sqlx::migrate!(); }",
        "fn production() { let _ = sqlx:: // explanatory comment\n migrate!(); }",
        "fn production() { let _ = sqlx /* comment */ :: migrate /* comment */ ! (); }",
        "mod nested { fn production() { let _ = ::r#sqlx::r#migrate!(); } }",
        "static MIGRATOR: Migrator = sqlx::migrate!(\"migrations\");",
        "sqlx::migrate!()",
        "{ let migrator = sqlx::migrate!(); migrator }",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("source.rs"), source).unwrap();
        let mut warnings = Vec::new();

        let inference = infer_sqlx(temp.path(), &mut warnings);

        assert!(inference.enabled.value, "{source}: {warnings:?}");
        assert_eq!(
            inference.enabled.sources,
            ["sqlx::migrate! macro in source.rs"],
            "{source}"
        );
        assert!(warnings.iter().all(|warning| !warning.contains("parse")));
    }
}

#[test]
fn sqlx_adoption_ignores_text_and_opaque_macro_tokens() {
    for source in [
        "const TEXT: &str = \"sqlx::migrate!()\";",
        "const TEXT: &str = r###\"sqlx::migrate!()\"###;",
        "/* multiline comment\n sqlx::migrate!()\n */",
        "/// sqlx::migrate!()\nfn example() {}",
        "fn example() { /* sqlx::migrate!() */ }",
        "macro_rules! example { () => { sqlx::migrate!() }; }",
        "example!(sqlx::migrate!());",
        "fn example() { let _ = other::sqlx::migrate!(); }",
        "fn example() { let _ = sqlx::migrate(); }",
        "fn example() { let _ = sqlx::migrate_more!(); }",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("source.rs"), source).unwrap();
        let mut warnings = Vec::new();

        let inference = infer_sqlx(temp.path(), &mut warnings);

        assert!(!inference.enabled.value, "{source}");
        assert!(warnings.is_empty(), "{source}: {warnings:?}");
    }
}

#[test]
fn sqlx_adoption_warns_on_invalid_rust_without_inventing_signals() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("broken.rs"),
        "sqlx::migrate!(); invalid trailing syntax",
    )
    .unwrap();
    let mut warnings = Vec::new();

    let inference = infer_sqlx(temp.path(), &mut warnings);

    assert!(!inference.enabled.value);
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("broken.rs")
                && warning.contains("Rust file parse:")
                && warning.contains("expression parse:")
        }),
        "{warnings:?}"
    );

    fs::write(temp.path().join("valid.rs"), "sqlx::migrate!()").unwrap();
    let inference = infer_sqlx(temp.path(), &mut Vec::new());
    assert!(inference.enabled.value);
    assert_eq!(
        inference.enabled.sources,
        ["sqlx::migrate! macro in valid.rs"]
    );
}
