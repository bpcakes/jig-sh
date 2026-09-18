use std::{fs, process::Command};

use anyhow::{Context, Result, ensure};
use jig_contract::ActionSpec;
use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, TableLike};

use crate::context::RepoContext;

const PATHS: [&str; 2] = [".jig.toml", ".agent/jig-contract.json"];

pub(super) fn prepare(
    ctx: &RepoContext,
    changes: &[(ActionSpec, ActionSpec)],
    include_patch: bool,
) -> Result<String> {
    let before = read_snapshot(ctx)?;
    if changes.is_empty() {
        return Ok(String::new());
    }
    let mut source: DocumentMut = before[0].parse()?;
    let mut manifest: Value = crate::strict_json::from_slice(before[1].as_bytes())?;
    let source_actions = source
        .get_mut("repository")
        .and_then(|item| item.get_mut("actions"))
        .context("freshness adoption requires authored repository actions")?;
    let resolved_actions = manifest["actions"]
        .as_array_mut()
        .context("missing resolved actions")?;
    for (original, proposed) in changes {
        edit_source_action(source_actions, original, proposed)?;
        let resolved = resolved_actions
            .iter_mut()
            .find(|action| action["target"] == serde_json::json!(proposed.target))
            .context("resolved target disappeared during freshness adoption")?;
        let proposed_json = serde_json::to_value(proposed)?;
        for field in changed_fields(original, proposed) {
            resolved[field] = proposed_json[field].clone();
        }
    }
    let after = [
        source.to_string(),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    ];
    let staging = tempfile::tempdir()?;
    for (path, text) in PATHS.into_iter().zip(&after) {
        let destination = staging.path().join("b").join(path);
        fs::create_dir_all(
            destination
                .parent()
                .context("patch destination has no parent")?,
        )?;
        fs::write(destination, text)?;
    }
    // Validate both strict source configuration and resolved projection together.
    RepoContext::load_from_root(staging.path().join("b"))
        .context("freshness patch would produce an invalid configuration/contract pair")?;
    let mut patch = String::new();
    if include_patch {
        for (index, path) in PATHS.iter().enumerate() {
            if before[index] == after[index] {
                continue;
            }
            let old_path = format!("a/{path}");
            let new_path = format!("b/{path}");
            let old = staging.path().join(&old_path);
            fs::create_dir_all(old.parent().context("patch source has no parent")?)?;
            fs::write(old, &before[index])?;
            let output = Command::new("git")
                .current_dir(staging.path())
                .args([
                    "diff",
                    "--no-index",
                    "--text",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--no-renames",
                    "--no-prefix",
                    "--unified=3",
                    "--diff-algorithm=myers",
                    "--no-indent-heuristic",
                    "--line-prefix=",
                    "--output-indicator-new=+",
                    "--output-indicator-old=-",
                    "--output-indicator-context= ",
                    "--",
                    &old_path,
                    &new_path,
                ])
                .output()
                .context("failed to generate the freshness patch with git diff")?;
            ensure!(
                matches!(output.status.code(), Some(0 | 1)),
                "git diff could not generate the freshness patch: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            patch.push_str(
                std::str::from_utf8(&output.stdout)
                    .context("git diff returned non-UTF-8 patch text")?,
            );
        }
    }
    ensure!(
        read_snapshot(ctx)? == before,
        "repository authority changed during freshness preview; retry"
    );
    Ok(patch)
}

fn read_snapshot(ctx: &RepoContext) -> Result<[String; 2]> {
    let mut texts = Vec::new();
    for (path, expected) in PATHS.into_iter().zip(ctx.configuration_content_digests()) {
        let text = fs::read_to_string(ctx.root().join(path))?;
        ensure!(
            format!("sha256:{:x}", Sha256::digest(text.as_bytes())) == *expected,
            "{path} changed since the repository context was loaded; retry freshness preview"
        );
        texts.push(text);
    }
    Ok(texts.try_into().expect("exactly two authority files"))
}

fn changed_fields(original: &ActionSpec, proposed: &ActionSpec) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if original.source_state != proposed.source_state {
        fields.push("source_state");
    }
    if original.inputs_policy != proposed.inputs_policy {
        fields.push("inputs_policy");
    }
    if original.inputs != proposed.inputs {
        fields.push("inputs");
    }
    if original.provenance != proposed.provenance {
        fields.push("provenance");
    }
    fields
}

fn edit_source_action(
    actions: &mut Item,
    original: &ActionSpec,
    proposed: &ActionSpec,
) -> Result<()> {
    let mut found = false;
    let mut edit = |table: &mut dyn TableLike| -> Result<()> {
        let target = table.get("target").context("missing source target")?;
        if target.get("component").and_then(Item::as_str)
            != Some(proposed.target.component.as_str())
            || target.get("action").and_then(Item::as_str) != Some(proposed.target.action.as_str())
        {
            return Ok(());
        }
        ensure!(!found, "duplicate source target '{}'", proposed.target);
        found = true;
        let value = serde_json::to_value(proposed)?;
        for field in changed_fields(original, proposed) {
            if field == "provenance" {
                // Merge individual keys so unrelated provenance comments survive.
                let item =
                    table
                        .entry(field)
                        .or_insert(Item::Value(toml_edit::Value::InlineTable(
                            toml_edit::InlineTable::new(),
                        )));
                let provenance = item
                    .as_table_like_mut()
                    .context("invalid provenance table")?;
                for (key, changed) in &proposed.provenance {
                    if original.provenance.get(key) != Some(changed) {
                        provenance.insert(
                            key,
                            toml_edit::value(
                                serde_json::to_value(changed)?
                                    .as_str()
                                    .context("invalid provenance value")?,
                            ),
                        );
                    }
                }
            } else if field == "inputs" {
                let inputs: toml_edit::Array = proposed.inputs.iter().collect();
                table.insert(field, toml_edit::value(inputs));
            } else {
                table.insert(
                    field,
                    toml_edit::value(value[field].as_str().context("invalid policy value")?),
                );
            }
        }
        Ok(())
    };
    match actions {
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                edit(table)?;
            }
        }
        Item::Value(toml_edit::Value::Array(values)) => {
            for value in values.iter_mut() {
                edit(
                    value
                        .as_inline_table_mut()
                        .context("invalid inline action table")?,
                )?;
            }
        }
        _ => anyhow::bail!("repository actions must be an array of tables"),
    }
    ensure!(
        found,
        "source target '{}' disappeared during freshness adoption",
        proposed.target
    );
    Ok(())
}
