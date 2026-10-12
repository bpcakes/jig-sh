//! The backend document check proves that the committed documents match the
//! backend, whatever the general backend test commands run. A scaffold's
//! drift check, beside its client-only contract script, reads no backend; one
//! upgraded from an earlier generator keeps reading it, since its own script
//! still exports from the backend.

use super::*;

/// Backend test commands that skip the committed-document tests, as
/// `(command key, answer key, command)`.
fn custom_test_commands(backend: &str) -> [(&'static str, &'static str, &'static str); 2] {
    if backend == "rust" {
        [
            (
                "api_test_command",
                "rust_test_command",
                "cargo test --workspace -- --skip committed_",
            ),
            (
                "api_test_locked_command",
                "rust_test_locked_command",
                "cargo test --workspace --locked -- --skip committed_",
            ),
        ]
    } else {
        [
            (
                "api_test_command",
                "go_test_command",
                "go test -skip TestOpenAPIIsCurrent ./...",
            ),
            (
                "api_test_locked_command",
                "go_test_locked_command",
                "go mod verify && go test -mod=readonly -skip TestOpenAPIIsCurrent ./...",
            ),
        ]
    }
}

/// A model generated for a repository upgraded from an earlier generator.
fn generated_model(backend: &str, answers: &str) -> RepositoryRenderModel {
    RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = {backend:?}\n{answers}{APP}"
    )))
    .unwrap()
}

/// A model generated with the scaffold's client-only contract script.
fn scaffolded_model(backend: &str, answers: &str) -> RepositoryRenderModel {
    let mut scaffold = scaffold_answers(&format!("backend_language = {backend:?}\n{answers}{APP}"));
    scaffold.enable_client_only_contract_drift();
    RepositoryRenderModel::from_answers(&scaffold).unwrap()
}

fn find<'a>(model: &'a RepositoryRenderModel, target: &str) -> Option<&'a ActionSpec> {
    model
        .actions
        .iter()
        .find(|item| item.target.to_string() == target)
}

fn drift_reads_backend(model: &RepositoryRenderModel) -> bool {
    let drift = &find(model, "repo:frontend-contract-drift").unwrap().inputs;
    BACKEND_CONTRACT_INPUTS
        .iter()
        .all(|input| drift.iter().any(|item| item == input))
}

/// A current scaffold as `jig init` saves it.
fn saved_current_model(backend: &str) -> RepositoryRenderModel {
    let mut current = scaffolded_model(backend, "");
    current
        .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    current
        .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
        .unwrap();
    current
}

fn document_check_mut(model: &mut RepositoryRenderModel) -> &mut ActionSpec {
    model
        .actions
        .iter_mut()
        .find(|item| item.target.to_string() == "api:openapi")
        .unwrap()
}

/// Marks `model` custom without touching its contract or backend checks.
fn customize_frontend(model: &mut RepositoryRenderModel) {
    model
        .components
        .iter_mut()
        .find(|component| component.id.as_str() == "web")
        .unwrap()
        .description = Some("Authored web frontend.".into());
}

