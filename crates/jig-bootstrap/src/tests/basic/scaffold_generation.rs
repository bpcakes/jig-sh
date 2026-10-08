use super::*;

fn assert_contains_all(contents: &str, expected: &[&str]) {
    for value in expected {
        assert!(contents.contains(value), "missing expected text: {value}");
    }
}

fn assert_contains_none(contents: &str, forbidden: &[&str]) {
    for value in forbidden {
        assert!(!contents.contains(value), "found forbidden text: {value}");
    }
}

fn assert_contains_count(contents: &str, expected: &[(&str, usize)]) {
    for (value, count) in expected {
        assert_eq!(
            contents.matches(value).count(),
            *count,
            "unexpected occurrence count for {value}"
        );
    }
}

fn assert_text_before(contents: &str, earlier: &str, later: &str) {
    let earlier = contents
        .find(earlier)
        .unwrap_or_else(|| panic!("missing earlier text: {earlier}"));
    let later = contents
        .find(later)
        .unwrap_or_else(|| panic!("missing later text: {later}"));
    assert!(earlier < later, "expected text ordering was reversed");
}

fn assert_paths_exist(root: &Path, paths: &[&str]) {
    for path in paths {
        assert!(root.join(path).exists(), "missing generated path: {path}");
    }
}

fn assert_paths_absent(root: &Path, paths: &[&str]) {
    for path in paths {
        assert!(
            !root.join(path).exists(),
            "unexpected generated path: {path}"
        );
    }
}

fn rendered_contents<'a>(rendered: &'a [scaffold::ScaffoldFile], path: &str) -> &'a str {
    rendered
        .iter()
        .find(|file| file.relative == path)
        .unwrap_or_else(|| panic!("missing rendered file: {path}"))
        .contents
        .as_str()
}

include!("scaffold_generation_parts/part_01.rs");
include!("scaffold_generation_parts/part_02_assertions.rs");
include!("scaffold_generation_parts/part_02_backend_assertions.rs");
include!("scaffold_generation_parts/part_02.rs");
include!("scaffold_generation_parts/part_03.rs");
include!("scaffold_generation_parts/part_04.rs");
include!("scaffold_generation_parts/part_05.rs");
include!("scaffold_generation_parts/rust_only_acceptance.rs");
include!("scaffold_generation_parts/rust_only_compatibility.rs");
include!("scaffold_generation_parts/rust_library.rs");
include!("scaffold_generation_parts/rust_cli.rs");
include!("scaffold_generation_parts/clippy_defaults.rs");
