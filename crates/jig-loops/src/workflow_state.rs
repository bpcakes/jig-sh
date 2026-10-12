#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum WorkflowOutcome {
    #[default]
    Succeeded,
    Failed,
    NeedsAttention,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum RepositoryRevisionState {
    #[default]
    NotApplicable,
    Unchanged,
    Changed,
    Unknown,
}

impl RepositoryRevisionState {
    pub(super) const fn changed(self) -> bool {
        matches!(self, Self::Changed)
    }

    pub(super) const fn requires_dispatch_stop(self) -> bool {
        matches!(self, Self::Changed | Self::Unknown)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum WorkflowExecution {
    #[default]
    Executed,
    Unexecuted(UnexecutedReason),
}

impl WorkflowExecution {
    pub(super) const fn unexecuted_reason(self) -> Option<UnexecutedReason> {
        match self {
            Self::Executed => None,
            Self::Unexecuted(reason) => Some(reason),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnexecutedReason {
    BlockedByActiveOccurrence,
    BlockedByAttention,
    CancelledBeforeStart,
    PreExecutionError,
}

impl UnexecutedReason {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::BlockedByActiveOccurrence => "blocked_by_active_occurrence",
            Self::BlockedByAttention => "blocked_by_attention",
            Self::CancelledBeforeStart => "cancelled_before_start",
            Self::PreExecutionError => "pre_execution_error",
        }
    }
}
