//! Agent providers Jig launches: Claude and Codex home discovery, inspection,
//! credential lookup, and transparent launch preparation behind the shared
//! `AgentProvider` boundary.

pub mod agent_provider;
pub mod claude;
pub mod codex;
pub mod home_paths;

#[cfg(test)]
use jig_context::test_support as test_env;
