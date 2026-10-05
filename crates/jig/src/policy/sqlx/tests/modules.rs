use std::path::Path;

use super::*;
use crate::policy::sqlx::check_non_test;

pub(super) const CALL: &str = "fn example() { let _ = sqlx::query(\"SELECT 1\"); }\n";
pub(super) const MANIFEST: &str = "[package]\nname = \"example-project\"\nversion = \"0.1.0\"\n";

mod cargo_targets;

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

/// A file's directory depends on the declaration that loaded it, so the
/// directory its own name implies is only a guess. Once a declaration
/// resolves the file, the guess must leave no claim behind: here `src/a.rs`
/// is loaded through an explicit path and loads `src/helper.rs`, so the
/// unrelated `src/a/helper.rs` the guess would have named stays production.
#[test]
fn a_guessed_directory_leaves_no_claim_once_a_declaration_resolves_the_file() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"a.rs\"]\nmod cases;\n",
        ),
        ("src/a.rs", "mod helper;\n"),
        ("src/helper.rs", CALL),
        ("src/a/helper.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/a/helper.rs:1`"), "{non_test}");
    assert!(!non_test.contains("`src/helper.rs:1`"), "{non_test}");
    assert!(test.contains("`src/helper.rs:1`"), "{test}");
}

/// Retracting a guessed directory can leave a file nothing loads after all,
/// and such a file is a root of its own, so its declarations still resolve.
#[test]
fn a_file_left_unloaded_by_a_retracted_guess_becomes_a_root() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "#[cfg(test)]\n#[path = \"a.rs\"]\nmod cases;\n",
        ),
        ("src/a.rs", "mod helper;\n"),
        ("src/helper.rs", CALL),
        (
            "src/a/helper.rs",
            "#[cfg(test)]\n#[path = \"../only.rs\"]\nmod only;\n",
        ),
        ("src/only.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/helper.rs:1`"), "{test}");
    assert!(test.contains("`src/only.rs:1`"), "{test}");
}

/// A conventional test path names the file's own calls as test code without
/// saying anything about what reaches it, so what it declares keeps its
/// production classification.
#[test]
fn a_test_named_file_production_loads_does_not_reclassify_what_it_declares() {
    let temp = inventory(&[
        ("src/lib.rs", "mod test_support;\n"),
        (
            "src/test_support.rs",
            &format!("#[path = \"queries.rs\"]\nmod queries;\n{CALL}"),
        ),
        ("src/queries.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/queries.rs:1`"), "{non_test}");
    assert!(test.contains("`src/test_support.rs:3`"), "{test}");
}

/// An attribute that compiles a whole file for tests alone does keep what it
/// declares out of a production build.
#[test]
fn a_cfg_test_file_attribute_reclassifies_what_it_declares() {
    let temp = inventory(&[
        ("src/lib.rs", "mod support;\n"),
        (
            "src/support.rs",
            "#![cfg(test)]\n#[path = \"queries.rs\"]\nmod queries;\n",
        ),
        ("src/queries.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/queries.rs:1`"), "{test}");
}

/// The same file can be loaded twice, and `mod helper;` inside it then names
/// a different file under each loading. Only the one the test loading reaches
/// is test code.
#[test]
fn each_loading_of_a_file_carries_its_own_test_ancestry() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "mod shared;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod cases;\n",
        ),
        ("src/shared.rs", "mod helper;\n"),
        ("src/shared/helper.rs", CALL),
        ("src/helper.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/shared/helper.rs:1`"), "{non_test}");
    assert!(test.contains("`src/helper.rs:1`"), "{test}");
}

/// A declaration that only loads its own file leaves that file a root, so an
/// unreferenced cycle cannot unsettle the classification of anything else.
#[test]
fn a_self_loading_declaration_does_not_disturb_other_files() {
    let temp = inventory(&[
        ("src/cycle.rs", "#[path = \"cycle.rs\"]\nmod again;\n"),
        ("src/lib.rs", "#[cfg(test)]\nmod unit_cases;\n"),
        ("src/unit_cases.rs", "mod fixture;\n"),
        ("src/unit_cases/fixture.rs", CALL),
        ("src/standalone.rs", "#[path = \"keep.rs\"]\nmod keep;\n"),
        ("src/keep.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/keep.rs:1`"), "{non_test}");
    assert!(test.contains("`src/unit_cases/fixture.rs:1`"), "{test}");
}

/// Declarations that load one another cannot compile, so the root set never
/// settles. The inventory then reclassifies nothing rather than guessing,
/// leaving every call site exactly where it already was.
#[test]
fn declarations_that_load_one_another_reclassify_nothing() {
    let temp = inventory(&[
        ("src/a.rs", "#[path = \"b.rs\"]\nmod b;\n"),
        ("src/b.rs", "#[path = \"a.rs\"]\nmod a;\n"),
        ("src/lib.rs", "#[cfg(test)]\nmod unit_cases;\n"),
        ("src/unit_cases.rs", "mod fixture;\n"),
        ("src/unit_cases/fixture.rs", CALL),
        ("src/standalone.rs", "#[path = \"keep.rs\"]\nmod keep;\n"),
        ("src/keep.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/keep.rs:1`"), "{non_test}");
    assert!(
        non_test.contains("`src/unit_cases/fixture.rs:1`"),
        "{non_test}"
    );
    assert!(test.contains("_None_"), "{test}");
}
