use std::fs;

use tempfile::TempDir;

use super::*;

fn answers(contents: &str) -> RenderAnswers {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{contents}"
            ),
        )
        .unwrap();
    RenderAnswers::from_answers_file(&path).unwrap()
}

fn scaffold_answers(contents: &str) -> RenderAnswers {
    let mut answers = answers(contents);
    answers.enable_scaffolded_frontend_contracts();
    answers
}

#[test]
fn generated_affected_defaults_ignore_guidance_but_keep_execution_authority_fail_closed() {
    let model = RepositoryRenderModel::from_answers(&answers("")).unwrap();

    for pattern in [
        "README.md",
        "**/README.md",
        "AGENTS.md",
        "**/AGENTS.md",
        "agent-map.md",
        "CHANGELOG.md",
        "docs/**",
        "LICENSE",
        "LICENSE.*",
        ".github/**",
    ] {
        assert!(
            model.affected_ignore.iter().any(|value| value == pattern),
            "missing reviewed affected-ignore pattern {pattern}"
        );
    }
    for path in ["fixture.md", ".gitignore", "Makefile", "justfile"] {
        assert!(
            !model.affected_ignore.iter().any(|value| value == path),
            "potential execution input {path} must remain fail-closed"
        );
    }
}

#[test]
fn go_and_multiple_frontends_have_distinct_component_targets() {
    let answers = scaffold_answers(
        r#"
backend_language = "go"
go_database = "postgres"

[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"

[[frontend_apps]]
name = "admin"
dir = "frontend/admin"
coverage_threshold = 85
kind = "vite"
role = "admin"
"#,
    );
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(
        model
            .components
            .iter()
            .any(|component| component.id.as_str() == "api"
                && component.adapters == ["go", "go-postgres"])
    );
    for target in ["api:test", "web:test", "admin:test"] {
        assert!(
            model
                .actions
                .iter()
                .any(|action| action.target.to_string() == target),
            "missing {target}"
        );
    }
    let migration = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:migration-add")
        .unwrap();
    assert_eq!(
        migration.inputs,
        ["internal/database/migrations/**".to_owned()]
    );
    let sqlc = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:sqlc")
        .unwrap();
    assert_eq!(
        sqlc.inputs,
        [
            "sqlc.yaml".to_owned(),
            "**/sqlc.yaml".to_owned(),
            "**/*.sql".to_owned(),
        ]
    );
    assert!(model.required_commands.contains(&"web_test_command".into()));
    assert!(
        model
            .required_commands
            .contains(&"admin_test_command".into())
    );
    assert!(
        model
            .profiles
            .first()
            .unwrap()
            .targets
            .iter()
            .any(|target| target.to_string() == "web:test")
    );

    let drift_target = target_id(REPO_COMPONENT, FRONTEND_CONTRACT_DRIFT_ACTION).unwrap();
    let boundary_target = target_id(REPO_COMPONENT, FRONTEND_PUBLIC_BOUNDARY_ACTION).unwrap();
    assert_eq!(
        model
            .actions
            .iter()
            .filter(|action| action.target == drift_target)
            .count(),
        1
    );
    assert_eq!(
        model
            .actions
            .iter()
            .filter(|action| action.target == boundary_target)
            .count(),
        1
    );
    let drift = model
        .actions
        .iter()
        .find(|action| action.target == drift_target)
        .unwrap();
    let boundary = model
        .actions
        .iter()
        .find(|action| action.target == boundary_target)
        .unwrap();
    assert_eq!(
        boundary.description.as_deref(),
        Some("Check public frontend manifests and artifacts for privileged markers.")
    );
    for artifact in ["docs/public/**", "public-docs/**"] {
        assert!(boundary.inputs.iter().any(|input| input == artifact));
        assert!(!drift.inputs.iter().any(|input| input == artifact));
    }
    for component in ["web", "admin"] {
        let typecheck = model
            .actions
            .iter()
            .find(|action| action.target.to_string() == format!("{component}:typecheck"))
            .unwrap();
        assert!(typecheck.depends_on.contains(&drift_target));
        assert!(typecheck.depends_on.contains(&boundary_target));

        let build = model
            .actions
            .iter()
            .find(|action| action.target.to_string() == format!("{component}:build"))
            .unwrap();
        assert_eq!(build.depends_on, std::slice::from_ref(&boundary_target));
    }
}

#[test]
fn adopted_frontends_omit_scaffold_only_contract_actions() {
    let answers = answers(
        r#"
[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    );
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert!(!model.frontend_contracts_enabled());
    assert!(model.actions.iter().all(|action| {
        !matches!(
            action.target.action.as_str(),
            FRONTEND_CONTRACT_DRIFT_ACTION | FRONTEND_PUBLIC_BOUNDARY_ACTION
        )
    }));
    for action in model.actions.iter().filter(|action| {
        matches!(action.target.action.as_str(), "typecheck" | "build")
            && action.target.component.as_str() == "web"
    }) {
        assert!(action.depends_on.is_empty());
    }
}

#[test]
fn frontend_component_ids_reject_reserved_repository_names() {
    for name in ["api", "API", "repo"] {
        let error = frontend_component_id(name).unwrap_err().to_string();

        assert!(
            error.contains("reserved repository component id"),
            "{error}"
        );
        assert!(error.contains(name), "{error}");
    }
}

#[test]
fn truncated_frontend_component_id_has_one_digest_separator() {
    let name = format!("{}-{}", "a".repeat(50), "b".repeat(20));
    let id = frontend_component_id(&name).unwrap();

    assert_eq!(id.as_str().len(), 63);
    assert!(!id.as_str().contains("--"));
}

#[test]
fn rust_repository_uses_adapter_actions_without_backend_identity_fields() {
    let model = RepositoryRenderModel::from_answers(&answers("")).unwrap();
    let authored = model.authored_toml().unwrap();
    let commands = model.commands_toml().unwrap();

    assert!(authored.contains("adapters = [\"rust\"]"));
    assert!(!authored.contains("backend_language"));
    assert!(commands.contains("api_test_command"));
    assert!(!commands.contains("rust_test_command"));
}

#[test]
fn schema_freshness_is_part_of_the_default_profile_when_enabled() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"
sqlx_enabled = true
schema_dump_enabled = true
migration_dir = "migrations"
schema_dump_command = "scripts/dump-schema.sh"
"#,
    )
    .unwrap();
    let answers = RenderAnswers::from_answers_file(&path).unwrap();

    let model = RepositoryRenderModel::from_answers(&answers).unwrap();

    let profile = model.profiles.first().unwrap();
    assert!(
        profile
            .targets
            .iter()
            .any(|target| target.to_string() == "api:schema")
    );
    let schema = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:schema")
        .unwrap();
    assert!(
        schema
            .effects
            .contains(&jig_contract::ActionEffect::ReadOnly)
    );
    assert!(
        !schema
            .effects
            .contains(&jig_contract::ActionEffect::Worktree)
    );
}

