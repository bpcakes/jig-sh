//! Receipt and supported batch-evidence reference collection.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::RunLinkageCollector;
use crate::state::diagnostics::deep::{visit_array_values, visit_object_members};
use crate::state::json_scan::first_non_whitespace;
use crate::state::{WORK_CHECK_EVIDENCE_SCHEMA, WORK_CHECK_TARGETS_SCHEMA};

pub(super) struct BatchReference {
    receipt_id: String,
    children: Vec<BatchChild>,
}

struct BatchChild {
    receipt_id: Option<String>,
    run_id: Option<String>,
}

#[derive(Deserialize)]
struct TargetEvidenceEntry<'a> {
    #[serde(default, borrow)]
    receipt_id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    run_id: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct GateEvidenceEntry<'a> {
    #[serde(default, borrow)]
    tool_receipt_id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    source_tool_receipt_id: Option<Cow<'a, str>>,
}

#[derive(Default)]
pub(super) struct RunReferences {
    pub(super) receipt_ids: BTreeSet<String>,
    pub(super) batch_receipt_ids: BTreeSet<String>,
}

#[derive(Default)]
pub(super) struct CollectedReferences {
    pub(super) runs: BTreeMap<String, RunReferences>,
    pub(super) unresolved_batch_links: u64,
    pub(super) conflicting_batch_links: u64,
}

/// Records the identity and optional run reference carried by one valid
/// receipt. Receipt existence is independent from run association: ordinary
/// tool receipts are valid batch children even though they have no `run_id`.
pub(in crate::state) fn analyze_receipt_linkage(
    record: &[u8],
    collector: &mut RunLinkageCollector,
) -> Result<()> {
    let mut id = None;
    let mut run_id = None;
    let mut evidence = None;
    visit_object_members(record, 0..record.len(), &mut |key, value| {
        match key {
            "id" => id = Some(decode_string(record, &value).context("receipt id")?),
            "run_id" => {
                run_id = decode_optional_string(record, &value).context("receipt run_id")?
            }
            "evidence" => evidence = Some(value),
            _ => {}
        }
        Ok(())
    })?;
    let Some(id) = id else {
        bail!("receipt record has no string id");
    };
    let newly_tracked_receipt = collector.track_references(1);
    let tracked_receipt = newly_tracked_receipt || collector.receipt_ids.contains(&id);
    if newly_tracked_receipt {
        collector.receipt_ids.insert(id.clone());
    }
    if let Some(run_id) = run_id {
        collector.receipts_with_run_id += 1;
        if tracked_receipt {
            let runs = collector.receipt_runs.entry(id.clone()).or_default();
            runs.insert(run_id);
            if runs.len() > 1 {
                collector.conflicting_receipt_runs.insert(id.clone());
            }
        }
    }
    if let Some(evidence) = evidence
        && first_non_whitespace(record, &evidence) == Some(b'{')
    {
        let mut retained_children = Vec::new();
        let child_count = visit_batch_children(record, evidence, |receipt_id, run_id| {
            if collector.track_references(1) {
                retained_children.push(BatchChild {
                    receipt_id: receipt_id.map(str::to_owned),
                    run_id: run_id.map(str::to_owned),
                });
            }
        })?;
        if child_count > 0 {
            collector.batch_receipts += 1;
            collector.batch_links = collector.batch_links.saturating_add(child_count);
            if !retained_children.is_empty() {
                collector.batches.push(BatchReference {
                    receipt_id: id,
                    children: retained_children,
                });
            }
        }
    }
    Ok(())
}

