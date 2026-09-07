use super::*;
use crate::bootstrap::adopt_infer::ComponentCandidates;
use crate::bootstrap::adopt_infer::components::{Disposition, Ecosystem};

impl RepositoryRenderModel {
    pub(in crate::bootstrap) fn from_adoption(
        answers: &RenderAnswers,
        candidates: &ComponentCandidates,
    ) -> Result<(AuthoredRepositoryModel, BTreeMap<String, String>)> {
        let accepted = candidates
            .candidates
            .iter()
            .filter(|candidate| candidate.disposition == Disposition::Included)
            .collect::<Vec<_>>();
        let ids = accepted
            .iter()
            .map(|candidate| &candidate.proposed_id)
            .collect::<BTreeSet<_>>();
        if ids.len() != accepted.len() {
            bail!("adoption candidates have conflicting component IDs");
        }
        let backend_ecosystem = if answers.backend_language().is_go() {
            Ecosystem::Go
        } else {
            Ecosystem::Rust
        };
        let backend = accepted
            .iter()
            .find(|candidate| candidate.root == "." && candidate.ecosystem == backend_ecosystem);
        let mut builder = ModelBuilder::new(answers)?;
        builder.add_repository_component()?;
        if accepted.is_empty() {
            builder
                .actions
                .remove(&target_id(REPO_COMPONENT, "file-budget")?);
            builder.tools.remove(tool::FILE_BUDGET);
        }
        if backend.is_some() {
            builder.add_backend_component()?;
        }
        builder.add_frontend_components()?;
        let mut model = builder.finish()?;
        let mut renamed = BTreeMap::new();
        if let Some(candidate) = backend {
            renamed.insert(
                component_id(BACKEND_COMPONENT)?,
                component_id(&candidate.proposed_id)?,
            );
        }
        for candidate in &accepted {
            if let Some(name) = &candidate.frontend_name {
                renamed.insert(
                    frontend_component_id(name)?,
                    component_id(&candidate.proposed_id)?,
                );
            }
        }
        for component in &mut model.components {
            if let Some(id) = renamed.get(&component.id) {
                component.id = id.clone();
            }
            // These are newly generated dependencies, never user-authored edges.
            component
                .depends_on
                .retain(|id| id.as_str() != BACKEND_COMPONENT || backend.is_some());
            for dependency in &mut component.depends_on {
                if let Some(id) = renamed.get(dependency) {
                    *dependency = id.clone();
                }
            }
        }
        for candidate in accepted {
            let id = component_id(&candidate.proposed_id)?;
            if let Some(component) = model
                .components
                .iter_mut()
                .find(|component| component.id == id)
            {
                component.root.clone_from(&candidate.root);
                if candidate.workspace && candidate.frontend_name.is_none() {
                    component.tags = vec!["workspace".into()];
                    component.description = Some("Repository workspace.".into());
                }
            } else {
                let mut component = ComponentSpec::new(id, &candidate.root);
                component.description = Some("Component accepted during repository adoption; add actions explicitly as needed.".into());
                component.adapters = vec![
                    match candidate.ecosystem {
                        Ecosystem::Rust => "rust",
                        Ecosystem::Node => "typescript",
                        Ecosystem::Go => "go",
                        Ecosystem::Authored => {
                            bail!("opaque authored components require a complete preserved repository model and a valid [commands] table")
                        }
                    }
                    .into(),
                ];
                if candidate.workspace {
                    component.tags.push("workspace".into());
                }
                component.provenance = provenance(&[
                    ("id", FieldProvenance::Inferred),
                    ("root", FieldProvenance::Declared),
                ]);
                model.components.push(component);
            }
        }
        let rename_target = |target: &mut TargetId| {
            if let Some(id) = renamed.get(&target.component) {
                target.component = id.clone();
            }
        };
        for action in &mut model.actions {
            rename_target(&mut action.target);
            for dependency in &mut action.depends_on {
                rename_target(dependency);
            }
        }
        for profile in &mut model.profiles {
            for target in &mut profile.targets {
                rename_target(target);
            }
        }
        model.components.sort_by(|a, b| a.id.cmp(&b.id));
        if model
            .components
            .windows(2)
            .any(|pair| pair[0].id == pair[1].id)
        {
            bail!(
                "adoption candidates resolve to conflicting component IDs; choose distinct frontend names"
            );
        }
        Ok((
            AuthoredRepositoryModel {
                default_check_profile: model.default_check_profile,
                affected_ignore: model.affected_ignore,
                components: model.components,
                actions: model.actions,
                profiles: model.profiles,
            },
            model.commands,
        ))
    }
}
