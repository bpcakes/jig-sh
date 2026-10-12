//! Only document tests that the generated `openapi` check would run count.

use super::*;

const WORKSPACE: &str =
    "[workspace]\nmembers = [\"apps/*\", \"crates/*\"]\nexclude = [\"crates/legacy\"]\n";
const PACKAGE: &str = "[package]\nname = \"example\"\nversion = \"0.1.0\"\n";
const PUBLIC_TEST: &str =
    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn committed_public_openapi_is_current() {}\n}\n";
const ADMIN_TEST: &str = "#[test]\nfn committed_admin_openapi_is_current() {}\n";
const GO_TEST: &str =
    "package httpapi\n\nimport \"testing\"\n\nfunc TestOpenAPIIsCurrent(t *testing.T) {}\n";

fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    temp
}

/// A workspace whose `crates/example-http` member has `source`.
fn member(source: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Cargo.toml", WORKSPACE.into()),
        ("crates/example-http/Cargo.toml", PACKAGE.into()),
        ("crates/example-http/src/lib.rs", source.into()),
    ]
}

#[test]
fn rust_document_tests_count_only_where_cargo_test_runs_them() {
    let test =
        |prefix: &str| format!("{prefix}#[test]\nfn committed_public_openapi_is_current() {{}}\n");
    let mut cases = vec![
        ("member", member(PUBLIC_TEST), true),
        (
            "async test",
            member("#[tokio::test]\nasync fn committed_public_openapi_is_current() {}\n"),
            true,
        ),
        (
            "lifetimes, quotes, and comment markers in strings",
            member(&test(
                "fn keep<'a>(value: &'a str) -> &'a str { value }\nconst Q: char = '\\'';\nconst DQ: char = '\"';\nconst EQ: char = '\\\"';\nconst S: &str = \"/* // \\\" \";\nconst R: &str = r#\"\"/*\"#;\n",
            )),
            true,
        ),
        (
            "line comment",
            member("// #[test]\n// fn committed_public_openapi_is_current() {}\n"),
            false,
        ),
        (
            "nested block comment",
            member("/* /* */ #[test]\nfn committed_public_openapi_is_current() {} */\n"),
            false,
        ),
        (
            "string",
            member("const S: &str = \"#[test] fn committed_public_openapi_is_current() {}\";\n"),
            false,
        ),
        (
            "raw string",
            member("const S: &str = r#\"#[test] fn committed_public_openapi_is_current() {}\"#;\n"),
            false,
        ),
        (
            "ignored",
            member(&test(""))
                .into_iter()
                .map(|(path, source)| {
                    (
                        path,
                        source.replace("#[test]\n", "#[test]\n#[ignore = \"flaky\"]\n"),
                    )
                })
                .collect(),
            false,
        ),
        (
            "not a test",
            member("fn committed_public_openapi_is_current() {}\n"),
            false,
        ),
        (
            "attribute that is not a test",
            member("#[allow(dead_code)]\nfn committed_public_openapi_is_current() {}\n"),
            false,
        ),
        (
            "renamed",
            member("#[test]\nfn public_openapi_document_matches() {}\n"),
            false,
        ),
    ];
    cases.extend([
        (
            "integration test of a member",
            vec![
                ("Cargo.toml", WORKSPACE.into()),
                ("apps/example-api/Cargo.toml", PACKAGE.into()),
                ("apps/example-api/tests/openapi.rs", test("")),
            ],
            true,
        ),
        (
            "root package without a workspace",
            vec![
                ("Cargo.toml", PACKAGE.into()),
                ("src/lib.rs", PUBLIC_TEST.into()),
            ],
            true,
        ),
        (
            "package outside the workspace",
            vec![
                ("Cargo.toml", WORKSPACE.into()),
                ("fixtures/copy/Cargo.toml", PACKAGE.into()),
                ("fixtures/copy/src/lib.rs", PUBLIC_TEST.into()),
            ],
            false,
        ),
        (
            "excluded member",
            vec![
                ("Cargo.toml", WORKSPACE.into()),
                ("crates/legacy/Cargo.toml", PACKAGE.into()),
                ("crates/legacy/src/lib.rs", PUBLIC_TEST.into()),
            ],
            false,
        ),
        (
            "package nested in a member",
            vec![
                ("Cargo.toml", WORKSPACE.into()),
                ("crates/example-http/Cargo.toml", PACKAGE.into()),
                (
                    "crates/example-http/fixtures/demo/Cargo.toml",
                    PACKAGE.into(),
                ),
                (
                    "crates/example-http/fixtures/demo/src/lib.rs",
                    PUBLIC_TEST.into(),
                ),
            ],
            false,
        ),
        (
            "no manifest",
            vec![("src/lib.rs", PUBLIC_TEST.into())],
            false,
        ),
    ]);
    let mut admin = member(PUBLIC_TEST);
    admin.push(("openapi/admin.json", "{}\n".into()));
    cases.push(("admin document without its test", admin.clone(), false));
    admin.extend([
        ("crates/example-admin-http/Cargo.toml", PACKAGE.into()),
        ("crates/example-admin-http/src/tests.rs", ADMIN_TEST.into()),
    ]);
    cases.push(("admin document", admin, true));

    for (label, files, found) in cases {
        let files = files
            .iter()
            .map(|(path, source)| (*path, source.as_str()))
            .collect::<Vec<_>>();
        let repository = repository(&files);
        assert_eq!(
            has_scaffold_document_tests(repository.path(), false),
            found,
            "{label}"
        );
    }
}

