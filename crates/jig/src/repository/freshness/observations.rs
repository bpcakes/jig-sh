//! Test-only physical-work counters. They never enter proof identity or output schemas.
use std::cell::Cell;
use std::time::Instant;

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct Metrics {
    pub(crate) original_index_scans: u64,
    pub(crate) original_index_us: u64,
    pub(crate) original_reads: u64,
    pub(crate) original_read_us: u64,
    pub(crate) source_collections: u64,
    pub(crate) source_collection_us: u64,
    pub(crate) identity_collections: u64,
    pub(crate) identity_collection_us: u64,
}

thread_local! {
    static METRICS: Cell<Metrics> = Cell::new(Metrics::default());
}

pub(crate) fn reset() {
    METRICS.set(Metrics::default());
}

pub(crate) fn snapshot() -> Metrics {
    METRICS.get()
}

pub(crate) enum Phase {
    OriginalIndex,
    OriginalRead,
    Source,
    Identity,
}

pub(crate) struct Measurement {
    phase: Phase,
    started: Instant,
}

pub(crate) fn measure(phase: Phase) -> Measurement {
    Measurement {
        phase,
        started: Instant::now(),
    }
}

impl Drop for Measurement {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        let mut metrics = METRICS.get();
        let (count, time) = match self.phase {
            Phase::OriginalIndex => (
                &mut metrics.original_index_scans,
                &mut metrics.original_index_us,
            ),
            Phase::OriginalRead => (&mut metrics.original_reads, &mut metrics.original_read_us),
            Phase::Source => (
                &mut metrics.source_collections,
                &mut metrics.source_collection_us,
            ),
            Phase::Identity => (
                &mut metrics.identity_collections,
                &mut metrics.identity_collection_us,
            ),
        };
        *count += 1;
        *time = time.saturating_add(elapsed);
        METRICS.set(metrics);
    }
}

#[test]
#[ignore = "bounded process-per-sample inspection profile; run the dedicated driver"]
fn measurement() {
    use serde_json::{Value, json};

    let root = std::path::PathBuf::from(std::env::var_os("JIG_INSPECTION_PROFILE_ROOT").unwrap());
    let command = std::env::var("JIG_INSPECTION_PROFILE_COMMAND").unwrap();
    let plan = std::env::var("JIG_INSPECTION_PROFILE_PLAN").unwrap();
    reset();
    crate::state::reset_dashboard_scan_counts();
    crate::state::reset_work_gate_receipt_index_scan_count();
    let started = Instant::now();
    let ctx = crate::context::RepoContext::load_from(&root).unwrap();
    let result = match command.as_str() {
        "status" => {
            crate::status::snapshot_with_freshness_timeout(&ctx, &|| false, Some(30_000)).unwrap()
        }
        "compact" => crate::runtime::call_tool_on_surface(
            &ctx,
            crate::tool_defs::tool::WORK_GATES,
            json!({"plan_id": plan, "freshness_timeout_ms": 30_000}),
            crate::surface::ResponseSurface::AgentV1,
        )
        .unwrap(),
        _ => panic!("unsupported profile command"),
    };
    let elapsed_us = started.elapsed().as_micros();
    let gates: Vec<&Value> = if command == "status" {
        result["work"]["gates"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|plan| plan["snapshot"]["gates"].as_array().unwrap())
            .collect()
    } else {
        result["gates"].as_array().unwrap().iter().collect()
    };
    println!(
        "PROFILE {}",
        json!({
            "command": command, "api_elapsed_us": elapsed_us, "metrics": snapshot(),
            "receipt_reducer_scans": crate::state::work_gate_receipt_index_scan_count(),
            "dashboard_receipt_scans": crate::state::dashboard_scan_count(&ctx.state_file("receipts.jsonl")),
            "gate_statuses": gates.iter().map(|gate| &gate["status"]).collect::<Vec<_>>(),
            "collection": gates.iter().filter_map(|gate| gate.get("freshness_collection")).collect::<Vec<_>>(),
        })
    );
}
