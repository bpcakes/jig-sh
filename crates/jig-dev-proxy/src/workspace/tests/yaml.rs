use super::*;

#[test]
fn yaml_preserves_quoted_content_and_resolves_escapes_and_aliases() {
    let globs = parse_pnpm_workspace(
        r#"
---
patterns: &patterns
  - 'apps/comma,name'
  - 'apps/it''s-web'
  - "apps/\u0077eb#client" # comment
  - "!apps/excluded"
"packages": *patterns
"#,
    )
    .unwrap();
    assert_eq!(
        globs,
        [
            "apps/comma,name",
            "apps/it's-web",
            "apps/web#client",
            "!apps/excluded"
        ]
    );
    assert_eq!(
        parse_pnpm_workspace("packages: ['apps/comma,name', 'apps/#web']").unwrap(),
        ["apps/comma,name", "apps/#web"]
    );
}

#[test]
fn yaml_empty_workspace_forms_remain_empty() {
    for source in [
        "",
        "# comment",
        "{}",
        "packages:",
        "packages: null",
        "packages: []",
    ] {
        assert!(parse_pnpm_workspace(source).unwrap().is_empty(), "{source}");
    }
}

#[test]
fn yaml_rejects_malformed_documents_and_non_string_patterns() {
    for source in [
        "packages: [unterminated",
        "packages: []\npackages: ['apps/*']",
        "packages: []\n---\npackages: ['apps/*']",
        "packages: [*undefined]",
        "packages: ['apps/*', 42]",
        "packages: [true]",
        "packages: [null]",
        "packages: [{app: 'apps/*'}]",
        "packages: [['apps/*']]",
        "['apps/*']",
    ] {
        let error = parse_pnpm_workspace(source).unwrap_err();
        assert!(
            error.to_string().contains("pnpm-workspace.yaml"),
            "{source}: {error}"
        );
    }
}

#[test]
fn discovery_uses_decoded_yaml_paths_and_exclusions() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        r#""packages": [
  'apps/comma,name',
  "apps/\u0077eb#client", # escaped directory name
  '!apps/comma,name',
]
"#,
    )
    .unwrap();
    for directory in ["comma,name", "web#client"] {
        let path = temp.path().join("apps").join(directory);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("package.json"),
            r#"{"name":"example-web","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
    }

    let apps = discover(temp.path(), "example", "localhost", "pnpm").unwrap();

    assert_eq!(apps.len(), 1);
    assert_eq!(
        apps[0].dir,
        temp.path().join("apps/web#client").canonicalize().unwrap()
    );
}

#[test]
fn decoded_yaml_paths_still_require_repository_containment() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        r#"packages: ["\u002e\u002e/outside/*"]"#,
    )
    .unwrap();

    let error = discover(temp.path(), "example", "localhost", "pnpm").unwrap_err();

    assert!(error.to_string().contains("must stay within the repo root"));
}
