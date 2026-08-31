use std::fs::{self, File};
use std::io::Read;

use anyhow::{Context, Result};
use jig_contract::{
    ComparisonPreparationFailureV1, ComparisonPreparationV1, ComparisonRequestV1, CurrentViewV1,
    FindingSeverity, MissingComparisonV1, NativeFileBudgetConfigV1, PolicyPreparationFailureV1,
    PolicyPreparationV1, PolicySourceV1, PreparedDiagnosticV1, PreparedNativeInputV1,
    ResolvedComparisonV1, StrictInventoryFallbackV1, StrictInventoryReasonV1,
};
use jig_file_budget::{
    BudgetDiagnosticV1, BudgetSeverityV1, MAX_POLICY_BYTES_V1, PolicyDateV1, parse_policy_v1,
};
use sha2::{Digest, Sha256};

use crate::context::RepoContext;
use crate::git_receipts::{
    fetch_exact_push_before_object_v1, read_index_blob_v1, resolve_comparison_v1,
};

const POLICY_PATH_V1: &str = ".jig/file-budget.toml";
const MAX_PREPARED_DIAGNOSTICS_V1: usize = 64;
const MAX_PREPARED_DIAGNOSTIC_CHARS_V1: usize = 1_024;
const MAX_SYMBOLIC_COMPARISON_REF_BYTES_V1: usize = 1_024;
const MAX_EXACT_OBJECT_ID_BYTES_V1: usize = 64;

pub(crate) fn prepare_file_budget_input_v1(
    ctx: &RepoContext,
    request: Option<ComparisonRequestV1>,
    configuration: NativeFileBudgetConfigV1,
    work_plan_id: Option<String>,
) -> Result<PreparedNativeInputV1> {
    let now = time::OffsetDateTime::now_utc().date();
    let current_date = PolicyDateV1::new(now.year() as u16, now.month() as u8, now.day())
        .map_err(anyhow::Error::msg)?;
    prepare_file_budget_input_at_v1(ctx, request, configuration, work_plan_id, current_date)
}

fn prepare_file_budget_input_at_v1(
    ctx: &RepoContext,
    request: Option<ComparisonRequestV1>,
    configuration: NativeFileBudgetConfigV1,
    work_plan_id: Option<String>,
    current_date: PolicyDateV1,
) -> Result<PreparedNativeInputV1> {
    let work_plan_id = normalize_work_plan_id(work_plan_id)?;
    let request = normalize_request(match request {
        Some(request) => request,
        None => default_comparison_request(ctx, work_plan_id.as_deref())?,
    })?;
    let view = current_view(&request);
    let policy = prepare_policy(ctx, view, current_date);
    let comparison = prepare_comparison(ctx, &request, configuration.missing_comparison);
    Ok(PreparedNativeInputV1 {
        schema_version: PreparedNativeInputV1::SCHEMA_VERSION,
        view,
        request,
        configuration,
        policy_source: PolicySourceV1 {
            path: POLICY_PATH_V1.to_owned(),
        },
        work_plan_id,
        policy,
        comparison,
    })
}

fn default_comparison_request(
    ctx: &RepoContext,
    work_plan_id: Option<&str>,
) -> Result<ComparisonRequestV1> {
    if let Some(work_plan_id) = work_plan_id {
        let baseline = crate::state::plan_baseline(ctx, work_plan_id)?
            .ok_or_else(|| anyhow::anyhow!("work plan '{work_plan_id}' does not exist"))?;
        if let Some(error) = baseline.error {
            anyhow::bail!("work plan '{work_plan_id}' has no usable comparison baseline: {error}");
        }
        let requested_oid = baseline
            .commit_oid
            .or(baseline.empty_tree_oid)
            .ok_or_else(|| {
                anyhow::anyhow!("work plan '{work_plan_id}' has no exact comparison identity")
            })?;
        return Ok(ComparisonRequestV1::ExactTree {
            requested_oid,
            provenance: jig_contract::ExactTreeProvenanceV1::WorkPlan,
        });
    }
    if crate::git_receipts::resolve_git_commit(ctx.root(), "HEAD").is_err()
        && let Ok(Some(empty_tree_oid)) =
            crate::git_receipts::resolve_empty_tree_for_unborn_repository(ctx.root())
    {
        return Ok(ComparisonRequestV1::ExactTree {
            requested_oid: empty_tree_oid,
            provenance: jig_contract::ExactTreeProvenanceV1::UnbornWorktree,
        });
    }
    Ok(ComparisonRequestV1::MergeBaseRef {
        requested_ref: ctx.default_branch().to_owned(),
    })
}

