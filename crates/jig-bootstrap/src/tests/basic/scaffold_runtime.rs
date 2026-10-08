use super::*;

fn assert_text_contains_all(contents: &str, expected: &[&str]) {
    for value in expected {
        assert!(contents.contains(value), "missing expected text: {value}");
    }
}

fn assert_text_contains_none(contents: &str, forbidden: &[&str]) {
    for value in forbidden {
        assert!(!contents.contains(value), "found forbidden text: {value}");
    }
}

fn assert_command_succeeded(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

include!("scaffold_runtime_parts/part_01.rs");
include!("scaffold_runtime_parts/part_02.rs");
include!("scaffold_runtime_parts/part_03.rs");
include!("scaffold_runtime_parts/part_04.rs");
