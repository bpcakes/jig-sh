//! The generated document check commands, run against fake and real tools:
//! they fail unless each document test passed, and leave lockfiles and
//! checksum files as they found them.

use super::*;

/// The Rust scaffold's packages are named after the repository.
const REPO_NAME: &str = "example-project";

/// A Cargo workspace whose packages, by suffix, are named after the
/// repository, each at `version`, with the committed public document.
#[cfg(unix)]
fn rust_workspace(root: &std::path::Path, version: &str, suffixes: &[&str]) {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();
    for suffix in suffixes {
        let package = format!("{REPO_NAME}-{suffix}");
        fs::create_dir_all(root.join(format!("crates/{package}/src"))).unwrap();
        fs::write(root.join(format!("crates/{package}/src/lib.rs")), "").unwrap();
        fs::write(
            root.join(format!("crates/{package}/Cargo.toml")),
            format!(
                "[package]\nname = \"{package}\"\nversion = \"{version}\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
    }
    fs::create_dir_all(root.join("openapi")).unwrap();
    fs::write(root.join("openapi/public.json"), "{}\n").unwrap();
}

/// Locks the dependencies of [`rust_workspace`], as `--locked` requires.
#[cfg(unix)]
fn generate_lockfile(root: &std::path::Path) {
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Gives each package of [`rust_workspace`], by suffix, the named tests and
/// runs the Rust document check with quiet Cargo. A document test reads its
/// committed document, as the scaffold's do.
#[cfg(unix)]
fn run_rust_document_check(
    root: &std::path::Path,
    tests: &[(&str, &[&str])],
) -> std::process::Output {
    for (suffix, tests) in tests {
        let source = tests
            .iter()
            .map(|test| {
                let body = ["public", "admin"]
                    .into_iter()
                    .find(|name| *test == format!("committed_{name}_openapi_is_current"))
                    .map_or_else(String::new, |name| {
                        format!(
                            "std::fs::read_to_string(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../openapi/{name}.json\")).unwrap();"
                        )
                    });
                format!("#[test]\nfn {test}() {{ {body} }}\n")
            })
            .collect::<String>();
        fs::write(
            root.join(format!("crates/{REPO_NAME}-{suffix}/src/lib.rs")),
            source,
        )
        .unwrap();
    }
    std::process::Command::new("bash")
        .arg("-c")
        .arg(openapi_document_check_command(false, REPO_NAME))
        .current_dir(root)
        .env("CARGO_TERM_QUIET", "true")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap()
}

/// Real Cargo: quiet Cargo prints each passing test as a dot unless the
/// format is pinned, and a read-only check must not rewrite the lockfile.
#[cfg(unix)]
#[test]
fn rust_document_check_runs_quiet_cargo_without_rewriting_the_lockfile() {
    let root = TempDir::new().unwrap();
    rust_workspace(root.path(), "0.1.0", &["http"]);
    generate_lockfile(root.path());
    let lockfile = fs::read(root.path().join("Cargo.lock")).unwrap();
    for (test, passes) in [
        ("committed_public_openapi_is_current", true),
        ("committed_public_document_renamed", false),
        ("unrelated_committed_public_openapi_is_current", false),
    ] {
        let output = run_rust_document_check(root.path(), &[("http", &[test])]);
        assert_eq!(
            output.status.success(),
            passes,
            "{test}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lockfile);
    }

    // An outdated lockfile fails the check instead of being rewritten.
    rust_workspace(root.path(), "0.2.0", &["http"]);
    let output = run_rust_document_check(
        root.path(),
        &[("http", &["committed_public_openapi_is_current"])],
    );
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lockfile);
}

/// Real Cargo: each document test counts only in the package that the
/// scaffold puts it in, so a test of that name in another crate of the
/// workspace cannot pass the check for it, and an admin crate requires its
/// test even when its document is gone.
#[cfg(unix)]
#[test]
fn rust_document_check_runs_each_document_test_in_its_own_package() {
    let root = TempDir::new().unwrap();
    let public = "committed_public_openapi_is_current";
    let admin = "committed_admin_openapi_is_current";
    let check = |tests: &[(&str, &[&str])], passes: bool, expected: &str| {
        let result = run_rust_document_check(root.path(), tests);
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.status.success(), passes, "{tests:?}: {output}");
        assert!(output.contains(expected), "{tests:?}: {output}");
    };
    let missing = |name: &str, suffix: &str| {
        format!("committed_{name}_openapi_is_current did not pass in {REPO_NAME}-{suffix}.")
    };

    // Without an admin API, only the public test counts, and only from its
    // package; an admin document without the admin crate cannot be checked.
    rust_workspace(root.path(), "0.1.0", &["http", "worker"]);
    generate_lockfile(root.path());
    check(
        &[("http", &[]), ("worker", &[public])],
        false,
        &missing("public", "http"),
    );
    check(&[("http", &[public]), ("worker", &[admin])], true, "");
    fs::write(root.path().join("openapi/admin.json"), "{}\n").unwrap();
    check(
        &[("http", &[public]), ("worker", &[admin])],
        false,
        &format!("{REPO_NAME}-admin-http"),
    );
    fs::remove_file(root.path().join("openapi/admin.json")).unwrap();

    // An admin crate requires its test, which fails without its document.
    rust_workspace(root.path(), "0.1.0", &["http", "admin-http", "worker"]);
    generate_lockfile(root.path());
    let scaffold: &[(&str, &[&str])] = &[("http", &[public]), ("admin-http", &[admin])];
    check(scaffold, false, &format!("test {admin} ... FAILED"));
    fs::write(root.path().join("openapi/admin.json"), "{}\n").unwrap();
    check(scaffold, true, "");
    check(
        &[
            ("http", &[public, admin]),
            ("admin-http", &[]),
            ("worker", &[admin]),
        ],
        false,
        &missing("admin", "admin-http"),
    );
    check(
        &[
            ("http", &[admin]),
            ("admin-http", &[public]),
            ("worker", &[public]),
        ],
        false,
        &missing("public", "http"),
    );
}

#[cfg(unix)]
#[test]
fn document_check_commands_fail_unless_each_document_test_passed() {
    use std::os::unix::fs::PermissionsExt;

    let run = |go: bool, files: &[&str], output: &str, status: &str| {
        let root = TempDir::new().unwrap();
        for file in files {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "{}\n").unwrap();
        }
        let bin = root.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let tool = bin.join(if go { "go" } else { "cargo" });
        fs::write(
            &tool,
            "#!/bin/sh\n[ \"$1\" = env ] && exit 0\nprintf '%s\\n' \"$*\" >> \"$FAKE_ARGS\"\nprintf '%s\\n' \"$FAKE_OUTPUT\"\nexit \"$FAKE_STATUS\"\n",
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        let args = root.path().join("args");
        let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();
        let result = std::process::Command::new("bash")
            .arg("-c")
            .arg(openapi_document_check_command(go, REPO_NAME))
            .current_dir(root.path())
            .env("PATH", path)
            .env("FAKE_ARGS", &args)
            .env("FAKE_OUTPUT", output)
            .env("FAKE_STATUS", status)
            .output()
            .unwrap();
        (result, fs::read_to_string(args).unwrap())
    };
    let public = "test tests::committed_public_openapi_is_current ... ok";
    let admin = "test tests::committed_admin_openapi_is_current ... ok";
    let both = format!("{public}\n{admin}");
    let go_pass = "--- PASS: TestOpenAPIIsCurrent (0.00s)";
    let public_document = "openapi/public.json";
    let admin_document = "openapi/admin.json";
    let admin_crate = format!("crates/{REPO_NAME}-admin-http/Cargo.toml");
    let admin_crate = admin_crate.as_str();
    for (go, files, output, status, passes) in [
        (
            false,
            &[public_document, admin_document][..],
            both.as_str(),
            "0",
            true,
        ),
        (
            false,
            &[public_document, admin_document],
            public,
            "0",
            false,
        ),
        // The admin crate requires its test without its document.
        (
            false,
            &[public_document, admin_crate],
            both.as_str(),
            "0",
            true,
        ),
        (false, &[public_document, admin_crate], public, "0", false),
        (false, &[public_document], public, "0", true),
        (
            false,
            &[public_document],
            "test committed_public_openapi_is_current ... ok",
            "0",
            true,
        ),
        // A test whose name only ends with the document test's does not count.
        (
            false,
            &[public_document],
            "test tests::unrelated_committed_public_openapi_is_current ... ok",
            "0",
            false,
        ),
        (
            false,
            &[public_document],
            "test unrelated_committed_public_openapi_is_current ... ok",
            "0",
            false,
        ),
        (false, &[public_document], "running 0 tests", "0", false),
        // Removing the public document does not remove its test.
        (false, &[], "running 0 tests", "0", false),
        (false, &[public_document], public, "101", false),
        (true, &[public_document], go_pass, "0", true),
        (
            true,
            &[public_document],
            "testing: warning: no tests to run",
            "0",
            false,
        ),
        (
            true,
            &[public_document],
            "--- FAIL: TestOpenAPIIsCurrent (0.00s)",
            "1",
            false,
        ),
    ] {
        let label = format!("{go} {files:?} {output:?} {status}");
        let (result, args) = run(go, files, output, status);
        assert_eq!(result.status.success(), passes, "{label}");
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(output),
            "{label}"
        );
        let expected = if go {
            "test -mod=readonly -count=1 -run ^TestOpenAPIIsCurrent$ -v ./internal/httpapi\n".into()
        } else {
            // Each document test runs in its own package, the admin one
            // beside its document or crate.
            let admin_required = files.contains(&admin_document) || files.contains(&admin_crate);
            [("public", "http"), ("admin", "admin-http")]
                .into_iter()
                .filter(|(name, _)| *name == "public" || admin_required)
                .map(|(name, suffix)| {
                    format!(
                        "test --locked -p {REPO_NAME}-{suffix} committed_{name}_openapi_is_current -- --format pretty\n"
                    )
                })
                .collect::<String>()
        };
        assert_eq!(args, expected, "{label}");
        if status != "0" {
            assert_eq!(result.status.code(), status.parse().ok(), "{label}");
        }
    }
}

/// Go records workspace checksums even in read-only mode, and a workspace
/// may live above the repository, so the check builds outside any workspace
/// and names that when a workspace would have been in effect.
#[cfg(unix)]
#[test]
fn go_document_check_runs_outside_any_workspace() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new().unwrap();
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("go");
    // `go env GOWORK` names the workspace Go would otherwise use.
    fs::write(
        &tool,
        "#!/bin/sh\nif [ \"$1\" = env ]; then printf '%s\\n' \"$FAKE_WORKSPACE\"; exit 0; fi\nprintf 'GOWORK=%s\\n' \"$GOWORK\"\nif [ \"$FAKE_STATUS\" = 0 ]; then printf '%s\\n' '--- PASS: TestOpenAPIIsCurrent (0.00s)'; fi\nexit \"$FAKE_STATUS\"\n",
    )
    .unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();

    for (status, workspace, hint) in [
        ("0", "/example/go.work", false),
        ("1", "/example/go.work", true),
        ("1", "", false),
        ("1", "off", false),
    ] {
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(openapi_document_check_command(true, REPO_NAME))
            .current_dir(root.path())
            .env("PATH", &path)
            .env("GOWORK", root.path().join("go.work"))
            .env("FAKE_STATUS", status)
            .env("FAKE_WORKSPACE", workspace)
            .output()
            .unwrap();

        let label = format!("{status} {workspace:?}");
        assert_eq!(output.status.success(), status == "0", "{label}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("GOWORK=off"),
            "{label}"
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).contains("outside its go.work workspace"),
            hint,
            "{label}"
        );
    }
}