fn normalize_work_plan_id(work_plan_id: Option<String>) -> Result<Option<String>> {
    let Some(work_plan_id) = work_plan_id else {
        return Ok(None);
    };
    let normalized = work_plan_id.trim();
    anyhow::ensure!(!normalized.is_empty(), "work_plan_id must not be empty");
    anyhow::ensure!(
        normalized.len() <= 128
            && normalized
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')),
        "work_plan_id contains unsupported characters or exceeds 128 bytes"
    );
    Ok(Some(normalized.to_owned()))
}

fn normalize_request(request: ComparisonRequestV1) -> Result<ComparisonRequestV1> {
    match request {
        ComparisonRequestV1::MergeBaseRef { requested_ref } => {
            let requested_ref = requested_ref.trim();
            anyhow::ensure!(
                !requested_ref.is_empty()
                    && requested_ref.len() <= MAX_SYMBOLIC_COMPARISON_REF_BYTES_V1
                    && !requested_ref.starts_with('-')
                    && !requested_ref.contains(['\0', '\n', '\r']),
                "comparison ref is empty, unsafe, or exceeds {MAX_SYMBOLIC_COMPARISON_REF_BYTES_V1} bytes"
            );
            Ok(ComparisonRequestV1::MergeBaseRef {
                requested_ref: requested_ref.to_owned(),
            })
        }
        ComparisonRequestV1::ExactTree {
            requested_oid,
            provenance,
        } => {
            let requested_oid = requested_oid.trim();
            anyhow::ensure!(
                requested_oid.len() <= MAX_EXACT_OBJECT_ID_BYTES_V1,
                "exact comparison identity exceeds {MAX_EXACT_OBJECT_ID_BYTES_V1} bytes"
            );
            Ok(ComparisonRequestV1::ExactTree {
                requested_oid: requested_oid.to_ascii_lowercase(),
                provenance,
            })
        }
        other => Ok(other),
    }
}

const fn current_view(request: &ComparisonRequestV1) -> CurrentViewV1 {
    match request {
        ComparisonRequestV1::MergeBaseRef { .. } | ComparisonRequestV1::ExactTree { .. } => {
            CurrentViewV1::Worktree
        }
        ComparisonRequestV1::IndexAgainstHead => CurrentViewV1::Index,
        ComparisonRequestV1::StrictInventory { .. } => CurrentViewV1::Inventory,
    }
}

fn prepare_policy(
    ctx: &RepoContext,
    view: CurrentViewV1,
    current_date: PolicyDateV1,
) -> PolicyPreparationV1 {
    let bytes = match read_policy_bytes(ctx, view) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            return invalid_policy(
                None,
                PolicyPreparationFailureV1::Missing,
                vec![PreparedDiagnosticV1 {
                    severity: FindingSeverity::Error,
                    code: "file_budget.policy_invalid".into(),
                    message: format!("required file-budget policy '{POLICY_PATH_V1}' is missing"),
                    path: Some(POLICY_PATH_V1.into()),
                }],
            );
        }
        Err(message) => {
            return invalid_policy(
                None,
                PolicyPreparationFailureV1::Unreadable,
                vec![PreparedDiagnosticV1 {
                    severity: FindingSeverity::Error,
                    code: "file_budget.policy_invalid".into(),
                    message,
                    path: Some(POLICY_PATH_V1.into()),
                }],
            );
        }
    };
    match parse_policy_v1(&bytes, current_date) {
        Ok(policy) => PolicyPreparationV1::Ready {
            policy_raw_digest: format!("sha256:{}", policy.identity().raw_sha256()),
            policy_semantic_digest: format!("sha256:{}", policy.identity().semantic_sha256()),
        },
        Err(error) => invalid_policy(
            Some(format!("sha256:{}", error.raw_sha256())),
            PolicyPreparationFailureV1::Invalid,
            error
                .diagnostics()
                .iter()
                .map(prepared_diagnostic)
                .collect(),
        ),
    }
}

