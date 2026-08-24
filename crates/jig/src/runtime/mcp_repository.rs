use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jig_contract::{ActionEffect, RunStatus};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::context::RepoContext;
use crate::repository::{InspectRequest, PlanRunRequest, RepositoryCatalog};
use crate::tool_defs::{
    CancelRunArgs, CancelRunOutput, ExecuteRunArgs, ExecuteRunOutput, PlanRunArgs, PlanRunOutput,
    RepositoryInspectArgs, RepositoryInspectOutput, RepositoryInspectResult, RepositoryTool,
    RunInspection,
};

use super::run_execution::{
    ExecuteCheckRunRequest, execute_started_check_run, start_check_run_with_event_cursor,
};

const REPOSITORY_MCP_SCHEMA_VERSION: u32 = 1;
const DURABLE_CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LiveRunKey {
    root: PathBuf,
    run_id: String,
}

static LIVE_RUNS: LazyLock<Mutex<HashMap<LiveRunKey, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct LiveRunGuard {
    key: LiveRunKey,
}

impl Drop for LiveRunGuard {
    fn drop(&mut self) {
        live_runs().remove(&self.key);
    }
}

struct RunCancellationProbe {
    ctx: RepoContext,
    run_id: String,
    signalled: Arc<AtomicBool>,
    last_durable_check: Mutex<Option<Instant>>,
    event_cursor: Mutex<crate::state::RunEventCursor>,
    poll_failure: Mutex<Option<String>>,
}

impl RunCancellationProbe {
    fn new(ctx: RepoContext, run_id: String, event_cursor: crate::state::RunEventCursor) -> Self {
        Self {
            ctx,
            run_id,
            signalled: Arc::new(AtomicBool::new(false)),
            last_durable_check: Mutex::new(None),
            event_cursor: Mutex::new(event_cursor),
            poll_failure: Mutex::new(None),
        }
    }

    fn signal(&self) {
        self.signalled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> Result<bool> {
        if let Some(message) = self
            .poll_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            bail!(message);
        }
        if self.signalled.load(Ordering::Acquire) {
            return Ok(true);
        }

        let now = Instant::now();
        let mut last_check = self
            .last_durable_check
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last_check
            .is_some_and(|last| now.duration_since(last) < DURABLE_CANCELLATION_POLL_INTERVAL)
        {
            return Ok(false);
        }
        *last_check = Some(now);
        drop(last_check);

        let mut cursor = self
            .event_cursor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let requested =
            crate::state::run_cancel_requested_since(&self.ctx, &self.run_id, &mut cursor, &|| {
                self.signalled.load(Ordering::Acquire)
            });
        match requested {
            Ok(true) => {
                self.signal();
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(_) if self.signalled.load(Ordering::Acquire) => Ok(true),
            Err(error) => {
                let message = format!(
                    "failed to inspect durable cancellation state for run '{}': {error:#}",
                    self.run_id
                );
                *self
                    .poll_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message.clone());
                self.signal();
                bail!(message)
            }
        }
    }
}

pub(super) fn call(ctx: &RepoContext, tool: RepositoryTool, args: Value) -> Result<Value> {
    match tool {
        RepositoryTool::Inspect => inspect(ctx, parse_inspect_args(args)?),
        RepositoryTool::PlanRun => plan(ctx, parse_args(args, "jig.plan_run")?),
        RepositoryTool::ExecuteRun => execute(ctx, parse_args(args, "jig.execute_run")?),
        RepositoryTool::CancelRun => cancel(ctx, parse_args(args, "jig.cancel_run")?),
    }
}

fn inspect(ctx: &RepoContext, args: RepositoryInspectArgs) -> Result<Value> {
    let kind = args.kind();
    let result = match args {
        RepositoryInspectArgs::Workspace => catalog_inspection(ctx, InspectRequest::Workspace)?,
        RepositoryInspectArgs::Components => catalog_inspection(ctx, InspectRequest::Components)?,
        RepositoryInspectArgs::Component { id } => {
            catalog_inspection(ctx, InspectRequest::Component(id))?
        }
        RepositoryInspectArgs::Targets => catalog_inspection(ctx, InspectRequest::Targets)?,
        RepositoryInspectArgs::Target { id } => {
            catalog_inspection(ctx, InspectRequest::Target(id))?
        }
        RepositoryInspectArgs::Profiles => catalog_inspection(ctx, InspectRequest::Profiles)?,
        RepositoryInspectArgs::Profile { id } => {
            catalog_inspection(ctx, InspectRequest::Profile(id))?
        }
        RepositoryInspectArgs::Run { run_id } => {
            let run = crate::state::reconcile_run_for_inspection(ctx, &run_id)?;
            RepositoryInspectResult::Run(RunInspection { run })
        }
    };
    serialize_output(RepositoryInspectOutput {
        ok: true,
        schema_version: REPOSITORY_MCP_SCHEMA_VERSION,
        kind,
        result,
    })
}

