//! Declarations inside a block, which Rust requires a `#[path]` for.

use super::*;

/// A production function body can load a file through a `#[path]` module, and
/// Rust resolves it against the directory holding the declaring file. A test
/// module loading the same file does not make its queries test code.
#[test]
fn a_block_local_declaration_keeps_a_shared_file_in_production() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "mod production;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod cases;\n",
        ),
        (
            "src/production.rs",
            "pub fn load() {\n    #[path = \"shared.rs\"]\n    mod shared;\n}\n",
        ),
        ("src/shared.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/shared.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// A block drops the module name its file keeps pending, and an inline module
/// inside that block extends the directory from there.
#[test]
fn a_block_resolves_against_the_directory_holding_its_file() {
    let temp = inventory(&[
        ("src/lib.rs", "#[cfg(test)]\nmod unit_cases;\n"),
        (
            "src/unit_cases.rs",
            "fn setup() {\n    #[path = \"beside.rs\"]\n    mod beside;\n\
             \n    mod inner {\n        #[path = \"nested.rs\"]\n        mod nested;\n    }\n}\n",
        ),
        ("src/beside.rs", CALL),
        ("src/inner/nested.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/beside.rs:1`"), "{test}");
    assert!(test.contains("`src/inner/nested.rs:1`"), "{test}");
}

/// A `#[cfg(test)]` function's body is compiled for tests alone, so what it
/// declares is test code too.
#[test]
fn a_cfg_test_function_body_declares_test_modules() {
    let temp = inventory(&[
        ("src/lib.rs", "mod support;\n"),
        (
            "src/support.rs",
            "#[cfg(test)]\nfn setup() {\n    #[path = \"fixture.rs\"]\n    mod fixture;\n}\n",
        ),
        ("src/fixture.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("_None_"), "{non_test}");
    assert!(test.contains("`src/fixture.rs:1`"), "{test}");
}
