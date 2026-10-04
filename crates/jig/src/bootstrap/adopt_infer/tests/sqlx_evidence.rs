use super::*;

const SQLX_PACKAGE: &str = "[package]\nname = \"example-service\"\nversion = \"0.1.0\"\n\n[dependencies]\nsqlx = \"0.9\"\n";
const PLAIN_PACKAGE: &str = "[package]\nname = \"example-service\"\nversion = \"0.1.0\"\n";
const GO_MODULE: &str = "module example.com/ExampleProject\n\ngo 1.24\n";
const GOOSE_SQL: &str =
    "-- +goose Up\nCREATE TABLE example (id integer);\n\n-- +goose Down\nDROP TABLE example;\n";
const GENERIC_SQL: &str = "CREATE TABLE example (id integer);\n";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for (path, text) in files {
        write(temp.path(), path, text);
    }
    temp
}

fn migration_dir(sqlx: &SqlxInference) -> Option<&str> {
    sqlx.migration_dir
        .as_ref()
        .map(|value| value.value.as_str())
}

fn assert_no_sqlx(files: &[(&str, &str)]) {
    let temp = repo(files);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);
    assert!(!sqlx.enabled.value, "{files:?}: {:?}", sqlx.enabled.sources);
    assert!(sqlx.migration_dirs.value.is_empty(), "{files:?}");
    assert_eq!(migration_dir(&sqlx), None, "{files:?}");
    assert!(sqlx.metadata_dir.is_none(), "{files:?}");
    assert!(sqlx.check_command.is_none(), "{files:?}");
    assert!(warnings.is_empty(), "{files:?}: {warnings:?}");
}

#[test]
fn generic_and_goose_migrations_are_not_sqlx_evidence() {
    assert_no_sqlx(&[("migrations/0001_init.sql", GENERIC_SQL)]);
    assert_no_sqlx(&[
        ("go.mod", GO_MODULE),
        ("migrations/00001_init.sql", GOOSE_SQL),
    ]);
    assert_no_sqlx(&[
        ("backend/go.mod", GO_MODULE),
        ("backend/migrations/00001_init.sql", GOOSE_SQL),
    ]);
    assert_no_sqlx(&[
        ("Cargo.toml", PLAIN_PACKAGE),
        ("migrations/0001_init.sql", GENERIC_SQL),
    ]);
}

#[test]
fn sqlx_specific_evidence_still_enables_sqlx() {
    for (files, source) in [
        (
            vec![
                ("crates/db/Cargo.toml", SQLX_PACKAGE),
                ("crates/db/src/lib.rs", ""),
            ],
            "SQLx dependency in crates/db/Cargo.toml [dependencies].sqlx",
        ),
        (vec![(".sqlx/query-example.json", "{}")], ".sqlx/"),
        (
            vec![("src/db.rs", "fn run() { sqlx::migrate!(); }\n")],
            "sqlx::migrate! macro in src/db.rs",
        ),
        (
            vec![("scripts/db.sh", "set -e\ncargo sqlx migrate run\n")],
            "cargo sqlx command in scripts/db.sh",
        ),
        (
            vec![(
                "scripts/check.sh",
                "SQLX_OFFLINE=true cargo sqlx prepare --check\n",
            )],
            "cargo sqlx command in scripts/check.sh",
        ),
    ] {
        let temp = repo(&files);
        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);
        assert!(sqlx.enabled.value, "{files:?}");
        assert!(
            sqlx.enabled
                .sources
                .iter()
                .chain(&sqlx.signals)
                .any(|value| value == source),
            "{files:?}: {:?} / {:?}",
            sqlx.enabled.sources,
            sqlx.signals
        );
    }
}

