use serde::Deserialize;

#[derive(Debug)]
pub enum LoopCommand {
    Tick(LoopTickRequest),
    Dispatch(LoopDispatchRequest),
    Status(LoopStatusRequest),
    Show(LoopShowRequest),
    Run(LoopRunRequest),
    ClearAttempt(LoopClearAttemptRequest),
    AcknowledgeOccurrence(LoopAcknowledgeOccurrenceRequest),
}

#[derive(Debug, Deserialize)]
pub struct LoopDispatchRequest {}

#[derive(Debug, Deserialize)]
pub struct LoopTickRequest {
    pub workflow: Option<String>,
    pub lease_ttl_seconds: Option<u64>,
    pub max_attempts: Option<u32>,
    pub backoff_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct LoopStatusRequest {
    pub workflow: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoopShowRequest {
    pub occurrence: String,
}

#[derive(Debug, Deserialize)]
pub struct LoopRunRequest {
    pub workflow: Option<String>,
    #[serde(default = "default_until")]
    pub until: String,
    #[serde(default = "default_max_ticks")]
    pub max_ticks: u32,
    pub lease_ttl_seconds: Option<u64>,
    pub max_attempts: Option<u32>,
    pub backoff_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct LoopClearAttemptRequest {
    pub workflow: String,
    pub item: String,
}

#[derive(Debug, Deserialize)]
pub struct LoopAcknowledgeOccurrenceRequest {
    pub occurrence: String,
}

fn default_until() -> String {
    "idle".into()
}

const fn default_max_ticks() -> u32 {
    10
}
