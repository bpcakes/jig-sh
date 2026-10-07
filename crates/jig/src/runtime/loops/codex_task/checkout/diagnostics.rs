use std::path::Path;

use anyhow::Result;
use jig_context::RepoContext;
use serde::Serialize;

use crate::execution::NoopExecutionObserver;

use super::super::git_output;

const OBSERVATION_LIMIT: usize = 100;
const VALUE_LIMIT: usize = 512;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    ApplicationChanges,
    OperationalStateChanges,
    CheckoutUnverifiable,
}

#[derive(Serialize)]
pub(in crate::runtime::loops::codex_task) struct CheckoutDiagnostics {
    reasons: Vec<Reason>,
    observed_paths: Vec<String>,
    paths_incomplete: bool,
    inspection_commands: Vec<Vec<&'static str>>,
    recovery: &'static str,
}

impl CheckoutDiagnostics {
    pub(super) fn inspect(
        ctx: &RepoContext,
        path: &Path,
        dirty: &Result<bool>,
        final_head: &Result<String>,
    ) -> Self {
        let mut reasons = Vec::new();
        let (paths, paths_incomplete) = if matches!(dirty, Ok(true)) {
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
        Self {
            reasons,
            observed_paths: paths,
            paths_incomplete,
            inspection_commands: vec![
                vec!["git", "status", "--short"],
                vec!["scripts/jig", "loop", "status"],
            ],
            recovery: "Inspect the retained checkout and the worker output that `scripts/jig loop show` reports for this occurrence. Resolve the result before acknowledging the exact occurrence. Do not rerun a started worker or completed checks.",
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
    fn observed_paths_are_bounded() {
        let bytes = b"?? example\0".repeat(OBSERVATION_LIMIT + 1);
        let (paths, incomplete) = parse_paths(&bytes);
        assert_eq!(paths.len(), OBSERVATION_LIMIT);
        assert!(incomplete);
    }
}