#[test]
fn mixed_repository_uses_only_the_rust_owned_migration_dir() {
    let temp = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("migrations/0001_init.sql", GENERIC_SQL),
        ("backend/go.mod", GO_MODULE),
        ("backend/migrations/000001_init.up.sql", GENERIC_SQL),
        ("db/migrations/00001_init.sql", GOOSE_SQL),
    ]);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(sqlx.enabled.value);
    assert_eq!(migration_dir(&sqlx), Some("migrations"));
    assert_eq!(sqlx.migration_dirs.value, vec!["migrations".to_string()]);
    assert!(
        !sqlx
            .enabled
            .sources
            .iter()
            .any(|source| source.ends_with(".sql"))
    );
    for signal in [
        "migration directory not used for SQLx: backend/migrations (inside Go module backend)",
        "migration directory not used for SQLx: db/migrations (contains Goose migration annotations)",
        "migration directory migrations (Cargo manifest at . declares sqlx)",
    ] {
        assert!(
            sqlx.signals.iter().any(|value| value == signal),
            "{signal}: {:?}",
            sqlx.signals
        );
    }
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("migrations/") || warning.contains("cannot infer")),
        "{warnings:?}"
    );
}

#[test]
fn excluded_candidates_keep_the_default_fallback_unless_they_occupy_it() {
    let temp = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("backend/go.mod", GO_MODULE),
        ("backend/migrations/00001_init.sql", GOOSE_SQL),
    ]);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);
    assert_eq!(migration_dir(&sqlx), Some("migrations"));
    assert!(warnings.iter().any(|warning| {
        warning.contains("SQLx was detected but migration and metadata directories were not")
    }));

    let temp = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("migrations/00001_init.sql", GOOSE_SQL),
    ]);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);
    assert!(sqlx.enabled.value);
    assert_eq!(migration_dir(&sqlx), None);
    assert!(sqlx.migration_dirs.value.is_empty());
    assert!(
        warnings.iter().any(|warning| warning.contains(
            "cannot infer the SQLx migration directory from migrations (contains Goose migration annotations)"
        )),
        "{warnings:?}"
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("default migrations/")
                || warning.contains("default SQLx paths")),
        "{warnings:?}"
    );
}

#[test]
fn unjustified_multiple_migration_dirs_are_ambiguous() {
    let temp = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("crates/api/migrations/20240101_init/up.sql", GENERIC_SQL),
        ("services/billing/migrations/0001.sql", GENERIC_SQL),
    ]);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(sqlx.enabled.value);
    assert_eq!(migration_dir(&sqlx), None);
    assert_eq!(
        sqlx.migration_dirs.value,
        vec![
            "crates/api/migrations".to_string(),
            "services/billing/migrations".to_string(),
        ]
    );
    assert!(sqlx.signals.iter().any(|signal| signal
        == "migration directories detected: crates/api/migrations, services/billing/migrations"));
    let ambiguity = "cannot infer the SQLx migration directory from crates/api/migrations (Cargo manifest at . declares sqlx), services/billing/migrations (Cargo manifest at . declares sqlx); pass --rust-migration-dir <dir>";
    assert!(
        warnings.iter().any(|warning| warning.contains(ambiguity)),
        "{warnings:?}"
    );
    assert!(
        sqlx.migration_dirs
            .warnings
            .iter()
            .any(|warning| warning.contains(ambiguity))
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("alphabetically"))
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("default migrations/"))
    );
}

#[test]
fn an_owner_declaring_sqlx_justifies_its_migration_dir() {
    let temp = repo(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
        ),
        ("crates/db/Cargo.toml", SQLX_PACKAGE),
        ("crates/db/migrations/0001_init.sql", GENERIC_SQL),
        ("crates/legacy/Cargo.toml", PLAIN_PACKAGE),
        ("crates/legacy/migrations/0001_init.sql", GENERIC_SQL),
    ]);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert_eq!(migration_dir(&sqlx), Some("crates/db/migrations"));
    assert_eq!(sqlx.migration_dirs.value.len(), 2);
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("cannot infer"))
    );
}

