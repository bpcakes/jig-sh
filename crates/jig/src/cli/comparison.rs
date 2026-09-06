use anyhow::{Result, bail};
use clap::ValueEnum;
use jig_contract::{ComparisonRequestV1, ExactTreeProvenanceV1, StrictInventoryReasonV1};

/// Provenance a caller may supply explicitly, excluding runtime-only authorities.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum CliExactTreeProvenance {
    #[value(name = "explicit")]
    Explicit,
    #[value(name = "push_before")]
    PushBefore,
}

pub(super) fn comparison_request(
    base: Option<&str>,
    exact_tree: Option<&str>,
    provenance: Option<CliExactTreeProvenance>,
    staged: bool,
    strict_inventory: bool,
    flag_prefix: &str,
) -> Result<Option<ComparisonRequestV1>> {
    let selector_count = usize::from(base.is_some())
        + usize::from(exact_tree.is_some())
        + usize::from(staged)
        + usize::from(strict_inventory);
    if selector_count > 1 {
        bail!(
            "--{flag_prefix}base, --{flag_prefix}exact-tree, --{flag_prefix}staged, and --{flag_prefix}strict-inventory are mutually exclusive"
        );
    }
    if exact_tree.is_some() != provenance.is_some() {
        bail!("--{flag_prefix}exact-tree and --{flag_prefix}provenance must be supplied together");
    }
    if let Some(requested_ref) = base {
        return Ok(Some(ComparisonRequestV1::MergeBaseRef {
            requested_ref: requested_ref.to_owned(),
        }));
    }
    if let (Some(requested_oid), Some(provenance)) = (exact_tree, provenance) {
        return Ok(Some(ComparisonRequestV1::ExactTree {
            requested_oid: requested_oid.to_owned(),
            provenance: match provenance {
                CliExactTreeProvenance::Explicit => ExactTreeProvenanceV1::Explicit,
                CliExactTreeProvenance::PushBefore => ExactTreeProvenanceV1::PushBefore,
            },
        }));
    }
    if staged {
        return Ok(Some(ComparisonRequestV1::IndexAgainstHead));
    }
    Ok(
        strict_inventory.then_some(ComparisonRequestV1::StrictInventory {
            reason: StrictInventoryReasonV1::ExplicitCheck,
        }),
    )
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{check::CheckComparisonOpts, file_budget::FileBudgetComparisonOpts};

    #[derive(Parser)]
    struct Check {
        #[command(flatten)]
        comparison: CheckComparisonOpts,
    }

    #[derive(Parser)]
    struct Direct {
        #[command(flatten)]
        comparison: FileBudgetComparisonOpts,
    }

    #[test]
    fn comparison_entrypoints_preserve_each_authority() {
        let cases = [
            (vec![], None),
            (
                vec!["--base", "main"],
                Some(ComparisonRequestV1::MergeBaseRef {
                    requested_ref: "main".into(),
                }),
            ),
            (
                vec!["--exact-tree", "abcd", "--provenance", "explicit"],
                Some(ComparisonRequestV1::ExactTree {
                    requested_oid: "abcd".into(),
                    provenance: ExactTreeProvenanceV1::Explicit,
                }),
            ),
            (
                vec!["--exact-tree", "abcd", "--provenance", "push_before"],
                Some(ComparisonRequestV1::ExactTree {
                    requested_oid: "abcd".into(),
                    provenance: ExactTreeProvenanceV1::PushBefore,
                }),
            ),
            (
                vec!["--staged"],
                Some(ComparisonRequestV1::IndexAgainstHead),
            ),
            (
                vec!["--strict-inventory"],
                Some(ComparisonRequestV1::StrictInventory {
                    reason: StrictInventoryReasonV1::ExplicitCheck,
                }),
            ),
        ];
        for (flags, expected) in cases {
            let direct =
                Direct::try_parse_from(std::iter::once("jig").chain(flags.iter().copied()))
                    .unwrap();
            assert_eq!(direct.comparison.request().unwrap(), expected);
            let check = Check::try_parse_from(check_args(&flags)).unwrap();
            assert_eq!(check.comparison.request().unwrap(), expected);
        }
    }

    #[test]
    fn comparison_entrypoints_reject_conflicts_and_internal_provenance() {
        for flags in [
            vec!["--base", "main", "--staged"],
            vec!["--strict-inventory", "--staged"],
            vec!["--exact-tree", "abcd"],
            vec!["--provenance", "explicit"],
            vec!["--exact-tree", "abcd", "--provenance", "work_plan"],
            vec!["--exact-tree", "abcd", "--provenance", "unborn_worktree"],
        ] {
            assert!(
                Direct::try_parse_from(std::iter::once("jig").chain(flags.iter().copied()))
                    .is_err()
            );
            assert!(Check::try_parse_from(check_args(&flags)).is_err());
        }
    }

    fn check_args(flags: &[&str]) -> Vec<String> {
        std::iter::once("jig".to_owned())
            .chain(flags.iter().map(|flag| {
                flag.strip_prefix("--")
                    .map_or_else(|| (*flag).to_owned(), |name| format!("--comparison-{name}"))
            }))
            .collect()
    }
}
