use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use ulid::Ulid;

use crate::command::{
    LoopClearAttemptRequest, LoopCommand, LoopRunRequest, LoopStatusRequest, LoopTickRequest,
};
use crate::context::{LoopConfig, LoopWorkflowConfig, RepoContext};
use crate::execution::{ExecutionControl, ExecutionEvent, PhasePosition};
use crate::state::{ReceiptInput, now_ms, open_plan_summaries, record_receipt};
use crate::tool_defs::{LOOP_CLEAR_ATTEMPT_TOOL, LOOP_TICK_TOOL};

mod github;
mod pr_manager;

const DEFAULT_WORKFLOW_ID: &str = "noop-status";
const GITHUB_PR_STATUS_KIND: &str = "github_pr_status";
const NOOP_STATUS_KIND: &str = "noop_status";
const PR_MANAGER_KIND: &str = "pr_manager";
const LOOP_CACHE_DIR: &str = ".agent/.cache/loop";
const WORKFLOW_LEASE_PREFIX: &str = "workflow:";

pub(super) fn dispatch_with_observer(
    ctx: &RepoContext,
    command: LoopCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        LoopCommand::Tick(request) => tick_with_observer(ctx, request, observer),
        LoopCommand::Status(request) => status(ctx, request),
        LoopCommand::Run(request) => run_until_with_observer(ctx, request, observer),
        LoopCommand::ClearAttempt(request) => clear_attempt(ctx, request),
    }
}

fn tick_with_observer(
    ctx: &RepoContext,
    request: LoopTickRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let workflow = resolve_workflow(
        ctx,
        request.workflow.as_deref(),
        TuningOverrides {
            lease_ttl_seconds: request.lease_ttl_seconds,
            max_attempts: request.max_attempts,
            backoff_seconds: request.backoff_seconds,
        },
    )?;
    let mut lease_store = LeaseStore::new(ctx);
    let mut attempt_store = AttemptStore::new(ctx);

    let mut status = "idle";
    let mut idle = true;
    let mut lease = None;
    let mut release_warning = None;
    let mut observed = Value::Null;
    let mut actions = Vec::new();
    let mut tick_error = None;

    if !workflow.enabled {
        status = "disabled";
    } else {
        let lease_key = workflow.lease_key();
        match lease_store.acquire(&lease_key, workflow.lease_ttl_seconds)? {
            LeaseAcquire::Acquired(acquired) => {
                lease = Some(acquired.clone());
                match run_workflow_tick(
                    ctx,
                    &workflow,
                    &mut lease_store,
                    &mut attempt_store,
                    observer,
                ) {
                    Ok(tick) => {
                        observed = tick.observed;
                        actions = tick.actions;
                    }
                    Err(error) => {
                        tick_error = Some(format!("{error:#}"));
                    }
                }
                let released = lease_store.release(&lease_key, &acquired.owner);
                if let Err(error) = released {
                    release_warning = Some(format!("{error:#}"));
                }
            }
            LeaseAcquire::Held(existing) => {
                lease = Some(existing);
            }
        }
    }

    let live_leases = lease_store.active_leases()?;
    let attempts = attempt_store.snapshot()?;
    let attempt_check_at_ms = now_ms();
    let attempt_sections = AttemptSections::new(&attempts, attempt_check_at_ms);

    let blocked_by_runtime =
        release_warning.is_some() || !live_leases.is_empty() || attempt_sections.blocks_idle();

    // Idleness is machine-global for now: `loop run --until idle` should not
    // claim quiescence while any workflow lease or attempt backoff is live.
    if tick_error.is_some() {
        idle = false;
        status = "failed";
    } else if !attempt_sections.needs_attention.is_empty() {
        idle = false;
        status = "needs_attention";
    } else if !workflow.enabled {
        idle = !blocked_by_runtime;
        status = "disabled";
    } else if blocked_by_runtime {
        idle = false;
        status = "waiting";
    } else if actions_include_work(&actions) {
        idle = false;
        status = "acted";
    } else if actions_include_waiting(&actions) {
        idle = false;
        status = "waiting";
    }

    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_tick",
        "schema_version": 1,
        "workflow": workflow.value(),
        "status": status,
        "idle": idle,
        "observed": observed,
        "actions": actions,
        "lease": lease,
        "live_leases": live_leases,
        "attempts": attempts,
        "waiting_attempts": attempt_sections.waiting,
        "needs_attention": {
            "exhausted_attempts": attempt_sections.needs_attention,
        },
        "release_warning": release_warning,
        "error": tick_error,
    });
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: LOOP_TICK_TOOL,
            args: json!({
                "workflow": &workflow.id,
                "kind": &workflow.kind,
            }),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: if evidence["error"].is_null() { 0 } else { 1 },
            stdout: "",
            stderr: evidence["error"]
                .as_str()
                .or(release_warning.as_deref())
                .unwrap_or(""),
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
    )?;

    if let Some(error) = evidence["error"].as_str() {
        bail!(
            "Loop workflow '{}' failed; receipt {}: {}",
            workflow.id,
            receipt_id,
            error
        );
    }

    Ok(json!({
        "ok": true,
        "command": "loop tick",
        "receipt_id": receipt_id,
        "workflow": evidence["workflow"],
        "status": status,
        "idle": idle,
        "observed": evidence["observed"],
        "actions": evidence["actions"],
        "lease": evidence["lease"],
        "live_leases": evidence["live_leases"],
        "attempts": evidence["attempts"],
        "waiting_attempts": evidence["waiting_attempts"],
        "needs_attention": evidence["needs_attention"],
        "release_warning": release_warning,
    }))
}

