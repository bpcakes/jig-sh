use super::*;

#[test]
fn parity_test_parser_rejects_textual_ignored_and_cfg_gated_fakes() {
    assert!(source_declares_active_test("#[test]\nfn real() {}", "real").unwrap());
    assert!(
        !source_declares_active_test("const FAKE: &str = \"#[test]\\nfn fake() {}\";", "fake")
            .unwrap()
    );
    assert!(!source_declares_active_test("// #[test]\n// fn fake() {}", "fake").unwrap());
    assert!(
        !source_declares_active_test("#[test]\n#[ignore]\nfn ignored() {}", "ignored").unwrap()
    );
    assert!(
        !source_declares_active_test("#[test]\n#[cfg(any())]\nfn gated() {}", "gated").unwrap()
    );
    assert!(
        !source_declares_active_test(
            "#[test]\n#[cfg_attr(test, ignore)]\nfn conditionally_ignored() {}",
            "conditionally_ignored"
        )
        .unwrap()
    );
    assert!(!source_declares_active_test("fn merely_named() {}", "merely_named").unwrap());
    assert!(
        source_declares_active_test("mod nested { #[test] fn nested_test() {} }", "nested_test")
            .unwrap()
    );
    assert!(
        source_declares_active_test(
            "#[cfg(test)] mod nested { #[test] fn test_only_nested_test() {} }",
            "test_only_nested_test"
        )
        .unwrap()
    );
    assert!(
        !source_declares_active_test(
            "#[cfg(test)] mod nested { #[test] fn gated_nested_test() {} }",
            "different_name"
        )
        .unwrap()
    );
    assert!(
        !source_declares_active_test(
            "#[cfg(feature = \"optional\")] mod nested { #[test] fn gated_nested_test() {} }",
            "gated_nested_test"
        )
        .unwrap()
    );
    assert!(source_declares_active_test("this is not Rust", "broken").is_err());
}

#[test]
fn parity_source_resolution_rejects_unsafe_and_uncollected_paths() {
    assert!(is_safe_repository_relative_path(Path::new(
        "crates/jig-ui/src/terminal/tests.rs"
    )));
    assert!(!is_safe_repository_relative_path(Path::new("../tests.rs")));
    assert!(!is_safe_repository_relative_path(Path::new(
        "/tmp/tests.rs"
    )));
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(
        std::panic::catch_unwind(|| {
            assert_test_source_is_collected(
                manifest_dir,
                &root,
                "crates/jig-ui/tests/helpers/case.rs",
            );
        })
        .is_err()
    );
    assert!(source_declares_active_module("mod tests;", "tests").unwrap());
    assert!(source_declares_active_module("#[cfg(test)] mod tests;", "tests").unwrap());
    assert!(
        !source_declares_active_module("#[cfg(feature = \"optional\")] mod tests;", "tests")
            .unwrap()
    );
}
