use super::*;

impl AnswerResolution {
    pub(in crate::bootstrap) fn from_opts(
        opts: &AnswerOpts,
        destination: &Path,
        use_defaults: bool,
    ) -> Result<Self> {
        Self::from_input(
            AnswerInput::from_opts(opts)?,
            opts,
            destination,
            use_defaults,
        )
    }

    pub(in crate::bootstrap) fn from_input(
        input: AnswerInput,
        opts: &AnswerOpts,
        destination: &Path,
        use_defaults: bool,
    ) -> Result<Self> {
        let (mut raw, authored_repository_commands, preserve_repository_model, mut notes) =
            input.into_parts();
        raw.merge_opts(opts);
        let vault_note = raw.apply_existing_vault_default(destination)?;
        let sqlx_defaulted_to_tooling_only = if use_defaults {
            raw.apply_sqlx_default_for_cli_defaults()
        } else {
            false
        };
        let mut answers = resolve_render_answers(
            raw,
            default_repo_name(destination),
            authored_repository_commands,
            preserve_repository_model,
        )?;
        answers.install_adoption_components(opts)?;
        if sqlx_defaulted_to_tooling_only {
            notes.push(
                "SQLx answers were omitted under --defaults, so Jig rendered a tooling-only profile. Pass --sqlx-enabled true --rust-migration-dir <dir> for SQLx repos, or pass --sqlx-enabled false for tooling-only repos."
                    .into(),
            );
        }
        if let Some(note) = vault_note {
            notes.push(note);
        }
        Ok(Self { answers, notes })
    }

    pub(in crate::bootstrap) fn into_parts(self) -> (RenderAnswers, Vec<String>) {
        (self.answers, self.notes)
    }
}
