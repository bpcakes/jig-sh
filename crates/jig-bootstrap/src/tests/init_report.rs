use super::*;

#[test]
fn run_init_json_report_preserves_the_documented_top_level_contract() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let report = run_init(InitOpts {
        path: destination,
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    let output = serde_json::to_value(report).unwrap();

    let top_level_keys = output
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        top_level_keys,
        std::collections::BTreeSet::from([
            "answers_file",
            "command",
            "destination",
            "git_initialized",
            "next_steps",
            "notes",
            "ok",
            "render_mode",
            "render_report",
            "scaffold",
            "template",
        ])
    );
    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "init");
    assert_eq!(output["render_mode"], "copy");
    assert_eq!(output["answers_file"], ".jig.toml");
    assert!(output["scaffold"].is_null());
}