/// Builds a module zip for the local proxy with the standard library only.
const MODULE_ZIP_WRITER: &str = r#"package main

import (
	"archive/zip"
	"os"
)

func main() {
	file, err := os.Create(os.Args[1])
	if err != nil {
		panic(err)
	}
	archive := zip.NewWriter(file)
	for name, body := range map[string]string{
		"example.invalid/dep@v1.0.0/go.mod": "module example.invalid/dep\n\ngo 1.22\n",
		"example.invalid/dep@v1.0.0/dep.go": "package dep\n\nconst Name = \"dep\"\n",
	} {
		entry, err := archive.Create(name)
		if err != nil {
			panic(err)
		}
		if _, err := entry.Write([]byte(body)); err != nil {
			panic(err)
		}
	}
	if err := archive.Close(); err != nil {
		panic(err)
	}
	if err := file.Close(); err != nil {
		panic(err)
	}
}
"#;

/// Real Go, offline: a local module proxy serves a dependency whose
/// checksums neither go.sum nor go.work.sum holds yet.
#[cfg(unix)]
#[test]
fn go_document_check_leaves_module_and_workspace_checksums_alone() {
    if std::process::Command::new("go")
        .arg("version")
        .output()
        .is_err()
    {
        return;
    }
    let root = TempDir::new().unwrap();
    let proxy = root.path().join("proxy");
    let versions = proxy.join("example.invalid/dep/@v");
    fs::create_dir_all(&versions).unwrap();
    fs::write(versions.join("list"), "v1.0.0\n").unwrap();
    fs::write(
        versions.join("v1.0.0.info"),
        "{\"Version\":\"v1.0.0\",\"Time\":\"2026-01-01T00:00:00Z\"}\n",
    )
    .unwrap();
    fs::write(
        versions.join("v1.0.0.mod"),
        "module example.invalid/dep\n\ngo 1.22\n",
    )
    .unwrap();
    let go_command = |program: &str, dir: &std::path::Path, goflags: &str, workspace: bool| {
        let mut command = std::process::Command::new(program);
        command
            .current_dir(dir)
            .env("GOPATH", root.path().join("gopath"))
            .env("GOMODCACHE", root.path().join("modcache"))
            .env("GOCACHE", root.path().join("gocache"))
            .env("GOPROXY", format!("file://{}", proxy.display()))
            .env("GOSUMDB", "off")
            .env("GOTOOLCHAIN", "local")
            .env("GOFLAGS", goflags);
        if !workspace {
            command.env("GOWORK", "off");
        }
        command
    };
    let writer = root.path().join("writer");
    fs::create_dir_all(&writer).unwrap();
    fs::write(
        writer.join("go.mod"),
        "module example.invalid/writer\n\ngo 1.22\n",
    )
    .unwrap();
    fs::write(writer.join("main.go"), MODULE_ZIP_WRITER).unwrap();
    let written = go_command("go", &writer, "-modcacherw", false)
        .args(["run", "."])
        .arg(versions.join("v1.0.0.zip"))
        .output()
        .unwrap();
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );

    let app = root.path().join("app");
    fs::create_dir_all(app.join("internal/httpapi")).unwrap();
    fs::write(
        app.join("go.mod"),
        "module example.invalid/app\n\ngo 1.22\n\nrequire example.invalid/dep v1.0.0\n",
    )
    .unwrap();
    fs::write(
        app.join("internal/httpapi/httpapi_test.go"),
        "package httpapi\n\nimport (\n\t\"testing\"\n\n\t\"example.invalid/dep\"\n)\n\nfunc TestOpenAPIIsCurrent(t *testing.T) {\n\tif dep.Name != \"dep\" {\n\t\tt.Fatal(dep.Name)\n\t}\n}\n",
    )
    .unwrap();
    let check = |goflags: &str| {
        go_command("bash", &app, goflags, true)
            .arg("-c")
            .arg(openapi_document_check_command(true, REPO_NAME))
            .output()
            .unwrap()
    };
    let checksum_files = || {
        [
            app.join("go.sum"),
            app.join("go.work.sum"),
            root.path().join("go.work.sum"),
        ]
        .map(|path| fs::read(path).ok())
    };

    // An inherited -mod=mod would add go.sum entries; the check refuses.
    let output = check("-modcacherw -mod=mod");
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(checksum_files(), [None, None, None]);

    // Workspaces in the module and above it would record the missing
    // checksums in their go.work.sum; the check ignores both, and says so.
    fs::write(app.join("go.work"), "go 1.22\n\nuse .\n").unwrap();
    fs::write(root.path().join("go.work"), "go 1.22\n\nuse ./app\n").unwrap();
    let output = check("-modcacherw");
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside its go.work workspace"));
    assert_eq!(checksum_files(), [None, None, None]);

    // With complete module checksums the check passes and changes nothing.
    let tidied = go_command("go", &app, "-modcacherw", false)
        .args(["mod", "tidy"])
        .output()
        .unwrap();
    assert!(
        tidied.status.success(),
        "{}",
        String::from_utf8_lossy(&tidied.stderr)
    );
    let before = checksum_files();
    assert!(before[0].is_some());
    let output = check("-modcacherw");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(checksum_files(), before);

    // A module that gets a sibling only through its workspace passes its own
    // tests there, but the check builds it as a release would.
    let sibling = root.path().join("sibling");
    let workspace_app = root.path().join("workspace-app");
    fs::create_dir_all(&sibling).unwrap();
    fs::create_dir_all(workspace_app.join("internal/httpapi")).unwrap();
    fs::write(
        sibling.join("go.mod"),
        "module example.invalid/sibling\n\ngo 1.22\n",
    )
    .unwrap();
    fs::write(
        sibling.join("sibling.go"),
        "package sibling\n\nconst Name = \"sibling\"\n",
    )
    .unwrap();
    fs::write(
        workspace_app.join("go.mod"),
        "module example.invalid/workspaceapp\n\ngo 1.22\n\nrequire example.invalid/sibling v0.0.0\n",
    )
    .unwrap();
    fs::write(
        workspace_app.join("internal/httpapi/httpapi_test.go"),
        "package httpapi\n\nimport (\n\t\"testing\"\n\n\t\"example.invalid/sibling\"\n)\n\nfunc TestOpenAPIIsCurrent(t *testing.T) {\n\t_ = sibling.Name\n}\n",
    )
    .unwrap();
    fs::write(
        workspace_app.join("go.work"),
        "go 1.22\n\nuse (\n\t.\n\t../sibling\n)\n",
    )
    .unwrap();
    let workspace_test = go_command("go", &workspace_app, "-modcacherw", true)
        .args(["test", "./..."])
        .output()
        .unwrap();
    assert!(
        workspace_test.status.success(),
        "{}",
        String::from_utf8_lossy(&workspace_test.stderr)
    );
    let output = go_command("bash", &workspace_app, "-modcacherw", true)
        .arg("-c")
        .arg(openapi_document_check_command(true, REPO_NAME))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("outside its go.work workspace"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
