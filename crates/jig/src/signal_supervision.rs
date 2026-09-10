use anyhow::Result;

/// Runs one operation with an owned cancellation callback, then retires its session.
pub(crate) fn supervise<T>(
    start_message: &'static str,
    retirement_message: &'static str,
    operation: impl FnOnce(Box<dyn Fn() -> bool + Send + Sync>) -> Result<T>,
) -> Result<T> {
    #[cfg(all(unix, not(test)))]
    {
        let session = crate::doctor::DoctorSignalSession::start()
            .map_err(|_| anyhow::anyhow!(start_message))?;
        let cancellation = session.cancellation();
        let outcome = operation(Box::new(move || cancellation.cancelled()));
        finish(outcome, session.finish(), retirement_message)
    }
    #[cfg(any(not(unix), test))]
    {
        let _ = (start_message, retirement_message);
        operation(Box::new(|| false))
    }
}

#[cfg(unix)]
pub(crate) fn finish<T>(
    outcome: Result<T>,
    retirement: std::io::Result<()>,
    retirement_message: &'static str,
) -> Result<T> {
    match retirement {
        Ok(()) => outcome,
        Err(_) => match outcome {
            Ok(_) => Err(anyhow::anyhow!(retirement_message)),
            Err(error) => Err(error.context(format!(
                "{retirement_message}; the supervised operation also failed"
            ))),
        },
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn retirement_preserves_success_and_operation_errors() {
        assert_eq!(finish(Ok(42), Ok(()), "retirement failed").unwrap(), 42);
        let error = finish::<()>(
            Err(anyhow::anyhow!("operation failed")),
            Ok(()),
            "retirement failed",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "operation failed");
    }

    #[test]
    fn retirement_failure_prevents_success() {
        let error = finish(
            Ok(42),
            Err(std::io::Error::other("internal detail")),
            "retirement failed",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "retirement failed");
    }

    #[test]
    fn signal_retirement_failure_retains_the_operation_error() {
        let error = finish::<()>(
            Err(anyhow::anyhow!("picker drawing failed")),
            Err(std::io::Error::other("handler restoration failed")),
            "Home picker signal supervision could not retire safely",
        )
        .unwrap_err();
        let rendered = format!("{error:#}");
        assert!(
            rendered.contains("signal supervision could not retire safely"),
            "{rendered}"
        );
        assert!(rendered.contains("picker drawing failed"), "{rendered}");
    }
}
