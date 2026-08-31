use std::collections::BTreeMap;

use anyhow::{Result, bail};
use jig_contract::{
    ActionEffect, ActionIntent, ActionRunner, ActionSpec, FieldProvenance,
    NativeActionConfigurationV1, NativeFileBudgetConfigV1, tool,
};

use super::{
    AuthoredRepositoryModel, ModelBuilder, REPO_COMPONENT, RUST_FILE_LOC_COMMAND_KEY,
    RepositoryRenderModel, is_generated_rust_file_loc_command, provenance, rust_file_loc,
    target_id,
};

const FILE_BUDGET_ACTION: &str = "file-budget";

struct SeedPolicyGroup {
    id: &'static str,
    extensions: &'static [&'static str],
    exclusions: &'static [&'static str],
}

pub(super) fn add_file_budget_action(builder: &mut ModelBuilder<'_>) -> Result<()> {
    let action = generated_file_budget_action()?;
    builder.insert_tool(
        tool::FILE_BUDGET,
        "Enforce repository-owned source file budgets.",
        None,
    )?;
    builder.insert_action(action)
}

pub(in crate::bootstrap) fn generated_file_budget_action() -> Result<ActionSpec> {
    let mut action = ActionSpec::new(
        target_id(REPO_COMPONENT, FILE_BUDGET_ACTION)?,
        ActionIntent::Check,
        ActionRunner::native_configured(
            tool::FILE_BUDGET,
            NativeActionConfigurationV1::file_budget(NativeFileBudgetConfigV1::default()),
        ),
    );
    action.description = Some("Enforce repository-owned source file budgets.".into());
    action.effects = vec![ActionEffect::ReadOnly];
    action.inputs = vec!["**".into()];
    action.legacy_aliases = vec![tool::FILE_BUDGET.into()];
    action.provenance = provenance(&[
        ("target", FieldProvenance::Inherited),
        ("intent", FieldProvenance::Inherited),
        ("effects", FieldProvenance::Inherited),
        ("runner", FieldProvenance::Inherited),
        ("inputs", FieldProvenance::Inherited),
        ("legacy_aliases", FieldProvenance::Inherited),
    ]);
    Ok(action)
}

pub(super) fn matches_legacy_projection(
    current: &RepositoryRenderModel,
    authored: &AuthoredRepositoryModel,
    authored_commands: &BTreeMap<String, String>,
) -> bool {
    if current.affected_ignore != authored.affected_ignore
        || current.components != authored.components
        || current.default_check_profile != authored.default_check_profile
    {
        return false;
    }
    let Ok(file_budget_target) = target_id(REPO_COMPONENT, FILE_BUDGET_ACTION) else {
        return false;
    };
    let Ok(legacy_target) = target_id(REPO_COMPONENT, "rust-file-loc") else {
        return false;
    };
    let has_rust = current.components.iter().any(|component| {
        component
            .adapters
            .iter()
            .any(|adapter| matches!(adapter.as_str(), "rust" | "sqlx"))
    });

    let mut expected_actions = current
        .actions
        .iter()
        .filter(|action| action.target != file_budget_target)
        .cloned()
        .collect::<Vec<_>>();
    if has_rust {
        let Ok(legacy) = rust_file_loc::generated_legacy_rust_file_loc_action() else {
            return false;
        };
        expected_actions.push(legacy);
    }
    expected_actions.sort_by(|left, right| left.target.cmp(&right.target));
    if expected_actions != authored.actions {
        return false;
    }

    let mut expected_profiles = current.profiles.clone();
    for profile in &mut expected_profiles {
        profile
            .targets
            .retain(|target| target != &file_budget_target);
        if has_rust {
            profile.targets.push(legacy_target.clone());
            profile.targets.sort();
            profile.targets.dedup();
        }
    }
    if expected_profiles != authored.profiles {
        return false;
    }

    if current
        .commands
        .iter()
        .any(|(key, value)| authored_commands.get(key) != Some(value))
    {
        return false;
    }
    let expected_command_count = current.commands.len() + usize::from(has_rust);
    authored_commands.len() == expected_command_count
        && (!has_rust
            || authored_commands
                .get(RUST_FILE_LOC_COMMAND_KEY)
                .is_some_and(|command| is_generated_rust_file_loc_command(command)))
}

