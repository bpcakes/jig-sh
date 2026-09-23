use super::*;

struct SourceEncoders {
    identity: IdentityEncoder,
    content: IdentityEncoder,
}

impl SourceEncoders {
    fn new(epoch: u32, action: &ActionSpec, head: Option<&str>) -> Self {
        let mut identity = IdentityEncoder::new("jig-target-source-v1", epoch);
        if epoch >= jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION {
            identity.text(match action.source_state.unwrap_or_default() {
                ActionSourceState::Git => "git",
                ActionSourceState::Worktree => "worktree",
            });
        }
        // The existing identity retains Git placement. The copy omits it and
        // is diagnostic only; it cannot make a Git-sensitive receipt fresh.
        let content = identity.clone();
        if epoch >= jig_contract::freshness::WORKTREE_FRESHNESS_CONTRACT_VERSION
            && action.source_state.unwrap_or_default() == ActionSourceState::Git
        {
            identity.optional(head);
        }
        Self { identity, content }
    }

    fn text(&mut self, value: &str) {
        self.identity.text(value);
        self.content.text(value);
    }

    fn number(&mut self, value: u64) {
        self.identity.number(value);
        self.content.number(value);
    }

    fn finish(self) -> (String, String) {
        (self.identity.finish(), self.content.finish())
    }
}

impl SourceSnapshot {
    pub(in crate::repository::freshness) fn for_action(
        &self,
        epoch: u32,
        action: &ActionSpec,
        whole_repository_token: Option<&str>,
        budget: &CollectionBudget<'_>,
    ) -> CollectionResult<SourceDigest> {
        budget.ensure_active()?;
        let mut hashes = SourceEncoders::new(epoch, action, self.head.as_deref());
        let policy = action.inputs_policy.unwrap_or_default();
        hashes.text(match policy {
            ActionInputsPolicy::WholeRepository => "whole_repository",
            ActionInputsPolicy::Exhaustive => "exhaustive",
        });
        let normalized = action.inputs.iter().collect::<BTreeSet<_>>();
        hashes.number(normalized.len() as u64);
        for pattern in normalized {
            hashes.text(pattern);
        }
        let worktree = action.source_state == Some(ActionSourceState::Worktree);
        if policy == ActionInputsPolicy::WholeRepository && !worktree {
            let whole_repository_token = whole_repository_token.ok_or_else(|| {
                CollectionFailure::new(
                    FreshnessReasonCode::CollectionFailed,
                    "whole-repository source authority could not be collected",
                )
            })?;
            hashes.text(whole_repository_token);
            let (digest, content_digest) = hashes.finish();
            return Ok(SourceDigest {
                digest,
                content_digest,
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
            hashes.number(count);
        }
        hashes.number(paths.len() as u64);
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
            hashes.text(path);
            hashes.text(&digest);
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
        let (digest, content_digest) = hashes.finish();
        Ok(SourceDigest {
            digest,
            content_digest,
            truncated: preview.len() < paths.len(),
            preview,
            count: paths.len() as u64,
        })
    }
}
