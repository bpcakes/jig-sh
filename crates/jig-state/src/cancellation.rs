use anyhow::{Result, bail};

#[derive(Debug)]
pub struct StatusCollectionCancelled;

impl std::fmt::Display for StatusCollectionCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("status collection was cancelled")
    }
}

impl std::error::Error for StatusCollectionCancelled {}

pub fn ensure_status_collection_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        bail!(StatusCollectionCancelled);
    }
    Ok(())
}

pub fn status_collection_cancellation() -> anyhow::Error {
    StatusCollectionCancelled.into()
}

pub fn is_status_collection_cancellation(error: &anyhow::Error) -> bool {
    error.is::<StatusCollectionCancelled>()
}