#[test]
fn frontend_actions_depend_on_their_shared_runner() {
    let inputs = frontend_inputs("apps/web", &["src/**"]);

    assert!(inputs.contains(&"apps/web/src/**".to_owned()));
    assert!(inputs.contains(&"scripts/check-webapps.sh".to_owned()));
    assert!(inputs.contains(&"scripts/contracts.mjs".to_owned()));
    assert!(inputs.contains(&"scripts/enforce-coverage.cjs".to_owned()));
    assert!(inputs.contains(&"scripts/web-node.cjs".to_owned()));
    assert!(inputs.contains(&"openapi/**".to_owned()));
    assert!(inputs.contains(&"packages/*-api-client/**".to_owned()));
    assert!(inputs.contains(&"pnpm-workspace.yaml".to_owned()));
}

#[test]
fn generated_migration_action_uses_the_effective_configured_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"
sqlx_enabled = true
migration_dir = "database/changes"
"#,
    )
    .unwrap();
    let answers = RenderAnswers::from_answers_file(&path).unwrap();

    let model = RepositoryRenderModel::from_answers(&answers).unwrap();
    let migration = model
        .actions
        .iter()
        .find(|action| action.target.to_string() == "api:migration-add")
        .unwrap();

    assert_eq!(migration.inputs, ["database/changes/**".to_owned()]);
}

#[test]
fn generated_migration_action_rejects_the_repository_root_as_its_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"
sqlx_enabled = true
migration_dir = "."
"#,
    )
    .unwrap();

    let error = RenderAnswers::from_answers_file(&path)
        .unwrap_err()
        .to_string();

    assert!(error.contains("below the repository root"), "{error}");
}

