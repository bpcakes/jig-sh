use std::fmt;

use jig_file_budget::{MeasurementErrorKindV1, MeasurementErrorV1};
use jig_owned_process::OwnedProcessTreeError;

#[derive(Debug)]
pub(super) struct ChangedDuringRead(pub(super) String);

impl fmt::Display for ChangedDuringRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "file-budget path `{}` changed while it was being measured",
            self.0
        )
    }
}

impl std::error::Error for ChangedDuringRead {}

pub(super) fn diagnostic_code(error: &anyhow::Error) -> &'static str {
    if error
        .downcast_ref::<MeasurementErrorV1>()
        .is_some_and(|error| {
            matches!(
                error.kind,
                MeasurementErrorKindV1::PerFileLimit
                    | MeasurementErrorKindV1::TotalLimit
                    | MeasurementErrorKindV1::CounterOverflow
            )
        })
        || matches!(
            error.downcast_ref::<OwnedProcessTreeError>(),
            Some(OwnedProcessTreeError::OutputLimitExceeded(_))
        )
    {
        "file_budget.resource_limit"
    } else if error.is::<ChangedDuringRead>()
        || error.is::<crate::native_input::PolicyChangedDuringRead>()
    {
        "file_budget.changed_during_read"
    } else {
        "file_budget.scope_incomplete"
    }
}

#[cfg(test)]
mod tests {
    use super::{ChangedDuringRead, diagnostic_code};
    use jig_file_budget::{MeasurementErrorKindV1, MeasurementErrorV1};
    use jig_owned_process::{OwnedProcessOutputStream, OwnedProcessTreeError};

    #[test]
    fn error_classification_uses_types_through_context() {
        for kind in [
            MeasurementErrorKindV1::PerFileLimit,
            MeasurementErrorKindV1::TotalLimit,
            MeasurementErrorKindV1::CounterOverflow,
        ] {
            let error = anyhow::Error::new(MeasurementErrorV1 {
                kind,
                observed_bytes: 2,
                limit_bytes: Some(1),
            })
            .context("additional context");
            assert_eq!(diagnostic_code(&error), "file_budget.resource_limit");
        }
        for stream in [
            OwnedProcessOutputStream::Stdout,
            OwnedProcessOutputStream::Stderr,
        ] {
            let error = anyhow::Error::new(OwnedProcessTreeError::OutputLimitExceeded(stream))
                .context("Git capture");
            assert_eq!(diagnostic_code(&error), "file_budget.resource_limit");
        }
        let error =
            anyhow::Error::new(ChangedDuringRead("example.rs".into())).context("measurement");
        assert_eq!(diagnostic_code(&error), "file_budget.changed_during_read");
        let error =
            anyhow::Error::new(crate::native_input::PolicyChangedDuringRead).context("policy");
        assert_eq!(diagnostic_code(&error), "file_budget.changed_during_read");
    }

    #[test]
    fn paths_and_error_wording_cannot_choose_a_diagnostic_code() {
        for message in [
            "could not open output limit/example.rs",
            "changed while/example.rs is not a file",
            "output limit changed while",
        ] {
            assert_eq!(
                diagnostic_code(&anyhow::anyhow!(message)),
                "file_budget.scope_incomplete"
            );
        }
    }
}
