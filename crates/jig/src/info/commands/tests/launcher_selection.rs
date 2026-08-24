use super::*;

#[test]
fn minimal_footprint_uses_the_installed_jig_even_if_a_launcher_is_present() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
harness_footprint = "minimal"
sqlx_enabled = false

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .write();
    std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
    std::fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let output = ready_local_inventory(&ctx);

    assert!(
        command_by_name(&output, "sqlx")["next_step"]
            .as_str()
            .unwrap()
            .contains("`jig adopt ")
    );
    assert!(
        command_by_name(&output, "sqlx")["next_step"]
            .as_str()
            .unwrap()
            .contains("--minimal")
    );
    assert!(
        command_by_name(&output, "sqlx")["next_step"]
            .as_str()
            .unwrap()
            .contains("--template /tmp/template")
    );
    assert!(
        command_by_name(&output, "sqlx")["next_step"]
            .as_str()
            .unwrap()
            .contains(&temp.path().display().to_string())
    );
    assert!(
        !command_by_name(&output, "sqlx")["next_step"]
            .as_str()
            .unwrap()
            .contains("scripts/jig")
    );
}

pub(super) fn sqlx_command_inventory_config(schema_dump_enabled: bool) -> String {
    format!(
        r#"
sqlx_enabled = true
rust_migration_dir = "migrations"
schema_dump_enabled = {schema_dump_enabled}
bootstrap_command = "printf bootstrap"
migration_add_command = "printf migration"
schema_dump_command = "printf schema"

[agent_tooling.codex]
marketplaces = []
"#
    )
}

pub(super) fn ready_local_inventory(ctx: &RepoContext) -> Value {
    info_with_capabilities(
        ctx,
        VaultCapability {
            available: true,
            initialized: true,
            home: None,
            scope: None,
            scope_id: None,
            error: None,
        },
        &json!({ "ok": true }),
    )
}

pub(super) fn assert_command_status(output: &Value, name: &str, status: &str, reason_code: &str) {
    let command = command_by_name(output, name);
    assert_eq!(command["status"], status, "{name}");
    assert_eq!(command["reason_code"], reason_code, "{name}");
    assert!(
        command["reason"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        command["next_step"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}

pub(super) fn command_by_name<'a>(output: &'a Value, name: &str) -> &'a Value {
    output["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("missing command {name}"))
}

#[cfg(feature = "dev-proxy")]
pub(super) fn write_full_launcher(root: &std::path::Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    for path in [
        root.join("scripts/jig"),
        root.join("scripts/install-jig.sh"),
    ] {
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
        }
    }
}
