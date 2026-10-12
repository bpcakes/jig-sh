use super::*;

#[test]
fn application_contract_inference_requires_the_complete_interface_marker() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    let checker = temp.path().join("scripts/contracts.mjs");

    for (contents, expected) in [
        ("// unrelated project script\n", false),
        (
            "// jig-application-contract-checker: v1 modes=check\n",
            false,
        ),
        (
            "// jig-application-contract-checker: v1 modes=check,public-check\n",
            true,
        ),
    ] {
        fs::write(&checker, contents).unwrap();
        let mut warnings = Vec::new();
        let scan = RepoScan::collect(temp.path(), &mut warnings);

        assert_eq!(
            infer_application_contracts_enabled(temp.path(), &scan, true, &mut warnings),
            expected,
            "unexpected inference for {contents:?}"
        );
        assert_eq!(
            warnings
                .iter()
                .any(|warning| warning.contains("interface marker")),
            !expected
        );
    }
}

#[test]
fn missing_workspace_glob_is_a_warning_not_a_failure() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["missing/*"]}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| warning.contains("could not read directory")),
        "expected scan warning, got {:?}",
        inference.warnings
    );
}

#[test]
fn absent_optional_frontend_workspace_conventions_do_not_warn() {
    let temp = tempfile::tempdir().unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
    assert!(
        inference.warnings.iter().all(|warning| {
            !warning.contains("pnpm-workspace.yaml")
                && !warning.contains("could not read directory")
        }),
        "absent optional frontend conventions should stay quiet: {:?}",
        inference.warnings
    );
}

#[test]
fn empty_pnpm_workspace_is_reported_as_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("pnpm-workspace.yaml"), "packages:\n").unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("pnpm-workspace.yaml did not declare supported packages globs")
    }));
}

#[test]
fn pnpm_workspace_flow_style_globs_are_supported() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("fixtures/demo")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages: [\"apps/*\"]\n",
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();
    fs::write(temp.path().join("fixtures/demo/package.json"), app_package).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn frontend_roles_distinguish_astro_from_other_non_vite_apps() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/docs")).unwrap();
    fs::create_dir_all(temp.path().join("apps/storefront")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/docs/package.json"),
        r#"{
  "scripts": {
    "dev": "astro dev",
    "lint": "eslint .",
    "typecheck": "astro check",
    "build:bundle": "astro build",
    "test:coverage": "vitest run --coverage"
  },
  "devDependencies": { "astro": "^5.0.0" }
}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/storefront/package.json"),
        r#"{
  "scripts": {
    "dev": "next dev",
    "lint": "next lint",
    "typecheck": "tsc --noEmit",
    "build:bundle": "next build",
    "test:coverage": "vitest run --coverage"
  },
  "dependencies": { "next": "^15.0.0" }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 2);
    assert_eq!(inference.frontend_apps[0].dir, "apps/docs");
    assert_eq!(inference.frontend_apps[0].kind, "env-port");
    assert_eq!(inference.frontend_apps[0].role, "astro");
    assert_eq!(inference.frontend_apps[1].dir, "apps/storefront");
    assert_eq!(inference.frontend_apps[1].kind, "env-port");
    assert_eq!(inference.frontend_apps[1].role, "spa");
}

#[test]
fn legacy_admin_panel_vite_package_infers_the_shared_admin_role() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/admin-panel")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/admin-panel/package.json"),
        r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].name, "admin-panel");
    assert_eq!(inference.frontend_apps[0].kind, "vite");
    assert_eq!(inference.frontend_apps[0].role, "admin");
}

#[test]
fn frontend_role_inference_is_dev_script_authoritative() {
    let temp = tempfile::tempdir().unwrap();
    for app in ["vite-with-astro", "astro-script", "astro-fallback"] {
        fs::create_dir_all(temp.path().join("apps").join(app)).unwrap();
    }
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();

    let package = |dev: &str, dependencies: &str| {
        format!(
            r#"{{
  "scripts": {{
    "dev": "{dev}",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "build",
    "test:coverage": "vitest run --coverage"
  }},
  "dependencies": {dependencies}
}}"#
        )
    };
    fs::write(
        temp.path().join("apps/vite-with-astro/package.json"),
        package("vite", r#"{ "astro": "^5.0.0" }"#),
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/astro-script/package.json"),
        package("astro dev", "{}"),
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/astro-fallback/package.json"),
        package("custom-dev-server", r#"{ "astro": "^5.0.0" }"#),
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());
    let app = |dir: &str| {
        inference
            .frontend_apps
            .iter()
            .find(|app| app.dir == dir)
            .unwrap()
    };

    assert_eq!(app("apps/vite-with-astro").kind, "vite");
    assert_eq!(app("apps/vite-with-astro").role, "spa");
    assert_eq!(app("apps/astro-script").kind, "env-port");
    assert_eq!(app("apps/astro-script").role, "astro");
    assert_eq!(app("apps/astro-fallback").kind, "env-port");
    assert_eq!(app("apps/astro-fallback").role, "spa");
}

#[test]
fn frontend_profiles_include_preferred_dev_ports_from_scripts() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/admin")).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/admin/package.json"),
        r#"{
  "scripts": {
    "dev": "cross-env PORT=3001 vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite --host 127.0.0.1 --port=5174",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let report = infer_adopt_answers(temp.path()).report();
    let profiles = report["frontend_profiles"].as_array().unwrap();

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["dir"], "apps/admin");
    assert_eq!(profiles[0]["preferred_dev_port"], 3001);
    assert_eq!(profiles[1]["dir"], "apps/web");
    assert_eq!(profiles[1]["preferred_dev_port"], 5174);
    assert_eq!(
        report["metadata"]["frontend_profiles"]["confidence"],
        "medium"
    );
}

