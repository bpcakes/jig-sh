use clap::ValueEnum;

/// Process-selected response surface. Standard preserves the existing public
/// wire contract; agent-v1 carries explicitly opted-in agent-oriented schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum ResponseSurface {
    #[default]
    Standard,
    #[value(name = "agent-v1")]
    AgentV1,
}
