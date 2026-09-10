use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use cap_std::fs::Dir;
use globset::GlobMatcher;
use jig_contract::freshness::{
    FreshnessReasonCode, MAX_FRESHNESS_DIAGNOSTIC_BYTES, MAX_FRESHNESS_REASON_PREVIEWS,
    SourceIdentityPreview,
};
use jig_contract::{ActionInputsPolicy, ActionSpec, TargetId};

use super::{CollectionBudget, CollectionFailure, CollectionResult, encoding::IdentityEncoder};
use crate::context::RepoContext;

mod files;
mod git;
mod matches;
mod whole;
pub(crate) use whole::revalidate_whole_source;

pub(crate) use files::read_native_authority_bytes;
use files::{FileProjection, observable_dotenv, source_excluded};
use git::GitProjection;

#[derive(Clone)]
pub(super) struct InputPatterns {
    patterns: Vec<InputPattern>,
}

#[derive(Clone)]
struct InputPattern {
    text: String,
    matcher: GlobMatcher,
    prefix: String,
    literal: bool,
    max_depth: Option<usize>,
}

impl InputPatterns {
    pub(super) fn new(
        actions: &[&ActionSpec],
        budget: &CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        let mut patterns = Vec::new();
        let mut seen = BTreeSet::new();
        for action in actions
            .iter()
            .filter(|action| action.inputs_policy == Some(ActionInputsPolicy::Exhaustive))
        {
            for input in &action.inputs {
                budget.ensure_active()?;
                if !seen.insert(input.clone()) {
                    continue;
                }
                let matcher = super::super::affected::compile_input(&action.target, input)
                    .map_err(|_| {
                        CollectionFailure::new(
                            FreshnessReasonCode::UnsupportedAuthority,
                            "input pattern is invalid",
                        )
                    })?;
                let literal = !input.contains(['*', '?', '[', '{']);
                let prefix = if literal {
                    input.clone()
                } else {
                    let special = input
                        .find(['*', '?', '[', '{'])
                        .expect("pattern contains a glob token");
                    input[..special]
                        .rsplit_once('/')
                        .map_or(String::new(), |(parent, _)| parent.to_owned())
                };
                patterns.push(InputPattern {
                    text: input.clone(),
                    matcher,
                    prefix,
                    literal,
                    max_depth: (!input.contains("**")).then(|| input.split('/').count()),
                });
            }
        }
        patterns.sort_by(|left, right| left.text.cmp(&right.text));
        Ok(Self { patterns })
    }

    pub(super) fn matches(&self, path: &str) -> bool {
        self.patterns
            .iter()
            .any(|pattern| pattern.matcher.is_match(path))
    }

    pub(super) fn has_declared_descendant(&self, path: &str) -> bool {
        self.patterns.iter().any(|pattern| {
            pattern
                .prefix
                .strip_prefix(path)
                .is_some_and(|suffix| suffix.starts_with('/'))
        })
    }

    pub(super) fn descends(&self, path: &str) -> bool {
        self.patterns.iter().any(|pattern| pattern.descends(path))
    }

    fn matches_unsupported(&self, raw: &[u8], descendants: bool) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let candidate = std::path::Path::new(std::ffi::OsStr::from_bytes(raw));
            let lossy = String::from_utf8_lossy(raw);
            self.patterns.iter().any(|pattern| {
                pattern.matcher.is_match(candidate) || (descendants && pattern.descends(&lossy))
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (raw, descendants);
            true
        }
    }

    pub(super) fn intersects(&self, path: &str) -> bool {
        self.matches(path) || self.descends(path)
    }

