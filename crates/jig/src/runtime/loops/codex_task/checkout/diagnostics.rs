use std::fmt;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::context::RepoContext;
use crate::execution::NoopExecutionObserver;

use super::super::{WORKER_RECEIPT_EXCLUDE, WORKER_RECEIPT_PATH, git_output};

const OBSERVATION_LIMIT: usize = 100;
const VALUE_LIMIT: usize = 512;

#[derive(Default, Debug)]
pub(super) struct JournalFailure {
    pub(super) message: String,
    pub(super) ambiguous: bool,
    ids: Vec<String>,
    truncated: bool,
}

impl JournalFailure {
    pub(super) fn observe(&mut self, id: &str) {
        // Never present a shortened identifier as an exact receipt identity.
        if self.ids.len() == OBSERVATION_LIMIT || id.len() > VALUE_LIMIT {
            self.truncated = true;
        } else {
            self.ids.push(id.to_owned());
        }
    }
}

impl fmt::Display for JournalFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    ApplicationChanges,
    OperationalStateChanges,
    CheckoutUnverifiable,
    ReceiptAmbiguity,
    JournalUnverifiable,
}

#[derive(Serialize)]
pub(in crate::runtime::loops::codex_task) struct CheckoutDiagnostics {
    reasons: Vec<Reason>,
    observed_paths: Vec<String>,
    paths_incomplete: bool,
    parent_receipt_id: Option<String>,
    observed_receipt_ids: Vec<String>,
    receipt_ids_incomplete: bool,
    inspection_commands: Vec<Vec<&'static str>>,
    recovery: &'static str,
}

impl CheckoutDiagnostics {
    pub(super) fn inspect(
        ctx: &RepoContext,
        path: &Path,
        dirty: &Result<bool>,
        final_head: &Result<String>,
        receipt_append: &Result<()>,
        parent_receipt_id: Option<&str>,
    ) -> Self {
        let mut reasons = Vec::new();
        let (mut paths, paths_incomplete) = if matches!(dirty, Ok(true)) {
            observed_paths(ctx, path)
        } else {
            (Vec::new(), dirty.is_err())
        };
        if matches!(dirty, Ok(true)) {
            if paths.iter().any(|path| path.starts_with(".agent/state/")) {
                reasons.push(Reason::OperationalStateChanges);
            }
            if paths.iter().any(|path| !path.starts_with(".agent/state/")) {
                reasons.push(Reason::ApplicationChanges);
            }
        }
        if dirty.is_err() || final_head.is_err() || paths_incomplete {
            reasons.push(Reason::CheckoutUnverifiable);
        }
        let failure = receipt_append
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<JournalFailure>());
        if receipt_append.is_err() {
            reasons.push(if failure.is_some_and(|failure| failure.ambiguous) {
                Reason::ReceiptAmbiguity
            } else {
                Reason::JournalUnverifiable
            });
            paths.push(WORKER_RECEIPT_PATH.into());
        }
        Self {
            reasons,
            observed_paths: paths,
            paths_incomplete,
            parent_receipt_id: parent_receipt_id.map(str::to_owned),
            observed_receipt_ids: failure
                .map(|failure| failure.ids.clone())
                .unwrap_or_else(|| {
                    if receipt_append.is_ok() {
                        parent_receipt_id.map(str::to_owned).into_iter().collect()
                    } else {
                        Vec::new()
                    }
                }),
            receipt_ids_incomplete: failure.is_some_and(|failure| failure.truncated)
                || (receipt_append.is_err() && !failure.is_some_and(|failure| failure.ambiguous)),
            inspection_commands: vec![
                vec!["git", "status", "--short"],
                vec!["git", "diff", "--", ".agent/state/receipts.jsonl"],
                vec![
                    "git",
                    "diff",
                    "--cached",
                    "--",
                    ".agent/state/receipts.jsonl",
                ],
                vec!["scripts/jig", "loop", "status"],
            ],
            recovery: "Inspect the retained checkout, original worker output, and receipt identities. Preserve journal history and plan evidence. Resolve the result before acknowledging the exact occurrence. Do not rerun a started worker or completed checks. Run nested plan-linked validation outside the repo-mode worker or in an isolated task; never remove its plan ID to suppress evidence.",
        }
    }
}

fn observed_paths(ctx: &RepoContext, path: &Path) -> (Vec<String>, bool) {
    // Diagnostic-only observation: the original status verdict stays authoritative.
    let Ok(output) = git_output(
        ctx,
        path,
        [
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
            WORKER_RECEIPT_EXCLUDE,
        ],
        &mut NoopExecutionObserver,
    ) else {
        return (Vec::new(), true);
    };
    if !output.status.success() {
        return (Vec::new(), true);
    }
    parse_paths(&output.stdout)
}

fn parse_paths(bytes: &[u8]) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut incomplete = !bytes.is_empty() && !bytes.ends_with(b"\0");
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            incomplete = true;
            continue;
        }
        let renamed = record[..2].iter().any(|byte| matches!(byte, b'R' | b'C'));
        let original = if renamed { records.next() } else { None };
        incomplete |= renamed && original.is_none();
        for path in std::iter::once(&record[3..]).chain(original) {
            match std::str::from_utf8(path) {
                Ok(path) if paths.len() < OBSERVATION_LIMIT && path.len() <= VALUE_LIMIT => {
                    paths.push(path.to_owned());
                }
                _ => incomplete = true,
            }
        }
    }
    if paths.is_empty() {
        incomplete = true;
    }
    (paths, incomplete)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_paths_preserve_spaces_newlines_and_both_rename_paths() {
        assert_eq!(
            parse_paths(b" M example file\0R  new\nname\0old name\0?? .agent/state/runs.jsonl\0"),
            (
                vec![
                    "example file".into(),
                    "new\nname".into(),
                    "old name".into(),
                    ".agent/state/runs.jsonl".into()
                ],
                false
            )
        );
        assert!(parse_paths(b"R  new\0").1);
        assert!(parse_paths(b"?? partial").1);
        assert!(parse_paths(b"?? invalid-\xff\0").1);
    }

    #[test]
    fn observations_are_bounded_without_fabricating_truncated_identities() {
        let mut failure = JournalFailure::default();
        failure.observe(&"x".repeat(VALUE_LIMIT + 1));
        for _ in 0..=OBSERVATION_LIMIT {
            failure.observe("receipt-example");
        }
        assert_eq!(failure.ids.len(), OBSERVATION_LIMIT);
        assert!(failure.truncated);
        let bytes = b"?? example\0".repeat(OBSERVATION_LIMIT + 1);
        let (paths, incomplete) = parse_paths(&bytes);
        assert_eq!(paths.len(), OBSERVATION_LIMIT);
        assert!(incomplete);
    }
}
