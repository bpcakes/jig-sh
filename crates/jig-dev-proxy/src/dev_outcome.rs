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
        formatter.write_str("development operation failed after completing recovery actions")
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

pub(crate) fn command_failed_error(message: impl Into<String>) -> Value {
    serde_json::json!({
        "kind": "command_failed",
        "message": message.into(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn recovery_error_chain_does_not_repeat_its_source() {
        let error = with_recoveries(
            anyhow::anyhow!("inner failure").context("outer failure"),
            json!([]),
        );
        let chain = format!("{error:#}");

        assert_eq!(chain.matches("outer failure").count(), 1, "{chain}");
        assert_eq!(chain.matches("inner failure").count(), 1, "{chain}");
    }
}
