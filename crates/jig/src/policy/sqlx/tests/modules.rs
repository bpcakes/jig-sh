use std::path::Path;

use super::*;
use crate::policy::sqlx::check_non_test;

const CALL: &str = "fn example() { let _ = sqlx::query(\"SELECT 1\"); }\n";
const MANIFEST: &str = "[package]\nname = \"example-project\"\nversion = \"0.1.0\"\n";

/// Builds a Git repository whose only crate root is `src` and writes each
/// `(path, contents)` pair, creating parent directories as needed.
fn inventory(files: &[(&str, &str)]) -> tempfile::TempDir {
    inventory_rooted("[\"src\"]", files)
}

/// The same repository with `roots` as the configured Rust crate roots.
fn inventory_rooted(roots: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(format!(
            "rust_crate_roots = {roots}\nrust_test_command = \"cargo test\"\n"
        ))
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
    for (path, contents) in files {
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    temp
}

/// The non-test and test sections of the generated inventory. Every call stays
/// visible in exactly one of them, so the sections carry the classification.
fn sections(root: &Path) -> (String, String) {
    let ctx = RepoContext::load_from(root).unwrap();
    let generated = generate_todo(&ctx, &SqlxTodoInput { output: None }).unwrap();
    let body = fs::read_to_string(root.join("docs/sqlx-unchecked-queries-todo.md")).unwrap();
    let (non_test, test) = body.split_once("## TODO Items (Test Code)").unwrap();
    let non_test_count = non_test.matches("- [ ] `").count();
    assert_eq!(generated["non_test_count"], non_test_count, "{body}");
    assert_eq!(
        check_non_test(&ctx).unwrap()["non_test_count"],
        non_test_count,
        "{body}"
    );
    (non_test.to_string(), test.to_string())
}

#[test]
fn nested_descendants_of_a_cfg_test_module_are_test_code() {
    let temp = inventory(&[
        ("src/lib.rs", "#[cfg(test)]\nmod unit_cases;\n"),
        ("src/unit_cases/mod.rs", "mod fixture;\n"),
        ("src/unit_cases/fixture.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/unit_cases/fixture.rs:1`"), "{test}");
}

#[test]
fn path_attributes_resolve_against_the_source_directory() {
    let temp = inventory(&[
        ("src/lib.rs", "#[cfg(test)]\nmod unit_cases;\n"),
        (
            "src/unit_cases.rs",
            "#[path = \"support/helper.rs\"]\nmod helper;\n",
        ),
        ("src/support/helper.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/support/helper.rs:1`"), "{test}");
}

#[test]
fn a_path_attribute_on_an_inline_module_directs_its_children() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"cases\"]\nmod unit_cases {\n    mod fixture;\n}\n",
        ),
        ("src/cases/fixture.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/cases/fixture.rs:1`"), "{test}");
}

#[test]
fn raw_identifier_modules_resolve_to_their_canonical_file_name() {
    let temp = inventory(&[
        ("src/lib.rs", "#[cfg(test)]\nmod r#type;\n"),
        ("src/type.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/type.rs:1`"), "{test}");
}

/// A helper the production module tree still reaches describes production
/// behavior, whichever other module tree also declares it.
#[test]
fn a_helper_reachable_from_production_stays_non_test() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[cfg(test)]\nmod unit_cases;\nmod production;\n",
        ),
        (
            "src/unit_cases.rs",
            "#[path = \"shared/dual.rs\"]\nmod dual;\n",
        ),
        (
            "src/production.rs",
            "#[path = \"shared/dual.rs\"]\nmod dual;\n",
        ),
        ("src/shared/dual.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/shared/dual.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// Only an exact `#[cfg(test)]` classifies a module tree; the inventory does
/// not evaluate arbitrary cfg predicates.
#[test]
fn other_cfg_predicates_do_not_classify_descendants_as_test() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[cfg(feature = \"extra\")]\nmod extra;\n#[cfg(all(test))]\nmod combined;\n",
        ),
        ("src/extra.rs", CALL),
        ("src/combined.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/extra.rs:1`"), "{non_test}");
    assert!(non_test.contains("`src/combined.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// An unresolvable declaration leaves its target classified by the target's
/// own path and attributes, as it was before module edges were followed.
#[test]
fn unresolvable_declarations_leave_the_existing_classification() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"/absolute/helper.rs\"]\nmod absolute;\n\
             #[cfg(test)]\n#[path = \"../../escaped.rs\"]\nmod escaped;\n",
        ),
        ("src/helper.rs", CALL),
        ("src/tests/helper.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/helper.rs:1`"), "{non_test}");
    assert!(test.contains("`src/tests/helper.rs:1`"), "{test}");
}

