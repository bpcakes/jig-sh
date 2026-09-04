use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jig_contract::NativeFileBudgetConfigV1;
use jig_file_budget::{
    MeasurementBudgetV1, MeasurementV1, PathDispositionV1, PolicyDateV1, measure_stream_v1,
    parse_policy_v1,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::adopt_infer::adoption_candidate_files;
use super::staged_render::{FILE_BUDGET_POLICY_PATH, StagedRender};

const MAX_PREVIEW_FILES: usize = 100_000;
const MAX_PREVIEW_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREVIEW_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const LEGACY_MARKER_PREVIEW: usize = 64;

pub(super) struct AdoptionFileBudgetPreview {
    pub(super) report: Value,
    pub(super) human_required: bool,
}

pub(super) fn preview_adoption_file_budget(
    destination: &Path,
    staged: &StagedRender,
) -> Result<AdoptionFileBudgetPreview> {
    let existing_policy = destination.join(FILE_BUDGET_POLICY_PATH);
    let policy_path = match fs::symlink_metadata(&existing_policy) {
        Ok(metadata) if metadata.file_type().is_file() => existing_policy.clone(),
        Ok(_) => bail!(
            "Existing authored file-budget policy must be a regular file: {}",
            existing_policy.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            staged.destination.join(FILE_BUDGET_POLICY_PATH)
        }
        Err(error) => return Err(error.into()),
    };
    if !policy_path.is_file() {
        return Ok(AdoptionFileBudgetPreview {
            report: json!({
                "enabled": false,
                "reason": "the selected harness has no source file-budget policy",
            }),
            human_required: false,
        });
    }
    let policy_bytes = fs::read(&policy_path)?;
    let now = time::OffsetDateTime::now_utc().date();
    let date = PolicyDateV1::new(now.year() as u16, now.month() as u8, now.day())
        .map_err(anyhow::Error::msg)?;
    let policy = parse_policy_v1(&policy_bytes, date).map_err(|invalid| {
        anyhow::anyhow!(
            "Cannot preview adoption with invalid policy {}: {invalid}",
            policy_path.display()
        )
    })?;
    let (files, mut warnings) = adoption_candidate_files(destination);
    if files.len() > MAX_PREVIEW_FILES {
        bail!(
            "Adoption file-budget preview found more than {MAX_PREVIEW_FILES} repository files; narrow the repository or author policy explicitly"
        );
    }
    let head = crate::git_receipts::resolve_git_commit(destination, "HEAD").ok();
    let mut extensions = BTreeMap::<String, u64>::new();
    let mut candidate_count = 0_u64;
    let mut candidate_bytes = 0_u64;
    let mut debt_count = 0_u64;
    let mut markers = Vec::new();
    let mut required_waivers = Vec::new();
    let mut measurement_budget =
        MeasurementBudgetV1::new(MAX_PREVIEW_FILE_BYTES, MAX_PREVIEW_TOTAL_BYTES);

    for absolute in files {
        let relative = absolute.strip_prefix(destination).with_context(|| {
            format!(
                "Adoption scan escaped repository root: {}",
                absolute.display()
            )
        })?;
        let Some(path) = relative.to_str().map(|path| path.replace('\\', "/")) else {
            warnings.push(format!(
                "non-UTF-8 repository path was omitted from the file-budget preview: {}",
                relative.display()
            ));
            continue;
        };
        let rule = match policy.classify_path(&path) {
            Ok(PathDispositionV1::Governed(rule)) => rule,
            Ok(_) => continue,
            Err(diagnostic) => bail!(
                "Cannot classify adoption path {path}: {}",
                diagnostic.message
            ),
        };
        let metadata = fs::symlink_metadata(&absolute)?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let bytes = fs::read(&absolute)?;
        let measurement = measure_stream_v1(
            &mut Cursor::new(bytes.as_slice()),
            &mut measurement_budget,
            || false,
        )
        .map_err(|error| anyhow::anyhow!("Cannot measure adoption path {path}: {error}"))?;
        candidate_count += 1;
        candidate_bytes = candidate_bytes
            .checked_add(measurement.bytes)
            .context("Adoption candidate byte count overflowed")?;
        if let Some(extension) = relative.extension().and_then(|value| value.to_str()) {
            *extensions
                .entry(extension.to_ascii_lowercase())
                .or_default() += 1;
        }
        let exceeds = exceeds_rule(measurement, rule);
        debt_count += u64::from(exceeds);
        let marker = legacy_marker(&bytes);
        if let Some(marker) = marker {
            let baseline = head
                .as_deref()
                .map(|head| baseline_measurement(destination, head, &path))
                .transpose()?
                .flatten();
            let waiver_required = exceeds
                && debt_grew(measurement, baseline, rule)
                && policy.waiver_for(&rule.id, &path).is_none();
            if waiver_required {
                required_waivers.push(json!({
                    "draft_id": format!("legacy-{}", &digest(path.as_bytes())[..12]),
                    "path": path,
                    "rule": rule.id,
                    "ceiling_lines": measurement.lines,
                    "ceiling_bytes": rule.max_bytes.map(|_| measurement.bytes),
                    "reason": null,
                    "expires": null,
                    "authorization": "human_required",
                }));
            }
            if markers.len() < LEGACY_MARKER_PREVIEW {
                markers.push(json!({
                    "path": path,
                    "marker": marker,
                    "lines": measurement.lines,
                    "bytes": measurement.bytes,
                    "baseline_lines": baseline.map(|value| value.lines),
                    "waiver_required": waiver_required,
                }));
            }
        }
    }

    let defaults = NativeFileBudgetConfigV1::default();
    let proposed_max_candidates = (candidate_count > defaults.max_candidates).then(|| {
        candidate_count
            .saturating_add(candidate_count / 10)
            .saturating_add(1)
    });
    let proposed_max_total_bytes = (candidate_bytes > defaults.max_total_bytes).then(|| {
        candidate_bytes
            .saturating_add(candidate_bytes / 4)
            .min(MAX_PREVIEW_TOTAL_BYTES)
    });
    let human_required = !required_waivers.is_empty();
    Ok(AdoptionFileBudgetPreview {
        report: json!({
            "enabled": true,
            "policy": if policy_path == existing_policy { "preserve_existing" } else { "seed_once" },
            "common_source_extensions": extensions,
            "proposed_rules": policy.rules(),
            "proposed_exclusions": policy.exclusions(),
            "candidate_count": candidate_count,
            "candidate_bytes": candidate_bytes,
            "current_debt_file_count": debt_count,
            "legacy_marker_count": markers.len(),
            "legacy_markers": markers,
            "required_waivers": required_waivers,
            "human_authorization_required": human_required,
            "native_configuration_proposal": {
                "max_candidates": proposed_max_candidates,
                "max_total_bytes": proposed_max_total_bytes,
                "direct_flags": proposed_direct_flags(proposed_max_candidates, proposed_max_total_bytes),
            },
            "warnings": warnings,
        }),
        human_required,
    })
}

fn baseline_measurement(root: &Path, head: &str, path: &str) -> Result<Option<MeasurementV1>> {
    let bytes = crate::git_receipts::read_tree_path_blob_v1_with_cancellation(
        root,
        head,
        path,
        MAX_PREVIEW_FILE_BYTES as usize,
        &|| false,
    )?;
    bytes
        .map(|bytes| {
            let mut budget =
                MeasurementBudgetV1::new(MAX_PREVIEW_FILE_BYTES, MAX_PREVIEW_FILE_BYTES);
            measure_stream_v1(&mut Cursor::new(bytes), &mut budget, || false)
                .map_err(anyhow::Error::msg)
        })
        .transpose()
}

fn exceeds_rule(measurement: MeasurementV1, rule: &jig_file_budget::RuleV1) -> bool {
    rule.max_lines
        .is_some_and(|limit| measurement.lines > limit)
        || rule
            .max_bytes
            .is_some_and(|limit| measurement.bytes > limit)
}

fn debt_grew(
    current: MeasurementV1,
    baseline: Option<MeasurementV1>,
    rule: &jig_file_budget::RuleV1,
) -> bool {
    let baseline = baseline.unwrap_or_default();
    rule.max_lines.is_some_and(|limit| {
        current.lines.saturating_sub(limit) > baseline.lines.saturating_sub(limit)
    }) || rule.max_bytes.is_some_and(|limit| {
        current.bytes.saturating_sub(limit) > baseline.bytes.saturating_sub(limit)
    })
}

fn legacy_marker(bytes: &[u8]) -> Option<&'static str> {
    let first_lines = bytes
        .split(|byte| *byte == b'\n')
        .take(40)
        .collect::<Vec<_>>();
    if first_lines.iter().any(|line| {
        line.windows(b"agentic-loc-exception:".len())
            .any(|window| window == b"agentic-loc-exception:")
    }) {
        Some("agentic-loc-exception")
    } else if first_lines.iter().any(|line| {
        line.windows(b"@generated".len())
            .any(|window| window == b"@generated")
    }) {
        Some("@generated")
    } else {
        None
    }
}

fn proposed_direct_flags(candidates: Option<u64>, bytes: Option<u64>) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(candidates) = candidates {
        flags.push(format!("--max-candidates {candidates}"));
    }
    if let Some(bytes) = bytes {
        flags.push(format!("--max-total-bytes {bytes}"));
    }
    flags
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_detection_is_limited_to_the_legacy_header_window() {
        assert_eq!(
            legacy_marker(b"// @generated\nfn main() {}\n"),
            Some("@generated")
        );
        let late = format!("{}// agentic-loc-exception: late\n", "line\n".repeat(40));
        assert_eq!(legacy_marker(late.as_bytes()), None);
    }
}