#[test]
fn generation_adds_a_document_check_that_test_commands_cannot_skip() {
    for backend in ["rust", "go"] {
        let command = openapi_document_check_command(backend == "go");
        let customized = custom_test_commands(backend).map(|(_, answer, custom)| {
            scaffolded_model(backend, &format!("{answer} = {custom:?}\n"))
        });
        for model in std::iter::once(scaffolded_model(backend, "")).chain(customized) {
            let check = find(&model, "api:openapi").unwrap();
            assert_eq!(model.commands["api_openapi_command"], command, "{backend}");
            let mut inputs = find(&model, "api:test").unwrap().inputs.clone();
            inputs.push("openapi/**".into());
            inputs.sort();
            assert_eq!(check.inputs, inputs, "{backend}");
            assert!(
                model.profiles[0].targets.contains(&check.target),
                "{backend}"
            );
            for test in ["api:test", "api:test-locked"] {
                assert!(
                    !find(&model, test)
                        .unwrap()
                        .inputs
                        .contains(&"openapi/**".into()),
                    "{backend} {test}"
                );
            }
            assert!(!drift_reads_backend(&model), "{backend}");
        }
        // An upgraded repository's own script exports from the backend.
        let upgraded = generated_model(backend, "");
        assert!(find(&upgraded, "api:openapi").is_some(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
        assert_ne!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .description,
            find(
                &scaffolded_model(backend, ""),
                "repo:frontend-contract-drift"
            )
            .unwrap()
            .description,
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_a_scaffold_drift_check_client_only() {
    for backend in ["rust", "go"] {
        let saved = saved_current_model(backend);

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(!drift_reads_backend(&upgraded), "{backend}");
        assert_eq!(
            upgraded.authored_toml().unwrap(),
            scaffolded_model(backend, "").authored_toml().unwrap(),
            "{backend}"
        );
    }
}

#[test]
fn managed_update_keeps_a_document_check_removed_after_an_upgrade() {
    for backend in ["rust", "go"] {
        // The first update adds the check; the drift check keeps reading the
        // backend, so its inputs alone look like those of an older generator.
        let answers = load_saved(&legacy_model(backend), backend);
        let mut migrated = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_none(), "{backend}");
        assert!(find(&migrated, "api:openapi").is_some(), "{backend}");
        assert!(drift_reads_backend(&migrated), "{backend}");
        migrated
            .prepare_runner_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        migrated
            .prepare_freshness_epoch(jig_context::CURRENT_CONTRACT_VERSION)
            .unwrap();
        // Its owner then removes the check.
        let check = target_id("api", "openapi").unwrap();
        migrated.actions.retain(|action| action.target != check);
        for profile in &mut migrated.profiles {
            profile.targets.retain(|target| *target != check);
        }
        migrated.commands.remove("api_openapi_command");

        let answers = load_saved(&migrated, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
    }
}

#[test]
fn managed_update_adds_no_document_check_to_a_custom_model() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        customize_frontend(&mut saved);

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
    }
}

#[test]
fn managed_update_adds_the_document_check_beside_customized_test_commands() {
    for backend in ["rust", "go"] {
        for (key, answer, custom) in custom_test_commands(backend) {
            let label = format!("{backend} {key}");
            let mut saved = legacy_model(backend);
            saved.commands.insert(key.into(), custom.into());

            let answers = load_saved(&saved, backend);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            let current = generated_model(backend, &format!("{answer} = {custom:?}\n"));
            assert!(answers.authored_repository().is_none(), "{label}");
            assert_eq!(
                upgraded.authored_toml().unwrap(),
                current.authored_toml().unwrap(),
                "{label}"
            );
            assert_eq!(upgraded.commands, current.commands, "{label}");
            assert!(find(&upgraded, "api:openapi").is_some(), "{label}");
        }
    }
}

#[test]
fn managed_update_keeps_a_model_custom_when_the_check_command_key_is_taken() {
    for backend in ["rust", "go"] {
        let mut saved = legacy_model(backend);
        saved.commands.insert(
            "api_openapi_command".into(),
            "printf skip-openapi-check".into(),
        );

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");
    }
}

#[test]
fn managed_update_keeps_a_removed_document_check_removed() {
    for backend in ["rust", "go"] {
        let mut saved = saved_current_model(backend);
        let check = target_id("api", "openapi").unwrap();
        saved.actions.retain(|action| action.target != check);
        for profile in &mut saved.profiles {
            profile.targets.retain(|target| *target != check);
        }
        saved.commands.remove("api_openapi_command");

        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert_eq!(
            find(&upgraded, "repo:frontend-contract-drift")
                .unwrap()
                .inputs,
            find(&saved, "repo:frontend-contract-drift").unwrap().inputs,
            "{backend}"
        );
    }
}

#[test]
fn workflows_run_the_document_check_only_as_a_read_only_check_of_their_backend() {
    for (backend, other) in [("rust", "go"), ("go", "rust")] {
        let mut model = generated_model(backend, "");
        // An authored check may read more than the backend root, such as a
        // schema directory beside a nested backend.
        document_check_mut(&mut model)
            .inputs
            .push("schemas/**".into());
        let ci_inputs = |model: &RepositoryRenderModel| {
            if backend == "go" {
                model.go_ci_input_paths()
            } else {
                model.rust_ci_input_paths()
            }
        };
        assert_eq!(
            model.openapi_document_check_target(backend).as_deref(),
            Some("api:openapi")
        );
        assert_eq!(model.openapi_document_check_target(other), None);
        assert!(
            ci_inputs(&model).contains(&"schemas/**".into()),
            "{backend}"
        );

        // An authored action of the same name may generate the documents.
        let check = document_check_mut(&mut model);
        check.intent = ActionIntent::Generate;
        check.effects = vec![ActionEffect::Worktree];
        assert_eq!(model.openapi_document_check_target(backend), None);
        assert!(
            !ci_inputs(&model).contains(&"schemas/**".into()),
            "{backend}"
        );
    }
}

/// Real Cargo: quiet Cargo prints each passing test as a dot unless the
/// format is pinned, and a read-only check must not rewrite the lockfile.
#[cfg(unix)]
#[test]
fn rust_document_check_runs_quiet_cargo_without_rewriting_the_lockfile() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("openapi")).unwrap();
    let manifest = |version: &str| {
        fs::write(
            root.path().join("Cargo.toml"),
            format!(
                "[package]\nname = \"example-openapi-fixture\"\nversion = \"{version}\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
    };
    manifest("0.1.0");
    fs::write(root.path().join("openapi/public.json"), "{}\n").unwrap();
    fs::write(root.path().join("src/lib.rs"), "").unwrap();
    let lockfile_written = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(lockfile_written.success());
    let lockfile = fs::read(root.path().join("Cargo.lock")).unwrap();
    let check = |test: &str| {
        fs::write(
            root.path().join("src/lib.rs"),
            format!("#[test]\nfn {test}() {{}}\n"),
        )
        .unwrap();
        std::process::Command::new("bash")
            .arg("-c")
            .arg(openapi_document_check_command(false))
            .current_dir(root.path())
            .env("CARGO_TERM_QUIET", "true")
            .env("CARGO_TARGET_DIR", root.path().join("target"))
            .output()
            .unwrap()
    };
    for (test, passes) in [
        ("committed_public_openapi_is_current", true),
        ("committed_public_document_renamed", false),
    ] {
        let output = check(test);
        assert_eq!(
            output.status.success(),
            passes,
            "{test}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lockfile);
    }

    // An outdated lockfile fails the check instead of being rewritten.
    manifest("0.2.0");
    let output = check("committed_public_openapi_is_current");
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read(root.path().join("Cargo.lock")).unwrap(), lockfile);
}

#[cfg(unix)]
#[test]
fn document_check_commands_fail_unless_each_document_test_passed() {
    use std::os::unix::fs::PermissionsExt;

    let run = |go: bool, documents: &[&str], output: &str, status: &str| {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("openapi")).unwrap();
        for document in documents {
            fs::write(root.path().join(format!("openapi/{document}.json")), "{}\n").unwrap();
        }
        let bin = root.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let tool = bin.join(if go { "go" } else { "cargo" });
        fs::write(
            &tool,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$FAKE_ARGS\"\nprintf '%s\\n' \"$FAKE_OUTPUT\"\nexit \"$FAKE_STATUS\"\n",
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
            .arg(openapi_document_check_command(go))
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
    for (go, documents, output, status, passes) in [
        (false, &["public", "admin"][..], both.as_str(), "0", true),
        (false, &["public", "admin"], public, "0", false),
        (false, &["public"], public, "0", true),
        (false, &["public"], "running 0 tests", "0", false),
        // Removing the public document does not remove its test.
        (false, &[], "running 0 tests", "0", false),
        (false, &["public"], public, "101", false),
        (true, &["public"], go_pass, "0", true),
        (
            true,
            &["public"],
            "testing: warning: no tests to run",
            "0",
            false,
        ),
        (
            true,
            &["public"],
            "--- FAIL: TestOpenAPIIsCurrent (0.00s)",
            "1",
            false,
        ),
    ] {
        let label = format!("{go} {documents:?} {output:?} {status}");
        let (result, args) = run(go, documents, output, status);
        assert_eq!(result.status.success(), passes, "{label}");
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(output),
            "{label}"
        );
        let expected = if go {
            "test -mod=readonly -count=1 -run ^TestOpenAPIIsCurrent$ -v ./internal/httpapi\n"
        } else {
            "test --workspace --locked openapi_is_current -- --format pretty\n"
        };
        assert_eq!(args, expected, "{label}");
        if status != "0" {
            assert_eq!(result.status.code(), status.parse().ok(), "{label}");
        }
    }
}

/// Go records workspace checksums even in read-only mode; the check puts the
/// file back as it found it.
#[cfg(unix)]
#[test]
fn go_document_check_restores_the_workspace_checksum_file() {
    use std::os::unix::fs::PermissionsExt;

    for kept in [None, Some("kept\n")] {
        let root = TempDir::new().unwrap();
        let bin = root.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let tool = bin.join("go");
        fs::write(
            &tool,
            "#!/bin/sh\nprintf 'written\\n' >> go.work.sum\nprintf '%s\\n' '--- PASS: TestOpenAPIIsCurrent (0.00s)'\n",
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        if let Some(kept) = kept {
            fs::write(root.path().join("go.work.sum"), kept).unwrap();
        }
        let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();

        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(openapi_document_check_command(true))
            .current_dir(root.path())
            .env("PATH", path)
            .output()
            .unwrap();

        assert!(output.status.success(), "{kept:?}");
        assert_eq!(
            fs::read_to_string(root.path().join("go.work.sum"))
                .ok()
                .as_deref(),
            kept
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
    let check = |goflags: &str, workspace: bool| {
        go_command("bash", &app, goflags, workspace)
            .arg("-c")
            .arg(openapi_document_check_command(true))
            .output()
            .unwrap()
    };

    // An inherited -mod=mod would add go.sum entries; the check refuses.
    let output = check("-modcacherw -mod=mod", false);
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!app.join("go.sum").exists());

    // Go records the workspace checksums even in read-only mode; the check
    // removes the file it created.
    fs::write(app.join("go.work"), "go 1.22\n\nuse .\n").unwrap();
    let output = check("-modcacherw", true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!app.join("go.work.sum").exists());
    assert!(!app.join("go.sum").exists());
}
