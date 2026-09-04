use std::fs;

use serde_json::json;

use super::*;

#[test]
fn repository_affected_check_explains_and_executes_only_matching_v6_targets() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[repository]\ndefault_check_profile = \"verify\"",
        "[repository]\ndefault_check_profile = \"verify\"\naffected_ignore = [\".env\", \".env.*\", \"**/.env\", \"**/.env.*\"]",
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["affected_ignore"] = json!([".env", ".env.*", "**/.env", "**/.env.*"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join(".gitignore"), ".env\n.env.*\n").unwrap();
    init_git_repo(temp.path());
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://example.invalid/app\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let request = |explain| {
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: Some("HEAD".into()),
                comparison: None,
                explain,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, true),
            },
        ))
    };

    let dotenv_only = super::super::dispatch(&ctx, request(true)).unwrap();
    assert!(
        dotenv_only["plan"]["targets"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    fs::write(
        temp.path().join("api/example.go"),
        "package example\n\nconst changed = true\n",
    )
    .unwrap();
    let explained = super::super::dispatch(&ctx, request(true)).unwrap();
    assert_eq!(explained["executed"], false);
    assert_eq!(explained["plan"]["affected_base"], "HEAD");
    assert_eq!(explained["plan"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(
        explained["plan"]["targets"][0]["target"],
        json!({"component": "api", "action": "test"})
    );
    assert!(
        explained["plan"]["targets"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason["kind"] == "direct_input" && reason["path"] == "api/example.go"
            })
    );

    let executed = super::super::dispatch(&ctx, request(false)).unwrap();
    assert_eq!(executed["ok"], true);
    assert_eq!(executed["results"].as_array().unwrap().len(), 1);
    assert_eq!(executed["results"][0]["target"]["component"], "api");
    assert_eq!(executed["run"]["targets"].as_array().unwrap().len(), 1);
}
