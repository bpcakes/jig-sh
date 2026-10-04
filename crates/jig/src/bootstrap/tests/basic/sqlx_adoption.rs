use super::*;

const SQLX_PACKAGE: &str = "[package]\nname = \"example-service\"\nversion = \"0.1.0\"\n\n[dependencies]\nsqlx = \"0.9\"\n";
const PLAIN_PACKAGE: &str = "[package]\nname = \"example-service\"\nversion = \"0.1.0\"\n";
const GO_MODULE: &str = "module example.com/ExampleProject\n\ngo 1.24\n";
const GOOSE_SQL: &str =
    "-- +goose Up\nCREATE TABLE example (id integer);\n\n-- +goose Down\nDROP TABLE example;\n";
const GENERIC_SQL: &str = "CREATE TABLE example (id integer);\n";

fn fixture(temp: &Path, files: &[(&str, &str)]) -> PathBuf {
    let repo = temp.join("ExampleProject");
    for (path, text) in files {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::create_dir_all(&repo).unwrap();
    repo
}

fn adopt_opts(repo: &Path, template: &Path, answers: AnswerOpts) -> AdoptOpts {
    AdoptOpts {
        components: Default::default(),
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            ..answers
        },
    }
}

fn update(repo: &Path, template: &Path, recopy: bool) {
    run_update(UpdateOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        recopy,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
}

fn sqlx_review(output: &serde_json::Value) -> Vec<&str> {
    output["adoption_review"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item.as_str().filter(|item| item.starts_with("SQLx:")))
        .collect()
}

fn generates_sqlx_check(output: &serde_json::Value) -> bool {
    output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate.as_str().unwrap().ends_with("check sqlx"))
}

fn sqlx_warnings(output: &serde_json::Value) -> Vec<&str> {
    output["detection_report"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|warning| warning.as_str().filter(|warning| warning.contains("SQLx")))
        .collect()
}

fn config(repo: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(repo.join(".jig.toml")).unwrap()).unwrap()
}

#[test]
fn generic_and_goose_migrations_adopt_without_sqlx() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    for (files, include) in [
        (vec![("migrations/0001_init.sql", GENERIC_SQL)], None),
        (
            vec![
                ("go.mod", GO_MODULE),
                ("main.go", "package main\n\nfunc main() {}\n"),
                ("migrations/00001_init.sql", GOOSE_SQL),
            ],
            None,
        ),
        (
            vec![
                ("backend/go.mod", GO_MODULE),
                ("backend/main.go", "package main\n\nfunc main() {}\n"),
                ("backend/migrations/00001_init.sql", GOOSE_SQL),
            ],
            Some("backend"),
        ),
        (
            vec![
                ("Cargo.toml", PLAIN_PACKAGE),
                ("src/lib.rs", ""),
                ("migrations/0001_init.sql", GENERIC_SQL),
            ],
            None,
        ),
    ] {
        let temp = tempdir().unwrap();
        let repo = fixture(temp.path(), &files);
        let mut opts = adopt_opts(&repo, template.path(), AnswerOpts::default());
        opts.components.include = include.into_iter().map(Into::into).collect();
        let output = run_adopt(opts).unwrap();

        assert_eq!(
            output["detection_report"]["sqlx_enabled"], false,
            "{files:?}"
        );
        assert!(!generates_sqlx_check(&output), "{files:?}: {output}");
        assert!(sqlx_review(&output).is_empty(), "{files:?}: {output}");
        assert!(sqlx_warnings(&output).is_empty(), "{files:?}: {output}");
    }
}

#[test]
fn explicit_sqlx_disable_reaches_review_and_generated_checks() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let temp = tempdir().unwrap();
    let repo = fixture(
        temp.path(),
        &[
            ("Cargo.toml", SQLX_PACKAGE),
            ("src/lib.rs", ""),
            ("migrations/0001_init.sql", GENERIC_SQL),
        ],
    );
    let answers_file = temp.path().join("answers.toml");
    fs::write(&answers_file, "sqlx_enabled = false\n").unwrap();
    for answers in [
        AnswerOpts {
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
        AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    ] {
        let output = run_adopt(adopt_opts(&repo, template.path(), answers)).unwrap();

        // Detected evidence stays visible while the effective answer wins.
        assert_eq!(output["detection_report"]["sqlx_enabled"], true);
        assert_eq!(
            sqlx_review(&output),
            vec!["SQLx: disabled by explicit answer; detected SQLx evidence is not applied"]
        );
        assert!(!generates_sqlx_check(&output), "{output}");
    }
}

