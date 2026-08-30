use super::*;

impl From<LoopCommand> for command::LoopCommand {
    fn from(command: LoopCommand) -> Self {
        match command {
            LoopCommand::Tick(opts) => Self::Tick(opts.into()),
            LoopCommand::Dispatch(opts) => Self::Dispatch(opts.into()),
            LoopCommand::Status(opts) => Self::Status(opts.into()),
            LoopCommand::Run(opts) => Self::Run(opts.into()),
            LoopCommand::ClearAttempt(opts) => Self::ClearAttempt(opts.into()),
            LoopCommand::AcknowledgeOccurrence(opts) => Self::AcknowledgeOccurrence(opts.into()),
        }
    }
}

impl From<LoopDispatchOpts> for command::LoopDispatchRequest {
    fn from(_: LoopDispatchOpts) -> Self {
        Self {}
    }
}

impl From<LoopTickOpts> for command::LoopTickRequest {
    fn from(opts: LoopTickOpts) -> Self {
        Self {
            workflow: Some(opts.workflow),
            lease_ttl_seconds: opts.tuning.lease_ttl_seconds,
            max_attempts: opts.tuning.max_attempts,
            backoff_seconds: opts.tuning.backoff_seconds,
        }
    }
}

impl From<LoopStatusOpts> for command::LoopStatusRequest {
    fn from(opts: LoopStatusOpts) -> Self {
        Self {
            workflow: opts.workflow,
        }
    }
}

impl From<LoopRunOpts> for command::LoopRunRequest {
    fn from(opts: LoopRunOpts) -> Self {
        Self {
            workflow: Some(opts.workflow),
            until: opts.until,
            max_ticks: opts.max_ticks,
            lease_ttl_seconds: opts.tuning.lease_ttl_seconds,
            max_attempts: opts.tuning.max_attempts,
            backoff_seconds: opts.tuning.backoff_seconds,
        }
    }
}

impl From<LoopClearAttemptOpts> for command::LoopClearAttemptRequest {
    fn from(opts: LoopClearAttemptOpts) -> Self {
        Self {
            workflow: opts.workflow,
            item: opts.item,
        }
    }
}

impl From<LoopAcknowledgeOccurrenceOpts> for command::LoopAcknowledgeOccurrenceRequest {
    fn from(opts: LoopAcknowledgeOccurrenceOpts) -> Self {
        Self {
            occurrence: opts.occurrence,
        }
    }
}
