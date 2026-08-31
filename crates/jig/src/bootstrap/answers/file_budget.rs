use super::*;

impl RenderAnswers {
    pub(in crate::bootstrap) fn from_managed_answers_file(
        path: &Path,
        destination: &Path,
    ) -> Result<Self> {
        let AnswerInput {
            mut raw,
            authored_repository_commands,
            preserve_repository_model,
            ..
        } = AnswerInput::from_file(path)?;
        raw.normalize_legacy_sqlx_disabled_schema_dump();
        raw.normalize_legacy_generated_cargo_command_defaults();
        let mut answers = resolve_render_answers(
            raw,
            default_repo_name(destination),
            authored_repository_commands,
            preserve_repository_model,
        )?;
        answers.go_postgres_integration_script = path
            .parent()
            .is_some_and(has_go_postgres_integration_script);
        Ok(answers)
    }

    pub(in crate::bootstrap) fn file_budget_ci_enabled(&self) -> bool {
        self.authored_repository.as_ref().map_or_else(
            || !self.is_minimal_footprint(),
            |repository| {
                repository
                    .actions
                    .iter()
                    .any(|action| action.target.to_string() == "repo:file-budget")
            },
        )
    }
}