fn catalog_inspection(
    ctx: &RepoContext,
    request: InspectRequest,
) -> Result<RepositoryInspectResult> {
    let current = super::refreshed_repository_context(ctx)?;
    Ok(RepositoryInspectResult::Catalog(
        crate::repository::inspect_repository_data(&current, request)?,
    ))
}

fn plan(ctx: &RepoContext, args: PlanRunArgs) -> Result<Value> {
    let current = super::refreshed_repository_context(ctx)?;
    let catalog = RepositoryCatalog::from_context(&current)?;
    let arguments = args
        .arguments
        .into_iter()
        .map(|(target, arguments)| {
            let target = target.parse().with_context(|| {
                format!("Invalid target key {target:?} in jig.plan_run arguments")
            })?;
            Ok((target, arguments))
        })
        .collect::<Result<_>>()?;
    let plan = crate::repository::plan_action_run(
        &current,
        &catalog,
        PlanRunRequest {
            selectors: args.selectors,
            profile: args.profile,
            affected_base: args.affected_base,
        },
        arguments,
    )?;
    serialize_output(PlanRunOutput {
        ok: true,
        schema_version: REPOSITORY_MCP_SCHEMA_VERSION,
        plan,
    })
}

fn execute(ctx: &RepoContext, args: ExecuteRunArgs) -> Result<Value> {
    let current = super::refreshed_repository_context(ctx)?;
    let catalog = RepositoryCatalog::from_context(&current)?;
    validate_effect_approval(&args.plan.effects, &args.approved_effects)?;
    if let Some(plan_id) = args.work_plan_id.as_deref() {
        crate::state::ensure_plan_is_open(&current, plan_id)?;
    }

    let worker_ctx = current;
    let worker_catalog = catalog;
    let worker_plan = args.plan;
    let work_plan_id = args.work_plan_id;
    let request = ExecuteCheckRunRequest {
        work_plan_id: work_plan_id.clone(),
        record_receipts: args.record_receipts,
        fail_fast: args.fail_fast,
    };
    let (started_tx, started_rx) = mpsc::sync_channel::<Result<crate::state::DurableRun>>(1);
    thread::Builder::new()
        .name("jig-repository-run".into())
        .spawn(move || {
            // start_check_run is the single freshness-validation boundary.
            // The accepted handle is not published until that validation and
            // durable run creation both succeed, so validating once here keeps
            // the same safety ordering without an extra full fingerprint scan
            // on the request thread.
            let (run, _lease, event_cursor) = match start_check_run_with_event_cursor(
                &worker_ctx,
                &worker_catalog,
                worker_plan,
                work_plan_id,
            ) {
                Ok(started) => started,
                Err(error) => {
                    let _ = started_tx.send(Err(error));
                    return;
                }
            };
            let cancellation = RunCancellationProbe::new(
                worker_ctx.clone(),
                run.result.run_id.clone(),
                event_cursor,
            );
            let _guard =
                register_live_run(&worker_ctx, &run.result.run_id, &cancellation.signalled);
            if started_tx.send(Ok(run.clone())).is_err() {
                cancellation.signal();
            }
            // Execution owns terminalization. If that durable write itself
            // fails, releasing this worker's lease lets inspection reconcile
            // the abandoned run without a second competing terminalization.
            let _ = execute_started_check_run(&worker_ctx, &worker_catalog, run, request, &|| {
                cancellation.is_cancelled()
            });
        })
        .context("Failed to start the repository run worker")?;

    let run = started_rx
        .recv()
        .context("Repository run worker exited before publishing a durable handle")??;
    serialize_output(ExecuteRunOutput {
        ok: true,
        schema_version: REPOSITORY_MCP_SCHEMA_VERSION,
        accepted: true,
        run_id: run.result.run_id.clone(),
        status: run.result.status,
        run: run.result,
    })
}