fn status(ctx: &RepoContext, request: LoopStatusRequest) -> Result<Value> {
    status_with_cancellation(ctx, request, &|| false)
}

pub(super) fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_status_active(cancelled)?;
    let workflows = if let Some(workflow) = request.workflow.as_deref() {
        vec![
            resolve_workflow(
                ctx,
                Some(workflow),
                TuningOverrides {
                    lease_ttl_seconds: None,
                    max_attempts: None,
                    backoff_seconds: None,
                },
            )?
            .value(),
        ]
    } else {
        list_workflows(ctx)?
            .into_iter()
            .map(|workflow| workflow.value())
            .collect::<Vec<_>>()
    };
    ensure_status_active(cancelled)?;

    let attempts = AttemptStore::new(ctx).snapshot_read_only_with_cancellation(cancelled)?;
    ensure_status_active(cancelled)?;
    let attempt_sections = AttemptSections::new_with_cancellation(&attempts, now_ms(), cancelled)?;
    ensure_status_active(cancelled)?;
    let leases = LeaseStore::new(ctx).active_leases_read_only_with_cancellation(cancelled)?;
    ensure_status_active(cancelled)?;

    Ok(json!({
        "ok": true,
        "command": "loop status",
        "workflows": workflows,
        "leases": leases,
        "attempts": attempts,
        "waiting_attempts": attempt_sections.waiting,
        "needs_attention": {
            "exhausted_attempts": attempt_sections.needs_attention,
        },
    }))
}

fn ensure_status_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

fn run_until_with_observer(
    ctx: &RepoContext,
    request: LoopRunRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    if request.until != "idle" {
        bail!(
            "Unsupported loop run stop condition '{}'. Use --until idle.",
            request.until
        );
    }
    if request.max_ticks == 0 {
        bail!("--max-ticks must be greater than zero");
    }

    let mut ticks = Vec::new();
    let mut status = "max_ticks_reached".to_string();
    for index in 0..request.max_ticks {
        observer.event(ExecutionEvent::PhaseStarted {
            label: "loop tick",
            position: PhasePosition::new((index + 1) as usize, request.max_ticks as usize)
                .expect("loop tick progress is within the configured nonzero maximum"),
        });
        let tick = tick_with_observer(
            ctx,
            LoopTickRequest {
                workflow: request.workflow.clone(),
                lease_ttl_seconds: request.lease_ttl_seconds,
                max_attempts: request.max_attempts,
                backoff_seconds: request.backoff_seconds,
            },
            observer,
        )?;
        let tick_status = tick["status"].as_str().unwrap_or("unknown").to_string();
        let idle = tick["idle"].as_bool().unwrap_or(false);
        ticks.push(tick);
        if matches!(
            tick_status.as_str(),
            "waiting" | "disabled" | "failed" | "needs_attention"
        ) {
            status = tick_status;
            break;
        }
        if idle {
            status = "idle".into();
            break;
        }
    }

    Ok(json!({
        "ok": true,
        "command": "loop run",
        "until": request.until,
        "status": status,
        "tick_count": ticks.len(),
        "ticks": ticks,
    }))
}