#[test]
fn unattributable_migration_dirs_are_ambiguous() {
    for (files, expected) in [
        (
            vec![
                ("Cargo.toml", SQLX_PACKAGE),
                ("go.mod", GO_MODULE),
                ("migrations/000001_init.up.sql", GENERIC_SQL),
            ],
            "migrations (. has both Cargo.toml and go.mod)",
        ),
        (
            vec![
                ("services/api/Cargo.toml", SQLX_PACKAGE),
                ("backend/go.mod", GO_MODULE),
                ("db/migrations/0001_init.sql", GENERIC_SQL),
            ],
            "db/migrations (no owning Cargo or Go manifest)",
        ),
    ] {
        let temp = repo(&files);
        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);
        assert!(sqlx.enabled.value, "{files:?}");
        assert_eq!(migration_dir(&sqlx), None, "{files:?}");
        assert!(
            warnings.iter().any(|warning| warning.contains(expected)),
            "{files:?}: {warnings:?}"
        );
    }

    // Without Go modules, an unowned directory is the only SQLx candidate.
    let temp = repo(&[
        ("services/api/Cargo.toml", SQLX_PACKAGE),
        ("db/migrations/0001_init.sql", GENERIC_SQL),
    ]);
    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);
    assert_eq!(migration_dir(&sqlx), Some("db/migrations"));
}

#[test]
fn final_selection_replaces_discovery_sqlx_warnings() {
    let temp = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("migrations/0001_init.sql", GENERIC_SQL),
        ("tools/seed/Cargo.toml", SQLX_PACKAGE),
        ("tools/seed/migrations/0001_seed.sql", GENERIC_SQL),
    ]);
    let mut inference = infer_adopt_answers(temp.path());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| warning.contains("cannot infer the SQLx migration directory")),
        "{:?}",
        inference.warnings
    );
    assert_eq!(inference.rust_migration_dir, None);

    inference
        .select_components(temp.path(), &ComponentSelectionOpts::default())
        .unwrap();

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert_eq!(inference.rust_migration_dir.as_deref(), Some("migrations"));
    assert!(
        !inference
            .warnings
            .iter()
            .any(|warning| warning.contains("cannot infer") || warning.contains("tools/seed")),
        "{:?}",
        inference.warnings
    );
    assert_eq!(
        inference
            .warnings
            .iter()
            .filter(|warning| warning.contains("SQLx metadata directory was not detected"))
            .count(),
        1
    );
}

#[test]
fn accepted_plain_rust_root_keeps_generic_migrations_out_of_sqlx() {
    let temp = repo(&[
        ("Cargo.toml", PLAIN_PACKAGE),
        ("migrations/0001_init.sql", GENERIC_SQL),
    ]);
    let mut inference = infer_adopt_answers(temp.path());
    inference
        .select_components(temp.path(), &ComponentSelectionOpts::default())
        .unwrap();

    assert_eq!(inference.sqlx_enabled, Some(false));
    assert_eq!(inference.rust_migration_dir, None);
    assert!(inference.rust_migration_dirs.is_empty());
    assert!(
        !inference
            .warnings
            .iter()
            .any(|warning| warning.contains("SQLx")),
        "{:?}",
        inference.warnings
    );
    let mut answers = AnswerOpts::default();
    inference.apply_to_answers(&mut answers, &AnswerInputShape::default());
    assert_eq!(answers.sqlx_enabled, Some(false));
    assert_eq!(answers.rust_migration_dir, None);
}