#[test]
fn go_document_tests_count_only_in_test_files_that_every_build_includes() {
    let commented = GO_TEST.replace("func TestOpenAPIIsCurrent", "// func TestOpenAPIIsCurrent");
    let block = GO_TEST.replace(
        "func TestOpenAPIIsCurrent(t *testing.T) {}",
        "/* func TestOpenAPIIsCurrent(t *testing.T) {} */",
    );
    let raw = "package httpapi\n\nvar source = `func TestOpenAPIIsCurrent(t *testing.T) {}`\n";
    let tagged = format!("//go:build integration\n\n{GO_TEST}");
    let legacy_tagged = format!("// +build integration\n\n{GO_TEST}");
    for (label, path, source, found) in [
        (
            "test file",
            "internal/httpapi/httpapi_test.go",
            GO_TEST,
            true,
        ),
        (
            "platform word without a prefix",
            "internal/httpapi/linux_test.go",
            GO_TEST,
            true,
        ),
        (
            "line comment",
            "internal/httpapi/httpapi_test.go",
            commented.as_str(),
            false,
        ),
        (
            "block comment",
            "internal/httpapi/httpapi_test.go",
            block.as_str(),
            false,
        ),
        ("raw string", "internal/httpapi/httpapi_test.go", raw, false),
        (
            "build constraint",
            "internal/httpapi/httpapi_test.go",
            tagged.as_str(),
            false,
        ),
        (
            "legacy build constraint",
            "internal/httpapi/httpapi_test.go",
            legacy_tagged.as_str(),
            false,
        ),
        (
            "platform file",
            "internal/httpapi/openapi_linux_test.go",
            GO_TEST,
            false,
        ),
        (
            "architecture file",
            "internal/httpapi/openapi_linux_arm64_test.go",
            GO_TEST,
            false,
        ),
        (
            "file Go ignores",
            "internal/httpapi/_openapi_test.go",
            GO_TEST,
            false,
        ),
        (
            "not a test file",
            "internal/httpapi/httpapi.go",
            GO_TEST,
            false,
        ),
        ("other package", "internal/api/api_test.go", GO_TEST, false),
    ] {
        let repository = repository(&[(path, source)]);
        assert_eq!(
            has_scaffold_document_tests(repository.path(), true),
            found,
            "{label}"
        );
    }
}