#[test]
fn adapter_identity_survives_loading_v6_authored_answers() {
    let answers = answers(
        r#"
[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "."
adapters = ["go", "go-postgres"]
"#,
    );

    assert!(answers.backend_language().is_go());
    assert!(answers.go_database().is_postgres());
    let model = RepositoryRenderModel::from_answers(&answers).unwrap();
    assert!(
        model
            .actions
            .iter()
            .any(|action| action.target.to_string() == "api:sqlc")
    );
}

#[test]
fn component_command_overrides_survive_v6_answer_reload() {
    let initial = answers("rust_test_command = \"cargo nextest run\"\n");
    let model = RepositoryRenderModel::from_answers(&initial).unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                model.authored_toml().unwrap(),
                model.commands_toml().unwrap()
            ),
        )
        .unwrap();

    let reloaded = RenderAnswers::from_answers_file(&path).unwrap();

    assert_eq!(
        reloaded.repository_command("rust_test_command"),
        Some("cargo nextest run")
    );
}

#[test]
fn frontend_component_id_rejects_long_non_ascii_names_without_panicking() {
    let error = frontend_component_id(&"é".repeat(40))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Invalid frontend app name"), "{error}");
}

#[test]
fn go_and_typescript_command_overrides_survive_v6_answer_reload() {
    let initial = answers(
        r#"
backend_language = "go"
go_database = "postgres"
go_test_command = "go test -race ./..."
typescript_lint_command = "scripts/lint-all.sh"

[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    );
    let model = RepositoryRenderModel::from_answers(&initial).unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                model.authored_toml().unwrap(),
                model.commands_toml().unwrap()
            ),
        )
        .unwrap();

    let reloaded = RenderAnswers::from_answers_file(&path).unwrap();

    assert_eq!(
        reloaded.repository_command("go_test_command"),
        Some("go test -race ./...")
    );
    assert_eq!(
        reloaded.repository_command("typescript_lint_command"),
        Some("scripts/lint-all.sh")
    );
    let rerendered = RepositoryRenderModel::from_answers(&reloaded).unwrap();
    assert_eq!(
        serde_json::to_value(rerendered).unwrap(),
        serde_json::to_value(model).unwrap()
    );
}

#[test]
fn authored_multi_backend_model_survives_v6_recopy_resolution() {
    let api = ComponentSpec {
        adapters: vec!["go".into()],
        ..ComponentSpec::new(component_id("api").unwrap(), "services/api")
    };
    let worker = ComponentSpec {
        adapters: vec!["rust".into(), "sqlx".into()],
        ..ComponentSpec::new(component_id("worker").unwrap(), "services/worker")
    };
    let mut api_test = ActionSpec::new(
        target_id("api", "test").unwrap(),
        ActionIntent::Check,
        ActionRunner::command("api_test_command"),
    );
    api_test.effects = vec![jig_contract::ActionEffect::ReadOnly];
    let mut worker_test = ActionSpec::new(
        target_id("worker", "test").unwrap(),
        ActionIntent::Check,
        ActionRunner::command("worker_test_command"),
    );
    worker_test.effects = vec![jig_contract::ActionEffect::ReadOnly];
    let profile = ProfileSpec::new(
        ProfileId::parse("ci").unwrap(),
        vec![api_test.target.clone(), worker_test.target.clone()],
    );
    let authored = RepositoryRenderModel {
        affected_ignore: vec!["docs/**".into()],
        components: vec![api, worker],
        actions: vec![api_test, worker_test],
        profiles: vec![profile],
        default_check_profile: ProfileId::parse("ci").unwrap(),
        required_commands: vec!["api_test_command".into(), "worker_test_command".into()],
        tools: Vec::new(),
        commands: BTreeMap::from([
            ("api_test_command".into(), "go test ./...".into()),
            (
                "worker_test_command".into(),
                "cargo test -p example-worker".into(),
            ),
        ]),
    };
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
            &path,
        format!(
                "repo_name = \"ExampleProject\"\nbackend_language = \"go\"\nsqlx_enabled = true\nmigration_dir = \"database/migrations\"\nschema_dump_enabled = false\n{}\n{}",
                authored.authored_toml().unwrap(),
                authored.commands_toml().unwrap()
            ),
        )
        .unwrap();

    let answers = RenderAnswers::from_answers_file(&path).unwrap();
    assert!(answers.go_backend_enabled());
    assert!(answers.rust_backend_enabled());
    assert!(answers.sqlx_enabled());
    assert!(!answers.go_ci_workflow_enabled());
    assert!(!answers.rust_ci_workflow_enabled());
    assert!(
        crate::bootstrap::managed_paths::should_omit_unmanaged_rendered_path(
            std::path::Path::new(".github/workflows/go-tests.yml"),
            &answers,
        )
    );
    assert!(
        crate::bootstrap::managed_paths::should_omit_unmanaged_rendered_path(
            std::path::Path::new(".github/workflows/rust-tests.yml"),
            &answers,
        )
    );
    assert!(
        !crate::bootstrap::managed_paths::should_omit_unmanaged_rendered_path(
            std::path::Path::new(".github/workflows/repo-policy.yml"),
            &answers,
        )
    );
    let rerendered = RepositoryRenderModel::from_answers(&answers).unwrap();

    assert_eq!(
        rerendered
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>(),
        ["api", "worker"]
    );
    assert_eq!(
        rerendered
            .actions
            .iter()
            .map(|action| action.target.to_string())
            .collect::<Vec<_>>(),
        ["api:test", "worker:test"]
    );
    assert_eq!(
        rerendered.commands["worker_test_command"],
        "cargo test -p example-worker"
    );
    assert_eq!(rerendered.default_check_profile.as_str(), "ci");
}

