use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::rc::Rc;

use crate::context::{RepoContext, WorkCheckGate};
use crate::git_receipts::{
    GateApplicability, GateScopeSnapshot, PlanChangeSnapshot, gate_scope_snapshot_from_plan_change,
    gate_scope_snapshot_from_plan_change_with_cancellation, plan_change_snapshot,
    plan_change_snapshot_from_empty_tree, plan_change_snapshot_from_empty_tree_with_cancellation,
    plan_change_snapshot_with_cancellation,
};
use crate::state::{
    CurrentWorktreeFingerprint, PlanBaseline, current_worktree_fingerprint,
    current_worktree_fingerprint_with_cancellation, plan_baseline, plan_baseline_with_cancellation,
};

const GATE_SIGNATURE_DOMAIN: &[u8] = b"jig-work-gate-signature-v2\0";

pub(super) struct PlanGateContext {
    source: PlanScopeSource,
    legacy_fingerprint: RefCell<Option<CurrentWorktreeFingerprint>>,
}

enum PlanScopeSource {
    Legacy,
    BaselineUnavailable(PlanBaseline),
    Baseline {
        baseline: PlanBaseline,
        oid: String,
        change: PreparedPlanChange,
    },
}

#[derive(Clone)]
enum PreparedPlanChange {
    Ready(std::result::Result<Rc<PlanChangeSnapshot>, String>),
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum GateScopeEvaluation {
    Known(KnownGateScope),
    Unknown(UnknownGateScope),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct KnownGateScope {
    gate_signature: String,
    baseline_oid: Option<String>,
    applicability: GateApplicability,
    reason: String,
    changed_paths: Vec<String>,
    changed_path_count: usize,
    changed_paths_truncated: bool,
    changed_paths_digest: Option<String>,
    matching_paths: Vec<String>,
    matching_path_count: usize,
    matching_paths_truncated: bool,
    matching_paths_digest: Option<String>,
    scope_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UnknownGateScope {
    gate_signature: String,
    baseline_oid: Option<String>,
    error: String,
}

impl PlanGateContext {
    pub(super) fn load(ctx: &RepoContext, plan_id: &str) -> Result<Self> {
        let baseline = plan_baseline(ctx, plan_id)?;
        let plan_change = baseline.as_ref().and_then(|baseline| {
            baseline
                .commit_oid
                .as_deref()
                .map(|oid| plan_change_snapshot(ctx.root(), oid))
                .or_else(|| {
                    baseline
                        .empty_tree_oid
                        .as_deref()
                        .map(|oid| plan_change_snapshot_from_empty_tree(ctx.root(), oid))
                })
                .map(|result| result.map(Rc::new).map_err(|error| format!("{error:#}")))
        });
        Ok(Self::from_prepared(baseline, plan_change))
    }

    pub(super) fn load_with_cancellation(
        ctx: &RepoContext,
        plan_id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let baseline = plan_baseline_with_cancellation(ctx, plan_id, cancelled)?;
        Self::from_baseline_with_cancellation(ctx, baseline, cancelled)
    }

    pub(super) fn from_baseline_with_cancellation(
        ctx: &RepoContext,
        baseline: Option<PlanBaseline>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let plan_change = Self::prepare_plan_change_with_cancellation(ctx, &baseline, cancelled);
        Ok(Self::from_prepared(baseline, plan_change))
    }

    pub(super) fn prepare_plan_change_with_cancellation(
        ctx: &RepoContext,
        baseline: &Option<PlanBaseline>,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<std::result::Result<Rc<PlanChangeSnapshot>, String>> {
        baseline.as_ref().and_then(|baseline| {
            baseline
                .commit_oid
                .as_deref()
                .map(|oid| plan_change_snapshot_with_cancellation(ctx.root(), oid, cancelled))
                .or_else(|| {
                    baseline.empty_tree_oid.as_deref().map(|oid| {
                        plan_change_snapshot_from_empty_tree_with_cancellation(
                            ctx.root(),
                            oid,
                            cancelled,
                        )
                    })
                })
                .map(|result| result.map(Rc::new).map_err(|error| format!("{error:#}")))
        })
    }

    pub(super) fn from_prepared(
        baseline: Option<PlanBaseline>,
        plan_change: Option<std::result::Result<Rc<PlanChangeSnapshot>, String>>,
    ) -> Self {
        let source = match baseline {
            None => PlanScopeSource::Legacy,
            Some(baseline) => {
                let oid = baseline
                    .commit_oid
                    .clone()
                    .or_else(|| baseline.empty_tree_oid.clone());
                match oid {
                    Some(oid) => PlanScopeSource::Baseline {
                        baseline,
                        oid,
                        change: plan_change
                            .map(PreparedPlanChange::Ready)
                            .unwrap_or(PreparedPlanChange::Missing),
                    },
                    None => PlanScopeSource::BaselineUnavailable(baseline),
                }
            }
        };
        Self {
            source,
            legacy_fingerprint: RefCell::new(None),
        }
    }

    pub(super) fn seed_legacy_fingerprint(&self, fingerprint: CurrentWorktreeFingerprint) {
        *self.legacy_fingerprint.borrow_mut() = Some(fingerprint);
    }

    pub(super) fn baseline(&self) -> Option<&PlanBaseline> {
        match &self.source {
            PlanScopeSource::Legacy => None,
            PlanScopeSource::BaselineUnavailable(baseline)
            | PlanScopeSource::Baseline { baseline, .. } => Some(baseline),
        }
    }

    pub(super) fn evaluate(&self, ctx: &RepoContext, gate: &WorkCheckGate) -> GateScopeEvaluation {
        self.evaluate_inner(ctx, gate, None)
    }

    pub(super) fn evaluate_with_cancellation(
        &self,
        ctx: &RepoContext,
        gate: &WorkCheckGate,
        cancelled: &dyn Fn() -> bool,
    ) -> GateScopeEvaluation {
        self.evaluate_inner(ctx, gate, Some(cancelled))
    }

    fn evaluate_inner(
        &self,
        ctx: &RepoContext,
        gate: &WorkCheckGate,
        cancelled: Option<&dyn Fn() -> bool>,
    ) -> GateScopeEvaluation {
        let signature = match gate_signature(ctx, gate) {
            Ok(signature) => signature,
            Err(error) => return GateScopeEvaluation::unknown(None, None, format!("{error:#}")),
        };
        let (baseline_oid, plan_change) = match &self.source {
            PlanScopeSource::Legacy => {
                return self.evaluate_legacy_unconditional(ctx, gate, signature, cancelled);
            }
            PlanScopeSource::BaselineUnavailable(baseline) => {
                let error = baseline
                    .error
                    .as_deref()
                    .unwrap_or("plan baseline commit is unavailable");
                if gate.paths.is_none() && !gate.reuse {
                    return self.evaluate_legacy_unconditional(ctx, gate, signature, cancelled);
                }
                return GateScopeEvaluation::unknown(
                    Some(signature),
                    None,
                    format!(
                        "plan baseline '{}' is unavailable: {error}",
                        baseline.requested_ref
                    ),
                );
            }
            PlanScopeSource::Baseline { oid, change, .. } => {
                let change = match change {
                    PreparedPlanChange::Ready(Ok(change)) => change,
                    PreparedPlanChange::Ready(Err(error)) => {
                        return GateScopeEvaluation::unknown(
                            Some(signature),
                            Some(oid.clone()),
                            error.clone(),
                        );
                    }
                    PreparedPlanChange::Missing => {
                        return GateScopeEvaluation::unknown(
                            Some(signature),
                            Some(oid.clone()),
                            "plan change snapshot was not prepared".into(),
                        );
                    }
                };
                (oid.as_str(), change)
            }
        };
        let command_scope_safe = gate_command_scope_is_safe(ctx, gate);
        let effective_paths = if command_scope_safe {
            gate.paths.as_deref()
        } else {
            None
        };
        let effective_paths_ignore = if command_scope_safe {
            gate.paths_ignore.as_slice()
        } else {
            &[]
        };
        let snapshot = match cancelled {
            Some(cancelled) => gate_scope_snapshot_from_plan_change_with_cancellation(
                ctx.root(),
                plan_change,
                effective_paths,
                effective_paths_ignore,
                &signature,
                cancelled,
            ),
            None => gate_scope_snapshot_from_plan_change(
                ctx.root(),
                plan_change,
                effective_paths,
                effective_paths_ignore,
                &signature,
            ),
        };
        match snapshot {
            Ok(snapshot) => {
                let mut evaluation = GateScopeEvaluation::from_snapshot(signature, snapshot);
                if gate.paths.is_some() && !command_scope_safe {
                    evaluation.replace_reason("gate command is not a recognized scope-safe Jig command; classified conservatively against all baseline-relative changes".into());
                }
                evaluation
            }
            Err(error) => GateScopeEvaluation::unknown(
                Some(signature),
                Some(baseline_oid.to_string()),
                format!("{error:#}"),
            ),
        }
    }

    fn evaluate_legacy_unconditional(
        &self,
        ctx: &RepoContext,
        gate: &WorkCheckGate,
        signature: String,
        cancelled: Option<&dyn Fn() -> bool>,
    ) -> GateScopeEvaluation {
        if gate.paths.is_some() || gate.reuse {
            return GateScopeEvaluation::unknown(
                Some(signature),
                None,
                "plan predates Git baseline capture; reopen the work plan before using path-aware or reusable gates".into(),
            );
        }
        let fingerprint = if let Some(fingerprint) = self.legacy_fingerprint.borrow().clone() {
            Ok(fingerprint)
        } else {
            let fingerprint = match cancelled {
                Some(cancelled) => current_worktree_fingerprint_with_cancellation(ctx, cancelled),
                None => Ok(current_worktree_fingerprint(ctx)),
            };
            if let Ok(fingerprint) = &fingerprint {
                *self.legacy_fingerprint.borrow_mut() = Some(fingerprint.clone());
            }
            fingerprint
        };
        GateScopeEvaluation::legacy_unconditional(signature, fingerprint)
    }
}

impl GateScopeEvaluation {
    fn from_snapshot(gate_signature: String, snapshot: GateScopeSnapshot) -> Self {
        let GateScopeSnapshot {
            facts,
            scope_fingerprint,
        } = snapshot;
        Self::Known(KnownGateScope {
            gate_signature,
            baseline_oid: Some(facts.baseline_oid),
            applicability: facts.applicability,
            reason: facts.reason,
            changed_paths: facts.changed_paths,
            changed_path_count: facts.changed_path_count,
            changed_paths_truncated: facts.changed_paths_truncated,
            changed_paths_digest: Some(facts.changed_paths_digest),
            matching_paths: facts.matching_paths,
            matching_path_count: facts.matching_path_count,
            matching_paths_truncated: facts.matching_paths_truncated,
            matching_paths_digest: Some(facts.matching_paths_digest),
            scope_fingerprint: Some(scope_fingerprint),
        })
    }

    fn legacy_unconditional(
        gate_signature: String,
        fingerprint: Result<CurrentWorktreeFingerprint>,
    ) -> Self {
        match fingerprint {
            Ok(fingerprint) => Self::Known(KnownGateScope {
                gate_signature,
                baseline_oid: None,
                applicability: GateApplicability::Applicable,
                reason: "gate has no path filter and the legacy plan uses whole-worktree freshness"
                    .into(),
                changed_paths: Vec::new(),
                changed_path_count: 0,
                changed_paths_truncated: false,
                changed_paths_digest: None,
                matching_paths: Vec::new(),
                matching_path_count: 0,
                matching_paths_truncated: false,
                matching_paths_digest: None,
                scope_fingerprint: fingerprint.fingerprint,
                // Whole-worktree collection failure affects freshness, not the
                // applicability of a legacy unconditional gate. The batch
                // receipt retains the collection diagnostic and fails closed
                // during gate evaluation exactly as it did before path gates.
            }),
            Err(error) => Self::unknown(Some(gate_signature), None, format!("{error:#}")),
        }
    }

    fn unknown(
        gate_signature: Option<String>,
        baseline_oid: Option<String>,
        error: String,
    ) -> Self {
        Self::Unknown(UnknownGateScope {
            gate_signature: gate_signature.unwrap_or_default(),
            baseline_oid,
            error,
        })
    }

    fn replace_reason(&mut self, reason: String) {
        let Self::Known(scope) = self else {
            debug_assert!(false, "unknown gate scopes cannot carry a known reason");
            return;
        };
        scope.reason = reason;
    }

    pub(super) fn gate_signature(&self) -> &str {
        match self {
            Self::Known(scope) => &scope.gate_signature,
            Self::Unknown(scope) => &scope.gate_signature,
        }
    }

    pub(super) fn baseline_oid(&self) -> Option<&str> {
        match self {
            Self::Known(scope) => scope.baseline_oid.as_deref(),
            Self::Unknown(scope) => scope.baseline_oid.as_deref(),
        }
    }

    pub(super) fn applicability(&self) -> Option<GateApplicability> {
        match self {
            Self::Known(scope) => Some(scope.applicability),
            Self::Unknown(_) => None,
        }
    }

    pub(super) fn reason(&self) -> &str {
        match self {
            Self::Known(scope) => &scope.reason,
            Self::Unknown(_) => "gate applicability could not be determined",
        }
    }

    pub(super) fn changed_paths(&self) -> &[String] {
        match self {
            Self::Known(scope) => &scope.changed_paths,
            Self::Unknown(_) => &[],
        }
    }

    pub(super) fn changed_path_count(&self) -> usize {
        match self {
            Self::Known(scope) => scope.changed_path_count,
            Self::Unknown(_) => 0,
        }
    }

    pub(super) fn changed_paths_truncated(&self) -> bool {
        match self {
            Self::Known(scope) => scope.changed_paths_truncated,
            Self::Unknown(_) => false,
        }
    }

    pub(super) fn changed_paths_digest(&self) -> Option<&str> {
        match self {
            Self::Known(scope) => scope.changed_paths_digest.as_deref(),
            Self::Unknown(_) => None,
        }
    }

    pub(super) fn matching_paths(&self) -> &[String] {
        match self {
            Self::Known(scope) => &scope.matching_paths,
            Self::Unknown(_) => &[],
        }
    }

    pub(super) fn matching_path_count(&self) -> usize {
        match self {
            Self::Known(scope) => scope.matching_path_count,
            Self::Unknown(_) => 0,
        }
    }

    pub(super) fn matching_paths_truncated(&self) -> bool {
        match self {
            Self::Known(scope) => scope.matching_paths_truncated,
            Self::Unknown(_) => false,
        }
    }

    pub(super) fn matching_paths_digest(&self) -> Option<&str> {
        match self {
            Self::Known(scope) => scope.matching_paths_digest.as_deref(),
            Self::Unknown(_) => None,
        }
    }

    pub(super) fn scope_fingerprint(&self) -> Option<&str> {
        match self {
            Self::Known(scope) => scope.scope_fingerprint.as_deref(),
            Self::Unknown(_) => None,
        }
    }

    pub(super) fn error(&self) -> Option<&str> {
        match self {
            Self::Known(_) => None,
            Self::Unknown(scope) => Some(&scope.error),
        }
    }

    pub(super) fn is_known_applicable(&self) -> bool {
        self.applicability() == Some(GateApplicability::Applicable)
    }

    #[cfg(test)]
    pub(super) fn test_known(
        gate_signature: impl Into<String>,
        reason: impl Into<String>,
        scope_fingerprint: impl Into<String>,
    ) -> Self {
        Self::Known(KnownGateScope {
            gate_signature: gate_signature.into(),
            baseline_oid: None,
            applicability: GateApplicability::Applicable,
            reason: reason.into(),
            changed_paths: Vec::new(),
            changed_path_count: 0,
            changed_paths_truncated: false,
            changed_paths_digest: None,
            matching_paths: Vec::new(),
            matching_path_count: 0,
            matching_paths_truncated: false,
            matching_paths_digest: None,
            scope_fingerprint: Some(scope_fingerprint.into()),
        })
    }
}

pub(super) fn gate_signature(ctx: &RepoContext, gate: &WorkCheckGate) -> Result<String> {
    gate_signature_with_native_identity(ctx, gate, env!("JIG_BUILD_IDENTITY"))
}

fn gate_signature_with_native_identity(
    ctx: &RepoContext,
    gate: &WorkCheckGate,
    native_build_identity: &str,
) -> Result<String> {
    let tool = ctx
        .tool_spec(&gate.tool)
        .ok_or_else(|| anyhow!("Configured gate tool '{}' is not declared", gate.tool))?;
    let mut digest = Sha256::new();
    digest.update(GATE_SIGNATURE_DOMAIN);
    hash_field(&mut digest, ctx.contract_version().to_string().as_bytes());
    hash_field(&mut digest, gate.id.as_bytes());
    hash_field(&mut digest, gate.tool.as_bytes());
    hash_field(&mut digest, tool.kind.as_bytes());
    if tool.kind == crate::tool_defs::kind::NATIVE {
        hash_field(&mut digest, b"native-build-identity");
        hash_field(&mut digest, native_build_identity.as_bytes());
    }
    hash_field(
        &mut digest,
        tool.command.as_deref().unwrap_or("").as_bytes(),
    );
    if let Some(command_key) = tool.command.as_deref() {
        hash_field(
            &mut digest,
            ctx.command_for_key(command_key)
                .with_context(|| format!("Failed to resolve gate tool '{}':", gate.tool))?
                .as_bytes(),
        );
    }
    hash_field(
        &mut digest,
        &[u8::from(gate.required), u8::from(gate.reuse)],
    );
    hash_field(&mut digest, b"paths");
    hash_field(&mut digest, &[u8::from(gate.paths.is_some())]);
    hash_field(
        &mut digest,
        &(gate.paths.as_ref().map_or(0, Vec::len) as u64).to_be_bytes(),
    );
    for pattern in gate.paths.iter().flatten() {
        hash_field(&mut digest, pattern.as_bytes());
    }
    hash_field(&mut digest, b"paths-ignore");
    hash_field(&mut digest, &(gate.paths_ignore.len() as u64).to_be_bytes());
    for pattern in &gate.paths_ignore {
        hash_field(&mut digest, pattern.as_bytes());
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn gate_command_scope_is_safe(ctx: &RepoContext, gate: &WorkCheckGate) -> bool {
    if gate.paths.is_none() {
        return true;
    }
    let Some(tool) = ctx.tool_spec(&gate.tool) else {
        return false;
    };
    if gate.tool == crate::tool_defs::tool::SCHEMA_CHECK {
        return canonical_schema_dump_command(ctx.schema_dump_command());
    }
    if tool.kind == crate::tool_defs::kind::NATIVE {
        return true;
    }
    let Some(command_key) = tool.command.as_deref() else {
        return false;
    };
    let Ok(command) = ctx.command_for_key(command_key) else {
        return false;
    };
    match gate.tool.as_str() {
        crate::tool_defs::tool::FMT_CHECK => canonical_cargo_command(command, &["fmt"]),
        crate::tool_defs::tool::CLIPPY => canonical_cargo_command(command, &["clippy"]),
        crate::tool_defs::tool::TEST | crate::tool_defs::tool::TEST_LOCKED => {
            canonical_cargo_command(command, &["test"])
                || canonical_cargo_command(command, &["nextest", "run"])
        }
        crate::tool_defs::tool::SQLX_CHECK => canonical_sqlx_command(ctx, command),
        "jig.application_contract_check" => {
            command == "scripts/check-webapps.sh application-contracts"
        }
        "jig.public_artifacts_check" => command == "scripts/check-webapps.sh public-artifacts",
        tool if tool.starts_with("jig.typescript_") => canonical_app_check(ctx, tool, command),
        crate::tool_defs::tool::SCHEMA_DUMP => false,
        // A project-owned tool and its path policy are an explicit contract.
        // The conservative fallback is reserved for Jig's generated tool IDs,
        // whose paths may be refreshed while project-owned command values are
        // deliberately preserved during adoption.
        _ => true,
    }
}

fn canonical_cargo_command(command: &str, expected_subcommand: &[&str]) -> bool {
    let command =
        if let Some((cargo, fallback)) = crate::shell::optional_cargo_command_branches(command) {
            if !canonical_cargo_skip(fallback) {
                return false;
            }
            cargo
        } else {
            command
        };
    let Some(tokens) = simple_shell_words(command) else {
        return false;
    };
    if tokens.first() != Some(&"cargo")
        || !tokens
            .get(1..1 + expected_subcommand.len())
            .is_some_and(|actual| actual == expected_subcommand)
    {
        return false;
    }
    tokens[1 + expected_subcommand.len()..].iter().all(|token| {
        matches!(
            *token,
            "--all"
                | "--all-features"
                | "--all-targets"
                | "--locked"
                | "--no-default-features"
                | "--no-fail-fast"
                | "--workspace"
                | "--"
                | "--check"
                | "-D"
                | "warnings"
        )
    })
}

include!("scope/tail.rs");
