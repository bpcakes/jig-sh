//! A refresh can retire generated component and target dependencies. Name
//! each one so an owner who relied on it can restore it deliberately.

use super::*;

/// Lists the dependencies that the staged `.jig.toml` drops from components
/// and targets that both versions declare.
pub fn retired_dependency_notes(
    seed_repo_path: Option<&Path>,
    staged_destination: &Path,
) -> Result<Vec<String>> {
    let Some(existing) = read_existing_config(seed_repo_path)? else {
        return Ok(Vec::new());
    };
    let Some(staged) = read_existing_config(Some(staged_destination))? else {
        return Ok(Vec::new());
    };
    let mut notes = Vec::new();
    for (kind, table, key) in [
        (
            "Component",
            "components",
            component_key as fn(&toml::Table) -> Option<String>,
        ),
        ("Target", "actions", target_key),
    ] {
        let after = dependencies(&staged, table, key);
        for (owner, before) in dependencies(&existing, table, key) {
            let Some(after) = after.get(&owner) else {
                continue;
            };
            let removed = before
                .difference(after)
                .map(|dependency| format!("`{dependency}`"))
                .collect::<Vec<_>>();
            if !removed.is_empty() {
                notes.push(format!(
                    "{kind} `{owner}` no longer depends on {}. Restore any dependency you rely on in .jig.toml and mark its `depends_on` provenance `declared`.",
                    removed.join(", ")
                ));
            }
        }
    }
    Ok(notes)
}

fn dependencies(
    config: &toml::Table,
    table: &str,
    key: fn(&toml::Table) -> Option<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    config
        .get("repository")
        .and_then(toml::Value::as_table)
        .and_then(|repository| repository.get(table))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .filter_map(|entry| {
            let dependencies = entry
                .get("depends_on")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|dependency| match dependency {
                    toml::Value::String(id) => Some(id.clone()),
                    toml::Value::Table(target) => target_key(target),
                    _ => None,
                })
                .collect();
            Some((key(entry)?, dependencies))
        })
        .collect()
}

fn component_key(component: &toml::Table) -> Option<String> {
    component
        .get("id")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

fn target_key(entry: &toml::Table) -> Option<String> {
    let target = entry
        .get("target")
        .and_then(toml::Value::as_table)
        .unwrap_or(entry);
    let field = |name| target.get(name).and_then(toml::Value::as_str);
    Some(format!("{}:{}", field("component")?, field("action")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, config: &str) {
        fs::write(dir.join(ANSWERS_FILE), config).unwrap();
    }

    #[test]
    fn names_removed_component_and_target_dependencies() {
        let existing = tempfile::TempDir::new().unwrap();
        let staged = tempfile::TempDir::new().unwrap();
        write(
            existing.path(),
            r#"
[[repository.components]]
id = "web"
depends_on = ["api", "shared"]

[[repository.components]]
id = "renamed"
depends_on = ["api"]

[[repository.actions]]
target = { component = "web", action = "typecheck" }
depends_on = [{ component = "repo", action = "frontend-contract-drift" }]
"#,
        );
        write(
            staged.path(),
            r#"
[[repository.components]]
id = "web"
depends_on = ["shared"]

[[repository.components]]
id = "admin"
depends_on = []

[[repository.actions]]
target = { component = "web", action = "typecheck" }
"#,
        );

        let notes = retired_dependency_notes(Some(existing.path()), staged.path()).unwrap();

        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].starts_with("Component `web` no longer depends on `api`."));
        assert!(notes[1].starts_with(
            "Target `web:typecheck` no longer depends on `repo:frontend-contract-drift`."
        ));
    }

    #[test]
    fn unchanged_or_missing_configuration_has_no_notes() {
        let existing = tempfile::TempDir::new().unwrap();
        let staged = tempfile::TempDir::new().unwrap();
        let config = "[[repository.components]]\nid = \"web\"\ndepends_on = [\"api\"]\n";
        write(existing.path(), config);
        write(staged.path(), config);

        assert!(
            retired_dependency_notes(Some(existing.path()), staged.path())
                .unwrap()
                .is_empty()
        );
        assert!(
            retired_dependency_notes(None, staged.path())
                .unwrap()
                .is_empty()
        );
    }
}