/// Declarations that only claim each other never shed their production claim,
/// so a cycle no production root enters is not quietly reclassified.
#[test]
fn a_declaration_cycle_keeps_its_own_classification() {
    let temp = inventory(&[
        (
            "src/first.rs",
            "#[path = \"second.rs\"]\nmod second;\nfn example() { let _ = sqlx::query(\"SELECT 1\"); }\n",
        ),
        ("src/second.rs", "#[path = \"first.rs\"]\nmod first;\n"),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/first.rs:3`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// A `#[path]` declaration loads a file that owns the directory it sits in,
/// so its own children resolve beside it and not under its name.
#[test]
fn a_path_loaded_module_resolves_its_children_beside_itself() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[path = \"loader.rs\"]\nmod production;\n#[cfg(test)]\nmod shared;\n",
        ),
        ("src/loader.rs", "mod shared;\n"),
        ("src/shared.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/shared.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// Cargo compiles a discovered crate entrypoint whatever else declares it as
/// a module, so a test module loading one cannot turn its queries into test
/// code.
#[test]
fn a_discovered_entrypoint_declared_as_a_test_module_stays_non_test() {
    let temp = inventory(&[
        ("Cargo.toml", MANIFEST),
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"main.rs\"]\nmod binary_cases;\n",
        ),
        ("src/main.rs", &format!("mod runner;\n{CALL}")),
        ("src/runner.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/main.rs:2`"), "{non_test}");
    assert!(non_test.contains("`src/runner.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// Cargo compiles an explicitly configured target path as its own crate root,
/// so declaring that file as a test module cannot hide its queries.
#[test]
fn a_manifest_configured_target_declared_as_a_test_module_stays_non_test() {
    let temp = inventory(&[
        (
            "Cargo.toml",
            &format!("{MANIFEST}[[bin]]\nname = \"server\"\npath = \"src/server.rs\"\n"),
        ),
        ("src/lib.rs", "#[cfg(test)]\nmod server;\n"),
        ("src/server.rs", &format!("mod handler;\n{CALL}")),
        ("src/handler.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/server.rs:2`"), "{non_test}");
    assert!(non_test.contains("`src/handler.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// A manifest the inventory cannot parse would silently drop the targets it
/// configures, so it fails the inventory instead and preserves the report.
#[test]
fn an_unparseable_manifest_fails_the_inventory() {
    let temp = inventory(&[
        ("Cargo.toml", "[package\nname = \"example-project\"\n"),
        ("src/lib.rs", CALL),
    ]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = check_non_test(&ctx).unwrap_err().to_string();

    assert!(
        error.contains("cannot parse SQLx inventory manifest Cargo.toml"),
        "{error}"
    );
    assert!(
        generate_todo(&ctx, &SqlxTodoInput { output: None }).is_err(),
        "a failed manifest parse must not write a report"
    );
    assert!(
        !temp
            .path()
            .join("docs/sqlx-unchecked-queries-todo.md")
            .exists()
    );
}

/// Cargo discovers a build script at the package root without
/// `package.build` naming it, and it resolves its children beside itself.
#[test]
fn a_discovered_build_script_is_a_production_root() {
    let temp = inventory_rooted(
        "[\".\"]",
        &[
            ("Cargo.toml", MANIFEST),
            ("build.rs", "mod helper;\n"),
            (
                "src/lib.rs",
                "#[cfg(test)]\n#[path = \"../helper.rs\"]\nmod helper;\n",
            ),
            ("helper.rs", CALL),
        ],
    );

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`helper.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// A module named `main` is an ordinary module, not a Cargo target, so
/// extracting a test helper into one still classifies it as test code.
#[test]
fn a_nested_module_named_like_an_entrypoint_is_not_a_cargo_target() {
    let temp = inventory(&[
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", "#[cfg(test)]\nmod unit_cases;\n"),
        ("src/unit_cases/mod.rs", "mod main;\nmod lib;\n"),
        ("src/unit_cases/main.rs", CALL),
        ("src/unit_cases/lib.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/unit_cases/main.rs:1`"), "{test}");
    assert!(test.contains("`src/unit_cases/lib.rs:1`"), "{test}");
}

/// Cargo discovers every `src/bin/<name>.rs` as a binary, including one named
/// `main.rs`, which is not the nested `<name>/main.rs` shape.
#[test]
fn a_directly_discovered_binary_named_main_stays_non_test() {
    let temp = inventory(&[
        ("Cargo.toml", MANIFEST),
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"bin/main.rs\"]\nmod binary_cases;\n",
        ),
        ("src/bin/main.rs", &format!("mod support;\n{CALL}")),
        ("src/bin/support.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/bin/main.rs:2`"), "{non_test}");
    assert!(non_test.contains("`src/bin/support.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}
