use super::*;
use crate::bootstrap::repository_model::AuthoredRepositoryModel;

impl ComponentCandidates {
    pub(in crate::bootstrap) fn has_root_backend(&self, ecosystem: Ecosystem) -> bool {
        self.candidates.iter().any(|candidate| {
            candidate.root == "."
                && candidate.ecosystem == ecosystem
                && candidate.disposition == Disposition::Included
        })
    }

    pub(in crate::bootstrap) fn declare(
        &mut self,
        root: &Path,
        relative: &str,
        ecosystem: Ecosystem,
        app: Option<&FrontendApp>,
    ) -> Result<()> {
        let relative = normalize_portable_repo_path(relative, "explicit component root")?;
        if relative == "." {
            validate_directory(root, &relative)?;
        } else {
            validate_repository_directory_path(root, Path::new(&relative))?;
        }
        let exists = root.join(&relative).is_dir();
        if let Some(candidate) = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.root == relative && candidate.ecosystem == ecosystem)
        {
            candidate.include("explicit answer input", Confidence::High);
            candidate.evidence.push("explicit answer input".into());
            if let Some(app) = app {
                candidate.frontend_name = Some(app.name.clone());
                candidate.proposed_id = frontend_component_id(&app.name)?.to_string();
            }
        } else {
            self.candidates.push(ComponentCandidate {
                proposed_id: if let Some(app) = app {
                    frontend_component_id(&app.name)?.to_string()
                } else {
                    proposed_id(&relative, ecosystem)
                },
                root: relative,
                ecosystem,
                disposition: Disposition::Included,
                reason: if exists {
                    "explicit answer input"
                } else {
                    "explicit answer input; root does not exist yet"
                }
                .into(),
                evidence: vec!["explicit answer input".into()],
                confidence: Confidence::High,
                frontend_name: app.map(|app| app.name.clone()),
                workspace: false,
                valid_manifest: exists,
            });
        }
        Ok(())
    }

    pub(in crate::bootstrap) fn preserve(&mut self, model: &AuthoredRepositoryModel) {
        // Discovery remains reviewable, but the stored model is the authoring authority.
        for candidate in &mut self.candidates {
            candidate.disposition = Disposition::Excluded;
            candidate.reason = "not selected by the existing authored repository model".into();
        }
        for component in &model.components {
            if component.id.as_str() == "repo" {
                continue;
            }
            let ecosystem = if component.adapters.iter().any(|adapter| adapter == "rust") {
                Ecosystem::Rust
            } else if component.adapters.iter().any(|adapter| adapter == "go") {
                Ecosystem::Go
            } else if component
                .adapters
                .iter()
                .any(|adapter| adapter == "typescript")
            {
                Ecosystem::Node
            } else {
                Ecosystem::Authored
            };
            if let Some(candidate) = self.candidates.iter_mut().find(|candidate| {
                candidate.root == component.root
                    && candidate.ecosystem == ecosystem
                    && candidate.disposition == Disposition::Excluded
            }) {
                candidate.proposed_id = component.id.to_string();
                candidate.include("preserved authored component", Confidence::High);
                candidate
                    .evidence
                    .push(".jig.toml [repository.components]".into());
            } else {
                self.candidates.push(ComponentCandidate {
                    root: component.root.clone(),
                    proposed_id: component.id.to_string(),
                    ecosystem,
                    disposition: Disposition::Included,
                    reason: "preserved authored component".into(),
                    evidence: vec![".jig.toml [repository.components]".into()],
                    confidence: Confidence::High,
                    frontend_name: None,
                    workspace: false,
                    valid_manifest: true,
                });
            }
        }
        self.preserved = true;
        self.candidates
            .sort_by(|a, b| (&a.root, &a.proposed_id).cmp(&(&b.root, &b.proposed_id)));
    }
}
