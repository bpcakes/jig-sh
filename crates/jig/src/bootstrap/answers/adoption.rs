use super::*;
use crate::bootstrap::adopt_infer::components::{Disposition, Ecosystem};
use crate::bootstrap::adopt_infer::{AdoptInference, ComponentSelectionOpts};
use crate::bootstrap::repository_model::RepositoryRenderModel;

impl AnswerInput {
    pub(in crate::bootstrap) fn prepare_adoption(
        &mut self,
        inference: &mut AdoptInference,
        root: &Path,
        opts: &AnswerOpts,
        selection: &ComponentSelectionOpts,
    ) -> Result<()> {
        let mut explicit = self.raw.clone();
        explicit.merge_opts(opts);
        if let Some(model) = explicit
            .repository
            .as_ref()
            .filter(|model| model.is_complete())
        {
            if !selection.include.is_empty() || !selection.exclude.is_empty() {
                bail!(
                    "component selections apply to initial adoption; edit [repository.components] in .jig.toml to change an existing authored model"
                );
            }
            self.preserve_repository_model = true;
            inference.components_mut().preserve(model);
            inference.warn_preserved_workspace_changes(
                self.raw
                    .frontend_workspace_roots
                    .as_deref()
                    .unwrap_or_default(),
            );
            inference.components_mut().refresh = explicit.harness_footprint
                != self.raw.harness_footprint
                || explicit.sqlx_enabled != self.raw.sqlx_enabled
                || explicit.schema_dump_enabled != self.raw.schema_dump_enabled;
            if opts.sqlx_enabled == Some(false) && opts.schema_dump_enabled.is_none() {
                self.raw.schema_dump_enabled = Some(false);
            }
            return Ok(());
        }
        let candidates = inference.components_mut();
        for app in explicit.frontend_apps.as_deref().unwrap_or_default() {
            candidates.declare(root, &app.dir, Ecosystem::Node, Some(app))?;
        }
        for relative in explicit.rust_crate_roots.as_deref().unwrap_or_default() {
            candidates.declare(root, relative, Ecosystem::Rust, None)?;
        }
        let explicit_backend = explicit.backend_language.is_some()
            || explicit.sqlx_enabled == Some(true)
            || explicit.rust_migration_dir.is_some()
            || explicit.migration_dir.is_some()
            || explicit.rust_fmt_check_command.is_some()
            || explicit.rust_clippy_command.is_some()
            || explicit.rust_test_command.is_some()
            || explicit.rust_test_locked_command.is_some()
            || explicit.go_fmt_check_command.is_some()
            || explicit.go_lint_command.is_some()
            || explicit.go_test_command.is_some()
            || explicit.go_test_locked_command.is_some();
        if explicit_backend {
            candidates.declare(
                root,
                ".",
                if explicit.backend_language == Some(BackendLanguage::Go) {
                    Ecosystem::Go
                } else {
                    Ecosystem::Rust
                },
                None,
            )?;
        }
        candidates.stabilize_ids();
        let declared = candidates
            .candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .evidence
                    .iter()
                    .any(|evidence| evidence == "explicit answer input")
            })
            .map(|candidate| (candidate.root.clone(), candidate.ecosystem))
            .collect::<Vec<_>>();
        inference.select_components(root, selection)?;
        for (relative, ecosystem) in declared {
            if inference
                .components_mut()
                .candidates
                .iter()
                .any(|candidate| {
                    candidate.root == relative
                        && candidate.ecosystem == ecosystem
                        && candidate.disposition == Disposition::Excluded
                })
            {
                bail!(
                    "component exclusion '{relative}' conflicts with explicit answer input; remove the corresponding answer or the exclusion"
                );
            }
        }
        Ok(())
    }
}

impl RenderAnswers {
    pub(in crate::bootstrap) fn adoption_command_keys(
        &self,
        opts: &AnswerOpts,
    ) -> Result<BTreeSet<String>> {
        if opts
            .adoption_components
            .as_ref()
            .is_some_and(|c| c.preserved)
            && let Some(model) = self.authored_repository.as_ref()
        {
            return Ok(super::adoption_commands::overrides(opts, model)?
                .into_keys()
                .collect());
        }
        Ok(BTreeSet::new())
    }

    pub(super) fn install_adoption_components(&mut self, opts: &AnswerOpts) -> Result<()> {
        let Some(candidates) = opts.adoption_components.as_ref() else {
            return Ok(());
        };
        if let Some(prior) = self.authored_repository.as_ref() {
            if candidates.refresh {
                let (model, commands) =
                    crate::bootstrap::repository_model::adoption_refresh::refresh(
                        self,
                        candidates,
                        prior,
                        &self.authored_repository_commands,
                    )?;
                self.authored_repository = Some(model);
                self.authored_repository_commands = commands;
            }
            let overrides = super::adoption_commands::overrides(
                opts,
                self.authored_repository.as_ref().expect("preserved model"),
            )?;
            self.authored_repository_commands.extend(overrides);
            return Ok(());
        }
        let (model, commands) = RepositoryRenderModel::from_adoption(self, candidates)?;
        self.rust_crate_roots = model
            .components
            .iter()
            .filter(|component| {
                component
                    .adapters
                    .iter()
                    .any(|adapter| adapter == "rust" || adapter == "sqlx")
            })
            .map(|component| component.root.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.authored_repository = Some(model);
        self.authored_repository_commands = commands;
        Ok(())
    }
}