fn visit_batch_children(
    record: &[u8],
    evidence: Range<usize>,
    mut visit: impl FnMut(Option<&str>, Option<&str>),
) -> Result<u64> {
    let mut schema = None;
    let mut targets = None;
    let mut gates = None;
    visit_object_members(record, evidence, &mut |key, value| {
        match key {
            "schema" => {
                schema = decode_optional_string(record, &value).context("evidence schema")?
            }
            "targets" => targets = Some(value),
            "gates" => gates = Some(value),
            _ => {}
        }
        Ok(())
    })?;
    let mut child_count = 0u64;
    match schema.as_deref() {
        Some(WORK_CHECK_TARGETS_SCHEMA) => {
            let targets =
                required_batch_array(record, targets, WORK_CHECK_TARGETS_SCHEMA, "targets")?;
            visit_array_values(record, targets, &mut |entry| {
                let entry: TargetEvidenceEntry<'_> = serde_json::from_slice(&record[entry])
                    .context("work-check target evidence entry")?;
                if entry.receipt_id.is_some() || entry.run_id.is_some() {
                    child_count = child_count.saturating_add(1);
                    visit(entry.receipt_id.as_deref(), entry.run_id.as_deref());
                }
                Ok(())
            })?;
        }
        Some(WORK_CHECK_EVIDENCE_SCHEMA) => {
            let gates = required_batch_array(record, gates, WORK_CHECK_EVIDENCE_SCHEMA, "gates")?;
            visit_array_values(record, gates, &mut |entry| {
                let entry: GateEvidenceEntry<'_> = serde_json::from_slice(&record[entry])
                    .context("work-check gate evidence entry")?;
                for receipt_id in [
                    entry.tool_receipt_id.as_deref(),
                    entry.source_tool_receipt_id.as_deref(),
                ]
                .into_iter()
                .flatten()
                {
                    child_count = child_count.saturating_add(1);
                    visit(Some(receipt_id), None);
                }
                Ok(())
            })?;
        }
        _ => {}
    }
    Ok(child_count)
}

fn required_batch_array(
    record: &[u8],
    value: Option<Range<usize>>,
    schema: &str,
    field: &str,
) -> Result<Range<usize>> {
    let Some(value) = value else {
        bail!("supported evidence schema {schema:?} is missing required {field:?} array");
    };
    if first_non_whitespace(record, &value) != Some(b'[') {
        bail!("supported evidence schema {schema:?} requires {field:?} to be an array");
    }
    Ok(value)
}

fn decode_string(record: &[u8], value: &Range<usize>) -> Result<String> {
    serde_json::from_slice::<String>(&record[value.clone()]).context("expected a JSON string")
}

fn decode_optional_string(record: &[u8], value: &Range<usize>) -> Result<Option<String>> {
    serde_json::from_slice::<Option<String>>(&record[value.clone()])
        .context("expected a JSON string or null")
}

pub(super) fn collect_references(collector: &RunLinkageCollector) -> CollectedReferences {
    let mut collected = CollectedReferences::default();
    for (receipt_id, run_ids) in &collector.receipt_runs {
        for run_id in run_ids {
            collected
                .runs
                .entry(run_id.clone())
                .or_default()
                .receipt_ids
                .insert(receipt_id.clone());
        }
    }
    for batch in &collector.batches {
        for child in &batch.children {
            // Reused child evidence may belong to several runs. Record every
            // run the supported evidence names and every run the child receipt
            // itself carries; never infer one run for the whole batch.
            let mut run_ids = BTreeSet::new();
            if let Some(run_id) = &child.run_id {
                run_ids.insert(run_id.clone());
            }
            let receipt_run_ids = child
                .receipt_id
                .as_ref()
                .and_then(|receipt_id| collector.receipt_runs.get(receipt_id));
            if let Some(receipt_run_ids) = receipt_run_ids {
                if child
                    .run_id
                    .as_ref()
                    .is_some_and(|run_id| !receipt_run_ids.contains(run_id))
                {
                    collected.conflicting_batch_links =
                        collected.conflicting_batch_links.saturating_add(1);
                }
                run_ids.extend(receipt_run_ids.iter().cloned());
            }
            let receipt_exists = child
                .receipt_id
                .as_ref()
                .is_some_and(|receipt_id| collector.receipt_ids.contains(receipt_id));
            if child.receipt_id.is_some()
                && !receipt_exists
                && !collector.reference_budget_exceeded
            {
                collected.unresolved_batch_links =
                    collected.unresolved_batch_links.saturating_add(1);
            }
            for run_id in run_ids {
                let entry = collected.runs.entry(run_id).or_default();
                entry.batch_receipt_ids.insert(batch.receipt_id.clone());
                if let Some(receipt_id) = &child.receipt_id {
                    entry.receipt_ids.insert(receipt_id.clone());
                }
            }
        }
    }
    collected
}
