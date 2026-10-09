use super::*;

mod formatting;
mod go_react;
mod go_react_workflows;
mod identity_validation;
mod init_answers;
mod limit_validation;
mod rust_only;
mod rust_only_compatibility;
mod rust_react_admin;
mod rust_react_backend;
mod rust_react_spa;
mod rust_react_workspace;
mod service_options;

use go_react::*;
use rust_only::*;
use rust_react_admin::*;
use rust_react_backend::*;
use rust_react_spa::*;
use rust_react_workspace::*;

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

#[cfg(unix)]
fn test_program_is_available(program: &str, args: &[&str]) -> bool {
    match Command::new(program).args(args).output() {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("failed to probe {program}: {error}"),
    }
}

#[cfg(unix)]
fn assert_rust_only_command_output_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_rendered_paths(rendered: &[scaffold::ScaffoldFile], expected: &[&str]) {
    for path in expected {
        assert!(
            rendered.iter().any(|file| file.relative == *path),
            "missing nested Go component output {path}"
        );
    }
}

fn assert_rendered_paths_absent(rendered: &[scaffold::ScaffoldFile], forbidden: &[&str]) {
    for path in forbidden {
        assert!(
            rendered.iter().all(|file| file.relative != *path),
            "Go component output escaped to the repository root: {path}"
        );
    }
}
