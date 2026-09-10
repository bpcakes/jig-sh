use super::*;

/// Reuse complete match sets for shared declarations. Indices avoid copying a
/// source path into every matching pattern; each membership is budgeted.
#[derive(Default)]
pub(super) struct PatternMatches {
    pub(super) paths: Vec<String>,
    pub(super) by_pattern: BTreeMap<String, Vec<usize>>,
}

impl PatternMatches {
    pub(super) fn collect(
        patterns: &InputPatterns,
        git: &GitProjection,
        files: &FileProjection,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<Self> {
        let mut paths = BTreeSet::new();
        for path in git
            .committed
            .keys()
            .chain(git.index.keys())
            .chain(files.entries.keys())
        {
            budget.entries(1)?;
            if !source_excluded(path) {
                paths.insert(path.clone());
            }
        }
        let paths = paths.into_iter().collect::<Vec<_>>();
        let mut by_pattern = BTreeMap::new();
        for pattern in &patterns.patterns {
            let mut matched = Vec::new();
            for (index, path) in paths.iter().enumerate() {
                if index % 64 == 0 {
                    budget.ensure_active()?;
                }
                if pattern.matcher.is_match(path) {
                    budget.entries(1)?;
                    matched.push(index);
                }
            }
            by_pattern.insert(pattern.text.clone(), matched);
        }
        Ok(Self { paths, by_pattern })
    }
}
