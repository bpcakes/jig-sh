use super::*;

#[test]
fn adopt_minimal_preview_keeps_write_flag_in_next_steps() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let output = run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: false,
        minimal: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["render_mode"], "preview");
    assert_eq!(output["harness_footprint"], "minimal");
    assert!(!repo.join(".jig.toml").exists());
    assert!(
        output["adoption_profile"]["generated_gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| !command.as_str().unwrap().contains("scripts/jig"))
    );
    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(output["next_steps"].as_array().unwrap().iter().all(|step| {
        !step
            .as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write --force")
    }));
}

#[test]
fn full_to_minimal_preview_requires_force_in_the_emitted_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| { step.as_str() == Some("jig adopt . --minimal --write --force") })
    );
}

#[test]
fn minimal_to_minimal_preview_does_not_add_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| { !step.as_str().unwrap().contains("--force") })
    );
}

#[test]
fn invalid_prior_minimal_preview_does_not_add_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        "harness_footprint = \"not-a-footprint\"\n",
    )
    .unwrap();
    let mut preview = footprint_adopt_opts(&repo, template.path(), true, false);
    preview.write = false;

    let output = run_adopt(preview).unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("jig adopt . --minimal --write")
    }));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| { !step.as_str().unwrap().contains("--force") })
    );
}
