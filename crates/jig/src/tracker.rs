use std::path::{Path, PathBuf};
use std::time::Duration;

use jig_owned_process::ProcessOutputLimits;
use sha2::{Digest, Sha256};

mod path;
mod process;
#[cfg(test)]
pub(crate) use process::TestBrOverride;
#[allow(
    dead_code,
    reason = "T3/T4/T6/T7 consume the staged issue and mutation profile"
)]
mod profile_0_5_7;

#[cfg(test)]
mod tests;

#[allow(dead_code, reason = "T3 consumes the staged issue semantic revision")]
const SEMANTIC_REVISION_DOMAIN: &[u8] = b"jig.tracker.issue.semantic.v1\0";
const SUPPORTED_VERSION: &str = "0.5.7";
const MAX_TRACKER_ACTOR_BYTES: usize = 256;
const MAX_TRACKER_MUTATION_TEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "T3/T4/T6/T7 consume the staged issue and mutation operations"
)]
pub(crate) enum TrackerOperation {
    Version,
    Info,
    ShowIssue,
    ListComments,
    SyncStatus,
    AddComment,
    ClaimIssue,
    CloseIssue,
}

impl TrackerOperation {
    const fn uses_database(self) -> bool {
        !matches!(self, Self::Version | Self::Info)
    }

    const fn is_mutation(self) -> bool {
        matches!(self, Self::AddComment | Self::ClaimIssue | Self::CloseIssue)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Version => "version discovery",
            Self::Info => "workspace discovery",
            Self::ShowIssue => "issue read",
            Self::ListComments => "comment read",
            Self::SyncStatus => "storage-readiness check",
            Self::AddComment => "comment addition",
            Self::ClaimIssue => "issue claim",
            Self::CloseIssue => "issue close",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerOutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InvalidWorkspaceReason {
    RepositoryRoot,
    TrackerStore,
    Routing,
    Database,
    JsonlExport,
    HardLinkedAuthority,
}

#[derive(Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "T3/T4/T6/T7 consume the staged issue and mutation errors"
)]
pub(crate) enum TrackerError {
    InvalidInput {
        field: &'static str,
    },
    BinaryMissing,
    ExecutableCandidateInvalid,
    ExecutableSnapshotUnavailable,
    ExecutableChanged,
    UnsupportedBinary {
        version: String,
    },
    UnsupportedPlatform,
    InvalidWorkspace {
        reason: InvalidWorkspaceReason,
    },
    UnsupportedResponse {
        operation: TrackerOperation,
    },
    IssueMissing {
        issue_id: String,
    },
    IssueTombstoned {
        issue_id: String,
    },
    BlockedTransition {
        issue_id: String,
    },
    AssignmentConflict {
        issue_id: String,
    },
    AmbiguousIssueId {
        issue_id: String,
    },
    StaleStorage,
    StoreSnapshotTooLarge {
        limit_bytes: u64,
    },
    StoreChangedDuringSnapshot,
    StoreSnapshotTimedOut,
    UnsafeTemporaryDirectory {
        operation: TrackerOperation,
    },
    TimedOut {
        operation: TrackerOperation,
    },
    CancelledBeforeStart {
        operation: TrackerOperation,
    },
    Cancelled {
        operation: TrackerOperation,
    },
    OutputLimit {
        operation: TrackerOperation,
        stream: TrackerOutputStream,
    },
    ProcessFailure {
        operation: TrackerOperation,
        exit_code: Option<i32>,
    },
    IndeterminateWrite {
        operation: TrackerOperation,
    },
}

