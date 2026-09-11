use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use jig_contract::freshness::FreshnessReasonCode;

use super::super::{CollectionBudget, CollectionFailure, CollectionResult};
use super::{InputPatterns, SourceProblem};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GitEntry {
    pub(super) mode: String,
    pub(super) object: String,
}

pub(super) struct GitProjection {
    pub(super) committed: BTreeMap<String, GitEntry>,
    pub(super) index: BTreeMap<String, GitEntry>,
    pub(super) ignored: BTreeSet<String>,
    pub(super) problems: Vec<SourceProblem>,
    raw: Vec<u8>,
    allow_unborn: bool,
    entries: u64,
}

impl GitProjection {
    pub(super) fn capture(
        root: &Path,
        patterns: &InputPatterns,
        allow_unborn: bool,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        let started = Instant::now();
        let raw = git(root, patterns, allow_unborn, budget)?;
        budget.stats.git_us += started.elapsed().as_micros() as u64;
        let before_entries = budget.stats.discovered_entries;
        let mut problems = Vec::new();
        let mut blocks = Blocks(&raw);
        let format = blocks.scalar("format")?;
        if !matches!(format, "sha1" | "sha256") {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnsupportedAuthority,
                "Git object format is unsupported",
            ));
        }
        let head = blocks.scalar("head")?;
        if !(allow_unborn && head == "unborn") {
            validate_oid(head, format)?;
        }
        let committed_started = Instant::now();
        let raw_tree = blocks.take("tree")?;
        let committed = parse_entries(raw_tree, false, format, budget, &mut problems)?;
        budget.stats.committed_us += committed_started.elapsed().as_micros() as u64;
        let index_started = Instant::now();
        let raw_index = blocks.take("index")?;
        let mut index = parse_entries(raw_index, true, format, budget, &mut problems)?;
        let raw_visible = blocks.take("ita_visible")?;
        let raw_invisible = blocks.take("ita_invisible")?;
        let visible = parse_paths(raw_visible, budget, &mut problems)?;
        let invisible = parse_paths(raw_invisible, budget, &mut problems)?;
        let intent_to_add = visible
            .difference(&invisible)
            .cloned()
            .collect::<BTreeSet<_>>();
        for path in &intent_to_add {
            let entry = index.get_mut(path).ok_or_else(failed)?;
            entry.mode = "intent-to-add".into();
        }
        budget.stats.index_us += index_started.elapsed().as_micros() as u64;
        // Git reports ignored trees at their boundary without descending into
        // every generated file. Prefix queries also detect missing declarations.
        let raw_ignored = blocks.take("ignored")?;
        let mut ignored = parse_paths(raw_ignored, budget, &mut problems)?;
        let raw_ignored_declarations = blocks.take("declarations")?;
        ignored.extend(parse_paths(
            raw_ignored_declarations,
            budget,
            &mut problems,
        )?);
        if !blocks.0.is_empty() {
            return Err(failed());
        }
        Ok(Self {
            committed,
            index,
            ignored,
            problems,
            raw,
            allow_unborn,
            entries: budget.stats.discovered_entries - before_entries,
        })
    }
    pub(super) fn revalidate(
        &self,
        root: &Path,
        patterns: &InputPatterns,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        let started = Instant::now();
        let current = git(root, patterns, self.allow_unborn, budget)?;
        budget.stats.git_us += started.elapsed().as_micros() as u64;
        // The complete validated protocol is re-observed byte for byte. If it
        // matches, parsing its maps again adds no proof. Charge the same visited
        // records for this repeated observation, without allocating duplicate maps.
        if current != self.raw {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::SourceRaced,
                "Git source projections changed during freshness collection",
            ));
        }
        budget.entries(self.entries)
    }
}

struct Blocks<'a>(&'a [u8]);

impl<'a> Blocks<'a> {
    fn take(&mut self, expected: &str) -> CollectionResult<&'a [u8]> {
        let name_end = self
            .0
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(failed)?;
        if &self.0[..name_end] != expected.as_bytes() {
            return Err(failed());
        }
        self.0 = &self.0[name_end + 1..];
        let content = self.0;
        let mut length = 0;
        loop {
            let end = self
                .0
                .iter()
                .position(|byte| *byte == 0)
                .ok_or_else(failed)?;
            self.0 = &self.0[end + 1..];
            if end == 0 {
                return Ok(&content[..length]);
            }
            length += end + 1;
        }
    }

    fn scalar(&mut self, name: &str) -> CollectionResult<&'a str> {
        let raw = self.take(name)?;
        let value = std::str::from_utf8(raw)
            .map_err(|_| failed())?
            .strip_suffix("\n\0")
            .ok_or_else(failed)?;
        if value.is_empty() || value.contains(['\0', '\n', '\r']) {
            return Err(failed());
        }
        Ok(value)
    }
}