pub(crate) fn read_policy_bytes(
    ctx: &RepoContext,
    view: CurrentViewV1,
) -> std::result::Result<Option<Vec<u8>>, String> {
    if view == CurrentViewV1::Index {
        return match read_index_blob_v1(ctx.root(), POLICY_PATH_V1, MAX_POLICY_BYTES_V1 + 1) {
            Ok(Some(bytes)) if bytes.len() <= MAX_POLICY_BYTES_V1 => Ok(Some(bytes)),
            Ok(Some(bytes)) => Err(format!(
                "file-budget policy exceeds the {MAX_POLICY_BYTES_V1}-byte preparation limit (observed at least {} bytes)",
                bytes.len()
            )),
            Ok(None) => Ok(None),
            Err(error) => Err(bounded_message(&format!(
                "file-budget policy could not be read from the index: {}",
                redact_root(ctx, &format!("{error:#}"))
            ))),
        };
    }
    let path = ctx.root().join(POLICY_PATH_V1);
    let before = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("file-budget policy metadata could not be read".into()),
    };
    if !before.file_type().is_file() || before.file_type().is_symlink() {
        return Err("file-budget policy must be a regular file and may not be a symlink".into());
    }
    if before.len() > MAX_POLICY_BYTES_V1 as u64 {
        return Err(format!(
            "file-budget policy is {} bytes; preparation permits at most {MAX_POLICY_BYTES_V1}",
            before.len()
        ));
    }
    let mut file = File::open(&path).map_err(|_| "file-budget policy could not be opened")?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.by_ref()
        .take((MAX_POLICY_BYTES_V1 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "file-budget policy could not be read completely")?;
    let after = file
        .metadata()
        .map_err(|_| "file-budget policy identity could not be rechecked")?;
    if bytes.len() > MAX_POLICY_BYTES_V1 {
        return Err(format!(
            "file-budget policy exceeds the {MAX_POLICY_BYTES_V1}-byte preparation limit"
        ));
    }
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || !after.file_type().is_file()
    {
        return Err("file-budget policy changed while it was being prepared".into());
    }
    Ok(Some(bytes))
}

fn prepared_diagnostic(diagnostic: &BudgetDiagnosticV1) -> PreparedDiagnosticV1 {
    PreparedDiagnosticV1 {
        severity: match diagnostic.severity {
            BudgetSeverityV1::Error => FindingSeverity::Error,
            BudgetSeverityV1::Warning => FindingSeverity::Warning,
            BudgetSeverityV1::Notice => FindingSeverity::Notice,
        },
        code: diagnostic.code.as_str().into(),
        message: bounded_message(&diagnostic.message),
        path: diagnostic.path.as_deref().map(bounded_message),
    }
}

fn invalid_policy(
    policy_raw_digest: Option<String>,
    reason: PolicyPreparationFailureV1,
    diagnostics: Vec<PreparedDiagnosticV1>,
) -> PolicyPreparationV1 {
    let diagnostics_count = diagnostics.len() as u64;
    let diagnostics_digest = digest_json(
        b"jig-file-budget-preparation-diagnostics-v1\0",
        &diagnostics,
    );
    let diagnostics_preview = diagnostics
        .into_iter()
        .take(MAX_PREPARED_DIAGNOSTICS_V1)
        .collect();
    PolicyPreparationV1::InvalidPolicy {
        policy_raw_digest,
        reason,
        diagnostics_count,
        diagnostics_digest,
        diagnostics_preview,
    }
}

