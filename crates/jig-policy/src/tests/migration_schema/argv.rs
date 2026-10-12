use super::*;

#[test]
fn argv_schema_snapshot_preserves_literal_runner_cwd_and_environment() {
    for executable_header in [true, false] {
        let temp = tempdir().unwrap();
        write_v6_schema_policy_repo(temp.path(), "false", "false");
        let path = temp.path().join(".agent/jig-contract.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        manifest["contract_version"] = json!(8);
        manifest["actions"][1]["runner"] = json!({
            "kind":"argv", "program":"./dump ; $(touch injected)",
            "args":["literal ' * $(touch injected)"], "working_directory":"api",
            "environment":{"SCHEMA_VALUE":"changed"}
        });
        manifest["tools"][1]
            .as_object_mut()
            .unwrap()
            .remove("command");
        fs::write(path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        let path = temp.path().join(".jig.toml");
        let mut source: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        source["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
        fs::write(path, toml::to_string(&source).unwrap()).unwrap();
        let script = temp.path().join("api/dump ; $(touch injected)");
        let contents = if executable_header {
            "#!/usr/bin/env python3\nimport os, pathlib, sys\nassert sys.argv[1:] == [\"literal ' * $(touch injected)\"]\nassert pathlib.Path.cwd().name == 'api'\nassert pathlib.Path(os.environ['JIG_REPO_ROOT']).resolve(strict=True) == pathlib.Path.cwd().parent\npathlib.Path('../docs/schema/tables.sql').write_text(os.environ['SCHEMA_VALUE']+'\\n')\n"
        } else {
            "printf changed > ../docs/schema/tables.sql\n"
        };
        fs::write(&script, contents).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
        fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
        init_git(temp.path());
        git(temp.path(), &["add", "."]);
        git(
            temp.path(),
            &["commit", "-m", "Example schema fixture", "-q"],
        );
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        if executable_header {
            let output = schema_check(&ctx).unwrap();
            assert_eq!(output.exit_status, 1, "{output:?}");
            assert!(
                output
                    .stderr
                    .contains("Re-run scripts/jig run api:schema-dump and commit"),
                "{}",
                output.stderr
            );
        } else {
            let error = schema_check(&ctx).unwrap_err();
            assert!(
                format!("{error:#}").contains("Exec format error"),
                "{error:#}"
            );
        }
        assert_eq!(
            fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
            "stable\n"
        );
        assert!(!temp.path().join("api/injected").exists());
    }
}
