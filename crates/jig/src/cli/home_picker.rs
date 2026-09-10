use anyhow::Result;

pub(super) fn supervise<T>(
    pick: impl FnOnce(Box<dyn Fn() -> bool + Send + Sync>) -> Result<T>,
) -> Result<T> {
    crate::signal_supervision::supervise(
        "Home picker was not started because the process-wide signal session is unavailable",
        "Home picker signal supervision could not retire safely",
        pick,
    )
}
