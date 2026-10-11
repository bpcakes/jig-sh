//! The backend document check proves that the committed documents match the
//! backend, whatever the general backend test commands run; without it, the
//! drift check keeps reading the backend.

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

fn generated_model(backend: &str, answers: &str) -> RepositoryRenderModel {
    RepositoryRenderModel::from_answers(&scaffold_answers(&format!(
        "backend_language = {backend:?}\n{answers}{APP}"
    )))
    .unwrap()
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
    let mut current = generated_model(backend, "");
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

/// A legacy model whose owner already added the generated document check.
fn legacy_model_with_document_check(backend: &str) -> RepositoryRenderModel {
    let current = saved_current_model(backend);
    let mut saved = legacy_model(backend);
    let check = find(&current, "api:openapi").unwrap().clone();
    let index = saved
        .actions
        .iter()
        .position(|action| action.target > check.target)
        .unwrap();
    saved.actions.insert(index, check);
    saved.profiles.clone_from(&current.profiles);
    saved.commands.insert(
        "api_openapi_command".into(),
        current.commands["api_openapi_command"].clone(),
    );
    saved
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
            generated_model(backend, &format!("{answer} = {custom:?}\n"))
        });
        for model in std::iter::once(generated_model(backend, "")).chain(customized) {
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
fn managed_update_narrows_the_drift_check_of_a_custom_model_behind_its_document_check() {
    type Customize = fn(&mut RepositoryRenderModel);
    let cases: [(&str, Customize); 2] = [
        ("in the profile", customize_frontend),
        // The profile runs the document check through the drift check.
        ("through a dependency", |saved| {
            let check = target_id("api", "openapi").unwrap();
            for profile in &mut saved.profiles {
                profile.targets.retain(|target| *target != check);
            }
            saved
                .actions
                .iter_mut()
                .find(|item| item.target.to_string() == "repo:frontend-contract-drift")
                .unwrap()
                .depends_on
                .push(check);
        }),
    ];
    for backend in ["rust", "go"] {
        for (label, customize) in cases {
            let label = format!("{backend} {label}");
            let mut saved = legacy_model_with_document_check(backend);
            customize(&mut saved);

            let answers = load_saved(&saved, backend);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            assert!(answers.authored_repository().is_some(), "{label}");
            assert_eq!(
                find(&upgraded, "repo:frontend-contract-drift")
                    .unwrap()
                    .inputs,
                find(
                    &generated_model(backend, ""),
                    "repo:frontend-contract-drift"
                )
                .unwrap()
                .inputs,
                "{label}"
            );
        }
    }
}

#[test]
fn managed_update_keeps_drift_backend_inputs_without_the_generated_document_check() {
    type Customize = fn(&mut RepositoryRenderModel);
    let cases: [(&str, Customize); 6] = [
        ("documents only", |saved| {
            let check = document_check_mut(saved);
            check.inputs = vec!["openapi/**".into()];
            check
                .provenance
                .insert("inputs".into(), FieldProvenance::Declared);
        }),
        ("custom command", |saved| {
            saved.commands.insert(
                "api_openapi_command".into(),
                "scripts/check-openapi.sh".into(),
            );
        }),
        ("elsewhere", |saved| {
            let (ActionRunner::Command {
                working_directory, ..
            }
            | ActionRunner::Shell {
                working_directory, ..
            }) = &mut document_check_mut(saved).runner
            else {
                panic!("the generated document check runs a command");
            };
            *working_directory = Some("cmd".into());
        }),
        // The profile that runs the drift check never runs the document check.
        ("outside the profile", |saved| {
            let check = target_id("api", "openapi").unwrap();
            for profile in &mut saved.profiles {
                profile.targets.retain(|target| *target != check);
            }
        }),
        // The profile runs the drift check only through a dependency.
        ("drift through a dependency", |saved| {
            let check = target_id("api", "openapi").unwrap();
            let drift = target_id("repo", "frontend-contract-drift").unwrap();
            for profile in &mut saved.profiles {
                profile
                    .targets
                    .retain(|target| *target != check && *target != drift);
            }
            saved
                .actions
                .iter_mut()
                .find(|item| item.target.to_string() == "web:lint")
                .unwrap()
                .depends_on
                .push(drift);
        }),
        ("not a check", |saved| {
            let check = document_check_mut(saved);
            check.intent = ActionIntent::Generate;
            check.effects = vec![ActionEffect::Worktree];
        }),
    ];
    for backend in ["rust", "go"] {
        // A custom model without the check does not gain one.
        let mut saved = legacy_model(backend);
        customize_frontend(&mut saved);
        let answers = load_saved(&saved, backend);
        let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();
        assert!(answers.authored_repository().is_some(), "{backend}");
        assert!(find(&upgraded, "api:openapi").is_none(), "{backend}");
        assert!(drift_reads_backend(&upgraded), "{backend}");

        for (label, customize) in cases {
            let label = format!("{backend} {label}");
            let mut saved = legacy_model_with_document_check(backend);
            customize(&mut saved);
            let authored = find(&saved, "api:openapi").unwrap().inputs.clone();

            let answers = load_saved(&saved, backend);
            let upgraded = RepositoryRenderModel::from_answers(&answers).unwrap();

            assert!(answers.authored_repository().is_some(), "{label}");
            assert!(drift_reads_backend(&upgraded), "{label}");
            assert_eq!(
                find(&upgraded, "api:openapi").unwrap().inputs,
                authored,
                "{label}"
            );
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

/// Quiet Cargo prints each passing test as a dot unless the format is pinned.
#[cfg(unix)]
#[test]
fn rust_document_check_reads_test_names_under_quiet_cargo() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("openapi")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"example-openapi-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.path().join("openapi/public.json"), "{}\n").unwrap();
    for (test, passes) in [
        ("committed_public_openapi_is_current", true),
        ("committed_public_document_renamed", false),
    ] {
        fs::write(
            root.path().join("src/lib.rs"),
            format!("#[test]\nfn {test}() {{}}\n"),
        )
        .unwrap();
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(openapi_document_check_command(false))
            .current_dir(root.path())
            .env("CARGO_TERM_QUIET", "true")
            .env("CARGO_TARGET_DIR", root.path().join("target"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            passes,
            "{test}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
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
            "test -count=1 -run ^TestOpenAPIIsCurrent$ -v ./internal/httpapi\n"
        } else {
            "test --workspace openapi_is_current -- --format pretty\n"
        };
        assert_eq!(args, expected, "{label}");
        if status != "0" {
            assert_eq!(result.status.code(), status.parse().ok(), "{label}");
        }
    }
}
