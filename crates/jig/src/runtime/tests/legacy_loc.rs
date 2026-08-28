use super::*;

#[test]
fn supported_legacy_contract_executes_declared_loc_command_without_native_fallback() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join("Example.ts"), "one\ntwo\n").unwrap();
    let checker = r#"#!/usr/bin/env bash
set -eu
line_count="$(wc -l < Example.ts | tr -d ' ')"
if [ "$line_count" -gt 2 ]; then
  printf 'Example.ts is too large\n' >&2
  exit 1
fi
printf 'legacy command LOC passed\n'
"#;
    fs::write(temp.path().join("scripts/check-file-loc.sh"), checker).unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
rust_file_loc_command = "bash scripts/check-file-loc.sh"

[work]
checks = ["jig.rust_file_loc"]
"#,
        )
        .required_commands(["rust_file_loc_command"])
        .tool(json!({
            "name": "jig.rust_file_loc",
            "kind": "command",
            "description": "Run the repository-owned file LOC check.",
            "command": "rust_file_loc_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = super::super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: vec!["repo:rust-file-loc".into()],
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, false),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "legacy command LOC passed\n"
    );
    assert_eq!(output["run"]["targets"][0]["target"]["component"], "repo");
    assert_eq!(
        output["run"]["targets"][0]["target"]["action"],
        "rust-file-loc"
    );
}
