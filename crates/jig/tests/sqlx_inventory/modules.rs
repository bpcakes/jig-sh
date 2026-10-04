use super::*;

/// Writes `contents` at `path` under `root`, creating parent directories.
fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Generates the inventory and returns its non-test and test sections.
fn inventory(root: &Path, expected_non_test: usize) -> (String, String) {
    check(root, expected_non_test);
    let output = jig(root, &["generate-sqlx-unchecked-queries-todo", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["non_test_count"], expected_non_test, "{value}");
    let body = fs::read_to_string(root.join("docs/sqlx-unchecked-queries-todo.md")).unwrap();
    let (non_test, test) = body.split_once("## TODO Items (Test Code)").unwrap();
    (non_test.to_string(), test.to_string())
}

/// Extracting a test helper into its own file must not turn it into
/// production code, so an external `#[cfg(test)]` module tree and the
/// equivalent inline one report the same counts.
#[test]
fn external_cfg_test_modules_match_their_inline_equivalent() {
    let temp = inventory_root();
    let root = temp.path();
    let call = |query| format!("fn example() {{ let _ = sqlx::query(\"{query}\"); }}\n");

    write(
        root,
        "src/lib.rs",
        "#[cfg(test)]\nmod unit_cases;\nmod production;\n",
    );
    write(
        root,
        "src/unit_cases.rs",
        "mod fixture;\n#[path = \"support/helper.rs\"]\nmod helper;\n\
         #[path = \"../shared/dual.rs\"]\nmod dual;\n",
    );
    write(root, "src/unit_cases/fixture.rs", &call("SELECT 1"));
    write(root, "src/unit_cases/support/helper.rs", &call("SELECT 2"));
    write(
        root,
        "src/production.rs",
        "#[path = \"../shared/dual.rs\"]\nmod dual;\n",
    );
    write(root, "src/shared/dual.rs", &call("SELECT 3"));

    let (non_test, test) = inventory(root, 1);
    // A nested descendant and an explicit `#[path]` target are test code; the
    // helper the production tree also declares stays non-test.
    assert!(
        test.contains("- [ ] `src/unit_cases/fixture.rs:1`"),
        "{test}"
    );
    assert!(
        test.contains("- [ ] `src/unit_cases/support/helper.rs:1`"),
        "{test}"
    );
    assert!(
        non_test.contains("- [ ] `src/shared/dual.rs:1`"),
        "{non_test}"
    );
    assert!(non_test.contains("- Test call sites: 2\n"), "{non_test}");

    for path in [
        "src/unit_cases.rs",
        "src/unit_cases/fixture.rs",
        "src/unit_cases/support/helper.rs",
        "src/production.rs",
        "src/shared/dual.rs",
    ] {
        fs::remove_file(root.join(path)).unwrap();
    }
    write(
        root,
        "src/lib.rs",
        &format!(
            "#[cfg(test)]\nmod unit_cases {{\n    mod fixture {{ {} }}\n\
             mod support {{ mod helper {{ {} }} }}\n}}\nmod production {{ {} }}\n",
            call("SELECT 1"),
            call("SELECT 2"),
            call("SELECT 3")
        ),
    );

    let (inline_non_test, inline_test) = inventory(root, 1);
    assert!(
        inline_non_test.contains("- Test call sites: 2\n"),
        "{inline_non_test}"
    );
    assert_eq!(inline_test.matches("- [ ] `").count(), 2, "{inline_test}");
}

/// The documented boundary: resolution follows `mod` items, not `include!`
/// or cfg predicates it does not evaluate.
#[test]
fn unresolved_relationships_keep_their_documented_classification() {
    let temp = inventory_root();
    let root = temp.path();
    let call = "fn example() { let _ = sqlx::query(\"SELECT 1\"); }\n";

    write(
        root,
        "src/lib.rs",
        "#[cfg(test)]\nmod unit_cases;\n#[cfg(all(test))]\nmod combined;\n",
    );
    // `include!` relationships stay unresolved, so an included fragment is
    // classified by its own path even inside a test-only module tree.
    write(
        root,
        "src/unit_cases.rs",
        "const EXAMPLE: u32 = include!(\"included.rs\");\n",
    );
    write(root, "src/included.rs", "42\n");
    write(root, "src/combined.rs", call);

    let (non_test, test) = inventory(root, 1);
    assert!(non_test.contains("- [ ] `src/combined.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");

    write(root, "src/included.rs", "sqlx::query(\"SELECT 1\")\n");
    let (non_test, _) = inventory(root, 2);
    assert!(non_test.contains("- [ ] `src/included.rs:1`"), "{non_test}");
    assert!(
        non_test.contains("relationships are not resolved"),
        "{non_test}"
    );
}