impl std::fmt::Display for TrackerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { field } => write!(formatter, "invalid tracker {field}"),
            Self::BinaryMissing => {
                formatter.write_str("the configured Beads executable is unavailable")
            }
            Self::ExecutableCandidateInvalid => formatter.write_str(
                "a Beads executable candidate could not be captured safely",
            ),
            Self::ExecutableSnapshotUnavailable => formatter.write_str(
                "this environment cannot retain an immutable Beads executable snapshot",
            ),
            Self::ExecutableChanged => formatter
                .write_str("the configured Beads executable changed after profile discovery"),
            Self::UnsupportedBinary { version } => {
                write!(
                    formatter,
                    "Beads version {version} has no supported Jig profile"
                )
            }
            Self::UnsupportedPlatform => formatter.write_str(
                "the configured Beads adapter is unsupported on this operating system",
            ),
            Self::InvalidWorkspace {
                reason: InvalidWorkspaceReason::HardLinkedAuthority,
            } => formatter.write_str(
                "the configured Beads workspace contains a tracker authority file with multiple hard links",
            ),
            Self::InvalidWorkspace { reason } => write!(
                formatter,
                "the configured Beads workspace failed its {reason:?} boundary check"
            ),
            Self::UnsupportedResponse { operation } => {
                write!(
                    formatter,
                    "Beads returned an unsupported response for {}",
                    operation.label()
                )
            }
            Self::IssueMissing { issue_id } => {
                write!(formatter, "Beads issue {issue_id} was not found")
            }
            Self::IssueTombstoned { issue_id } => {
                write!(formatter, "Beads issue {issue_id} is tombstoned")
            }
            Self::BlockedTransition { issue_id } => write!(
                formatter,
                "Beads rejected the transition for issue {issue_id}"
            ),
            Self::AssignmentConflict { issue_id } => write!(
                formatter,
                "Beads issue {issue_id} cannot be assigned to this actor"
            ),
            Self::AmbiguousIssueId { issue_id } => {
                write!(formatter, "Beads issue ID {issue_id} is ambiguous")
            }
            Self::StaleStorage => formatter.write_str("Beads storage is stale or conflicted"),
            Self::StoreSnapshotTooLarge { limit_bytes } => write!(
                formatter,
                "the private Beads store snapshot exceeds its {limit_bytes}-byte limit"
            ),
            Self::StoreChangedDuringSnapshot => formatter.write_str(
                "the Beads store changed while Jig was creating a private snapshot; retry after concurrent tracker activity settles",
            ),
            Self::StoreSnapshotTimedOut => formatter.write_str(
                "the private Beads store snapshot did not complete within the operation deadline",
            ),
            Self::UnsafeTemporaryDirectory { operation } => write!(
                formatter,
                "the tracker {} refused a repository-local temporary directory",
                operation.label()
            ),
            Self::TimedOut { operation } => {
                write!(formatter, "the tracker {} timed out", operation.label())
            }
            Self::CancelledBeforeStart { operation } => write!(
                formatter,
                "the tracker {} was cancelled before start",
                operation.label()
            ),
            Self::Cancelled { operation } => {
                write!(formatter, "the tracker {} was cancelled", operation.label())
            }
            Self::OutputLimit { operation, stream } => write!(
                formatter,
                "the tracker {} exceeded its {stream:?} output limit",
                operation.label()
            ),
            Self::ProcessFailure {
                operation,
                exit_code,
            } => match exit_code {
                Some(code) => write!(
                    formatter,
                    "the tracker {} failed with exit status {code}",
                    operation.label()
                ),
                None => write!(formatter, "the tracker {} failed", operation.label()),
            },
            Self::IndeterminateWrite { operation } => write!(
                formatter,
                "the tracker {} may have been applied; reconcile before retrying",
                operation.label()
            ),
        }
    }
}

impl std::error::Error for TrackerError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrackerProcessPolicy {
    pub discovery_timeout: Duration,
    pub read_timeout: Duration,
    pub mutation_timeout: Duration,
    pub output_limits: ProcessOutputLimits,
}