pub(super) fn render_seed_policy(model: &RepositoryRenderModel) -> Result<Option<String>> {
    let groups = selected_extension_groups(model)?;
    if groups.is_empty() {
        return Ok(None);
    }
    let mut policy = String::from("version = 1\n");
    for group in groups {
        let includes = group
            .extensions
            .iter()
            .map(|extension| format!("  \"**/*.{extension}\","))
            .collect::<Vec<_>>()
            .join("\n");
        let excludes = if group.exclusions.is_empty() {
            "[]".to_owned()
        } else {
            format!(
                "[\n{}\n]",
                group
                    .exclusions
                    .iter()
                    .map(|path| format!("  \"{path}\","))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        let id = group.id;
        policy.push_str(&format!(
            "\n[[rules]]\nid = \"{id}\"\ncategory = \"source\"\ninclude = [\n{includes}\n]\nexclude = {excludes}\nnotice_lines = 400\nwarn_lines = 500\nmax_lines = 800\n"
        ));
    }
    let seed_date = jig_file_budget::PolicyDateV1::new(2000, 1, 1)
        .expect("the fixed seed validation date is valid");
    jig_file_budget::parse_policy_v1(policy.as_bytes(), seed_date).map_err(|diagnostics| {
        let details = diagnostics
            .diagnostics()
            .iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code.as_str(), diagnostic.message))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::anyhow!("Generated file-budget seed policy is invalid: {details}")
    })?;
    Ok(Some(policy))
}

fn selected_extension_groups(model: &RepositoryRenderModel) -> Result<Vec<SeedPolicyGroup>> {
    let mut rust = false;
    let mut go = false;
    let mut web = false;
    for component in &model.components {
        for adapter in &component.adapters {
            match adapter.as_str() {
                "rust" | "sqlx" => rust = true,
                "go" | "go-postgres" => go = true,
                "typescript" => web = true,
                "jig" => {}
                other => {
                    // Generated adapters are a closed product surface. Failing
                    // here prevents a newly introduced source adapter from
                    // silently receiving no repository policy.
                    if component.id.as_str() != REPO_COMPONENT {
                        bail!("Repository adapter '{other}' has no file-budget seed contribution");
                    }
                }
            }
        }
    }
    let mut groups = Vec::new();
    if go {
        groups.push(SeedPolicyGroup {
            id: "go-source",
            extensions: &["go"],
            exclusions: &[],
        });
    }
    if rust {
        groups.push(SeedPolicyGroup {
            id: "rust-source",
            extensions: &["rs"],
            exclusions: &[],
        });
    }
    if web {
        groups.push(SeedPolicyGroup {
            id: "web-source",
            extensions: &[
                "cjs", "cts", "js", "jsx", "mjs", "mts", "svelte", "ts", "tsx", "vue",
            ],
            exclusions: &["scripts/web-node.cjs"],
        });
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::repository_model::tests::answers;

    #[test]
    fn generated_policy_is_deterministic_and_stack_neutral() {
        let rust = RepositoryRenderModel::from_answers(&answers("")).unwrap();
        let rust_policy = render_seed_policy(&rust).unwrap().unwrap();
        assert!(rust_policy.contains("\"**/*.rs\""));
        assert!(!rust_policy.contains("\"**/*.go\""));

        let mixed = RepositoryRenderModel::from_answers(&answers(
            r#"
backend_language = "go"
go_database = "postgres"

[[frontend_apps]]
name = "web"
dir = "frontend/web"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
        ))
        .unwrap();
        let mixed_policy = render_seed_policy(&mixed).unwrap().unwrap();
        for pattern in ["**/*.go", "**/*.js", "**/*.ts", "**/*.tsx"] {
            assert!(mixed_policy.contains(&format!("\"{pattern}\"")));
        }
        for rule in ["go-source", "web-source"] {
            assert!(mixed_policy.contains(&format!("id = \"{rule}\"")));
        }
        assert!(mixed_policy.contains("\"scripts/web-node.cjs\""));
        assert!(!mixed_policy.contains("**/*.rs"));
    }
}