#[test]
fn authored_go_workflow_renders_exact_targets_from_its_capability_aliases() {
    for (action_ids, add_aliases, read_only, add_foreign_fmt, expected) in [
        (
            ["format", "vet", "verify"],
            true,
            true,
            false,
            [Some("api:format"), Some("api:vet"), Some("api:verify")],
        ),
        (
            ["fmt", "lint", "test-locked"],
            true,
            true,
            true,
            [Some("api:fmt"), Some("api:lint"), Some("api:test-locked")],
        ),
        (
            ["fmt", "lint", "test-locked"],
            false,
            true,
            false,
            [None, None, None],
        ),
        (
            ["fmt", "lint", "test-locked"],
            true,
            false,
            false,
            [None, None, None],
        ),
    ] {
        let component = ComponentSpec {
            adapters: vec!["go".into()],
            ..ComponentSpec::new(component_id("api").unwrap(), "services/api")
        };
        let mut actions = action_ids
            .into_iter()
            .zip([
                jig_contract::tool::FMT_CHECK,
                jig_contract::tool::LINT,
                jig_contract::tool::TEST_LOCKED,
            ])
            .map(|(action_id, alias)| {
                let mut action = ActionSpec::new(
                    target_id("api", action_id).unwrap(),
                    ActionIntent::Check,
                    ActionRunner::command(format!("go_{action_id}_command")),
                );
                action.effects = if read_only {
                    vec![
                        jig_contract::ActionEffect::ReadOnly,
                        jig_contract::ActionEffect::Process,
                    ]
                } else {
                    vec![jig_contract::ActionEffect::Worktree]
                };
                if add_aliases {
                    action.legacy_aliases.push(alias.into());
                }
                action
            })
            .collect::<Vec<_>>();
        let mut components = vec![component];
        if add_foreign_fmt {
            components.push(ComponentSpec {
                adapters: vec!["rust".into()],
                ..ComponentSpec::new(component_id("worker").unwrap(), "services/worker")
            });
            let mut action = ActionSpec::new(
                target_id("worker", "fmt").unwrap(),
                ActionIntent::Check,
                ActionRunner::command("rust_fmt_command"),
            );
            action.effects = vec![
                jig_contract::ActionEffect::ReadOnly,
                jig_contract::ActionEffect::Process,
            ];
            actions.push(action);
        }
        let targets = actions
            .iter()
            .map(|action| action.target.clone())
            .collect::<Vec<_>>();
        let commands = actions
            .iter()
            .map(|action| {
                let ActionRunner::Command { command, .. } = &action.runner else {
                    unreachable!()
                };
                (command.to_string(), "true".to_string())
            })
            .collect::<BTreeMap<_, _>>();
        let authored = RepositoryRenderModel {
            affected_ignore: Vec::new(),
            components,
            profiles: vec![ProfileSpec::new(ProfileId::parse("ci").unwrap(), targets)],
            default_check_profile: ProfileId::parse("ci").unwrap(),
            required_commands: commands.keys().cloned().collect(),
            tools: Vec::new(),
            commands,
            actions: std::mem::take(&mut actions),
        };
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("answers.toml");
        fs::write(
            &path,
            format!(
                "repo_name = \"ExampleProject\"\nsqlx_enabled = false\nschema_dump_enabled = false\n{}\n{}",
                authored.authored_toml().unwrap(),
                authored.commands_toml().unwrap()
            ),
        )
        .unwrap();

        let answers = RenderAnswers::from_answers_file(&path).unwrap();
        assert_eq!(
            [
                answers.go_fmt_ci_target(),
                answers.go_lint_ci_target(),
                answers.go_test_locked_ci_target(),
            ],
            expected.map(|target| target.map(str::to_owned))
        );
        assert_eq!(
            answers.go_ci_workflow_enabled(),
            expected.iter().all(Option::is_some)
        );
    }
}