fn run_workflow_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkflowTick> {
    match workflow.kind.as_str() {
        GITHUB_PR_STATUS_KIND => github::github_pr_status_tick(ctx),
        NOOP_STATUS_KIND => noop_status_tick(ctx),
        PR_MANAGER_KIND => {
            pr_manager::pr_manager_tick(ctx, workflow, lease_store, attempt_store, observer)
        }
        _ => bail!(
            "Unsupported loop workflow kind '{}'. Supported kinds: {NOOP_STATUS_KIND}, {GITHUB_PR_STATUS_KIND}, {PR_MANAGER_KIND}.",
            workflow.kind
        ),
    }
}

fn actions_include_work(actions: &[Value]) -> bool {
    actions.iter().any(|action| {
        !matches!(
            action.get("status").and_then(Value::as_str),
            Some("skipped" | "waiting" | "needs_attention")
        )
    })
}

fn actions_include_waiting(actions: &[Value]) -> bool {
    actions.iter().any(|action| {
        matches!(
            action.get("status").and_then(Value::as_str),
            Some("waiting")
        )
    })
}

fn clear_attempt(ctx: &RepoContext, request: LoopClearAttemptRequest) -> Result<Value> {
    let started = now_ms();
    if request.item.trim().is_empty() {
        bail!("--item must not be empty");
    }
    let workflow = resolve_workflow(
        ctx,
        Some(&request.workflow),
        TuningOverrides {
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        },
    )?;
    let mut attempt_store = AttemptStore::new(ctx);
    let cleared = attempt_store.clear_attempt(&workflow.id, &request.item)?;
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_clear_attempt",
        "schema_version": 1,
        "workflow": workflow.value(),
        "item_key": request.item,
        "cleared": cleared,
    });
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: LOOP_CLEAR_ATTEMPT_TOOL,
            args: json!({
                "workflow": &workflow.id,
                "item": evidence["item_key"],
            }),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "command": "loop clear-attempt",
        "receipt_id": receipt_id,
        "workflow": evidence["workflow"],
        "item_key": evidence["item_key"],
        "cleared": cleared,
    }))
}

fn noop_status_tick(ctx: &RepoContext) -> Result<WorkflowTick> {
    let open_plans = open_plan_summaries(ctx)?;
    Ok(WorkflowTick {
        observed: json!({
            "repo": {
                "name": ctx.repo_name(),
                "default_branch": ctx.default_branch(),
            },
            "open_plan_count": open_plans.len(),
            "open_plans": open_plans,
            "work_gate_count": ctx.work_gates().len(),
        }),
        actions: Vec::new(),
    })
}

struct WorkflowTick {
    observed: Value,
    actions: Vec<Value>,
}

#[derive(Clone, Copy)]
struct TuningOverrides {
    lease_ttl_seconds: Option<u64>,
    max_attempts: Option<u32>,
    backoff_seconds: Option<u64>,
}

#[derive(Clone, Debug)]
struct ResolvedWorkflow {
    id: String,
    kind: String,
    enabled: bool,
    configured: bool,
    lease_ttl_seconds: u64,
    max_attempts: u32,
    backoff_seconds: u64,
    codex_home_configured: Option<PathBuf>,
}

impl ResolvedWorkflow {
    fn lease_key(&self) -> String {
        format!("{WORKFLOW_LEASE_PREFIX}{}", self.id)
    }

    fn value(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind,
            "enabled": self.enabled,
            "configured": self.configured,
            "lease_ttl_seconds": self.lease_ttl_seconds,
            "max_attempts": self.max_attempts,
            "backoff_seconds": self.backoff_seconds,
            "codex_home_configured": self
                .codex_home_configured
                .as_ref()
                .map(|home| home.display().to_string()),
        })
    }
}

fn resolve_workflow(
    ctx: &RepoContext,
    requested: Option<&str>,
    overrides: TuningOverrides,
) -> Result<ResolvedWorkflow> {
    let workflow_id = requested.unwrap_or(DEFAULT_WORKFLOW_ID);
    if let Some(config) = ctx
        .loop_workflows()
        .iter()
        .find(|workflow| workflow.id == workflow_id)
    {
        return workflow_from_config(ctx.loop_config(), config, overrides);
    }

    if matches!(workflow_id, DEFAULT_WORKFLOW_ID | NOOP_STATUS_KIND) {
        return default_workflow(ctx.loop_config(), overrides);
    }

    bail!("Loop workflow not found: {workflow_id}")
}

