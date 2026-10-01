use super::*;

pub(super) fn adopt_repo_for_test(repo: &Path, template: &Path, template_mode: TemplateMode) {
    run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: Some(template_mode),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            backend_language: Some(BackendLanguage::Rust),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
}

/// Recreates a contract-8 repository from a current render: the manifest
/// declares contract 8 and `.jig.toml` carries the `[work]` gate it rendered.
pub(super) fn downgrade_to_contract_eight(repo: &Path) {
    let manifest_path = repo.join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = serde_json::json!(8);
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let path = repo.join(".jig.toml");
    let mut config = toml::from_str::<toml::Value>(&fs::read_to_string(&path).unwrap()).unwrap();
    let verify = toml::Table::from_iter([
        ("id".into(), toml::Value::from("verify")),
        ("kind".into(), toml::Value::from("evidence")),
        (
            "profile".into(),
            config["repository"]["default_check_profile"].clone(),
        ),
        ("conclusion".into(), toml::Value::from("success")),
    ]);
    config.as_table_mut().unwrap().insert(
        "work".into(),
        toml::Value::Table(toml::Table::from_iter([(
            "gates".into(),
            toml::Value::Array(vec![toml::Value::Table(verify)]),
        )])),
    );
    fs::write(&path, toml::to_string_pretty(&config).unwrap()).unwrap();
    let ctx = crate::context::RepoContext::load_from_root(repo.to_path_buf()).unwrap();
    assert_eq!(ctx.contract_version(), 8);
}
