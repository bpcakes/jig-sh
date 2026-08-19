use std::error::Error as StdError;
use std::fmt;

use serde_json::Value;

#[derive(Debug)]
struct DevErrorWithRecoveries {
    source: anyhow::Error,
    recoveries: Value,
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
    DevErrorWithRecoveries { source, recoveries }.into()
}

pub(crate) fn parts(error: &anyhow::Error) -> (&anyhow::Error, Option<&Value>) {
    error
        .downcast_ref::<DevErrorWithRecoveries>()
        .map_or((error, None), |context| {
            (&context.source, Some(&context.recoveries))
        })
}

pub(crate) fn source(error: &anyhow::Error) -> &anyhow::Error {
    parts(error).0
}
