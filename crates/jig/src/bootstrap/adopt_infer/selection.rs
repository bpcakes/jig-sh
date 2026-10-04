use super::*;
use anyhow::Result;

use super::components::{Disposition, Ecosystem};
use crate::backend::BackendLanguage;

impl AdoptInference {
    pub(in crate::bootstrap) fn warn_preserved_workspace_changes(&mut self, prior: &[String]) {
        let current = self
            .frontend_workspace_roots
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        let prior = prior.iter().collect::<std::collections::BTreeSet<_>>();
        if current != prior {
            self.warnings.push("JavaScript workspace membership changed; review frontend_workspace_roots and the owning [repository.actions].inputs in .jig.toml. Readoption preserves their authored values.".into());
        }
    }

    pub(in crate::bootstrap) fn components_mut(&mut self) -> &mut ComponentCandidates {
        &mut self.components
    }

    pub(in crate::bootstrap) fn select_components(
        &mut self,
        root: &Path,
        selection: &ComponentSelectionOpts,
    ) -> Result<()> {
        self.components.select(root, selection)?;
        self.frontend_apps.retain(|app| {
            self.components.candidates.iter().any(|candidate| {
                candidate.disposition == Disposition::Included
                    && candidate.frontend_name.as_deref() == Some(&app.name)
                    && candidate.root == app.dir
            })
        });
        for key in [
            "sqlx_enabled",
            "rust_migration_dir",
            "rust_migration_dirs",
            "rust_sqlx_metadata_dir",
            "sqlx_check_command",
        ] {
            self.metadata.remove(key);
        }
        if self.components.has_root_backend(Ecosystem::Rust) {
            let scan = self
                .scan
                .get_or_insert_with(|| RepoScan::collect(root, &mut self.warnings))
                .for_selected_components(root, &self.components);
            self.sqlx_enabled = None;
            self.rust_migration_dir = None;
            self.rust_sqlx_metadata_dir = None;
            self.sqlx_check_command = None;
            self.infer_and_apply_sqlx(root, &scan);
        } else {
            self.clear_sqlx_inference("no accepted root Rust component");
        }
        let mut seen = std::collections::BTreeSet::new();
        self.warnings.retain(|warning| seen.insert(warning.clone()));
        Ok(())
    }

    pub(in crate::bootstrap) fn apply_component_decisions(&self, answers: &mut AnswerOpts) {
        if self.components.preserved {
            answers.adoption_components = Some(self.components.clone());
            return;
        }
        if answers.backend_language.is_none()
            && self.components.has_root_backend(Ecosystem::Go)
            && !self.components.has_root_backend(Ecosystem::Rust)
        {
            answers.backend_language = Some(BackendLanguage::Go);
        }
        answers.rust_crate_roots = self
            .components
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.ecosystem == Ecosystem::Rust
                    && candidate.disposition == Disposition::Included
            })
            .map(|candidate| candidate.root.clone())
            .collect();
        answers.adoption_components = Some(self.components.clone());
    }
}