#[test]
fn explicit_sqlx_answers_control_migration_defaults() {
    let shape = AnswerInputShape::default();
    let generic = repo(&[
        ("Cargo.toml", PLAIN_PACKAGE),
        ("migrations/0001_init.sql", GENERIC_SQL),
        ("backend/go.mod", GO_MODULE),
        ("backend/migrations/00001_init.sql", GOOSE_SQL),
    ]);
    let inference = infer_adopt_answers(generic.path());
    let mut answers = AnswerOpts {
        sqlx_enabled: Some(true),
        ..AnswerOpts::default()
    };
    inference.apply_to_answers(&mut answers, &shape);
    assert_eq!(answers.rust_migration_dir.as_deref(), Some("migrations"));
    assert_eq!(answers.sqlx_check_command, None);

    let goose = repo(&[
        ("Cargo.toml", PLAIN_PACKAGE),
        ("backend/go.mod", GO_MODULE),
        ("backend/migrations/00001_init.sql", GOOSE_SQL),
    ]);
    let inference = infer_adopt_answers(goose.path());
    let mut answers = AnswerOpts {
        sqlx_enabled: Some(true),
        ..AnswerOpts::default()
    };
    inference.apply_to_answers(&mut answers, &shape);
    assert_eq!(answers.rust_migration_dir, None);

    // SQLx-shaped answers and schema dumps imply SQLx without sqlx_enabled.
    let inference = infer_adopt_answers(generic.path());
    for mut answers in [
        AnswerOpts {
            schema_dump_enabled: Some(true),
            ..AnswerOpts::default()
        },
        AnswerOpts {
            sqlx_check_command: Some("cargo sqlx prepare --check".into()),
            ..AnswerOpts::default()
        },
    ] {
        inference.apply_to_answers(&mut answers, &shape);
        assert_eq!(answers.sqlx_enabled, None);
        assert_eq!(answers.rust_migration_dir.as_deref(), Some("migrations"));
    }

    let detected = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("migrations/0001_init.sql", GENERIC_SQL),
    ]);
    let inference = infer_adopt_answers(detected.path());
    let mut answers = AnswerOpts {
        sqlx_enabled: Some(false),
        ..AnswerOpts::default()
    };
    inference.apply_to_answers(&mut answers, &shape);
    assert_eq!(answers.sqlx_enabled, Some(false));
    assert_eq!(answers.rust_migration_dir, None);
    assert_eq!(answers.sqlx_check_command, None);
    let disabled = EffectiveSqlx::default();
    assert_eq!(
        inference.sqlx_review_item(&disabled).as_deref(),
        Some("SQLx: disabled by explicit answer; detected SQLx evidence is not applied")
    );

    // A canonical answers-file migration_dir wins over the inferred directory.
    let table = [(
        "migration_dir".to_string(),
        toml::Value::String("db/migrations".into()),
    )]
    .into_iter()
    .collect();
    let mut answers = AnswerOpts::default();
    inference.apply_to_answers(&mut answers, &AnswerInputShape::from_table(&table));
    assert_eq!(answers.rust_migration_dir, None);
    assert!(answers.rust_sqlx_metadata_dir.is_some());
}

#[test]
fn ambiguous_sqlx_migrations_require_an_explicit_answer() {
    let temp = repo(&[
        ("Cargo.toml", SQLX_PACKAGE),
        ("crates/api/migrations/0001.sql", GENERIC_SQL),
        ("services/billing/migrations/0001.sql", GENERIC_SQL),
    ]);
    let inference = infer_adopt_answers(temp.path());
    let enabled = EffectiveSqlx {
        enabled: true,
        migration_dir: None,
    };
    let error = inference
        .require_sqlx_migration_answer(&enabled)
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("SQLx is enabled, but Jig cannot infer the SQLx migration directory from crates/api/migrations"),
        "{error}"
    );
    assert!(error.contains("--rust-migration-dir <dir>"), "{error}");
    assert!(
        inference
            .sqlx_review_item(&enabled)
            .unwrap()
            .starts_with("SQLx: enabled, but Jig cannot infer")
    );

    let chosen = EffectiveSqlx {
        enabled: true,
        migration_dir: Some("services/billing/migrations".into()),
    };
    inference.require_sqlx_migration_answer(&chosen).unwrap();
    assert_eq!(
        inference.sqlx_review_item(&chosen).as_deref(),
        Some("SQLx: enabled with migrations at services/billing/migrations")
    );
    inference
        .require_sqlx_migration_answer(&EffectiveSqlx::default())
        .unwrap();
}
