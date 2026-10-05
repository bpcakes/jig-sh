//! Classification of the crate roots Cargo compiles.

use super::*;

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

/// A package that turns automatic binary discovery off has no target at
/// `src/bin`, so a helper only a test module declares there is test code.
#[test]
fn disabled_auto_discovery_leaves_no_conventional_target() {
    let temp = inventory(&[
        ("Cargo.toml", &format!("{MANIFEST}autobins = false\n")),
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"bin/helper.rs\"]\nmod helper;\n",
        ),
        ("src/bin/helper.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/bin/helper.rs:1`"), "{test}");
}

/// A target table names its own file even where auto-discovery is off, so a
/// binary declared only by name keeps its production classification.
#[test]
fn a_target_named_without_a_path_stays_non_test() {
    let temp = inventory(&[
        (
            "Cargo.toml",
            &format!("{MANIFEST}autobins = false\n[[bin]]\nname = \"tool\"\n"),
        ),
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"bin/tool.rs\"]\nmod tool_cases;\n",
        ),
        ("src/bin/tool.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/bin/tool.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// `package.build = true` asks for the default build script, so `build.rs` is
/// still the crate root Cargo compiles.
#[test]
fn an_explicitly_enabled_build_script_is_a_production_root() {
    let temp = inventory_rooted(
        "[\".\"]",
        &[
            ("Cargo.toml", &format!("{MANIFEST}build = true\n")),
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

/// A `[lib]` table defines the library target, so a path it names replaces
/// `src/lib.rs` rather than adding to it.
#[test]
fn an_overridden_library_path_leaves_no_default_library_target() {
    let temp = inventory(&[
        (
            "Cargo.toml",
            &format!("{MANIFEST}[lib]\npath = \"src/root.rs\"\n"),
        ),
        (
            "src/root.rs",
            "#[cfg(test)]\n#[path = \"lib.rs\"]\nmod library_cases;\n",
        ),
        ("src/lib.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/lib.rs:1`"), "{test}");
}

/// A 2015-edition package stops discovering a kind of target once it declares
/// one manually; a later edition keeps discovering them.
#[test]
fn edition_decides_whether_a_manual_target_disables_discovery() {
    for (edition, discovered) in [("", false), ("edition = \"2021\"\n", true)] {
        let manifest =
            format!("{MANIFEST}{edition}[[bin]]\nname = \"tool\"\npath = \"src/tool.rs\"\n");
        let temp = inventory(&[
            ("Cargo.toml", &manifest),
            (
                "src/lib.rs",
                "#[cfg(test)]\n#[path = \"bin/helper.rs\"]\nmod helper;\n",
            ),
            ("src/tool.rs", ""),
            ("src/bin/helper.rs", CALL),
        ]);

        let (non_test, test) = sections(temp.path());

        if discovered {
            assert!(non_test.contains("`src/bin/helper.rs:1`"), "{non_test}");
        } else {
            assert!(test.contains("`src/bin/helper.rs:1`"), "{test}");
        }
    }
}
