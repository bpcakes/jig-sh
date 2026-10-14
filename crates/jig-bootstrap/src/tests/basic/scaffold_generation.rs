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

#[cfg(unix)]
fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .status()
        .is_ok_and(|status| status.success())
}

/// Stands in for `@hey-api/openapi-ts`: regenerating a client copies the
/// committed one for the same document, so only a changed document or a
/// changed dispatch can show drift. With `JIG_TEST_STALE_CLIENT` set, it adds
/// a file, as a changed document would.
#[cfg(unix)]
fn install_fake_openapi_ts(destination: &Path) {
    let module = destination.join("node_modules/@hey-api/openapi-ts");
    fs::create_dir_all(&module).unwrap();
    fs::write(
        module.join("package.json"),
        r#"{"name":"@hey-api/openapi-ts","type":"module","exports":"./index.js"}"#,
    )
    .unwrap();
    fs::write(
        module.join("index.js"),
        r#"import { cp, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";

export async function createClient({ input, output }) {
  const name = basename(input, ".json");
  await cp(`packages/${name}-api-client/src/generated`, output, { recursive: true });
  if (process.env.JIG_TEST_STALE_CLIENT) {
    await writeFile(join(output, "stale.ts"), "export {};\n");
  }
}
"#,
    )
    .unwrap();
}

/// Runs a `scripts/contracts.mjs` mode with `fake_bin` first on PATH.
#[cfg(unix)]
fn run_contracts_mode(
    destination: &Path,
    mode: &str,
    fake_bin: &Path,
    env: &[(&str, &std::ffi::OsStr)],
) -> std::process::Output {
    run_contracts(destination, &[mode], fake_bin, env)
}

/// Runs `scripts/contracts.mjs` with `args`, a mode and its arguments.
fn run_contracts(
    destination: &Path,
    args: &[&str],
    fake_bin: &Path,
    env: &[(&str, &std::ffi::OsStr)],
) -> std::process::Output {
    let path = std::env::join_paths(std::iter::once(fake_bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let mut command = Command::new("node");
    command
        .arg("scripts/contracts.mjs")
        .args(args)
        .current_dir(destination)
        .env("PATH", path);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

/// drift-check rejects a client that no longer matches its document and
/// leaves the tree unchanged.
#[cfg(unix)]
fn assert_drift_check_rejects_a_stale_client(destination: &Path, fake_bin: &Path) {
    let before = regular_file_tree_snapshot(destination);

    let output = run_contracts_mode(
        destination,
        "drift-check",
        fake_bin,
        &[("JIG_TEST_STALE_CLIENT", std::ffi::OsStr::new("1"))],
    );

    assert!(
        !output.status.success(),
        "drift-check accepted a stale client"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("OpenAPI client is stale (stale.ts)"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(regular_file_tree_snapshot(destination), before);
}

/// drift-check scans the committed public document for privileged markers.
#[cfg(unix)]
fn assert_drift_check_scans_the_public_document(destination: &Path, fake_bin: &Path) {
    let path = destination.join("openapi/public.json");
    let document = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        document.replacen('{', "{\"x-private\": \"openapi/admin.json\",", 1),
    )
    .unwrap();

    let output = run_contracts_mode(destination, "drift-check", fake_bin, &[]);
    fs::write(&path, document).unwrap();

    assert!(
        !output.status.success(),
        "drift-check accepted a private marker"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("committed public OpenAPI document contains privileged marker"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
