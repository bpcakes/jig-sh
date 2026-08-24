use super::*;

#[cfg(unix)]
#[test]
fn rust_react_package_stem_limit_is_applied_before_destination_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();

    let accepted_name = "r".repeat(216);
    let accepted_destination = temp.path().join("accepted");
    fs::create_dir(&accepted_destination).unwrap();
    let accepted_plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some(accepted_name.clone()),
            ..AnswerOpts::default()
        },
        &accepted_destination,
    )
    .unwrap()
    .unwrap();
    accepted_plan.write(&accepted_destination, false).unwrap();

    assert!(
        accepted_destination
            .join(format!("crates/{accepted_name}-test-support/Cargo.toml"))
            .is_file()
    );
    let vite_config = fs::read_to_string(accepted_destination.join("web/vite.config.ts")).unwrap();
    let repo_label = vite_config
        .split_once("http://api.")
        .unwrap()
        .1
        .split_once(".localhost:1355")
        .unwrap()
        .0;
    assert_eq!(repo_label.len(), 63);
    assert_eq!(repo_label, "r".repeat(63));

    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&accepted_destination)
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "maximum supported scaffold has invalid Cargo metadata\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&metadata.stdout),
        String::from_utf8_lossy(&metadata.stderr)
    );

    let rejected_destination = temp.path().join("rejected");
    let error = run_init(InitOpts {
        path: rejected_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(materialize_template_worktree().path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("r".repeat(217)),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("217-byte Cargo package stem"), "{error}");
    assert!(error.contains("at most 216 bytes"), "{error}");
    assert!(
        error.contains("lib<stem>_test_support-<hash>.rmeta"),
        "{error}"
    );
    assert!(!rejected_destination.exists());
}
