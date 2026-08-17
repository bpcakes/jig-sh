
fn write_doctor_fixture(root: &Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    TestRepoBuilder::new(root)
        .jig_version(env!("CARGO_PKG_VERSION"))
        .config(
            r#"
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .required_commands(["bootstrap_command"])
        .tool(json!({
            "name": tool::CONTRACT_CHECK,
            "kind": "native",
            "description": "Contract check."
        }))
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": "command",
            "description": "Bootstrap.",
            "command": "bootstrap_command"
        }))
        .write();
    fs::write(root.join(".mcp.json"), "{}").unwrap();
    fs::write(root.join("scripts/install-jig.sh"), "#!/usr/bin/env bash\n").unwrap();
    fs::write(
        root.join("scripts/jig"),
        format!(
            "#!/usr/bin/env bash\nJIG_VERSION=\"{}\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("scripts/jig"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}