fn validate_effect_approval(planned: &[ActionEffect], approved: &[ActionEffect]) -> Result<()> {
    let requires_approval = planned
        .iter()
        .copied()
        .filter(|effect| matches!(effect, ActionEffect::Worktree | ActionEffect::External))
        .collect::<std::collections::BTreeSet<_>>();
    let approved = approved
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if approved != requires_approval {
        bail!(
            "jig.execute_run requires approved_effects {:?} for this exact plan",
            requires_approval
        );
    }
    Ok(())
}

fn cancel(ctx: &RepoContext, args: CancelRunArgs) -> Result<Value> {
    if args.run_id.trim().is_empty() {
        bail!("jig.cancel_run requires a nonblank run_id");
    }
    // Cancellation deliberately uses the request's already-authorized root
    // context. It must remain available when a running target leaves current
    // repository configuration temporarily invalid; run IDs, journals, leases,
    // and live-worker keys are all rooted independently of mutable authority.
    let reconciled = crate::state::reconcile_run_for_inspection(ctx, &args.run_id)?;
    let run = if reconciled.result.status == RunStatus::Completed {
        reconciled
    } else {
        crate::state::request_run_cancel(ctx, &args.run_id)?
    };
    let worker_signalled =
        run.result.status != RunStatus::Completed && signal_live_run(ctx, &args.run_id);
    serialize_output(CancelRunOutput {
        ok: true,
        schema_version: REPOSITORY_MCP_SCHEMA_VERSION,
        run_id: args.run_id,
        cancellation_requested: worker_signalled || run.cancel_requested,
        worker_signalled,
        run: run.result,
    })
}

fn parse_args<T: DeserializeOwned>(args: Value, tool: &str) -> Result<T> {
    serde_json::from_value(args).with_context(|| format!("Invalid arguments for {tool}"))
}

fn parse_inspect_args(args: Value) -> Result<RepositoryInspectArgs> {
    let object = args
        .as_object()
        .ok_or_else(|| anyhow!("jig.inspect arguments must be an object"))?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("jig.inspect requires string field kind"))?;
    let allowed = match kind {
        "component" | "target" | "profile" => &["kind", "id"][..],
        "run" => &["kind", "run_id"][..],
        "workspace" | "components" | "targets" | "profiles" => &["kind"][..],
        _ => &["kind"][..],
    };
    if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        bail!("Invalid arguments for jig.inspect: unknown field '{unknown}'");
    }
    parse_args(args, "jig.inspect")
}

fn serialize_output(output: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(output).context("Failed to serialize repository MCP output")
}

fn register_live_run(
    ctx: &RepoContext,
    run_id: &str,
    cancellation: &Arc<AtomicBool>,
) -> LiveRunGuard {
    let key = live_run_key(ctx, run_id);
    live_runs().insert(key.clone(), Arc::clone(cancellation));
    LiveRunGuard { key }
}

fn signal_live_run(ctx: &RepoContext, run_id: &str) -> bool {
    let cancellation = live_runs().get(&live_run_key(ctx, run_id)).cloned();
    if let Some(cancellation) = cancellation {
        cancellation.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

fn live_run_key(ctx: &RepoContext, run_id: &str) -> LiveRunKey {
    LiveRunKey {
        root: ctx.root().to_path_buf(),
        run_id: run_id.to_owned(),
    }
}

fn live_runs() -> std::sync::MutexGuard<'static, HashMap<LiveRunKey, Arc<AtomicBool>>> {
    LIVE_RUNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub(super) fn is_live_run_registered(ctx: &RepoContext, run_id: &str) -> bool {
    live_runs().contains_key(&live_run_key(ctx, run_id))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    #[test]
    fn durable_cancellation_poll_failures_are_cached_and_reported() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let (run, _lease, cursor) = crate::state::start_run_with_event_cursor(
            &ctx,
            jig_contract::RunPlan::new(
                "run-plan_fixture",
                "sha256:config",
                jig_contract::SourceIdentity::new(None, "sha256:worktree"),
                Vec::new(),
                Vec::new(),
            ),
            None,
        )
        .unwrap();
        fs::write(ctx.state_file("runs.jsonl"), "not-json\n").unwrap();
        let probe = RunCancellationProbe::new(ctx, run.result.run_id, cursor);

        let first = probe.is_cancelled().unwrap_err().to_string();
        let second = probe.is_cancelled().unwrap_err().to_string();

        assert!(first.contains("failed to inspect durable cancellation state"));
        assert!(first.contains("Failed to parse run event identity"));
        assert_eq!(second, first);
    }
}
