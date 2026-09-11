use super::*;

impl SourceSnapshot {
    pub(in crate::repository::freshness) fn for_action(
        &self,
        epoch: u32,
        action: &ActionSpec,
        whole_repository_token: Option<&str>,
        budget: &CollectionBudget<'_>,
    ) -> CollectionResult<SourceDigest> {
        budget.ensure_active()?;
        let mut hash = IdentityEncoder::new("jig-target-source-v1", epoch);
        let policy = action.inputs_policy.unwrap_or_default();
        if epoch >= jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION {
            hash.text(match action.source_state.unwrap_or_default() {
                ActionSourceState::Git => "git",
                ActionSourceState::Worktree => "worktree",
            });
            if action.source_state.unwrap_or_default() == ActionSourceState::Git {
                hash.optional(self.head.as_deref());
            }
        }
        hash.text(match policy {
            ActionInputsPolicy::WholeRepository => "whole_repository",
            ActionInputsPolicy::Exhaustive => "exhaustive",
        });
        let normalized = action.inputs.iter().collect::<BTreeSet<_>>();
        hash.number(normalized.len() as u64);
        for pattern in normalized {
            hash.text(pattern);
        }
        let worktree = action.source_state == Some(ActionSourceState::Worktree);
        if policy == ActionInputsPolicy::WholeRepository && !worktree {
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
            if !(policy == ActionInputsPolicy::WholeRepository
                && (problem.ignored || self.is_receipt_metadata(&problem.path)))
                && problem.applies(patterns)
            {
                return Err(problem.failure.clone());
            }
        }
        let mut paths = BTreeSet::new();
        // Every declaration has a complete precomputed count, including zero.
        // Shared declarations never rescan the whole projection per target.
        for pattern in &patterns.patterns {
            let matched = &self.matched.by_pattern[&pattern.text];
            let mut count = 0;
            for index in matched {
                budget.ensure_active()?;
                let path = &self.matched.paths[*index];
                if (!worktree || files.entries.contains_key(path))
                    && !(policy == ActionInputsPolicy::WholeRepository
                        && self.is_receipt_metadata(path))
                {
                    paths.insert(*index);
                    count += 1;
                }
            }
            hash.number(count);
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
            for projection in [&git.committed, &git.index]
                .into_iter()
                .filter(|_| !worktree)
            {
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
}