#[test]
fn explicit_sqlx_enable_never_guesses_a_goose_migration_dir() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let enabled = || AnswerOpts {
        sqlx_enabled: Some(true),
        ..AnswerOpts::default()
    };
    let go_files = [
        ("Cargo.toml", PLAIN_PACKAGE),
        ("src/lib.rs", ""),
        ("backend/go.mod", GO_MODULE),
        ("backend/main.go", "package main\n\nfunc main() {}\n"),
        ("backend/migrations/00001_init.sql", GOOSE_SQL),
    ];

    // A Cargo ancestor that does not declare sqlx cannot claim migrations that
    // Go code may also use, so the path must be chosen explicitly.
    let temp = tempdir().unwrap();
    let mut files = go_files.to_vec();
    files.push(("db/migrations/0001_init.sql", GENERIC_SQL));
    let repo = fixture(temp.path(), &files);
    let mut opts = adopt_opts(&repo, template.path(), enabled());
    opts.components.include = vec!["backend".into()];
    let error = run_adopt(opts.clone()).unwrap_err().to_string();
    assert!(
        error.contains(
            "cannot infer the SQLx migration directory from db/migrations (inside Cargo manifest at ., which does not declare sqlx)"
        ),
        "{error}"
    );
    assert!(!error.contains("backend/migrations"), "{error}");
    opts.answers.rust_migration_dir = Some("db/migrations".into());
    let output = run_adopt(opts).unwrap();
    assert_eq!(output["detection_report"]["sqlx_enabled"], false);
    assert_eq!(
        sqlx_review(&output),
        vec!["SQLx: enabled with migrations at db/migrations"]
    );
    assert!(generates_sqlx_check(&output), "{output}");

    let temp = tempdir().unwrap();
    let repo = fixture(temp.path(), &go_files);
    let mut opts = adopt_opts(&repo, template.path(), enabled());
    opts.components.include = vec!["backend".into()];
    let error = run_adopt(opts).unwrap_err().to_string();
    assert!(
        error.contains("Missing required answer when sqlx_enabled is true"),
        "{error}"
    );
    assert!(!error.contains("backend/migrations"), "{error}");
}

#[test]
fn authored_sqlx_answers_survive_update_recopy_and_readoption() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let temp = tempdir().unwrap();
    // No SQLx evidence: only the authored answer keeps SQLx enabled.
    let repo = fixture(
        temp.path(),
        &[
            ("Cargo.toml", PLAIN_PACKAGE),
            ("src/lib.rs", ""),
            ("db/migrations/0001_init.sql", GENERIC_SQL),
            ("backend/go.mod", GO_MODULE),
            ("backend/migrations/00001_init.sql", GOOSE_SQL),
        ],
    );
    let mut opts = adopt_opts(
        &repo,
        template.path(),
        AnswerOpts {
            sqlx_enabled: Some(true),
            rust_migration_dir: Some("db/migrations".into()),
            ..AnswerOpts::default()
        },
    );
    opts.write = true;
    run_adopt(opts).unwrap();
    let assert_authored_sqlx = |stage: &str| {
        let config = config(&repo);
        assert_eq!(config["sqlx_enabled"].as_bool(), Some(true), "{stage}");
        assert_eq!(
            config["migration_dir"].as_str(),
            Some("db/migrations"),
            "{stage}"
        );
        let adapters = config["repository"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|component| component["adapters"].as_array().unwrap())
            .filter_map(toml::Value::as_str)
            .collect::<Vec<_>>();
        assert!(adapters.contains(&"sqlx"), "{stage}: {adapters:?}");
    };
    assert_authored_sqlx("adopt");
    update(&repo, template.path(), false);
    assert_authored_sqlx("update");
    update(&repo, template.path(), true);
    assert_authored_sqlx("recopy");

    let output = run_adopt(adopt_opts(&repo, template.path(), AnswerOpts::default())).unwrap();
    assert_eq!(output["detection_report"]["sqlx_enabled"], false);
    assert_eq!(
        sqlx_review(&output),
        vec!["SQLx: enabled with migrations at db/migrations"]
    );
    assert!(generates_sqlx_check(&output), "{output}");
}

