//! How attributes and identifiers are spelled, including raw spellings.

use super::*;

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

/// A raw identifier names the same attribute as its bare spelling, so a
/// production declaration spelled `#[r#path]` is still a production claim on
/// the file it shares with a test module.
#[test]
fn a_raw_path_attribute_keeps_a_shared_file_in_production() {
    let temp = inventory(&[
        (
            "src/lib.rs",
            "mod production;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod cases;\n",
        ),
        (
            "src/production.rs",
            "#[r#path = \"shared.rs\"]\nmod shared;\n",
        ),
        ("src/shared.rs", CALL),
    ]);

    let (non_test, test) = sections(temp.path());

    assert!(non_test.contains("`src/shared.rs:1`"), "{non_test}");
    assert!(test.contains("_None_"), "{test}");
}

/// Both spellings of each attribute classify identically, whichever side of
/// the declaration they appear on.
#[test]
fn raw_and_ordinary_attribute_spellings_classify_alike() {
    for path in ["path", "r#path"] {
        for cfg in ["cfg(test)", "r#cfg(r#test)"] {
            let temp = inventory(&[
                (
                    "src/lib.rs",
                    &format!("#[{cfg}]\n#[{path} = \"cases.rs\"]\nmod cases;\n"),
                ),
                (
                    "src/cases.rs",
                    &format!("#[{path} = \"fixture.rs\"]\nmod fixture;\n"),
                ),
                ("src/fixture.rs", CALL),
            ]);

            let (non_test, test) = sections(temp.path());

            assert!(non_test.contains("_None_"), "{path} {cfg}: {non_test}");
            assert!(test.contains("`src/fixture.rs:1`"), "{path} {cfg}: {test}");
        }
    }
}