fn prepare_comparison(
    ctx: &RepoContext,
    request: &ComparisonRequestV1,
    missing_comparison: MissingComparisonV1,
) -> ComparisonPreparationV1 {
    match resolve_comparison_with_push_before_fetch(ctx, request) {
        Ok(comparison) => ComparisonPreparationV1::Ready { comparison },
        Err(error) => {
            let attempted_object_ids = attempted_object_ids(request);
            let code = comparison_failure_code(request).to_owned();
            let message = bounded_message(&redact_root(ctx, &format!("{error:#}")));
            let failure_digest = digest_json(
                b"jig-file-budget-comparison-failure-v1\0",
                &(request, &code, &message, &attempted_object_ids),
            );
            let failure = ComparisonPreparationFailureV1 {
                code,
                message,
                failure_digest: failure_digest.clone(),
            };
            if missing_comparison == MissingComparisonV1::StrictInventory {
                ComparisonPreparationV1::Ready {
                    comparison: ResolvedComparisonV1::StrictInventory {
                        reason: StrictInventoryReasonV1::MissingComparisonFallback,
                        fallback_from: Some(StrictInventoryFallbackV1 {
                            original_request: request.clone(),
                            failure,
                            attempted_object_ids,
                            failure_digest,
                        }),
                    },
                }
            } else {
                ComparisonPreparationV1::ComparisonUnavailable {
                    reason: failure,
                    attempted_object_ids,
                }
            }
        }
    }
}

fn resolve_comparison_with_push_before_fetch(
    ctx: &RepoContext,
    request: &ComparisonRequestV1,
) -> Result<ResolvedComparisonV1> {
    match resolve_comparison_v1(ctx.root(), request.clone()) {
        Ok(comparison) => Ok(comparison),
        Err(initial_error)
            if matches!(
                request,
                ComparisonRequestV1::ExactTree {
                    requested_oid,
                    provenance: jig_contract::ExactTreeProvenanceV1::PushBefore,
                } if !requested_oid.bytes().all(|byte| byte == b'0')
                    && matches!(requested_oid.len(), 40 | 64)
                    && requested_oid.bytes().all(|byte| byte.is_ascii_hexdigit())
            ) =>
        {
            let ComparisonRequestV1::ExactTree { requested_oid, .. } = request else {
                unreachable!("guard selected exact-tree push-before request")
            };
            fetch_exact_push_before_object_v1(ctx.root(), requested_oid).with_context(|| {
                format!(
                    "exact push-before comparison was unavailable ({initial_error:#}); its one bounded fetch attempt also failed"
                )
            })?;
            resolve_comparison_v1(ctx.root(), request.clone()).with_context(|| {
                format!(
                    "exact push-before comparison remained unavailable after one bounded fetch attempt ({initial_error:#})"
                )
            })
        }
        Err(error) => Err(error),
    }
}

fn attempted_object_ids(request: &ComparisonRequestV1) -> Vec<String> {
    match request {
        ComparisonRequestV1::ExactTree { requested_oid, .. } => vec![requested_oid.clone()],
        _ => Vec::new(),
    }
}

const fn comparison_failure_code(request: &ComparisonRequestV1) -> &'static str {
    match request {
        ComparisonRequestV1::MergeBaseRef { .. } => "merge_base_unavailable",
        ComparisonRequestV1::ExactTree { .. } => "exact_tree_unavailable",
        ComparisonRequestV1::IndexAgainstHead => "index_head_unavailable",
        ComparisonRequestV1::StrictInventory { .. } => "strict_inventory_unavailable",
    }
}

