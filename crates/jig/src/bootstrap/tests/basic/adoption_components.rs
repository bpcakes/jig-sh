use super::*;

fn options(repo: &Path, template: &Path) -> AdoptOpts {
    AdoptOpts {
        components: Default::default(),
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    }
}

fn manifest(repo: &Path, directory: &str, name: &str) {
    let path = repo.join(directory);
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(
        path.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
    fs::write(path.join("src/lib.rs"), "").unwrap();
}

fn saved_components(repo: &Path) -> toml::Value {
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(repo.join(".jig.toml")).unwrap()).unwrap();
    config["repository"]["components"].clone()
}

#[test]
fn adopt_components_write_selected_roots_and_preserve_them_on_recopy_and_readoption() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let template = materialize_template_worktree();
    manifest(&repo, "crates/core", "core");
    manifest(&repo, "crates/optional", "optional");
    manifest(&repo, "fixtures/example", "example");
    manifest(&repo, "tools/incidental", "incidental");
    fs::write(
        repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .unwrap();
    let mut opts = options(&repo, template.path());
    opts.components.exclude = vec!["./crates/optional/".into()];
    opts.components.include = vec!["fixtures/example".into()];
    let preview = run_adopt(opts.clone()).unwrap();
    assert!(!repo.join(".jig.toml").exists());
    let candidates = preview["detection_report"]["component_candidates"]
        .as_array()
        .unwrap();
    for (root, expected) in [
        (".", "included"),
        ("crates/core", "included"),
        ("crates/optional", "excluded"),
        ("fixtures/example", "included"),
        ("tools/incidental", "review_required"),
    ] {
        assert!(
            candidates
                .iter()
                .any(|c| c["root"] == root && c["disposition"] == expected),
            "{root}: {candidates:?}"
        );
    }
    opts.write = true;
    let written = run_adopt(opts.clone()).unwrap();
    assert_eq!(
        preview["detection_report"]["component_candidates"],
        written["detection_report"]["component_candidates"]
    );
    let before = saved_components(&repo);
    let roots = before
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c["root"].as_str().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        roots,
        vec![
            ("crates-core", "crates/core"),
            ("fixtures-example", "fixtures/example"),
            ("repo", "."),
            ("workspace", ".")
        ]
    );
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let resolved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap())
            .unwrap();
    let resolved_components = &resolved["components"];
    assert!(resolved_components.as_array().is_some(), "{resolved}");
    assert_eq!(resolved_components.as_array().unwrap().len(), 4);
    manifest(&repo, "crates/new", "new");
    run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
    assert_eq!(saved_components(&repo), before);
    opts.components = Default::default();
    run_adopt(opts.clone()).unwrap();
    assert_eq!(saved_components(&repo), before);
    opts.components.include = vec!["crates/new".into()];
    let error = run_adopt(opts).unwrap_err();
    assert!(
        error.to_string().contains("existing authored model"),
        "{error:#}"
    );
    assert_eq!(saved_components(&repo), before);
}

#[test]
fn adopt_components_incidental_manifests_do_not_create_a_default_backend() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let template = materialize_template_worktree();
    manifest(&repo, "fixtures/incidental", "incidental");
    let mut opts = options(&repo, template.path());
    opts.write = true;
    run_adopt(opts).unwrap();
    let components = saved_components(&repo);
    let components = components.as_array().unwrap();
    assert_eq!(components.len(), 1);
    assert_eq!(components[0]["id"].as_str(), Some("repo"));
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let answers = RenderAnswers::from_answers_file(&repo.join(".jig.toml")).unwrap();
    assert!(!answers.rust_backend_enabled());
}

#[test]
fn adopt_components_reject_explicit_answer_exclusion_before_writing() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    fs::create_dir_all(&repo).unwrap();
    let template = materialize_template_worktree();
    let mut opts = options(&repo, template.path());
    opts.write = true;
    opts.answers.rust_test_command = Some("cargo test".into());
    opts.components.exclude = vec![".".into()];
    let error = run_adopt(opts).unwrap_err();
    assert!(
        error.to_string().contains("conflicts with explicit answer"),
        "{error:#}"
    );
    assert!(!repo.join(".jig.toml").exists());
    assert!(!repo.join(".agent").exists());
}