#[test]
fn invalid_numeric_frontend_dev_ports_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    fs::write(
        temp.path().join("web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite --port=999999",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(
        inference
            .warnings()
            .iter()
            .any(|warning| warning.contains("preferred_dev_port was not inferred")),
        "expected invalid frontend dev-port warning, got {:?}",
        inference.warnings()
    );
    assert_eq!(inference.frontend_profiles[0].preferred_dev_port, None);
    assert!(
        inference.report()["metadata"]["frontend_profiles"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("preferred_dev_port was not inferred"))
    );
}

#[test]
fn frontend_dev_port_scan_continues_after_invalid_literal() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    fs::write(
        temp.path().join("web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite --port 999999 --port 5174",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(
        inference.frontend_profiles[0].preferred_dev_port,
        Some(5174)
    );
    assert!(
        inference
            .warnings()
            .iter()
            .any(|warning| warning.contains("preferred_dev_port was not inferred"))
    );
}

#[test]
fn frontend_packages_missing_ci_scripts_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    fs::write(
        temp.path().join("web/package.json"),
        r#"{"scripts":{"dev":"vite","lint":"eslint ."}}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("missing required CI scripts")
            && warning.contains("typecheck")
            && warning.contains("build:bundle")
            && warning.contains("test:coverage")
    }));
}

#[test]
fn fallback_frontend_scan_ignores_non_conventional_package_dirs() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("examples/demo")).unwrap();
    fs::write(
        temp.path().join("examples/demo/package.json"),
        r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
}

#[test]
fn declared_workspaces_limit_frontend_app_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("fixtures/demo")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();
    fs::write(temp.path().join("fixtures/demo/package.json"), app_package).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn adoption_ignores_parent_components_in_positive_and_exclusion_globs() {
    let temp = tempfile::tempdir().unwrap();
    for directory in ["apps/web", "extra/web"] {
        let directory = temp.path().join(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("package.json"),
            r#"{"scripts":{
                "dev":"vite",
                "lint":"eslint .",
                "typecheck":"tsc --noEmit",
                "build:bundle":"vite build",
                "test:coverage":"vitest run --coverage"
            }}"#,
        )
        .unwrap();
    }
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/*","apps/../extra/*","!apps/../apps/web"]}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn workspace_exclusion_globs_remove_frontend_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("packages/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/private")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":["packages/*","!packages/private"]}"#,
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
    fs::write(
        temp.path().join("packages/private/package.json"),
        app_package,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "packages/web");
}

#[test]
fn pnpm_workspace_exclusion_globs_remove_frontend_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("packages/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/private")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - !packages/private\n",
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
    fs::write(
        temp.path().join("packages/private/package.json"),
        app_package,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "packages/web");
}

#[test]
fn quoted_pnpm_workspace_exclusion_globs_remove_frontend_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("packages/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/private")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages: [\"packages/*\", \"!packages/private\"]\n",
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
    fs::write(
        temp.path().join("packages/private/package.json"),
        app_package,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "packages/web");
}

#[test]
fn declared_workspaces_skip_root_frontend_app_candidate() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(
        temp.path().join("package.json"),
        r#"{
  "private": true,
  "workspaces": ["apps/*"],
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();
    fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn explicit_empty_workspaces_suppress_frontend_fallback_scan() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":[]}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
}

#[test]
fn frontend_discovery_requires_nonoverlapping_glob_edges() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["apps/ab*bc"]}"#,
    )
    .unwrap();
    for name in ["abc", "abbc"] {
        let dir = temp.path().join("apps").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "scripts": {"dev": "vite", "lint": "eslint .", "typecheck": "tsc",
                    "build:bundle": "vite build", "test:coverage": "vitest run --coverage"}
            })
            .to_string(),
        )
        .unwrap();
    }
    let inferred = infer_frontend_apps_with_metadata(temp.path(), None, &mut Vec::new());
    assert_eq!(
        inferred
            .apps
            .iter()
            .map(|app| app.dir.as_str())
            .collect::<Vec<_>>(),
        ["apps/abbc"]
    );
}