fn digest_json(domain: &[u8], value: &impl serde::Serialize) -> String {
    let encoded = serde_json::to_vec(value).expect("bounded preparation facts are serializable");
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    format!("sha256:{:x}", hasher.finalize())
}

fn bounded_message(message: &str) -> String {
    let mut bounded = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_PREPARED_DIAGNOSTIC_CHARS_V1)
        .collect::<String>();
    if message.chars().count() > MAX_PREPARED_DIAGNOSTIC_CHARS_V1 {
        bounded.push_str("...");
    }
    bounded
}

fn redact_root(ctx: &RepoContext, message: &str) -> String {
    message.replace(&ctx.root().display().to_string(), "<repository>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    use tempfile::{TempDir, tempdir};

    use crate::context::RepoContext;
    use crate::test_env::TestRepoBuilder;

    const VALID_POLICY: &str = r#"
version = 1

[[rules]]
id = "source"
include = ["**"]
max_lines = 100
"#;

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn prepared_repository(policy: &str) -> (TempDir, RepoContext) {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        std::fs::create_dir_all(temp.path().join(".jig")).unwrap();
        std::fs::write(temp.path().join(POLICY_PATH_V1), policy).unwrap();
        std::fs::write(temp.path().join("source.rs"), "fn example() {}\n").unwrap();
        git(temp.path(), &["init", "-q", "-b", "main"]);
        git(temp.path(), &["config", "user.name", "Jig Test"]);
        git(
            temp.path(),
            &["config", "user.email", "jig@example.invalid"],
        );
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-q", "-m", "fixture"]);
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        (temp, ctx)
    }

    fn fixed_date() -> PolicyDateV1 {
        PolicyDateV1::new(2026, 8, 30).unwrap()
    }

    #[test]
    fn failure_digest_changes_with_attempted_exact_identity() {
        let first = digest_json(
            b"test\0",
            &("exact", vec!["1111111111111111111111111111111111111111"]),
        );
        let second = digest_json(
            b"test\0",
            &("exact", vec!["2222222222222222222222222222222222222222"]),
        );
        assert_ne!(first, second);
    }

    #[test]
    fn comparison_request_text_is_normalized_with_closed_bounds() {
        assert_eq!(
            normalize_request(ComparisonRequestV1::MergeBaseRef {
                requested_ref: " refs/remotes/origin/main ".into(),
            })
            .unwrap(),
            ComparisonRequestV1::MergeBaseRef {
                requested_ref: "refs/remotes/origin/main".into(),
            }
        );
        assert!(
            normalize_request(ComparisonRequestV1::MergeBaseRef {
                requested_ref: "x".repeat(MAX_SYMBOLIC_COMPARISON_REF_BYTES_V1 + 1),
            })
            .is_err()
        );
        assert!(
            normalize_request(ComparisonRequestV1::ExactTree {
                requested_oid: "a".repeat(MAX_EXACT_OBJECT_ID_BYTES_V1 + 1),
                provenance: jig_contract::ExactTreeProvenanceV1::Explicit,
            })
            .is_err()
        );
    }

    #[test]
    fn preparation_persists_ready_merge_base_authority_and_policy_identities() {
        let (_temp, ctx) = prepared_repository(VALID_POLICY);
        let prepared = prepare_file_budget_input_at_v1(
            &ctx,
            None,
            NativeFileBudgetConfigV1::default(),
            None,
            fixed_date(),
        )
        .unwrap();

        assert!(matches!(prepared.policy, PolicyPreparationV1::Ready { .. }));
        assert!(matches!(
            prepared.comparison,
            ComparisonPreparationV1::Ready {
                comparison: ResolvedComparisonV1::MergeBase { .. }
            }
        ));
        assert_eq!(prepared.view, CurrentViewV1::Worktree);
    }

    #[test]
    fn work_plan_defaults_to_its_captured_exact_commit() {
        let (_temp, ctx) = prepared_repository(VALID_POLICY);
        let opened = crate::state::plans_open(
            &ctx,
            crate::state::PlanOpenRequest {
                title: "Example plan".into(),
                body: Some("# Example plan\n".into()),
                body_file: None,
                base: None,
            },
        )
        .unwrap();
        let plan_id = opened["plan_id"].as_str().unwrap().to_owned();
        let head = git(ctx.root(), &["rev-parse", "HEAD"]);
        let prepared = prepare_file_budget_input_at_v1(
            &ctx,
            None,
            NativeFileBudgetConfigV1::default(),
            Some(plan_id.clone()),
            fixed_date(),
        )
        .unwrap();

        assert_eq!(prepared.work_plan_id.as_deref(), Some(plan_id.as_str()));
        assert_eq!(
            prepared.request,
            ComparisonRequestV1::ExactTree {
                requested_oid: head.clone(),
                provenance: jig_contract::ExactTreeProvenanceV1::WorkPlan,
            }
        );
        assert!(matches!(
            prepared.comparison,
            ComparisonPreparationV1::Ready {
                comparison: ResolvedComparisonV1::ExactTree {
                    requested_oid,
                    peeled_commit_oid: Some(_),
                    ..
                }
            } if requested_oid == head
        ));
    }

    #[test]
    fn index_view_reads_policy_from_the_index_and_types_absence_as_missing() {
        let (_temp, ctx) = prepared_repository(VALID_POLICY);
        std::fs::write(ctx.root().join(POLICY_PATH_V1), "version = 2\n").unwrap();

        let indexed = prepare_file_budget_input_at_v1(
            &ctx,
            Some(ComparisonRequestV1::IndexAgainstHead),
            NativeFileBudgetConfigV1::default(),
            None,
            fixed_date(),
        )
        .unwrap();
        assert!(matches!(indexed.policy, PolicyPreparationV1::Ready { .. }));

        git(ctx.root(), &["rm", "--cached", "-q", POLICY_PATH_V1]);
        let missing = prepare_file_budget_input_at_v1(
            &ctx,
            Some(ComparisonRequestV1::IndexAgainstHead),
            NativeFileBudgetConfigV1::default(),
            None,
            fixed_date(),
        )
        .unwrap();
        assert!(matches!(
            missing.policy,
            PolicyPreparationV1::InvalidPolicy {
                reason: PolicyPreparationFailureV1::Missing,
                ..
            }
        ));
    }

    #[test]
    fn invalid_policy_and_missing_comparison_are_prepared_independently() {
        let (_temp, ctx) = prepared_repository("version = 2\n");
        let requested_oid = "0".repeat(40);
        let prepared = prepare_file_budget_input_at_v1(
            &ctx,
            Some(ComparisonRequestV1::ExactTree {
                requested_oid: requested_oid.clone(),
                provenance: jig_contract::ExactTreeProvenanceV1::Explicit,
            }),
            NativeFileBudgetConfigV1::default(),
            None,
            fixed_date(),
        )
        .unwrap();

        assert!(matches!(
            prepared.policy,
            PolicyPreparationV1::InvalidPolicy { .. }
        ));
        assert!(matches!(
            prepared.comparison,
            ComparisonPreparationV1::ComparisonUnavailable {
                attempted_object_ids,
                ..
            } if attempted_object_ids == [requested_oid]
        ));
    }

    #[test]
    fn strict_inventory_fallback_preserves_the_original_failure() {
        let (_temp, ctx) = prepared_repository(VALID_POLICY);
        let requested_oid = "0".repeat(40);
        let prepared = prepare_file_budget_input_at_v1(
            &ctx,
            Some(ComparisonRequestV1::ExactTree {
                requested_oid: requested_oid.clone(),
                provenance: jig_contract::ExactTreeProvenanceV1::Explicit,
            }),
            NativeFileBudgetConfigV1 {
                missing_comparison: MissingComparisonV1::StrictInventory,
                ..NativeFileBudgetConfigV1::default()
            },
            None,
            fixed_date(),
        )
        .unwrap();

        assert!(matches!(
            prepared.comparison,
            ComparisonPreparationV1::Ready {
                comparison: ResolvedComparisonV1::StrictInventory {
                    reason: StrictInventoryReasonV1::MissingComparisonFallback,
                    fallback_from: Some(StrictInventoryFallbackV1 {
                        original_request: ComparisonRequestV1::ExactTree { requested_oid: original, .. },
                        attempted_object_ids,
                        ..
                    }),
                }
            } if original == requested_oid && attempted_object_ids == [requested_oid]
        ));
    }

    #[test]
    fn missing_push_before_gets_one_exact_fetch_before_it_blocks() {
        let (source, source_ctx) = prepared_repository(VALID_POLICY);
        let before = git(source_ctx.root(), &["rev-parse", "HEAD"]);
        std::fs::write(source_ctx.root().join("source.rs"), "fn changed() {}\n").unwrap();
        git(source_ctx.root(), &["add", "source.rs"]);
        git(source_ctx.root(), &["commit", "-q", "-m", "second"]);

        let container = tempdir().unwrap();
        let clone_root = container.path().join("clone");
        let source_url = format!("file://{}", source.path().display());
        let output = Command::new("git")
            .args(["clone", "-q", "--depth=1", "--branch", "main", &source_url])
            .arg(&clone_root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "shallow clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !Command::new("git")
                .current_dir(&clone_root)
                .args(["cat-file", "-e", &before])
                .status()
                .unwrap()
                .success(),
            "fixture must begin without push-before authority"
        );
        let clone_ctx = RepoContext::load_from_root(clone_root).unwrap();
        let prepared = prepare_file_budget_input_at_v1(
            &clone_ctx,
            Some(ComparisonRequestV1::ExactTree {
                requested_oid: before.clone(),
                provenance: jig_contract::ExactTreeProvenanceV1::PushBefore,
            }),
            NativeFileBudgetConfigV1::default(),
            None,
            fixed_date(),
        )
        .unwrap();

        assert!(matches!(
            prepared.comparison,
            ComparisonPreparationV1::Ready {
                comparison: ResolvedComparisonV1::ExactTree {
                    requested_oid,
                    provenance: jig_contract::ExactTreeProvenanceV1::PushBefore,
                    ..
                }
            } if requested_oid == before
        ));
    }

    #[test]
    fn unavailable_nonzero_push_before_blocks_or_uses_only_configured_fallback() {
        let (_temp, ctx) = prepared_repository(VALID_POLICY);
        let requested_oid = "1".repeat(40);
        let request = ComparisonRequestV1::ExactTree {
            requested_oid,
            provenance: jig_contract::ExactTreeProvenanceV1::PushBefore,
        };
        let blocked = prepare_file_budget_input_at_v1(
            &ctx,
            Some(request.clone()),
            NativeFileBudgetConfigV1::default(),
            None,
            fixed_date(),
        )
        .unwrap();
        assert!(matches!(
            blocked.comparison,
            ComparisonPreparationV1::ComparisonUnavailable { .. }
        ));

        let fallback = prepare_file_budget_input_at_v1(
            &ctx,
            Some(request),
            NativeFileBudgetConfigV1 {
                missing_comparison: MissingComparisonV1::StrictInventory,
                ..NativeFileBudgetConfigV1::default()
            },
            None,
            fixed_date(),
        )
        .unwrap();
        assert!(matches!(
            fallback.comparison,
            ComparisonPreparationV1::Ready {
                comparison: ResolvedComparisonV1::StrictInventory {
                    reason: StrictInventoryReasonV1::MissingComparisonFallback,
                    fallback_from: Some(_),
                }
            }
        ));
    }
}
