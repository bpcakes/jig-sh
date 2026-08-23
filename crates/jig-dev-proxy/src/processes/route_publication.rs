use std::process::Child;

use anyhow::{Result, bail};

use crate::state::StateStore;
use crate::types::{Route, RouteMode};

use super::{
    TerminationReason, lock_outcome_or_interruption, verify_listener_ownership_and_observe_child,
    verify_process_route_owner,
};

pub(super) fn publish_process_route_interruptible(
    store: &StateStore,
    route: Route,
    app_name: &str,
    child: &mut Child,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    publish_process_route_interruptible_with_verifier(
        store,
        route,
        app_name,
        child,
        interrupt_requested,
        verify_process_route_candidate,
    )
}

pub(super) fn publish_process_route_interruptible_with_verifier(
    store: &StateStore,
    route: Route,
    app_name: &str,
    child: &mut Child,
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
    mut verify_owner: impl FnMut(&str, &Route, &mut Child) -> Result<()>,
) -> Result<()> {
    let cancelled = || interrupt_requested().is_some();
    let outcome = store.add_verified_route_interruptible(route, &cancelled, |candidate| {
        verify_listener_ownership_and_observe_child(
            app_name,
            candidate.target_host.as_str(),
            candidate.target_port,
            child,
            |child| verify_owner(app_name, candidate, child),
        )
    })?;
    lock_outcome_or_interruption(outcome, interrupt_requested)
}

fn verify_process_route_candidate(app_name: &str, route: &Route, child: &mut Child) -> Result<()> {
    if route.mode != RouteMode::Process {
        bail!(
            "Route '{}' must use process mode before owner verification",
            route.hostname
        );
    }
    let child_pid = child.id();
    if route.owner_pid != Some(child_pid) {
        bail!(
            "Process route '{}' records owner PID {:?}, but app '{}' is supervised as child PID {child_pid}; refusing to verify or publish a mismatched route",
            route.hostname,
            route.owner_pid,
            app_name
        );
    }
    let owner_start_token = route.owner_start_token.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Process route '{}' has no owner start token; refusing to verify or publish it",
            route.hostname
        )
    })?;
    verify_process_route_owner(
        app_name,
        route.target_host.as_str(),
        route.target_port,
        child_pid,
        Some(owner_start_token),
    )
}
