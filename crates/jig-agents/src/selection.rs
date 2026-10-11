//! Headless subscription selection. Identity stays in provider-owned choices;
//! reports are diagnostic data and are never resolved back into launch homes.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::agent_provider::{AgentProvider, Discovery, InspectionMode};
use assessment::{Assessment, Reason};

mod assessment;
#[cfg(test)]
mod tests;

/// Inspection is cooperative; owned child cleanup completes before returning.
const INSPECTION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Selection<H> {
    pub home: Option<H>,
    pub report: Value,
}

pub fn select<P: AgentProvider>(
    provider: &P,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Selection<P::Home>> {
    select_with_timeout(provider, cancelled, INSPECTION_TIMEOUT)
}

fn select_with_timeout<P: AgentProvider>(
    provider: &P,
    cancelled: &(dyn Fn() -> bool + Sync),
    timeout: Duration,
) -> Result<Selection<P::Home>> {
    let bucket = P::METADATA
        .subscription_bucket
        .filter(|_| P::METADATA.usage)
        .context("This provider does not support automatic subscription selection")?;
    if cancelled() {
        bail!("Automatic home selection was cancelled");
    }
    let Discovery {
        choices,
        warnings,
        inspection,
    } = provider.discover(InspectionMode::Headless)?;
    let inspection = inspection.context("This provider does not support usage inspection")?;
    let started = Instant::now();
    let stopped = || cancelled() || started.elapsed() >= timeout;
    let mut samples = (0..choices.len()).map(|_| None).collect::<Vec<_>>();
    inspection
        .inspect(
            &mut |index, details| {
                if stopped() {
                    return Ok(());
                }
                let sample = samples
                    .get_mut(index)
                    .ok_or("Inspection returned an unknown home index")?;
                *sample = Some((details, chrono::Utc::now().timestamp()));
                Ok(())
            },
            &stopped,
        )
        .map_err(anyhow::Error::msg)?;
    if cancelled() {
        bail!("Automatic home selection was cancelled");
    }
    let timed_out = started.elapsed() >= timeout;
    let now = chrono::Utc::now().timestamp();
    let mut best: Option<(usize, f64)> = None;
    let mut candidates = Vec::with_capacity(choices.len());
    let mut partial = timed_out || !warnings.is_empty();
    for (index, (choice, sample)) in choices.iter().zip(&samples).enumerate() {
        let assessment = match sample {
            Some((details, _)) => assessment::assess(details, bucket, now),
            None => Assessment::unavailable(Reason::InspectionUnavailable),
        };
        partial |= !matches!(
            assessment.reason,
            Reason::Eligible | Reason::Exhausted | Reason::SignedOut
        );
        if assessment.reason == Reason::Eligible
            && let Some(score) = assessment.remaining_percent
        {
            // Discovery order is stable; ties never depend on worker completion.
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((index, score));
            }
        }
        candidates.push(json!({
            "index": index,
            "name": choice.name,
            "path": choice.path.to_string_lossy(),
            "representation_lossy": choice.path.to_str().is_none(),
            "details": choice.details,
            "current": choice.current,
            "observed_at": sample.as_ref().map(|(_, at)| at),
            "assessment": assessment,
        }));
    }
    let selected_index = best.map(|(index, _)| index);
    let home = selected_index
        .map(|index| provider.revalidate(&choices[index].selection))
        .transpose()?;
    Ok(Selection {
        home,
        report: json!({
            "strategy": "headroom",
            "scope": "all_reported_windows",
            "reason": if selected_index.is_some() { "selected" } else { "no_eligible_home" },
            "selected_index": selected_index,
            "assessed_at": now,
            "partial": partial,
            "timed_out": timed_out,
            "warnings": warnings,
            "candidates": candidates,
        }),
    })
}
