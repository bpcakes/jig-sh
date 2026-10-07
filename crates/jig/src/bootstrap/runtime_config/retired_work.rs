//! Tracker ownership follows the rendered contract epoch. Contract 9 retires
//! `[work]`, moves ownership to `[repository] tracker`, and reports dropped settings.

use jig_context::{RepositoryTracker, WORK_CONFIG_RETIRED_CONTRACT_VERSION};

use super::*;

pub(super) fn apply_retained_tracker(existing: &toml::Table, destination: &Path) -> Result<()> {
    let Some(tracker) = retained_tracker(existing)? else {
        return Ok(());
    };
    let contract_version = RepoContext::declared_contract_version_from_root(destination)?;
    rewrite_rendered_config(destination, |rendered| {
        if contract_version >= WORK_CONFIG_RETIRED_CONTRACT_VERSION {
            rendered
                .get_mut("repository")
                .and_then(toml::Value::as_table_mut)
                .ok_or_else(|| anyhow::anyhow!("Rendered [repository] is not a TOML table"))?
                .insert("tracker".into(), tracker);
        } else {
            rendered
                .entry("work")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("Rendered [work] is not a TOML table"))?
                .insert("receipt_metadata".into(), toml::Value::Array(vec![tracker]));
        }
        Ok(())
    })
}

/// An existing `[repository] tracker`, or the Beads receipt metadata that an
/// older `[work]` section declared for the same purpose.
fn retained_tracker(existing: &toml::Table) -> Result<Option<toml::Value>> {
    if let Some(tracker) = existing
        .get("repository")
        .and_then(toml::Value::as_table)
        .and_then(|repository| repository.get("tracker"))
    {
        if tracker.clone().try_into::<RepositoryTracker>().is_err() {
            bail!(
                "existing [repository].tracker is invalid; repair it before refreshing the Jig harness"
            );
        }
        return Ok(Some(tracker.clone()));
    }
    Ok(declares_beads_metadata(existing)?.then(|| toml::Value::String("beads".into())))
}

fn declares_beads_metadata(existing: &toml::Table) -> Result<bool> {
    let Some(metadata) = existing_work(existing)?.and_then(|work| work.get("receipt_metadata"))
    else {
        return Ok(false);
    };
    if !schema_valid_work_field("receipt_metadata", metadata.clone()) {
        bail!(
            "existing [work].receipt_metadata is invalid; repair it before refreshing the Jig harness"
        );
    }
    Ok(metadata
        .as_array()
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("beads"))))
}

fn existing_work(existing: &toml::Table) -> Result<Option<&toml::Table>> {
    existing
        .get("work")
        .map(|work| {
            work.as_table()
                .ok_or_else(|| anyhow::anyhow!("Existing [work] is not a TOML table"))
        })
        .transpose()
}

/// Describes how a refresh that renders contract 9 or later treated an
/// existing `[work]` section. Generated gates are not listed.
pub(in crate::bootstrap) fn retired_work_notes(
    seed_repo_path: Option<&Path>,
    staged_destination: &Path,
) -> Result<Vec<String>> {
    let Some(existing) = read_existing_config(seed_repo_path)? else {
        return Ok(Vec::new());
    };
    if RepoContext::declared_contract_version_from_root(staged_destination)?
        < WORK_CONFIG_RETIRED_CONTRACT_VERSION
    {
        return Ok(Vec::new());
    }
    let Some(work) = existing_work(&existing)? else {
        return Ok(Vec::new());
    };
    let default_check_profile = existing
        .get("repository")
        .and_then(toml::Value::as_table)
        .and_then(|repository| repository.get("default_check_profile"))
        .and_then(toml::Value::as_str);
    let mut notes = Vec::new();
    if declares_beads_metadata(&existing)? {
        notes.push(
            "`[work] receipt_metadata = [\"beads\"]` moves to `[repository] tracker = \"beads\"`."
                .to_string(),
        );
    }
    let mut dropped = Vec::new();
    for (key, value) in work {
        match key.as_str() {
            "receipt_metadata" => {}
            "gates" => {
                let ids = value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_table)
                    .filter(|gate| !is_generated_gate(gate, default_check_profile))
                    .map(|gate| {
                        format!(
                            "`{}`",
                            gate.get("id")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("<missing id>")
                        )
                    })
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    dropped.push(format!("gates {}", ids.join(", ")));
                }
            }
            "checks" if value.as_array().is_some_and(Vec::is_empty) => {}
            other => dropped.push(format!("`{other}`")),
        }
    }
    if !dropped.is_empty() {
        notes.push(format!(
            "Retired [work] settings are dropped from .jig.toml: {}. Jig contract version {WORK_CONFIG_RETIRED_CONTRACT_VERSION} no longer reads them.",
            dropped.join("; ")
        ));
    }
    Ok(notes)
}

fn is_generated_gate(gate: &toml::Table, default_check_profile: Option<&str>) -> bool {
    let field = |name| gate.get(name).and_then(toml::Value::as_str);
    if !gate.iter().all(|(key, value)| match key.as_str() {
        "id" | "kind" => true,
        "tool" => field("kind") == Some("check"),
        "profile" | "conclusion" => field("kind") == Some("evidence"),
        "required" => value.as_bool() == Some(true),
        "reuse" => field("kind") == Some("check") && value.as_bool() == Some(false),
        _ => false,
    }) {
        return false;
    }
    match field("kind") {
        Some("evidence") => {
            field("id") == Some("verify")
                && field("profile").is_some()
                && field("profile") == default_check_profile
                && field("conclusion").unwrap_or("success") == "success"
        }
        Some("check") => {
            is_retired_generated_check_gate(gate)
                || matches!(
                    (field("id"), field("tool")),
                    (Some("sqlx"), Some("jig.sqlx_check"))
                        | (Some("sqlc"), Some("jig.sqlc_check"))
                        | (Some("schema"), Some("jig.schema_check"))
                )
        }
        _ => false,
    }
}