fn list_workflows(ctx: &RepoContext) -> Result<Vec<ResolvedWorkflow>> {
    let mut workflows = ctx
        .loop_workflows()
        .iter()
        .map(|workflow| {
            workflow_from_config(
                ctx.loop_config(),
                workflow,
                TuningOverrides {
                    lease_ttl_seconds: None,
                    max_attempts: None,
                    backoff_seconds: None,
                },
            )
        })
        .collect::<Result<Vec<_>>>()?;

    if !workflows
        .iter()
        .any(|workflow| workflow.id == DEFAULT_WORKFLOW_ID)
    {
        workflows.push(default_workflow(
            ctx.loop_config(),
            TuningOverrides {
                lease_ttl_seconds: None,
                max_attempts: None,
                backoff_seconds: None,
            },
        )?);
    }
    Ok(workflows)
}

fn workflow_from_config(
    loop_config: &LoopConfig,
    workflow: &LoopWorkflowConfig,
    overrides: TuningOverrides,
) -> Result<ResolvedWorkflow> {
    let lease_ttl_seconds = overrides
        .lease_ttl_seconds
        .or(workflow.lease_ttl_seconds)
        .unwrap_or(loop_config.lease_ttl_seconds);
    let max_attempts = overrides
        .max_attempts
        .or(workflow.max_attempts)
        .unwrap_or(loop_config.max_attempts);
    let backoff_seconds = overrides
        .backoff_seconds
        .or(workflow.backoff_seconds)
        .unwrap_or(loop_config.backoff_seconds);
    validate_tuning(lease_ttl_seconds, max_attempts, backoff_seconds)?;

    Ok(ResolvedWorkflow {
        id: workflow.id.clone(),
        kind: workflow.kind.clone(),
        enabled: workflow.enabled,
        configured: true,
        lease_ttl_seconds,
        max_attempts,
        backoff_seconds,
        codex_home_configured: workflow.codex_home.clone(),
    })
}

fn default_workflow(
    loop_config: &LoopConfig,
    overrides: TuningOverrides,
) -> Result<ResolvedWorkflow> {
    let lease_ttl_seconds = overrides
        .lease_ttl_seconds
        .unwrap_or(loop_config.lease_ttl_seconds);
    let max_attempts = overrides.max_attempts.unwrap_or(loop_config.max_attempts);
    let backoff_seconds = overrides
        .backoff_seconds
        .unwrap_or(loop_config.backoff_seconds);

    validate_tuning(lease_ttl_seconds, max_attempts, backoff_seconds)?;

    Ok(ResolvedWorkflow {
        id: DEFAULT_WORKFLOW_ID.into(),
        kind: NOOP_STATUS_KIND.into(),
        enabled: true,
        configured: false,
        lease_ttl_seconds,
        max_attempts,
        backoff_seconds,
        codex_home_configured: None,
    })
}