#[test]
fn authored_sqlx_disable_survives_readoption_with_sqlx_evidence() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let temp = tempdir().unwrap();
    let repo = fixture(
        temp.path(),
        &[
            ("Cargo.toml", SQLX_PACKAGE),
            ("src/lib.rs", ""),
            ("migrations/0001_init.sql", GENERIC_SQL),
        ],
    );
    let mut opts = adopt_opts(
        &repo,
        template.path(),
        AnswerOpts {
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    );
    opts.write = true;
    run_adopt(opts).unwrap();
    update(&repo, template.path(), true);
    assert_eq!(config(&repo)["sqlx_enabled"].as_bool(), Some(false));

    let output = run_adopt(adopt_opts(&repo, template.path(), AnswerOpts::default())).unwrap();
    assert_eq!(output["detection_report"]["sqlx_enabled"], true);
    assert_eq!(
        sqlx_review(&output),
        vec!["SQLx: disabled by explicit answer; detected SQLx evidence is not applied"]
    );
    assert!(!generates_sqlx_check(&output), "{output}");
    // Inferred SQLx defaults are moot for an authored model.
    assert!(sqlx_warnings(&output).is_empty(), "{output}");
}

#[test]
fn implied_sqlx_enablement_uses_the_owned_migration_dir() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let temp = tempdir().unwrap();
    // Schema dumps imply SQLx without an explicit sqlx_enabled answer.
    let repo = fixture(
        temp.path(),
        &[
            ("Cargo.toml", PLAIN_PACKAGE),
            ("src/lib.rs", ""),
            ("migrations/0001_init.sql", GENERIC_SQL),
        ],
    );
    let output = run_adopt(adopt_opts(
        &repo,
        template.path(),
        AnswerOpts {
            schema_dump_enabled: Some(true),
            ..AnswerOpts::default()
        },
    ))
    .unwrap();

    assert_eq!(output["detection_report"]["sqlx_enabled"], false);
    assert_eq!(
        sqlx_review(&output),
        vec!["SQLx: enabled with migrations at migrations"]
    );
    assert!(generates_sqlx_check(&output), "{output}");
}

#[test]
fn accepted_rust_root_requires_a_path_for_unowned_migrations() {
    let _guard = lock_env();
    let template = materialize_template_worktree();
    let temp = tempdir().unwrap();
    // No root Cargo.toml: the explicit answer accepts the Rust root, but no
    // manifest owns migrations/, so SQLx evidence cannot claim it.
    let repo = fixture(
        temp.path(),
        &[
            (".sqlx/query-example.json", "{}\n"),
            ("migrations/0001_init.sql", GENERIC_SQL),
        ],
    );
    let accepted = |rust_migration_dir: Option<&str>| AnswerOpts {
        backend_language: Some(crate::backend::BackendLanguage::Rust),
        rust_migration_dir: rust_migration_dir.map(Into::into),
        ..AnswerOpts::default()
    };

    let error = run_adopt(adopt_opts(&repo, template.path(), accepted(None)))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "cannot infer the SQLx migration directory from migrations (no owning Cargo or Go manifest)"
        ),
        "{error}"
    );

    let output = run_adopt(adopt_opts(
        &repo,
        template.path(),
        accepted(Some("migrations")),
    ))
    .unwrap();
    assert_eq!(output["detection_report"]["sqlx_enabled"], true);
    assert_eq!(
        sqlx_review(&output),
        vec!["SQLx: enabled with migrations at migrations"]
    );
    assert!(generates_sqlx_check(&output), "{output}");
}