fn parse_paths(
    raw: &[u8],
    budget: &mut CollectionBudget<'_>,
    problems: &mut Vec<SourceProblem>,
) -> CollectionResult<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for record in nul_records(raw)? {
        budget.entries(1)?;
        if let Some(path) = observed_path(record, problems)? {
            paths.insert(path.trim_end_matches('/').to_owned());
        }
    }
    Ok(paths)
}

fn validate_oid(oid: &str, format: &str) -> CollectionResult<()> {
    if oid.len() != if format == "sha1" { 40 } else { 64 }
        || !oid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || oid.bytes().all(|byte| byte == b'0')
    {
        return Err(failed());
    }
    Ok(())
}

fn parse_entries(
    raw: &[u8],
    index: bool,
    format: &str,
    budget: &mut CollectionBudget<'_>,
    problems: &mut Vec<SourceProblem>,
) -> CollectionResult<BTreeMap<String, GitEntry>> {
    let mut entries = BTreeMap::new();
    for record in nul_records(raw)? {
        budget.entries(1)?;
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(failed)?;
        let (tag, header) = if index {
            if tab < 2 || record[1] != b' ' || !matches!(record[0], b'H' | b'S' | b'M') {
                return Err(failed());
            }
            (Some(record[0]), &record[2..tab])
        } else {
            (None, &record[..tab])
        };
        let header = std::str::from_utf8(header).map_err(|_| failed())?;
        let fields = header.split(' ').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(failed());
        }
        let (mode, oid, kind) = if index {
            (fields[0], fields[1], fields[2])
        } else {
            (fields[0], fields[2], fields[1])
        };
        validate_oid(oid, format)?;
        if !matches!(mode, "100644" | "100755" | "120000" | "160000")
            || (index && !matches!(kind, "0" | "1" | "2" | "3"))
            || (!index && kind != if mode == "160000" { "commit" } else { "blob" })
        {
            return Err(failed());
        }
        // Preserve a marker so unrelated incomplete index entries do not
        // prevent an independent exhaustive target from proving its inputs.
        let mode = if index && kind != "0" {
            "unmerged"
        } else if tag == Some(b'S') {
            "skip-worktree"
        } else {
            mode
        };
        let Some(name) = observed_path(&record[tab + 1..], problems)? else {
            continue;
        };
        let entry = GitEntry {
            mode: mode.into(),
            object: format!("{format}:{oid}"),
        };
        if let Some(previous) = entries.insert(name.clone(), entry)
            && previous.mode != "unmerged"
        {
            return Err(failed());
        }
    }
    Ok(entries)
}

fn observed_path(
    raw: &[u8],
    problems: &mut Vec<SourceProblem>,
) -> CollectionResult<Option<String>> {
    match path(raw) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.reason.code == FreshnessReasonCode::UnobservableInput => {
            problems.push(SourceProblem::unsupported_path(raw));
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn path(bytes: &[u8]) -> CollectionResult<String> {
    let normalized = bytes.strip_suffix(b"/").unwrap_or(bytes);
    if normalized
        .split(|byte| *byte == b'/')
        .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(failed());
    }
    let value = std::str::from_utf8(bytes).map_err(|_| {
        CollectionFailure::new(
            FreshnessReasonCode::UnobservableInput,
            "source path encoding is unsupported",
        )
    })?;
    let trimmed = value.trim_end_matches('/');
    if value.len() > 4_096
        || value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || trimmed
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(CollectionFailure::new(
            FreshnessReasonCode::UnobservableInput,
            "source path is not supported repository-relative text",
        ));
    }
    Ok(value.to_owned())
}

fn nul_records(raw: &[u8]) -> CollectionResult<impl Iterator<Item = &[u8]>> {
    if !raw.is_empty() && !raw.ends_with(&[0]) {
        return Err(failed());
    }
    Ok(raw
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty()))
}

