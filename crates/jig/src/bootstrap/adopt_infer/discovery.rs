use super::*;

pub(in crate::bootstrap) fn infer_adopt_answers(root: &Path) -> AdoptInference {
    let mut warnings = Vec::new();
    let scan = RepoScan::collect(root, &mut warnings);
    let repo_name = infer_repo_name_with_metadata(root);
    let default_branch = infer_default_branch_with_metadata(root, &mut warnings);
    let mut rust_crate_roots = infer_rust_crate_roots_with_metadata(root, &mut warnings);
    if rust_crate_roots.roots.is_empty() && !root.join("Cargo.toml").is_file() {
        rust_crate_roots = infer_rust_crate_roots_from_scan(root, &scan, &mut warnings);
    }
    let repo_topology = infer_repo_topology(root, &scan, &rust_crate_roots.roots, &mut warnings);
    let mut package_manager_warnings = Vec::new();
    let package_manager =
        infer_package_manager_with_metadata(root, &scan, &mut package_manager_warnings);
    let scanned_rust_packages =
        rust_crate_roots.source_kind == RustCrateRootSourceKind::ScannedPackages;
    let nested_manifest_paths =
        scanned_rust_packages.then_some(rust_crate_roots.scanned_manifest_paths.as_slice());
    let commands = infer_commands(root, &scan, nested_manifest_paths, &mut warnings);
    if !rust_crate_roots.roots.is_empty() && commands.rust_clippy_command.is_none() {
        warnings.push(crate::bootstrap::clippy_policy::ALL_FEATURES_ADOPTION_WARNING.to_owned());
    }
    if scanned_rust_packages && commands.rust_test_locked_command.is_none() {
        warnings.push(
            "nested Rust manifest scan did not infer rust_test_locked_command; add a project-owned locked command once lockfiles are committed"
                .into(),
        );
    }
    let frontend_apps =
        infer_frontend_apps_with_metadata(root, repo_name.value.as_deref(), &mut warnings);
    if !frontend_apps.apps.is_empty() {
        warnings.extend(package_manager_warnings);
    }
    let components = ComponentCandidates::discover(
        root,
        &scan,
        &frontend_apps.apps,
        &frontend_apps.workspace_roots,
        &mut warnings,
    );
    let github_ci = infer_ci_github_runner_with_metadata(root, &scan, &mut warnings);
    let mut inference = AdoptInference {
        repo_name: repo_name.value.clone(),
        default_branch: default_branch.value.clone(),
        rust_crate_roots: rust_crate_roots.roots.clone(),
        rust_crate_root_source_kind: rust_crate_roots.source_kind,
        rust_fmt_check_command: commands
            .rust_fmt_check_command
            .as_ref()
            .map(CommandCandidate::command),
        rust_clippy_command: commands
            .rust_clippy_command
            .as_ref()
            .map(CommandCandidate::command),
        rust_test_command: commands
            .rust_test_command
            .as_ref()
            .map(CommandCandidate::command),
        rust_test_locked_command: commands
            .rust_test_locked_command
            .as_ref()
            .map(CommandCandidate::command),
        command_profile: commands.clone(),
        web_package_manager: package_manager.value.clone(),
        application_contracts_enabled: Some(infer_application_contracts_enabled(
            root,
            &scan,
            !frontend_apps.apps.is_empty(),
            &mut warnings,
        )),
        frontend_apps: frontend_apps.apps.clone(),
        frontend_workspace_roots: frontend_apps.workspace_roots.clone(),
        frontend_profiles: frontend_apps.profiles.clone(),
        ci_github_runner: github_ci.runner.clone(),
        ci_shape: github_ci.shape.clone(),
        repo_topology,
        components,
        warnings,
        ..AdoptInference::default()
    };
    record_repository_metadata(
        &mut inference,
        &repo_name,
        &default_branch,
        &rust_crate_roots,
        &commands,
    );
    record_frontend_and_ci_metadata(&mut inference, &package_manager, &frontend_apps, &github_ci);

    let sqlx = infer_sqlx(root, &scan, &mut inference.warnings);
    apply_sqlx_inference(&mut inference, &sqlx);
    record_inference_signals(&mut inference, &github_ci);

    inference.scan = Some(scan);
    inference
}
