use std::error::Error as StdError;
use std::fmt;

use serde_json::Value;

use crate::dev_sessions::OrphanRecoveryNotice;

#[derive(Debug)]
pub(crate) enum DevRecoveries {
    Serialized(Value),
    Notices(Vec<OrphanRecoveryNotice>),
}

impl DevRecoveries {
    pub(crate) fn to_value(&self) -> serde_json::Result<Value> {
        match self {
            Self::Serialized(value) => Ok(value.clone()),
            Self::Notices(notices) => serde_json::to_value(notices),
        }
    }
}

#[derive(Debug)]
struct DevErrorWithRecoveries {
    source: anyhow::Error,
    recoveries: DevRecoveries,
}

impl fmt::Display for DevErrorWithRecoveries {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if formatter.alternate() {
            write!(formatter, "{:#}", self.source)
        } else {
            write!(formatter, "{}", self.source)
        }
    }
}

impl StdError for DevErrorWithRecoveries {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

pub(crate) fn with_recoveries(source: anyhow::Error, recoveries: Value) -> anyhow::Error {
    DevErrorWithRecoveries {
        source,
        recoveries: DevRecoveries::Serialized(recoveries),
    }
    .into()
}

pub(crate) fn with_recovery_notices(
    source: anyhow::Error,
    recoveries: Vec<OrphanRecoveryNotice>,
) -> anyhow::Error {
    if recoveries.is_empty() {
        return source;
    }
    DevErrorWithRecoveries {
        source,
        recoveries: DevRecoveries::Notices(recoveries),
    }
    .into()
}

pub(crate) fn parts(error: &anyhow::Error) -> (&anyhow::Error, Option<&DevRecoveries>) {
    error
        .downcast_ref::<DevErrorWithRecoveries>()
        .map_or((error, None), |context| {
            (&context.source, Some(&context.recoveries))
        })
}

pub(crate) fn source(error: &anyhow::Error) -> &anyhow::Error {
    parts(error).0
}
