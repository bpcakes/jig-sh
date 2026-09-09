use super::*;

#[test]
fn required_tools_checks_argv_programs_with_declared_cwd_and_path() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    fs::create_dir_all(temp.path().join("api/bin")).unwrap();
    let script = temp.path().join("api/bin/example-tool");
    fs::write(&script, "#!/bin/sh\ntouch must-not-run\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["required_commands"] = json!([]);
    manifest["actions"][0]["runner"] = json!({
        "kind": "argv", "program": "example-tool", "args": [],
        "working_directory": "api", "environment": {"PATH": "bin"}
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
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let environment = DoctorEnvironment {
        search_path: Some(temp.path().join("missing").into_os_string()),
        ..DoctorEnvironment::default()
    };
    for present in [true, false] {
        if !present {
            fs::remove_file(&script).unwrap();
        }
        let check = required_tools_check_with_environment(&ctx, &environment);
        assert_eq!(check.ok, present, "{check:?}");
        let tools = &check.data["tools"];
        let action = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"] == "repo:bootstrap")
            .expect("argv action must be included in required tools");
        assert_eq!(action["programs"][0]["program"], "example-tool");
        assert_eq!(action["present"], present);
        if !present {
            assert!(check.detail.contains("repo:bootstrap"), "{check:?}");
            assert!(check.detail.contains("example-tool"), "{check:?}");
        }
        assert!(!temp.path().join("api/must-not-run").exists());
    }
}