fn validate_tuning(lease_ttl_seconds: u64, max_attempts: u32, backoff_seconds: u64) -> Result<()> {
    if lease_ttl_seconds == 0 {
        bail!("lease_ttl_seconds must be greater than zero");
    }
    if max_attempts == 0 {
        bail!("max_attempts must be greater than zero");
    }
    if backoff_seconds == 0 {
        bail!("backoff_seconds must be greater than zero");
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LeaseRecord {
    key: String,
    owner: String,
    acquired_at_ms: u64,
    expires_at_ms: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct LeaseFile {
    leases: BTreeMap<String, LeaseRecord>,
}

enum LeaseAcquire {
    Acquired(LeaseRecord),
    Held(LeaseRecord),
}

struct LeaseStore {
    dir: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
}

impl LeaseStore {
    fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_CACHE_DIR);
        Self {
            path: dir.join("leases.json"),
            lock_path: dir.join("leases.lock"),
            dir,
        }
    }

    fn acquire(&mut self, key: &str, ttl_seconds: u64) -> Result<LeaseAcquire> {
        self.with_locked(|store| {
            let now = now_ms();
            store.prune_expired(now);
            if let Some(existing) = store.leases.get(key) {
                return Ok(LeaseAcquire::Held(existing.clone()));
            }

            let record = LeaseRecord {
                key: key.to_string(),
                owner: format!("{}-{}", std::process::id(), Ulid::new()),
                acquired_at_ms: now,
                expires_at_ms: now.saturating_add(ttl_seconds.saturating_mul(1000)),
            };
            store.leases.insert(key.to_string(), record.clone());
            Ok(LeaseAcquire::Acquired(record))
        })
    }

    fn release(&mut self, key: &str, owner: &str) -> Result<()> {
        self.with_locked(|store| {
            if store
                .leases
                .get(key)
                .is_some_and(|lease| lease.owner == owner)
            {
                store.leases.remove(key);
            }
            Ok(())
        })
    }

    fn active_leases(&mut self) -> Result<Vec<LeaseRecord>> {
        self.with_locked(|store| {
            store.prune_expired(now_ms());
            Ok(store.leases.values().cloned().collect())
        })
    }

    fn active_leases_read_only_with_cancellation(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<LeaseRecord>> {
        let mut store = read_json_or_default_with_cancellation::<LeaseFile>(&self.path, cancelled)?;
        ensure_status_active(cancelled)?;
        store.prune_expired(now_ms());
        Ok(store.leases.into_values().collect())
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut LeaseFile) -> Result<T>) -> Result<T> {
        with_json_cache_lock(&self.dir, &self.lock_path, &self.path, action)
    }
}

impl LeaseFile {
    fn prune_expired(&mut self, now: u64) {
        self.leases.retain(|_, lease| lease.expires_at_ms > now);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AttemptRecord {
    key: String,
    workflow_id: String,
    item_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    item_version: Option<String>,
    attempts: u32,
    max_attempts: u32,
    last_attempt_ms: u64,
    next_eligible_ms: u64,
    exhausted: bool,
    last_status: String,
}

impl AttemptRecord {
    const fn in_backoff(&self, now_ms: u64) -> bool {
        !self.exhausted && self.next_eligible_ms > now_ms
    }
}

struct AttemptSections {
    waiting: Vec<AttemptRecord>,
    needs_attention: Vec<AttemptRecord>,
}

impl AttemptSections {
    fn new(attempts: &[AttemptRecord], now_ms: u64) -> Self {
        Self::new_with_cancellation(attempts, now_ms, &|| false)
            .expect("an always-false callback cannot cancel attempt classification")
    }

    fn new_with_cancellation(
        attempts: &[AttemptRecord],
        now_ms: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let mut waiting = Vec::new();
        let mut needs_attention = Vec::new();
        for attempt in attempts {
            ensure_status_active(cancelled)?;
            if attempt.in_backoff(now_ms) {
                waiting.push(attempt.clone());
            }
            if attempt.exhausted {
                needs_attention.push(attempt.clone());
            }
        }
        Ok(Self {
            waiting,
            needs_attention,
        })
    }

    fn blocks_idle(&self) -> bool {
        !self.waiting.is_empty() || !self.needs_attention.is_empty()
    }
}

#[derive(Default, Deserialize, Serialize)]
struct AttemptFile {
    attempts: BTreeMap<String, AttemptRecord>,
}

struct AttemptStore {
    dir: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
}

impl AttemptStore {
    fn new(ctx: &RepoContext) -> Self {
        let dir = ctx.root().join(LOOP_CACHE_DIR);
        Self {
            path: dir.join("attempts.json"),
            lock_path: dir.join("attempts.lock"),
            dir,
        }
    }

    fn record_attempt_for_version(
        &mut self,
        workflow: &ResolvedWorkflow,
        item_key: &str,
        item_version: Option<&str>,
        status: &str,
    ) -> Result<AttemptRecord> {
        let key = format!("{}:{item_key}", workflow.id);
        self.with_locked(|store| {
            let now = now_ms();
            let current = store
                .attempts
                .get(&key)
                .map(|record| record.attempts)
                .unwrap_or(0);
            let attempts = current.saturating_add(1);
            let exhausted = attempts >= workflow.max_attempts && status != "passed";
            let record = AttemptRecord {
                key: key.clone(),
                workflow_id: workflow.id.clone(),
                item_key: item_key.to_string(),
                item_version: item_version
                    .filter(|version| !version.is_empty())
                    .map(str::to_string),
                attempts,
                max_attempts: workflow.max_attempts,
                last_attempt_ms: now,
                next_eligible_ms: if exhausted {
                    u64::MAX
                } else {
                    now.saturating_add(workflow.backoff_seconds.saturating_mul(1000))
                },
                exhausted,
                last_status: status.to_string(),
            };
            if status == "passed" {
                store.attempts.remove(&key);
            } else {
                store.attempts.insert(key, record.clone());
            }
            Ok(record)
        })
    }

    fn get(&mut self, workflow_id: &str, item_key: &str) -> Result<Option<AttemptRecord>> {
        let key = format!("{workflow_id}:{item_key}");
        self.with_locked(|store| Ok(store.attempts.get(&key).cloned()))
    }

    fn snapshot(&mut self) -> Result<Vec<AttemptRecord>> {
        self.with_locked(|store| Ok(store.attempts.values().cloned().collect()))
    }

    fn snapshot_read_only_with_cancellation(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<AttemptRecord>> {
        Ok(
            read_json_or_default_with_cancellation::<AttemptFile>(&self.path, cancelled)?
                .attempts
                .into_values()
                .collect(),
        )
    }

    fn clear_attempt(&mut self, workflow_id: &str, item_key: &str) -> Result<bool> {
        let key = format!("{workflow_id}:{item_key}");
        self.with_locked(|store| Ok(store.attempts.remove(&key).is_some()))
    }

    fn with_locked<T>(&mut self, action: impl FnOnce(&mut AttemptFile) -> Result<T>) -> Result<T> {
        with_json_cache_lock(&self.dir, &self.lock_path, &self.path, action)
    }
}

fn with_json_cache_lock<T, S>(
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    action: impl FnOnce(&mut S) -> Result<T>,
) -> Result<T>
where
    S: Default + DeserializeOwned + Serialize,
{
    fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .with_context(|| format!("Failed to open loop cache lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("Failed to lock {}", lock_path.display()))?;

    let mut store = read_json_or_default(data_path)?;
    let result = action(&mut store)?;
    write_json(data_path, &store)?;
    drop(lock);
    Ok(result)
}

fn read_json_or_default<T>(path: &Path) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    read_json_or_default_with_cancellation(path, &|| false)
}

fn read_json_or_default_with_cancellation<T>(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    ensure_status_active(cancelled)?;
    match File::open(path) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();
            loop {
                ensure_status_active(cancelled)?;
                let read = file
                    .read(&mut chunk)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            ensure_status_active(cancelled)?;
            serde_json::from_slice(&bytes)
                .with_context(|| format!("Failed to parse {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("Loop cache path has no parent: {}", path.display()));
    };
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", Ulid::new()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).context("Failed to encode loop cache JSON")?,
    )
    .with_context(|| format!("Failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "Failed to replace loop cache file {} with {}",
            path.display(),
            tmp.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn read_only_loop_cache_scan_observes_cancellation_between_chunks() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("large.json");
        fs::write(
            &path,
            format!("{{\"padding\":\"{}\"}}", "x".repeat(256 * 1024)),
        )
        .unwrap();
        let checks = AtomicUsize::new(0);

        let error = read_json_or_default_with_cancellation::<Value>(&path, &|| {
            checks.fetch_add(1, Ordering::SeqCst) >= 2
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "status collection was cancelled");
    }

    #[test]
    fn attempt_store_exhausts_after_budget_and_clears_on_success() {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = AttemptStore::new(&ctx);
        let workflow = ResolvedWorkflow {
            id: "wf".into(),
            kind: NOOP_STATUS_KIND.into(),
            enabled: true,
            configured: false,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
        };

        let first = store
            .record_attempt_for_version(&workflow, "item-1", None, "failed")
            .unwrap();
        assert_eq!(first.attempts, 1);
        assert!(!first.exhausted);

        let second = store
            .record_attempt_for_version(&workflow, "item-1", None, "failed")
            .unwrap();
        assert_eq!(second.attempts, 2);
        assert!(second.exhausted);

        store
            .record_attempt_for_version(&workflow, "item-1", None, "passed")
            .unwrap();
        assert!(store.snapshot().unwrap().is_empty());
    }

    fn write_loop_fixture_repo(root: &Path) {
        crate::test_env::TestRepoBuilder::new(root)
            .required_commands(Vec::<String>::new())
            .write();
    }
}
