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