    pub(super) fn observation_prefixes(&self) -> Vec<String> {
        self.patterns
            .iter()
            .filter(|pattern| !pattern.prefix.is_empty())
            .map(|pattern| {
                if pattern.literal {
                    pattern.prefix.clone()
                } else {
                    format!("{}/", pattern.prefix)
                }
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn enumeration_roots(&self) -> Vec<String> {
        if self
            .patterns
            .iter()
            .any(|pattern| pattern.prefix.is_empty())
        {
            return vec![".".into()];
        }
        // Keep every possible Git ancestor (including a gitlink or symlink
        // replaced by an ordinary worktree directory). This intentionally
        // widens Git observation to top-level input trees; it never widens the
        // source entries hashed for an action.
        self.patterns
            .iter()
            .map(|pattern| pattern.prefix.split('/').next().unwrap().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn for_action(&self, action: &ActionSpec) -> Self {
        let patterns = action
            .inputs
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|input| {
                self.patterns
                    .binary_search_by(|pattern| pattern.text.cmp(input))
                    .ok()
            })
            .map(|index| self.patterns[index].clone())
            .collect();
        Self { patterns }
    }
}

impl InputPattern {
    fn descends(&self, path: &str) -> bool {
        if self
            .max_depth
            .is_some_and(|maximum| path.split('/').count() >= maximum)
        {
            return false;
        }
        if self.literal {
            self.text
                .strip_prefix(path)
                .is_some_and(|suffix| suffix.starts_with('/'))
        } else {
            self.prefix.is_empty()
                || self.prefix == path
                || self
                    .prefix
                    .strip_prefix(path)
                    .is_some_and(|suffix| suffix.starts_with('/'))
                || path
                    .strip_prefix(&self.prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }
    }
}

pub(super) struct SourceProblem {
    path: String,
    raw_path: Option<Vec<u8>>,
    descendants: bool,
    failure: CollectionFailure,
}

impl SourceProblem {
    fn unobservable(path: &str, descendants: bool, message: &str) -> Self {
        Self {
            path: path.into(),
            raw_path: None,
            descendants,
            failure: CollectionFailure::new(FreshnessReasonCode::UnobservableInput, message)
                .at(path),
        }
    }

    fn unsupported_path(raw: &[u8]) -> Self {
        let raw = raw.strip_suffix(b"/").unwrap_or(raw);
        // Scope matching uses exact Unix path bytes. Only the safely printable
        // ancestor enters diagnostics; malformed bytes never become authority.
        let mut components = Vec::new();
        for component in raw.split(|byte| *byte == b'/') {
            let Ok(value) = std::str::from_utf8(component) else {
                break;
            };
            if value.contains('\\') || value.chars().any(char::is_control) || value.len() > 255 {
                break;
            }
            components.push(value);
        }
        let preview = components.join("/");
        Self {
            path: preview.clone(),
            raw_path: Some(raw.to_vec()),
            descendants: true,
            failure: CollectionFailure::new(
                FreshnessReasonCode::UnobservableInput,
                "an input has unsupported path encoding or text beneath this directory",
            )
            .at(&preview),
        }
    }

    fn applies(&self, patterns: &InputPatterns) -> bool {
        if let Some(raw) = &self.raw_path {
            return patterns.matches_unsupported(raw, self.descendants);
        }
        patterns.matches(&self.path)
            || patterns.has_declared_descendant(&self.path)
            || (self.descendants && patterns.intersects(&self.path))
    }
}

pub(super) struct SourceDigest {
    pub(super) digest: String,
    pub(super) preview: Vec<SourceIdentityPreview>,
    pub(super) count: u64,
    pub(super) truncated: bool,
}

pub(super) struct SourceSnapshot {
    root: Dir,
    configuration: Vec<String>,
    patterns: InputPatterns,
    action_patterns: BTreeMap<TargetId, InputPatterns>,
    git: Option<GitProjection>,
    files: Option<FileProjection>,
    problems: Vec<SourceProblem>,
    directories: files::DirectoryAuthority,
    execution_runners: Option<files::RunnerFileAuthority>,
    matched: matches::PatternMatches,
}

mod execution;
pub(crate) use execution::ExecutionAuthorityGuard;

impl SourceSnapshot {
    pub(super) fn capture(
        ctx: &RepoContext,
        actions: &[&ActionSpec],
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        budget.ensure_active()?;
        let root = files::open_root(ctx.root())?;
        let configuration = files::configuration(&root, budget)?;
        // Match bounded observations to the bytes from which this context was
        // parsed. Reloading here would enter an unbounded parser and a separate
        // session-path Git subprocess within the collection deadline.
        if configuration.as_slice() != ctx.configuration_content_digests() {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::SourceRaced,
                "repository execution configuration changed before collection",
            ));
        }
        let patterns = InputPatterns::new(actions, budget)?;
        let action_patterns = actions
            .iter()
            .filter(|action| action.inputs_policy == Some(ActionInputsPolicy::Exhaustive))
            .map(|action| (action.target.clone(), patterns.for_action(action)))
            .collect();
        let mut problems = Vec::new();
        let (git, files) = if patterns.patterns.is_empty() {
            (None, None)
        } else {
            if !cfg!(unix) {
                return Err(CollectionFailure::new(
                    FreshnessReasonCode::UnsupportedAuthority,
                    "scoped source identity requires supported filesystem identity metadata",
                ));
            }
            let git = GitProjection::capture(ctx.root(), &patterns, budget)?;
            let gitlinks = git
                .committed
                .iter()
                .chain(&git.index)
                .filter(|(_, entry)| entry.mode == "160000")
                .map(|(path, _)| path.clone())
                .collect();
            for (path, entry) in git.committed.iter().chain(&git.index) {
                if !source_excluded(path) && !matches!(entry.mode.as_str(), "100644" | "100755") {
                    problems.push(SourceProblem::unobservable(
                        path,
                        true,
                        "relevant Git entry has unsupported symlink, submodule, or index authority",
                    ));
                }
            }
            for path in &git.ignored {
                if source_excluded(path) || observable_dotenv(path, &git.ignored) {
                    continue;
                }
                let descendants = root
                    .symlink_metadata(path)
                    .map_or(true, |metadata| metadata.is_dir());
                problems.push(SourceProblem::unobservable(
                    path,
                    descendants,
                    "required input is ignored and unobservable",
                ));
            }
            let started = Instant::now();
            let files = FileProjection::capture(&root, &patterns, &git.ignored, &gitlinks, budget)?;
            budget.stats.worktree_us += started.elapsed().as_micros() as u64;
            (Some(git), Some(files))
        };
        let matching_started = Instant::now();
        let matched = match (&git, &files) {
            (Some(git), Some(files)) => {
                matches::PatternMatches::collect(&patterns, git, files, budget)?
            }
            _ => matches::PatternMatches::default(),
        };
        budget.stats.matching_us += matching_started.elapsed().as_micros() as u64;
        Ok(Self {
            root,
            configuration,
            patterns,
            action_patterns,
            git,
            files,
            problems,
            directories: files::DirectoryAuthority::default(),
            execution_runners: None,
            matched,
        })
    }

    pub(super) fn for_action(
        &self,
        epoch: u32,
        action: &ActionSpec,
        whole_repository_token: Option<&str>,
        budget: &CollectionBudget<'_>,
    ) -> CollectionResult<SourceDigest> {
        budget.ensure_active()?;
        let mut hash = IdentityEncoder::new("jig-target-source-v1", epoch);
        let policy = action.inputs_policy.unwrap_or_default();
        hash.text(match policy {
            ActionInputsPolicy::WholeRepository => "whole_repository",
            ActionInputsPolicy::Exhaustive => "exhaustive",
        });
        let normalized = action.inputs.iter().collect::<BTreeSet<_>>();
        hash.number(normalized.len() as u64);
        for pattern in normalized {
            hash.text(pattern);
        }
        if policy == ActionInputsPolicy::WholeRepository {
            let whole_repository_token = whole_repository_token.ok_or_else(|| {
                CollectionFailure::new(
                    FreshnessReasonCode::CollectionFailed,
                    "whole-repository source authority could not be collected",
                )
            })?;
            hash.text(whole_repository_token);
            return Ok(SourceDigest {
                digest: hash.finish(),
                preview: Vec::new(),
                count: 0,
                truncated: false,
            });
        }
        let patterns = self
            .action_patterns
            .get(&action.target)
            .expect("exhaustive action patterns exist");
        let git = self
            .git
            .as_ref()
            .expect("exhaustive source projection exists");
        let files = self
            .files
            .as_ref()
            .expect("exhaustive worktree projection exists");
        for problem in self
            .problems
            .iter()
            .chain(&git.problems)
            .chain(&files.problems)
        {
            if problem.applies(patterns) {
                return Err(problem.failure.clone());
            }
        }
        let mut paths = BTreeSet::new();
        // Every declaration has a complete precomputed count, including zero.
        // Shared declarations never rescan the whole projection per target.
        for pattern in &patterns.patterns {
            let matched = &self.matched.by_pattern[&pattern.text];
            hash.number(matched.len() as u64);
            for index in matched {
                budget.ensure_active()?;
                paths.insert(*index);
            }
        }
        hash.number(paths.len() as u64);
        let mut preview = Vec::new();
        let mut preview_bytes = 2; // enclosing JSON array
        let mut preview_exhausted = false;
        for index in &paths {
            budget.ensure_active()?;
            let path = &self.matched.paths[*index];
            let mut entry = IdentityEncoder::new("jig-target-source-v1", epoch);
            entry.text("entry-preview");
            entry.text(path);
            for projection in [&git.committed, &git.index] {
                let value = projection.get(path);
                entry.optional(value.map(|value| value.mode.as_str()));
                entry.optional(value.map(|value| value.object.as_str()));
            }
            let current = files.entries.get(path);
            entry.optional(current.map(|current| current.kind));
            if let Some(current) = current {
                entry.number(current.mode);
                entry.optional(current.digest.as_deref());
            }
            let digest = entry.finish();
            hash.text(path);
            hash.text(&digest);
            if !preview_exhausted && preview.len() < MAX_FRESHNESS_REASON_PREVIEWS {
                let item = SourceIdentityPreview {
                    path: path.clone(),
                    digest,
                };
                let bytes = serde_json::to_vec(&item)
                    .expect("string preview encodes as JSON")
                    .len()
                    + usize::from(!preview.is_empty());
                if preview_bytes + bytes <= MAX_FRESHNESS_DIAGNOSTIC_BYTES {
                    preview_bytes += bytes;
                    preview.push(item);
                } else {
                    preview_exhausted = true;
                }
            }
        }
        Ok(SourceDigest {
            digest: hash.finish(),
            truncated: preview.len() < paths.len(),
            preview,
            count: paths.len() as u64,
        })
    }

    pub(super) fn require_runner_candidate(
        &mut self,
        action: &ActionSpec,
        path: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        if action.inputs_policy != Some(ActionInputsPolicy::Exhaustive) {
            return Ok(());
        }
        budget.ensure_active()?;
        if !self
            .action_patterns
            .get(&action.target)
            .is_some_and(|patterns| patterns.matches(path))
        {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnobservableInput,
                "repository-local runner is not covered by exhaustive inputs",
            )
            .at(path));
        }
        if self
            .files
            .as_ref()
            .and_then(|files| files.entries.get(path))
            .is_some_and(|entry| entry.kind != "regular")
        {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnobservableInput,
                "repository-local runner candidate is not an observable regular file",
            )
            .at(path));
        }
        if let Some(guard) = &mut self.execution_runners {
            guard.observe(&self.root, path, self.files.as_ref(), budget)?;
        }
        Ok(())
    }

    pub(super) fn observe_working_directory(
        &mut self,
        path: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.directories.observe(&self.root, path, budget)
    }

    pub(super) fn revalidate(
        &self,
        ctx: &RepoContext,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.revalidate_with_execution_policy(ctx, budget, false)
    }

    pub(super) fn revalidate_execution(
        &self,
        ctx: &RepoContext,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.revalidate_with_execution_policy(ctx, budget, true)
    }

    fn revalidate_with_execution_policy(
        &self,
        ctx: &RepoContext,
        budget: &mut CollectionBudget<'_>,
        execution: bool,
    ) -> CollectionResult<()> {
        files::same_directory(&self.root, &files::open_root(ctx.root())?)?;
        if let Some(git) = &self.git {
            git.revalidate(ctx.root(), &self.patterns, budget)?;
        }
        let paths_started = Instant::now();
        if let Some(files) = &self.files {
            if execution {
                files.revalidate_execution(&self.root, budget)?;
            } else {
                files.revalidate(&self.root, budget)?;
            }
        }
        if execution {
            self.directories.revalidate_identity(&self.root, budget)?;
        } else {
            self.directories.revalidate(&self.root, budget)?;
        }
        if self.configuration != files::configuration(&self.root, budget)? {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::SourceRaced,
                "repository configuration changed during freshness collection",
            ));
        }
        budget.stats.path_revalidation_us += paths_started.elapsed().as_micros() as u64;
        budget.ensure_active()
    }
}