fn git(
    root: &Path,
    patterns: &InputPatterns,
    allow_unborn: bool,
    budget: &CollectionBudget<'_>,
) -> CollectionResult<Vec<u8>> {
    let mut command = std::process::Command::new("bash");
    let roots = patterns.enumeration_roots();
    command
        .current_dir(root)
        .args([
            "--noprofile",
            "--norc",
            "-c",
            include_str!("git.sh"),
            "jig-freshness-git",
        ])
        .arg(if allow_unborn { "1" } else { "0" })
        .arg(roots.len().to_string())
        .args(roots)
        .args(patterns.observation_prefixes());
    crate::shell::sanitize_bash_environment(&mut command);
    let result = crate::git_receipts::read_freshness_git_batch(
        root,
        &mut command,
        budget.limits.git_output,
        budget.remaining()?,
        &|| budget.stopped(),
    );
    budget.ensure_active()?;
    result.map_err(|error| {
        if matches!(
            error.downcast_ref::<jig_owned_process::OwnedProcessTreeError>(),
            Some(jig_owned_process::OwnedProcessTreeError::OutputLimitExceeded(_)),
        ) {
            CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness Git enumeration exceeded its output limit",
            )
        } else if let Some(detail) =
            error.downcast_ref::<crate::git_receipts::FreshnessGitObservationFailure>()
        {
            CollectionFailure::new(FreshnessReasonCode::CollectionFailed, &detail.0)
        } else {
            failed()
        }
    })
}

// Retain HEAD and branch identity for Git-sensitive epoch-10 actions even
// when their declared file scope is unchanged by a commit or branch switch.
pub(super) fn head_authority(
    root: &Path,
    budget: &CollectionBudget<'_>,
) -> CollectionResult<String> {
    let mut command = std::process::Command::new("bash");
    command.current_dir(root).args([
        "--noprofile",
        "--norc",
        "-c",
        include_str!("head.sh"),
        "jig-freshness-head",
    ]);
    crate::shell::sanitize_bash_environment(&mut command);
    let result = crate::git_receipts::read_freshness_git_batch(
        root,
        &mut command,
        8192,
        budget.remaining()?,
        &|| budget.stopped(),
    );
    budget.ensure_active()?;
    String::from_utf8(result.map_err(|_| failed())?).map_err(|_| failed())
}

fn failed() -> CollectionFailure {
    CollectionFailure::new(
        FreshnessReasonCode::CollectionFailed,
        "Git source enumeration failed or returned incomplete authority; verify repository HEAD, index, and Git diagnostics before retrying",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::freshness::CollectionLimits;
    use std::time::Duration;

    #[test]
    fn batch_protocol_rejects_truncation_duplicate_scalars_and_wrong_blocks() {
        for raw in [
            b"format\0sha1\n\0".as_slice(),
            b"format\0sha1\n\0extra\0\0",
            b"head\0sha1\n\0\0",
        ] {
            assert!(Blocks(raw).scalar("format").is_err());
        }
        assert_eq!(
            Blocks(b"format\0sha1\n\0\0").scalar("format").unwrap(),
            "sha1"
        );
        assert!(Blocks(b"tree\0unfinished").take("tree").is_err());
    }

    #[test]
    fn malformed_modes_objects_and_index_stages_cannot_become_authority() {
        for (index, header) in [
            (false, "100644 commit"),
            (false, "040000 tree"),
            (false, "invalid blob"),
        ] {
            let raw = format!("{header} {}\tfile\0", "1".repeat(40));
            let mut budget = CollectionBudget::new(
                CollectionLimits::with_timeout(Duration::from_secs(1)),
                &|| false,
            );
            assert!(
                parse_entries(raw.as_bytes(), index, "sha1", &mut budget, &mut Vec::new()).is_err()
            );
        }
        for (tag, oid, stage) in [
            ("H", "0".repeat(40), "0"),
            ("H", "z".repeat(40), "0"),
            ("H", "1".repeat(40), "4"),
            ("X", "1".repeat(40), "0"),
        ] {
            let raw = format!("{tag} 100644 {oid} {stage}\tfile\0");
            let mut budget = CollectionBudget::new(
                CollectionLimits::with_timeout(Duration::from_secs(1)),
                &|| false,
            );
            assert!(
                parse_entries(raw.as_bytes(), true, "sha1", &mut budget, &mut Vec::new()).is_err()
            );
        }
    }
}
