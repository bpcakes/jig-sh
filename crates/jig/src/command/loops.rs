use serde::Deserialize;

#[derive(Debug)]
pub(crate) enum LoopCommand {
    Tick(LoopTickRequest),
    Dispatch(LoopDispatchRequest),
    Status(LoopStatusRequest),
    Run(LoopRunRequest),
    ClearAttempt(LoopClearAttemptRequest),
    AcknowledgeOccurrence(LoopAcknowledgeOccurrenceRequest),
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoopDispatchRequest {}

#[derive(Debug, Deserialize)]
pub(crate) struct LoopTickRequest {
    pub(crate) workflow: Option<String>,
    pub(crate) lease_ttl_seconds: Option<u64>,
    pub(crate) max_attempts: Option<u32>,
    pub(crate) backoff_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoopStatusRequest {
    pub(crate) workflow: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoopRunRequest {
    pub(crate) workflow: Option<String>,
    #[serde(default = "default_until")]
    pub(crate) until: String,
    #[serde(default = "default_max_ticks")]
    pub(crate) max_ticks: u32,
    pub(crate) lease_ttl_seconds: Option<u64>,
    pub(crate) max_attempts: Option<u32>,
    pub(crate) backoff_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoopClearAttemptRequest {
    pub(crate) workflow: String,
    pub(crate) item: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoopAcknowledgeOccurrenceRequest {
    pub(crate) occurrence: String,
}

fn default_until() -> String {
    "idle".into()
}

const fn default_max_ticks() -> u32 {
    10
}
