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

fn assert_text_occurrences(contents: &str, expected: &[(&str, usize)]) {
    for (value, count) in expected {
        assert_eq!(
            contents.matches(value).count(),
            *count,
            "unexpected count for {value}"
        );
    }
}

fn assert_environment_contains_all(environment: &str, expected: &[&str]) {
    for value in expected {
        assert!(
            environment.lines().any(|line| line == *value),
            "missing environment entry {value}"
        );
    }
}

fn assert_environment_contains_none(environment: &str, forbidden_prefixes: &[&str]) {
    for prefix in forbidden_prefixes {
        assert!(
            !environment.lines().any(|line| line.starts_with(prefix)),
            "found forbidden environment entry {prefix}"
        );
    }
}

fn assert_output_succeeded(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_output_failed(label: &str, output: &std::process::Output) {
    assert!(
        !output.status.success(),
        "{label} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_dependency_readiness(label: &str, actual: bool, expected: bool) {
    assert_eq!(
        actual, expected,
        "unexpected dependency readiness for {label}"
    );
}

fn assert_json_array_contains(value: &serde_json::Value, expected: &str) {
    assert!(
        value
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().is_some_and(|item| item.contains(expected))),
        "missing array value containing {expected}"
    );
}

fn assert_json_array_contains_none(value: &serde_json::Value, forbidden: &str) {
    assert!(
        value
            .as_array()
            .unwrap()
            .iter()
            .all(|item| !item.as_str().is_some_and(|item| item.contains(forbidden))),
        "found array value containing {forbidden}"
    );
}

#[cfg(unix)]
fn wait_for_positive_pid_file(
    path: &std::path::Path,
    timeout: std::time::Duration,
) -> std::result::Result<u32, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let observation = match fs::read_to_string(path) {
            Ok(contents) => match contents.trim().parse::<u32>() {
                Ok(pid) if pid > 0 => return Ok(pid),
                Ok(pid) => format!("invalid non-positive PID {pid}"),
                Err(error) => format!("unparseable contents {contents:?}: {error}"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                "file does not exist yet".to_owned()
            }
            Err(error) => format!("could not read file: {error}"),
        };
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for a parseable PID in {} ({observation})",
                path.display()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

mod adoption_validation;
mod configuration;
#[cfg(unix)]
mod dependency_receipts;
#[cfg(unix)]
mod dependency_state;
#[cfg(unix)]
mod install_locking;
#[cfg(unix)]
mod pnpm;
mod workflows;
#[cfg(unix)]
mod yarn;
