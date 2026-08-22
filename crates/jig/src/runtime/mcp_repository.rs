use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jig_contract::RunStatus;
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
    ExecuteCheckRunRequest, block_started_check_run, execute_started_check_run, start_check_run,
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
}

impl RunCancellationProbe {
    fn new(ctx: RepoContext, run_id: String) -> Self {
        Self {
            ctx,
            run_id,
            signalled: Arc::new(AtomicBool::new(false)),
            last_durable_check: Mutex::new(None),
        }
    }

    fn signal(&self) {
        self.signalled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        if self.signalled.load(Ordering::Acquire) {
            return true;
        }

        let now = Instant::now();
        let mut last_check = self
            .last_durable_check
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last_check
            .is_some_and(|last| now.duration_since(last) < DURABLE_CANCELLATION_POLL_INTERVAL)
        {
            return false;
        }
        *last_check = Some(now);
        drop(last_check);

        let requested =
            crate::state::run_by_id(&self.ctx, &self.run_id).is_ok_and(|run| run.cancel_requested);
        if requested {
            self.signal();
        }
        requested
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
            let run = crate::state::run_by_id(ctx, &run_id)?;
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
    Ok(RepositoryInspectResult::Catalog(
        crate::repository::inspect_repository_data(ctx, request)?,
    ))
}

fn plan(ctx: &RepoContext, args: PlanRunArgs) -> Result<Value> {
    let catalog = RepositoryCatalog::from_context(ctx)?;
    let plan = crate::repository::plan_run(
        ctx,
        &catalog,
        PlanRunRequest {
            selectors: args.selectors,
            profile: args.profile,
            affected_base: args.affected_base,
        },
    )?;
    serialize_output(PlanRunOutput {
        ok: true,
        schema_version: REPOSITORY_MCP_SCHEMA_VERSION,
        plan,
    })
}

fn execute(ctx: &RepoContext, args: ExecuteRunArgs) -> Result<Value> {
    let catalog = RepositoryCatalog::from_context(ctx)?;
    crate::repository::validate_run_plan(ctx, &catalog, &args.plan)?;
    if let Some(plan_id) = args.work_plan_id.as_deref() {
        crate::state::ensure_plan_is_open(ctx, plan_id)?;
    }

    let worker_ctx = ctx.clone();
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
            let run = match start_check_run(&worker_ctx, &worker_catalog, worker_plan, work_plan_id)
            {
                Ok(run) => run,
                Err(error) => {
                    let _ = started_tx.send(Err(error));
                    return;
                }
            };
            let cancellation =
                RunCancellationProbe::new(worker_ctx.clone(), run.result.run_id.clone());
            let _guard =
                register_live_run(&worker_ctx, &run.result.run_id, &cancellation.signalled);
            if started_tx.send(Ok(run.clone())).is_err() {
                cancellation.signal();
            }
            let run_id = run.result.run_id.clone();
            if let Err(error) =
                execute_started_check_run(&worker_ctx, &worker_catalog, run, request, &|| {
                    cancellation.is_cancelled()
                })
            {
                let _ = block_started_check_run(&worker_ctx, &run_id, &error);
            }
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

fn cancel(ctx: &RepoContext, args: CancelRunArgs) -> Result<Value> {
    if args.run_id.trim().is_empty() {
        bail!("jig.cancel_run requires a nonblank run_id");
    }
    let run = crate::state::request_run_cancel(ctx, &args.run_id)?;
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