impl Default for TrackerProcessPolicy {
    fn default() -> Self {
        Self {
            discovery_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(10),
            mutation_timeout: Duration::from_secs(15),
            output_limits: ProcessOutputLimits {
                stdout: 1024 * 1024,
                stderr: 64 * 1024,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerProfile {
    Beads0_5_7,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerCapability {
    ShowIssue,
    ListComments,
    AddComment,
    ClaimIssue,
    CloseIssue,
}

const PROFILE_CAPABILITIES: &[TrackerCapability] = &[
    TrackerCapability::ShowIssue,
    TrackerCapability::ListComments,
    TrackerCapability::AddComment,
    TrackerCapability::ClaimIssue,
    TrackerCapability::CloseIssue,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerDiscovery {
    pub version: String,
    pub profile: TrackerProfile,
    pub capabilities: &'static [TrackerCapability],
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code, reason = "T3 consumes the staged normalized issue")]
pub(crate) struct TrackerIssue {
    pub provider: &'static str,
    pub workspace_id: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub status: String,
    pub assignee: Option<String>,
    pub provider_revision: Option<String>,
    pub semantic_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code, reason = "T4 consumes the staged normalized comment")]
pub(crate) struct TrackerComment {
    pub id: String,
    pub issue_id: String,
    pub author: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "T6/T7 consume the staged normalized mutation result"
)]
pub(crate) struct TrackerMutation {
    pub issue_id: String,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub provider_revision: Option<String>,
}

#[derive(Debug)]
pub(crate) struct BeadsAdapter {
    root: PathBuf,
    executable: process::ResolvedExecutable,
    version: String,
    #[allow(
        dead_code,
        reason = "T3/T4/T6/T7 consume the staged workspace-bound operations"
    )]
    workspace_id: String,
    store: Option<process::RetainedStore>,
    profile: TrackerProfile,
    policy: TrackerProcessPolicy,
}

#[cfg(test)]
type AfterProfiledProcessHook = Box<dyn FnMut(TrackerOperation)>;

#[cfg(test)]
thread_local! {
    static TEST_AFTER_PROFILED_PROCESS_HOOK:
        std::cell::RefCell<Option<AfterProfiledProcessHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct TestAfterProfiledProcessHook {
    previous: Option<AfterProfiledProcessHook>,
}

#[cfg(test)]
impl TestAfterProfiledProcessHook {
    fn set(hook: impl FnMut(TrackerOperation) + 'static) -> Self {
        let previous =
            TEST_AFTER_PROFILED_PROCESS_HOOK.with(|value| value.replace(Some(Box::new(hook))));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestAfterProfiledProcessHook {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_AFTER_PROFILED_PROCESS_HOOK.with(|value| {
            value.replace(previous);
        });
    }
}

#[cfg(test)]
fn run_test_after_profiled_process_hook(operation: TrackerOperation) {
    TEST_AFTER_PROFILED_PROCESS_HOOK.with(|value| {
        if let Some(hook) = value.borrow_mut().as_mut() {
            hook(operation);
        }
    });
}

#[cfg(not(test))]
fn run_test_after_profiled_process_hook(_operation: TrackerOperation) {}

impl BeadsAdapter {
    pub(crate) fn discover(
        root: &Path,
        workspace_id: &str,
        policy: TrackerProcessPolicy,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(Self, TrackerDiscovery), TrackerError> {
        validate_workspace_id(workspace_id)?;
        let budget = process::OperationBudget::new(policy.discovery_timeout);
        budget.checkpoint_before_spawn(TrackerOperation::Version, cancelled)?;
        let root = path::canonical_repository_root(root)?;
        budget.checkpoint_before_spawn(TrackerOperation::Version, cancelled)?;
        path::validate_store(&root)?;
        budget.checkpoint_before_spawn(TrackerOperation::Version, cancelled)?;
        let executable = process::resolve_br(&root, budget, TrackerOperation::Version, cancelled)?;
        let runner = process::ProcessRunner::new(&root, &executable, None, policy);
        let version_value = runner.run_json_with_budget(
            TrackerOperation::Version,
            &["version"],
            budget,
            cancelled,
        )?;
        let version = profile_0_5_7::parse_version(&version_value)?;
        let profile = if version == SUPPORTED_VERSION {
            TrackerProfile::Beads0_5_7
        } else {
            TrackerProfile::Unsupported
        };

        if profile == TrackerProfile::Unsupported {
            let discovery = TrackerDiscovery {
                version,
                profile,
                capabilities: &[],
            };
            return Ok((
                Self {
                    root,
                    executable,
                    version: discovery.version.clone(),
                    workspace_id: workspace_id.into(),
                    store: None,
                    profile,
                    policy,
                },
                discovery,
            ));
        }

        let info_value = runner.run_json_with_budget(
            TrackerOperation::Info,
            &["--no-db", "where"],
            budget,
            cancelled,
        )?;
        let info = profile_0_5_7::parse_info(&info_value)?;
        let (database_path, jsonl_path) = path::validate_discovered_paths(&root, &info)?;
        let store = process::RetainedStore::capture(
            database_path,
            jsonl_path,
            budget,
            TrackerOperation::Info,
            cancelled,
        )?;
        let discovery = TrackerDiscovery {
            version,
            profile,
            capabilities: PROFILE_CAPABILITIES,
        };
        Ok((
            Self {
                root,
                executable,
                version: discovery.version.clone(),
                workspace_id: workspace_id.into(),
                store: Some(store),
                profile,
                policy,
            },
            discovery,
        ))
    }

    #[allow(dead_code, reason = "T3 consumes the staged issue read")]
    pub(crate) fn show_issue(
        &self,
        issue_id: &str,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<TrackerIssue, TrackerError> {
        validate_issue_id(issue_id)?;
        let value = self.run_profiled(
            TrackerOperation::ShowIssue,
            &["show", "--", issue_id],
            cancelled,
        )?;
        profile_0_5_7::parse_issue(&value, &self.workspace_id, issue_id)
    }

    #[allow(dead_code, reason = "T4 consumes the staged comment read")]
    pub(crate) fn list_comments(
        &self,
        issue_id: &str,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<TrackerComment>, TrackerError> {
        validate_issue_id(issue_id)?;
        let value = self.run_profiled(
            TrackerOperation::ListComments,
            &["comments", "list", "--", issue_id],
            cancelled,
        )?;
        profile_0_5_7::parse_comments(&value, issue_id)
    }

    #[allow(dead_code, reason = "T4 consumes the staged comment mutation")]
    pub(crate) fn add_comment(
        &self,
        issue_id: &str,
        actor: &str,
        message: &str,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<TrackerComment, TrackerError> {
        validate_issue_id(issue_id)?;
        validate_argument("actor", actor, MAX_TRACKER_ACTOR_BYTES)?;
        validate_argument("comment", message, MAX_TRACKER_MUTATION_TEXT_BYTES)?;
        let budget = process::OperationBudget::new(self.policy.mutation_timeout);
        self.check_storage_readiness_with_budget(budget, cancelled)?;
        let actor_argument = format!("--actor={actor}");
        let message_argument = format!("--message={message}");
        let value = self.run_profiled_with_budget(
            TrackerOperation::AddComment,
            &[
                "comments",
                "add",
                &actor_argument,
                &message_argument,
                "--",
                issue_id,
            ],
            budget,
            cancelled,
        )?;
        profile_0_5_7::parse_comment(&value, issue_id, actor, message)
            .map_err(|error| mutation_response_error(error, TrackerOperation::AddComment))
    }

    #[allow(dead_code, reason = "T6 consumes the staged claim mutation")]
    pub(crate) fn claim_issue(
        &self,
        issue_id: &str,
        actor: &str,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<TrackerMutation, TrackerError> {
        validate_issue_id(issue_id)?;
        validate_argument("actor", actor, MAX_TRACKER_ACTOR_BYTES)?;
        let budget = process::OperationBudget::new(self.policy.mutation_timeout);
        self.check_storage_readiness_with_budget(budget, cancelled)?;
        let actor_argument = format!("--actor={actor}");
        let value = self.run_profiled_with_budget(
            TrackerOperation::ClaimIssue,
            &["update", "--claim", &actor_argument, "--", issue_id],
            budget,
            cancelled,
        )?;
        profile_0_5_7::parse_claim(&value, issue_id, actor)
            .map_err(|error| mutation_response_error(error, TrackerOperation::ClaimIssue))
    }

    #[allow(dead_code, reason = "T7 consumes the staged close mutation")]
    pub(crate) fn close_issue(
        &self,
        issue_id: &str,
        actor: &str,
        reason: &str,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<TrackerMutation, TrackerError> {
        validate_issue_id(issue_id)?;
        validate_argument("actor", actor, MAX_TRACKER_ACTOR_BYTES)?;
        validate_argument("close reason", reason, MAX_TRACKER_MUTATION_TEXT_BYTES)?;
        let budget = process::OperationBudget::new(self.policy.mutation_timeout);
        self.check_storage_readiness_with_budget(budget, cancelled)?;
        let actor_argument = format!("--actor={actor}");
        let reason_argument = format!("--reason={reason}");
        let value = self.run_profiled_with_budget(
            TrackerOperation::CloseIssue,
            &["close", &actor_argument, &reason_argument, "--", issue_id],
            budget,
            cancelled,
        )?;
        profile_0_5_7::parse_close(&value, issue_id)
            .map_err(|error| mutation_response_error(error, TrackerOperation::CloseIssue))
    }

    fn run_profiled(
        &self,
        operation: TrackerOperation,
        args: &[&str],
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<serde_json::Value, TrackerError> {
        let budget = process::OperationBudget::new(self.operation_timeout(operation));
        self.run_profiled_with_budget(operation, args, budget, cancelled)
    }

    fn run_profiled_with_budget(
        &self,
        operation: TrackerOperation,
        args: &[&str],
        budget: process::OperationBudget,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<serde_json::Value, TrackerError> {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        self.require_supported()?;
        self.revalidate_workspace_with_budget(operation, budget, cancelled)?;
        let store = self
            .store
            .as_ref()
            .ok_or(TrackerError::UnsupportedResponse { operation })?;
        let result =
            process::ProcessRunner::new(&self.root, &self.executable, Some(store), self.policy)
                .run_json_with_budget(operation, args, budget, cancelled);
        run_test_after_profiled_process_hook(operation);
        let workspace = self.revalidate_workspace();
        match (result, workspace) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (_, Err(_)) if operation.is_mutation() => {
                Err(TrackerError::IndeterminateWrite { operation })
            }
            (_, Err(error)) => Err(error),
        }
    }

    pub(crate) fn check_storage_readiness(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), TrackerError> {
        let budget = process::OperationBudget::new(self.policy.read_timeout);
        self.check_storage_readiness_with_budget(budget, cancelled)
    }

    fn check_storage_readiness_with_budget(
        &self,
        budget: process::OperationBudget,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), TrackerError> {
        budget.checkpoint_before_spawn(TrackerOperation::SyncStatus, cancelled)?;
        self.require_supported()?;
        self.revalidate_workspace_with_budget(TrackerOperation::SyncStatus, budget, cancelled)?;
        let value = self.run_profiled_with_budget(
            TrackerOperation::SyncStatus,
            &["sync", "--allow-external-jsonl", "--status"],
            budget,
            cancelled,
        )?;
        profile_0_5_7::require_mutation_ready(&value)?;
        self.revalidate_workspace()
    }

    fn require_supported(&self) -> Result<(), TrackerError> {
        if self.profile == TrackerProfile::Beads0_5_7 {
            Ok(())
        } else {
            Err(TrackerError::UnsupportedBinary {
                version: self.version.clone(),
            })
        }
    }

    fn revalidate_workspace(&self) -> Result<(), TrackerError> {
        let store = self.store.as_ref().ok_or(TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Database,
        })?;
        path::revalidate_paths(&self.root, store.database_path(), store.jsonl_path())?;
        store.verify_paths()
    }

    fn revalidate_workspace_with_budget(
        &self,
        operation: TrackerOperation,
        budget: process::OperationBudget,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), TrackerError> {
        budget.checkpoint_before_spawn(operation, cancelled)?;
        self.revalidate_workspace()
    }

    const fn operation_timeout(&self, operation: TrackerOperation) -> Duration {
        match operation {
            TrackerOperation::Version | TrackerOperation::Info => self.policy.discovery_timeout,
            operation if operation.is_mutation() => self.policy.mutation_timeout,
            _ => self.policy.read_timeout,
        }
    }
}

fn validate_workspace_id(workspace_id: &str) -> Result<(), TrackerError> {
    let parsed = ulid::Ulid::from_string(workspace_id).map_err(|_| TrackerError::InvalidInput {
        field: "workspace_id",
    })?;
    if parsed.to_string() != workspace_id {
        return Err(TrackerError::InvalidInput {
            field: "workspace_id",
        });
    }
    Ok(())
}

#[allow(dead_code, reason = "T3/T4/T6/T7 consume the staged issue operations")]
fn validate_issue_id(issue_id: &str) -> Result<(), TrackerError> {
    if issue_id.is_empty()
        || issue_id.len() > 256
        || issue_id.contains('\0')
        || issue_id.chars().any(char::is_control)
    {
        return Err(TrackerError::InvalidInput { field: "issue id" });
    }
    Ok(())
}

#[allow(dead_code, reason = "T4/T6/T7 consume the staged mutation operations")]
fn validate_argument(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), TrackerError> {
    if value.is_empty() || value.len() > max_bytes || value.contains('\0') {
        Err(TrackerError::InvalidInput { field })
    } else {
        Ok(())
    }
}

#[allow(dead_code, reason = "T3 consumes the staged issue read")]
fn semantic_revision(
    workspace_id: &str,
    issue_id: &str,
    title: &str,
    description: &str,
    acceptance_criteria: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(SEMANTIC_REVISION_DOMAIN);
    for field in [
        workspace_id,
        issue_id,
        title,
        description,
        acceptance_criteria,
    ] {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field.as_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

#[allow(dead_code, reason = "T4/T6/T7 consume the staged mutation operations")]
fn mutation_response_error(error: TrackerError, operation: TrackerOperation) -> TrackerError {
    if matches!(error, TrackerError::UnsupportedResponse { .. }) {
        TrackerError::IndeterminateWrite { operation }
    } else {
        error
    }
}
