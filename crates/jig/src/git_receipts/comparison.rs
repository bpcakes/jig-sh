use super::*;

const MAX_COMPARISON_GIT_OUTPUT_BYTES: usize = 64 * 1024;

#[cfg(test)]
thread_local! {
    static MERGE_BASE_RESOLUTION_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[allow(dead_code, reason = "staged native file-budget exact-tree provenances")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ExactTreeProvenanceV1 {
    Explicit,
    WorkPlan,
    PushBefore,
    UnbornWorktree,
}

#[allow(dead_code, reason = "staged native file-budget inventory reasons")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum StrictInventoryReasonV1 {
    ExplicitAudit,
    ExplicitCheck,
}

#[allow(dead_code, reason = "staged native file-budget comparison requests")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ComparisonRequestV1 {
    MergeBaseRef {
        requested_ref: String,
    },
    ExactTree {
        requested_oid: String,
        provenance: ExactTreeProvenanceV1,
    },
    IndexAgainstHead,
    StrictInventory {
        reason: StrictInventoryReasonV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolvedComparisonV1 {
    MergeBase {
        requested_ref: String,
        resolved_ref_oid: String,
        head_oid: String,
        merge_base_oid: String,
    },
    ExactTree {
        requested_oid: String,
        peeled_commit_oid: Option<String>,
        tree_oid: String,
        provenance: ExactTreeProvenanceV1,
    },
    IndexAgainstHead {
        head_or_empty_oid: String,
    },
    StrictInventory {
        reason: StrictInventoryReasonV1,
    },
}

impl ResolvedComparisonV1 {
    #[allow(dead_code, reason = "staged native file-budget baseline accessor")]
    pub(crate) fn baseline_oid(&self) -> Option<&str> {
        match self {
            Self::MergeBase { merge_base_oid, .. } => Some(merge_base_oid),
            Self::ExactTree { tree_oid, .. } => Some(tree_oid),
            Self::IndexAgainstHead { head_or_empty_oid } => Some(head_or_empty_oid),
            Self::StrictInventory { .. } => None,
        }
    }
}

pub(crate) fn resolve_comparison_v1(
    root: &Path,
    request: ComparisonRequestV1,
) -> Result<ResolvedComparisonV1> {
    resolve_comparison_inner(root, request, GitReceiptCollection::Blocking)
}

#[allow(dead_code, reason = "staged cancellable native comparison API")]
pub(crate) fn resolve_comparison_v1_with_cancellation(
    root: &Path,
    request: ComparisonRequestV1,
    cancelled: &dyn Fn() -> bool,
) -> Result<ResolvedComparisonV1> {
    resolve_comparison_inner(root, request, GitReceiptCollection::Cancellable(cancelled))
}

fn resolve_comparison_inner(
    root: &Path,
    request: ComparisonRequestV1,
    collection: GitReceiptCollection<'_>,
) -> Result<ResolvedComparisonV1> {
    collection.ensure_active()?;
    match request {
        ComparisonRequestV1::MergeBaseRef { requested_ref } => {
            #[cfg(test)]
            MERGE_BASE_RESOLUTION_COUNT.set(MERGE_BASE_RESOLUTION_COUNT.get() + 1);
            let requested_ref = validate_symbolic_ref(&requested_ref)?;
            let resolved_ref_oid = resolve_commit(root, &requested_ref, collection)
                .with_context(|| format!("Failed to resolve requested ref `{requested_ref}`"))?;
            let head_oid = resolve_commit(root, "HEAD", collection)
                .context("Failed to resolve HEAD for merge-base comparison")?;
            let output = comparison_git_output(
                root,
                &[
                    "--no-replace-objects",
                    "merge-base",
                    "--all",
                    &resolved_ref_oid,
                    &head_oid,
                ],
                "git merge-base comparison",
                collection,
            )?;
            let merge_base_oid = parse_unique_merge_base(&output.stdout)?;
            Ok(ResolvedComparisonV1::MergeBase {
                requested_ref,
                resolved_ref_oid,
                head_oid,
                merge_base_oid,
            })
        }
        ComparisonRequestV1::ExactTree {
            requested_oid,
            provenance,
        } => resolve_exact_tree(root, requested_oid, provenance, collection),
        ComparisonRequestV1::IndexAgainstHead => {
            let head_or_empty_oid = match resolve_commit(root, "HEAD", collection) {
                Ok(head) => head,
                Err(head_error) => {
                    if !has_unborn_symbolic_head(root, collection)? {
                        return Err(head_error)
                            .context("Failed to resolve HEAD for index comparison");
                    }
                    resolve_empty_tree_for_comparison(root, collection)?
                }
            };
            Ok(ResolvedComparisonV1::IndexAgainstHead { head_or_empty_oid })
        }
        ComparisonRequestV1::StrictInventory { reason } => {
            Ok(ResolvedComparisonV1::StrictInventory { reason })
        }
    }
}

#[cfg(test)]
pub(super) fn comparison_merge_base_resolution_count() -> usize {
    MERGE_BASE_RESOLUTION_COUNT.get()
}

#[cfg(test)]
pub(super) fn reset_comparison_merge_base_resolution_count() {
    MERGE_BASE_RESOLUTION_COUNT.set(0);
}

fn resolve_exact_tree(
    root: &Path,
    requested_oid: String,
    provenance: ExactTreeProvenanceV1,
    collection: GitReceiptCollection<'_>,
) -> Result<ResolvedComparisonV1> {
    let oid_bytes = object_format_oid_bytes(root, collection)?;
    let requested_oid = validate_exact_oid(&requested_oid, oid_bytes)?;
    if requested_oid.bytes().all(|byte| byte == b'0') {
        if provenance != ExactTreeProvenanceV1::PushBefore {
            bail!("an all-zero exact-tree identity is valid only for push-before provenance");
        }
        return Ok(ResolvedComparisonV1::ExactTree {
            requested_oid,
            peeled_commit_oid: None,
            tree_oid: resolve_empty_tree_for_comparison(root, collection)?,
            provenance,
        });
    }

    let peeled_expression = format!("{requested_oid}^{{}}");
    let peeled = comparison_git_output(
        root,
        &[
            "--no-replace-objects",
            "rev-parse",
            "--verify",
            "--end-of-options",
            &peeled_expression,
        ],
        "git rev-parse exact object",
        collection,
    )?;
    let peeled_oid = parse_git_object_oid(&peeled.stdout, "exact object")?;
    let kind = comparison_git_output(
        root,
        &["--no-replace-objects", "cat-file", "-t", &peeled_oid],
        "git cat-file exact object type",
        collection,
    )?;
    match parse_single_line(&kind.stdout, "exact object type")? {
        "commit" => {
            let tree_expression = format!("{peeled_oid}^{{tree}}");
            let tree = comparison_git_output(
                root,
                &[
                    "--no-replace-objects",
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &tree_expression,
                ],
                "git rev-parse exact commit tree",
                collection,
            )?;
            Ok(ResolvedComparisonV1::ExactTree {
                requested_oid,
                peeled_commit_oid: Some(peeled_oid),
                tree_oid: parse_git_object_oid(&tree.stdout, "exact tree")?,
                provenance,
            })
        }
        "tree" => Ok(ResolvedComparisonV1::ExactTree {
            requested_oid,
            peeled_commit_oid: None,
            tree_oid: peeled_oid,
            provenance,
        }),
        kind => bail!("exact comparison object has unsupported Git type `{kind}`"),
    }
}

fn resolve_commit(
    root: &Path,
    reference: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    let commit_expression = format!("{reference}^{{commit}}");
    let output = comparison_git_output(
        root,
        &[
            "--no-replace-objects",
            "rev-parse",
            "--verify",
            "--end-of-options",
            &commit_expression,
        ],
        "git rev-parse comparison commit",
        collection,
    )?;
    parse_git_object_oid(&output.stdout, "comparison commit")
}

fn object_format_oid_bytes(root: &Path, collection: GitReceiptCollection<'_>) -> Result<usize> {
    let output = comparison_git_output(
        root,
        &["--no-replace-objects", "rev-parse", "--show-object-format"],
        "git rev-parse object format",
        collection,
    )?;
    match parse_single_line(&output.stdout, "Git object format")? {
        "sha1" => Ok(40),
        "sha256" => Ok(64),
        other => bail!("Git reported unsupported object format `{other}`"),
    }
}

pub(super) fn has_unborn_symbolic_head(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<bool> {
    let symbolic = comparison_git_output(
        root,
        &["--no-replace-objects", "symbolic-ref", "-q", "HEAD"],
        "git symbolic-ref unborn HEAD",
        collection,
    );
    let symbolic = match symbolic {
        Ok(output) => output,
        Err(error) if is_git_receipt_collection_cancellation(&error) => return Err(error),
        Err(_) => return Ok(false),
    };
    let target = parse_single_line(&symbolic.stdout, "symbolic HEAD target")?.to_owned();
    let refs = comparison_git_output(
        root,
        &[
            "--no-replace-objects",
            "for-each-ref",
            "--format=%(refname)",
            "--count=2",
            &target,
        ],
        "git enumerate symbolic HEAD target",
        collection,
    )?;
    let refs = std::str::from_utf8(&refs.stdout).context("Git ref names were not UTF-8")?;
    Ok(!refs.lines().any(|reference| reference == target))
}

fn resolve_empty_tree_for_comparison(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    resolve_empty_tree_oid_inner(root, collection)
}

fn parse_unique_merge_base(stdout: &[u8]) -> Result<String> {
    let value = std::str::from_utf8(stdout).context("Git merge bases were not UTF-8")?;
    let merge_bases = value
        .lines()
        .filter(|line| !line.is_empty())
        .map(|merge_base| parse_git_object_oid(merge_base.as_bytes(), "merge base"))
        .collect::<Result<Vec<_>>>()?;
    match merge_bases.as_slice() {
        [] => bail!("Git returned no merge base"),
        [merge_base] => Ok(merge_base.clone()),
        _ => {
            let preview = merge_bases
                .iter()
                .take(8)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let omitted = merge_bases.len().saturating_sub(8);
            let suffix = if omitted == 0 {
                String::new()
            } else {
                format!(", … and {omitted} more")
            };
            bail!(
                "Git returned {} equally valid merge bases ({preview}{suffix}); comparison is ambiguous. Select one of these commit IDs as the explicit comparison ref",
                merge_bases.len()
            )
        }
    }
}

fn validate_symbolic_ref(reference: &str) -> Result<String> {
    let reference = reference.trim();
    if reference.is_empty() || reference.starts_with('-') || reference.contains(['\0', '\n', '\r'])
    {
        bail!("unsupported Git comparison ref `{reference}`");
    }
    Ok(reference.to_owned())
}

fn validate_exact_oid(oid: &str, expected_bytes: usize) -> Result<String> {
    if oid.len() != expected_bytes || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(
            "exact comparison identity must be a full {expected_bytes}-character hexadecimal object ID"
        );
    }
    Ok(oid.to_ascii_lowercase())
}

fn parse_single_line<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str> {
    let value = std::str::from_utf8(bytes)
        .with_context(|| format!("{label} was not UTF-8"))?
        .trim();
    if value.is_empty() || value.contains(['\n', '\r']) {
        bail!("Git returned invalid {label}");
    }
    Ok(value)
}

fn comparison_git_output(
    root: &Path,
    args: &[&str],
    label: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Output> {
    collection.git_bounded_output(
        root,
        args,
        label,
        MAX_COMPARISON_GIT_OUTPUT_BYTES,
        "comparison",
    )
}