#[test]
fn adopt_components_frontend_exclusion_removes_actions_and_phantom_backend_edges() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    fs::create_dir_all(&repo).unwrap();
    let template = materialize_template_worktree();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*","packages/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "{}\n").unwrap();
    for name in ["web", "admin"] {
        let dir = repo.join("apps").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), format!(r#"{{"name":"{name}","scripts":{{"dev":"vite","lint":"eslint .","typecheck":"tsc","build:bundle":"vite build","test:coverage":"vitest run --coverage"}}}}"#)).unwrap();
    }
    fs::create_dir_all(repo.join("packages/shared")).unwrap();
    fs::write(
        repo.join("packages/shared/package.json"),
        r#"{"name":"shared"}"#,
    )
    .unwrap();
    let mut opts = options(&repo, template.path());
    opts.components.exclude = vec!["apps/admin".into()];
    opts.write = true;
    let output = run_adopt(opts.clone()).unwrap();
    let candidates = output["detection_report"]["component_candidates"]
        .as_array()
        .unwrap();
    let web = candidates.iter().find(|c| c["root"] == "apps/web").unwrap();
    let admin = candidates
        .iter()
        .find(|c| c["root"] == "apps/admin")
        .unwrap();
    assert_eq!(web["disposition"], "included");
    assert_eq!(admin["disposition"], "excluded");
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    assert_eq!(ctx.frontend_apps().len(), 1);
    assert_eq!(ctx.frontend_apps()[0].dir, "apps/web");
    assert!(
        !ctx.component_specs()
            .iter()
            .any(|c| c.id.as_str() == "api" || c.root == "apps/admin")
    );
    assert!(
        ctx.component_specs()
            .iter()
            .any(|c| c.root == "packages/shared")
    );
    assert!(
        ctx.component_specs()
            .iter()
            .all(|c| c.depends_on.is_empty())
    );
    assert!(
        ctx.action_specs()
            .iter()
            .any(|a| a.target.component.as_str() == web["proposed_id"].as_str().unwrap())
    );
    assert!(
        !ctx.action_specs()
            .iter()
            .any(|a| a.target.component.as_str() == admin["proposed_id"].as_str().unwrap())
    );
    assert_frontend_readoption_and_minimal_transition(
        &repo,
        template.path(),
        opts,
        web["proposed_id"].as_str().unwrap(),
    );
}

