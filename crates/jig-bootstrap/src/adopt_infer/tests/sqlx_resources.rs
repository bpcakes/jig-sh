use super::*;

const RESOURCE_CHILD_ENV: &str = "JIG_TEST_SQLX_ADOPTION_RESOURCES";
const RESOURCE_CHILD_TEST: &str = "adopt_infer::tests::sqlx_resources::sqlx_resource_child";

#[test]
fn sqlx_adoption_resources_are_bounded_in_subprocess() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", RESOURCE_CHILD_TEST, "--nocapture"])
        .env(RESOURCE_CHILD_ENV, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "resource helper failed: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("resource cases completed"));
}

#[test]
fn sqlx_resource_child() {
    if std::env::var_os(RESOURCE_CHILD_ENV).is_none() {
        return;
    }
    // Keep a real macro in each candidate so the negative prefilter cannot mask
    // parser failures. Exercise flat recursive ASTs as well as token groups.
    for expression in [
        format!("{}0{}", "(".repeat(5_000), ")".repeat(5_000)),
        format!("{}0{}", "(".repeat(200_000), ")".repeat(200_000)),
        format!("{}0", "!".repeat(5_000)),
        format!("0{}", "+0".repeat(5_000)),
        format!("value{}", ".method()".repeat(5_000)),
        format!("None::<{}u8{}>", "Vec<".repeat(5_000), ">".repeat(5_000)),
        format!("{}0", "(".repeat(5_000)), // lexical error must also drop safely
    ] {
        let source =
            format!("fn production() {{ let _ = sqlx::migrate!(); let _ = {expression}; }}");
        let (enabled, warnings) = infer_source(&source);
        assert!(!enabled);
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("source.rs")
                    && (warning.contains("token complexity limit")
                        || warning.contains("Rust tokenization"))
            }),
            "{warnings:?}"
        );
    }
    // Near-budget recursion remains supported, including AST visiting/drop.
    for expression in [
        format!("{}0{}", "(".repeat(2_000), ")".repeat(2_000)),
        format!("{}0", "!".repeat(2_000)),
        format!("0{}", "+0".repeat(1_000)),
        format!("value{}", ".method()".repeat(500)),
        format!("None::<{}u8{}>", "Vec<".repeat(650), ">".repeat(650)),
    ] {
        let source =
            format!("fn production() {{ let _ = sqlx::migrate!(); let _ = {expression}; }}");
        let (enabled, warnings) = infer_source(&source);
        assert!(enabled, "{warnings:?}");
    }
    // Comments and strings cannot inflate the syntax budget with fake brackets.
    let source = format!("const TEXT: &str = \"migrate{}\";", "(".repeat(5_000));
    let (enabled, warnings) = infer_source(&source);
    assert!(!enabled);
    assert!(warnings.is_empty(), "{warnings:?}");
    println!("resource cases completed");
}

fn infer_source(source: &str) -> (bool, Vec<String>) {
    assert!(source.len() < super::super::scan::MAX_SCAN_FILE_BYTES as usize);
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("source.rs"), source).unwrap();
    let mut warnings = Vec::new();
    let inference = infer_sqlx(temp.path(), &mut warnings);
    (inference.enabled.value, warnings)
}

#[test]
fn irrelevant_rust_templates_do_not_crowd_out_sqlx_path_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[dependencies]\nsqlx = '0.9'\n",
    )
    .unwrap();
    for index in 0..30 {
        fs::write(
            temp.path().join(format!("template_{index}.rs")),
            "fn {{name}}() {}",
        )
        .unwrap();
    }
    let mut warnings = Vec::new();
    let inference = infer_sqlx(temp.path(), &mut warnings);
    assert!(inference.enabled.value);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("using default SQLx paths"));
}

#[test]
fn production_shaped_rust_sources_preserve_migration_signals() {
    let scaffold = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../templates/scaffolds/rust-react/workspace/crates/db/src/lib.rs.jinja"
    ))
    .replace("<<[ db_pool ]>>", "PgPool")
    .replace("<<[ db_database ]>>", "Postgres")
    .replace("<<[ migration_path ]>>", "./migrations");
    // Exercise a larger maintained module as well as the generated database
    // source; broad item/block structure must not exhaust the parser budget.
    let module = format!(
        "{}\nfn example_migrations() {{ sqlx::migrate!(); }}",
        include_str!("../rust_sqlx.rs")
    );
    for source in [scaffold, module] {
        let (enabled, warnings) = infer_source(&source);
        assert!(enabled, "{warnings:?}");
        assert!(
            warnings
                .iter()
                .all(|warning| !warning.contains("source.rs")),
            "{warnings:?}"
        );
    }
}
