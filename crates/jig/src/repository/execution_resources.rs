//! Resolve declared identities; admission and ownership remain executor-owned.
use std::time::{Duration, Instant};

use anyhow::Result;
use jig_contract::{ExecutionResourceV1, PlannedTarget};

use crate::{context::RepoContext, state::ResourceClaim};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResolvedResources {
    pub(crate) claims: Vec<ResourceClaim>,
    /// Only unproved Cargo authority has a repository-local partial fallback.
    pub(crate) partial_reason: Option<&'static str>,
    pub(super) identity: Vec<String>,
}

impl ResolvedResources {
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self == other
    }
}

pub(crate) fn has_browser_policy(planned: &PlannedTarget) -> bool {
    planned
        .resources
        .iter()
        .any(|resource| matches!(resource, ExecutionResourceV1::PlaywrightServersV1 {}))
}

pub(crate) fn waiting_message(planned: &PlannedTarget) -> &'static [u8] {
    if has_browser_policy(planned) {
        b"Waiting for an owned browser endpoint resource...\n"
    } else {
        b"Waiting for a Cargo build resource...\n"
    }
}

pub(crate) fn resolve(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<ResolvedResources> {
    let started = Instant::now();
    let remaining = || -> Result<Duration> {
        if cancelled() {
            return Err(super::cargo_resources::CargoResourceStop::Cancelled.into());
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(super::cargo_resources::CargoResourceStop::TimedOut.into());
        }
        Ok(remaining)
    };
    let mut resolved = ResolvedResources::default();
    if planned
        .resources
        .iter()
        .any(|resource| matches!(resource, ExecutionResourceV1::CargoV1 { .. }))
    {
        resolved = super::cargo_resources::resolve(ctx, planned, remaining()?, cancelled)?;
    }
    if has_browser_policy(planned) {
        let browser = super::playwright_resources::resolve(ctx, planned, remaining()?, cancelled)?;
        resolved.claims.extend(browser.claims);
        resolved.identity.extend(browser.identity);
    }
    remaining()?;
    Ok(resolved)
}