fn assert_frontend_readoption_and_minimal_transition(
    repo: &Path,
    template: &Path,
    mut opts: AdoptOpts,
    web_id: &str,
) {
    let before = saved_components(repo);
    opts.components = Default::default();
    fs::create_dir_all(repo.join("shared-libs/new")).unwrap();
    fs::write(
        repo.join("shared-libs/new/package.json"),
        r#"{"name":"example-shared"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*","packages/*","shared-libs/*"]}"#,
    )
    .unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let authored_actions = config["repository"]["actions"].clone();
    let authored_commands = config["commands"].clone();
    config["frontend_apps"].as_array_mut().unwrap()[0]["coverage_threshold"] =
        toml::Value::Integer(99);
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    opts.force = true;
    fs::write(repo.join("Makefile"), "test:\n\tprintf inferred-test\n").unwrap();
    let refreshed_report = run_adopt(opts.clone()).unwrap();
    assert!(
        refreshed_report["detection_report"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("frontend_workspace_roots"))
    );
    let refreshed: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(refreshed["repository"]["actions"], authored_actions);
    assert_eq!(refreshed["commands"], authored_commands);
    assert_eq!(
        refreshed["frontend_workspace_roots"],
        config["frontend_workspace_roots"]
    );
    run_update(UpdateOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
    let recopied: toml::Value = toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(recopied["repository"]["actions"], authored_actions);
    assert_eq!(recopied["commands"], authored_commands);
    assert_eq!(saved_components(repo), before);
    opts.minimal = true;
    opts.force = true;
    run_adopt(opts).unwrap();
    assert_eq!(saved_components(repo), before);
    assert!(!repo.join("scripts/check-webapps.sh").exists());
    let minimal: toml::Value =
        toml::from_str(&fs::read_to_string(repo.join(".jig.toml")).unwrap()).unwrap();
    assert!(
        minimal["commands"]
            .as_table()
            .unwrap()
            .values()
            .all(|value| !value.as_str().unwrap().contains("scripts/check-webapps.sh"))
    );
    let ctx = crate::context::RepoContext::load_from(repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    assert!(
        !ctx.action_specs()
            .iter()
            .any(|action| action.target.component.as_str() == web_id)
    );
}

#[test]
fn adopt_components_readoption_preserves_opaque_authored_components() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let template = materialize_template_worktree();
    manifest(&repo, ".", "example-service");
    fs::create_dir_all(repo.join("tools")).unwrap();
    let mut opts = options(&repo, template.path());
    opts.write = true;
    run_adopt(opts.clone()).unwrap();
    let path = repo.join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let custom: toml::Value = toml::from_str(
        "id = 'custom-tools'\nroot = 'tools'\nadapters = []\ntags = ['project-owned']\n",
    )
    .unwrap();
    config["repository"]["components"]
        .as_array_mut()
        .unwrap()
        .push(custom.clone());
    fs::write(&path, toml::to_string_pretty(&config).unwrap()).unwrap();
    opts.minimal = true;
    opts.force = true;
    run_adopt(opts).unwrap();
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let saved = ctx
        .component_specs()
        .iter()
        .find(|c| c.id.as_str() == "custom-tools")
        .unwrap();
    let expected: jig_contract::ComponentSpec = custom.try_into().unwrap();
    assert_eq!(saved, &expected);
}

#[test]
fn adopt_components_readoption_applies_command_overrides_to_authored_alias_owner() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let template = materialize_template_worktree();
    manifest(&repo, ".", "example-service");
    let mut opts = options(&repo, template.path());
    opts.write = true;
    run_adopt(opts.clone()).unwrap();
    let path = repo.join(".jig.toml");
    opts.force = true;
    opts.answers.rust_test_command = Some("printf first-test".into());
    run_adopt(opts.clone()).unwrap();
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        config["commands"]["api_test_command"].as_str(),
        Some("printf first-test")
    );
    let action = config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| {
            a["legacy_aliases"].as_array().is_some_and(|aliases| {
                aliases
                    .iter()
                    .any(|alias| alias.as_str() == Some("jig.test"))
            })
        })
        .unwrap();
    action["runner"]["command"] = toml::Value::String("project_test_command".into());
    action["target"]["action"] = toml::Value::String("project-test".into());
    for profile in config["repository"]["profiles"].as_array_mut().unwrap() {
        for target in profile["targets"].as_array_mut().unwrap() {
            if target["component"].as_str() == Some("api")
                && target["action"].as_str() == Some("test")
            {
                target["action"] = toml::Value::String("project-test".into());
            }
        }
    }
    config["commands"]
        .as_table_mut()
        .unwrap()
        .remove("api_test_command");
    config["commands"].as_table_mut().unwrap().insert(
        "project_test_command".into(),
        toml::Value::String("printf old-test".into()),
    );
    fs::write(&path, toml::to_string_pretty(&config).unwrap()).unwrap();
    opts.answers.rust_test_command = Some("printf second-test".into());
    run_adopt(opts.clone()).unwrap();
    let updated: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        updated["commands"]["project_test_command"].as_str(),
        Some("printf second-test")
    );
    assert!(updated["commands"].get("api_test_command").is_none());
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    fs::write(repo.join("Makefile"), "test:\n\tprintf inferred-test\n").unwrap();
    opts.answers.rust_test_command = None;
    run_adopt(opts.clone()).unwrap();
    let preserved: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        preserved["commands"]["project_test_command"].as_str(),
        Some("printf second-test")
    );
    opts.minimal = true;
    run_adopt(opts.clone()).unwrap();
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    assert!(
        ctx.action_specs()
            .iter()
            .any(|action| action.target.to_string() == "api:project-test")
    );
    assert!(
        !ctx.action_specs()
            .iter()
            .any(|action| action.target.to_string() == "api:test")
    );
    let minimal: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        minimal["commands"]["project_test_command"].as_str(),
        Some("printf second-test")
    );
    assert!(minimal["commands"].get("api_test_command").is_none());
    assert!(
        minimal["repository"]["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|target| target["component"].as_str() == Some("api")
                    && target["action"].as_str() == Some("project-test")))
    );
    let before = fs::read(&path).unwrap();
    opts.answers.contract_check_command = Some("printf unsupported".into());
    let error = run_adopt(opts).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot override the native action"),
        "{error:#}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn adopt_components_explicit_future_rust_root_remains_supported() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("ExampleProject");
    let template = materialize_template_worktree();
    manifest(&repo, ".", "example-service");
    let mut opts = options(&repo, template.path());
    opts.answers.rust_crate_roots = vec!["future-crate".into()];
    opts.write = true;
    run_adopt(opts).unwrap();
    let components = saved_components(&repo);
    assert!(
        components
            .as_array()
            .unwrap()
            .iter()
            .any(|component| component["root"].as_str() == Some("future-crate"))
    );
    assert!(!repo.join("future-crate").exists());
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
}
