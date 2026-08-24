use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use fs4::fs_std::FileExt;
use jig_contract::{
    Finding, FindingSeverity, RunConclusion, RunPlan, RunResult, RunStatus, TargetId,
    TargetRunResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(test)]
use std::cell::Cell;
use ulid::Ulid;

use crate::context::RepoContext;

use super::jsonl::{
    JsonlWriteGuard, RawJsonlRecord, RawJsonlRewrite, append_jsonl, append_jsonl_with_end_offset,
    rewrite_jsonl_raw_locked, scan_jsonl_raw, scan_jsonl_raw_from, scan_jsonl_raw_locked,
    with_jsonl_write_lock,
};
use super::records::RunEventRecord;
use super::support::{ensure_state_layout, new_id, now_ms};
use super::{MAINTENANCE_WRITER_COORDINATION_NOTE, compression::write_gzip_atomic};

const RUNS_FILE: &str = "runs.jsonl";
const RUN_LEASE_DIR: &str = ".agent/.cache/run-leases";
const EVENT_QUEUED: &str = "queued";
const EVENT_RUNNING: &str = "running";
const EVENT_TARGET_STARTED: &str = "target_started";
const EVENT_TARGET_COMPLETED: &str = "target_completed";
const EVENT_COMPLETED: &str = "completed";
const EVENT_CANCEL_REQUESTED: &str = "cancel_requested";

#[cfg(test)]
thread_local! {
    static FULL_RUN_EVENT_PARSE_COUNT: Cell<usize> = const { Cell::new(0) };
    static RUN_EVENT_IDENTITY_PARSE_COUNT: Cell<usize> = const { Cell::new(0) };
}

const REVERSE_RUN_READ_CHUNK: usize = 16 * 1024;

/// The accepted plan and current state reconstructed from the append-only run log.
#[derive(Clone, Debug, JsonSchema, Serialize)]
pub(crate) struct DurableRun {
    pub(crate) plan: RunPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) work_plan_id: Option<String>,
    pub(crate) result: RunResult,
    pub(crate) cancel_requested: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunEventCursor(u64);

pub(crate) struct RunLease {
    // The path is deliberately stable for the repository lifetime. Removing
    // an advisory-lock file after unlock permits another process to open and
    // lock a new inode while an inspector still holds the old inode.
    _file: File,
}

#[derive(Deserialize)]
struct RunEventIdentity {
    run_id: String,
    event: String,
}

pub(crate) fn run_cancel_requested_since(
    ctx: &RepoContext,
    run_id: &str,
    cursor: &mut RunEventCursor,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool> {
    let path = ctx.state_file(RUNS_FILE);
    let mut requested = false;
    let (offset, _) = scan_jsonl_raw_from(&path, cursor.0, cancelled, |raw| {
        let event = parse_run_event_identity(raw, &path)?;
        if event.run_id == run_id && event.event == EVENT_CANCEL_REQUESTED {
            requested = true;
        }
        Ok(())
    })?;
    cursor.0 = offset;
    Ok(requested)
}

pub(crate) fn start_run(
    ctx: &RepoContext,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(DurableRun, RunLease)> {
    let (run, lease, _) = start_run_with_event_cursor(ctx, plan, work_plan_id)?;
    Ok((run, lease))
}

pub(crate) fn start_run_with_event_cursor(
    ctx: &RepoContext,
    plan: RunPlan,
    work_plan_id: Option<String>,
) -> Result<(DurableRun, RunLease, RunEventCursor)> {
    validate_run_plan_structure(&plan)?;
    ensure_state_layout(ctx)?;
    let run_id = new_id("run");
    let lease = acquire_run_lease(ctx, &run_id)?;
    let timestamp_ms = now_ms();
    let event_cursor = append_event_with_cursor(
        ctx,
        RunEventRecord {
            id: new_id("run_event"),
            run_id: run_id.clone(),
            event: EVENT_QUEUED.into(),
            timestamp_ms,
            work_plan_id: work_plan_id.clone(),
            plan: Some(plan.clone()),
            target: None,
            result: None,
            conclusion: None,
        },
    )?;
    let run = DurableRun {
        result: RunResult::queued(
            run_id,
            plan.id.clone(),
            timestamp_ms,
            plan.targets
                .iter()
                .map(|target| {
                    TargetRunResult::queued(
                        target.target.clone(),
                        plan.config_digest.clone(),
                        target.input_digest.clone(),
                    )
                })
                .collect(),
        ),
        plan,
        work_plan_id,
        cancel_requested: false,
    };
    Ok((run, lease, event_cursor))
}

fn validate_run_plan_structure(plan: &RunPlan) -> Result<()> {
    let planned_targets = plan
        .targets
        .iter()
        .map(|target| target.target.clone())
        .collect::<BTreeSet<_>>();
    if planned_targets.len() != plan.targets.len() {
        bail!("run plan contains duplicate targets");
    }

    let mut target_layers = BTreeMap::<TargetId, usize>::new();
    for (layer_index, layer) in plan.execution_layers.iter().enumerate() {
        if layer.is_empty() {
            bail!("run plan execution layer {layer_index} is empty");
        }
        for target in layer {
            if !planned_targets.contains(target) {
                bail!("run plan execution layers reference unknown target '{target}'");
            }
            if target_layers.insert(target.clone(), layer_index).is_some() {
                bail!("run plan execution layers contain duplicate target '{target}'");
            }
        }
    }

    let missing = planned_targets
        .iter()
        .filter(|target| !target_layers.contains_key(*target))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "run plan execution layers omit planned target(s): {}",
            missing.join(", ")
        );
    }

    for target in &plan.targets {
        let target_layer = target_layers[&target.target];
        for dependency in &target.depends_on {
            let dependency_layer = target_layers.get(dependency).ok_or_else(|| {
                anyhow!(
                    "run plan target '{}' depends on missing target '{dependency}'",
                    target.target
                )
            })?;
            if *dependency_layer >= target_layer {
                bail!(
                    "run plan target '{}' must execute after dependency '{dependency}'",
                    target.target
                );
            }
        }
    }
    Ok(())
}

fn acquire_run_lease(ctx: &RepoContext, run_id: &str) -> Result<RunLease> {
    let file = open_run_lease(ctx, run_id)?;
    file.lock_exclusive()
        .with_context(|| format!("Failed to acquire worker lease for run '{run_id}'"))?;
    Ok(RunLease { _file: file })
}

pub(crate) fn reconcile_run_for_inspection(ctx: &RepoContext, run_id: &str) -> Result<DurableRun> {
    let run = run_by_id(ctx, run_id)?;
    if run.result.status == RunStatus::Completed {
        return Ok(run);
    }

    let file = open_run_lease(ctx, run_id)?;
    match file.try_lock_exclusive() {
        Ok(true) => {
            let _lease = RunLease { _file: file };
            let current = run_by_id(ctx, run_id)?;
            if current.result.status != RunStatus::Completed {
                block_nonterminal_run(
                    ctx,
                    run_id,
                    "repository run worker lease is no longer held; the worker process likely exited",
                )?;
            }
            run_by_id(ctx, run_id)
        }
        Ok(false) => Ok(run),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(run),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect worker lease for run '{run_id}'"))
        }
    }
}

fn open_run_lease(ctx: &RepoContext, run_id: &str) -> Result<File> {
    let path = run_lease_path(ctx, run_id)?;
    ensure_state_layout(ctx)?;
    let lease_dir = ctx.root().join(RUN_LEASE_DIR);
    fs::create_dir_all(&lease_dir)
        .with_context(|| format!("Failed to create {}", lease_dir.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open run lease {}", path.display()))?;
    Ok(file)
}

fn run_lease_path(ctx: &RepoContext, run_id: &str) -> Result<std::path::PathBuf> {
    validate_run_id_for_lease(run_id)?;
    Ok(ctx
        .root()
        .join(RUN_LEASE_DIR)
        .join(format!("{run_id}.lock")))
}

fn validate_run_id_for_lease(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || run_id.len() > 128
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        bail!("run id cannot be used as a safe worker lease filename");
    }
    Ok(())
}

fn run_lease_is_idle(ctx: &RepoContext, run_id: &str) -> Result<bool> {
    let path = run_lease_path(ctx, run_id)?;
    let file = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect run lease {}", path.display()));
        }
    };
    match file.try_lock_exclusive() {
        Ok(true) => {
            FileExt::unlock(&file)
                .with_context(|| format!("Failed to release run lease {}", path.display()))?;
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to inspect run lease {}", path.display()))
        }
    }
}

fn remove_run_lease(ctx: &RepoContext, run_id: &str) -> Result<bool> {
    let path = run_lease_path(ctx, run_id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to remove run lease {}", path.display()))
        }
    }
}

pub(crate) fn mark_run_running(ctx: &RepoContext, run_id: &str) -> Result<()> {
    append_simple_event(ctx, run_id, EVENT_RUNNING, None, None)
}

pub(crate) fn mark_target_started(ctx: &RepoContext, run_id: &str, target: TargetId) -> Result<()> {
    append_simple_event(ctx, run_id, EVENT_TARGET_STARTED, Some(target), None)
}

pub(crate) fn record_target_result(
    ctx: &RepoContext,
    run_id: &str,
    result: TargetRunResult,
) -> Result<()> {
    append_event(
        ctx,
        RunEventRecord {
            id: new_id("run_event"),
            run_id: run_id.to_owned(),
            event: EVENT_TARGET_COMPLETED.into(),
            timestamp_ms: result.ended_at_ms.unwrap_or_else(now_ms),
            work_plan_id: None,
            plan: None,
            target: Some(result.target.clone()),
            result: Some(result),
            conclusion: None,
        },
    )
}

pub(crate) fn complete_run(
    ctx: &RepoContext,
    run_id: &str,
    conclusion: RunConclusion,
) -> Result<()> {
    append_simple_event(ctx, run_id, EVENT_COMPLETED, None, Some(conclusion))
}

pub(crate) fn block_nonterminal_run(ctx: &RepoContext, run_id: &str, message: &str) -> Result<()> {
    let run = run_by_id(ctx, run_id)?;
    if run.result.status == RunStatus::Completed {
        return Ok(());
    }

    let mut conclusions = run
        .result
        .targets
        .iter()
        .filter_map(|target| target.conclusion)
        .collect::<Vec<_>>();
    for mut target in run
        .result
        .targets
        .into_iter()
        .filter(|target| target.status != RunStatus::Completed)
    {
        target.status = RunStatus::Completed;
        target.conclusion = Some(RunConclusion::Blocked);
        target.ended_at_ms = Some(super::support::now_ms());
        let mut finding = Finding::new(FindingSeverity::Error, message);
        finding.source = Some("jig".into());
        target.findings.push(finding);
        record_target_result(ctx, run_id, target)?;
        conclusions.push(RunConclusion::Blocked);
    }
    let conclusion = conclusions
        .into_iter()
        .max_by_key(|conclusion| run_conclusion_priority(*conclusion))
        .unwrap_or(RunConclusion::Blocked);
    complete_run(ctx, run_id, conclusion)
}

const fn run_conclusion_priority(conclusion: RunConclusion) -> u8 {
    match conclusion {
        RunConclusion::Failure => 5,
        RunConclusion::TimedOut => 4,
        RunConclusion::Blocked => 3,
        RunConclusion::Cancelled => 2,
        RunConclusion::Skipped => 1,
        RunConclusion::Success => 0,
    }
}

pub(crate) fn request_run_cancel(ctx: &RepoContext, run_id: &str) -> Result<DurableRun> {
    let run = run_by_id(ctx, run_id)?;
    if run.result.status == RunStatus::Completed || run.cancel_requested {
        return Ok(run);
    }
    append_simple_event(ctx, run_id, EVENT_CANCEL_REQUESTED, None, None)?;
    run_by_id(ctx, run_id)
}

pub(crate) fn run_by_id(ctx: &RepoContext, run_id: &str) -> Result<DurableRun> {
    ensure_state_layout(ctx)?;
    let path = ctx.state_file(RUNS_FILE);
    let events = read_run_events_reverse(&path, run_id)?;
    fold_events(run_id, events)
}

fn read_run_events_reverse(path: &Path, run_id: &str) -> Result<Vec<RunEventRecord>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to open {}", path.display()));
        }
    };
    loop {
        match FileExt::lock_shared(&file) {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                return read_run_events_forward(path, run_id);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to shared-lock {}", path.display()));
            }
        }
    }
    let result = scan_run_events_reverse(&file, path, run_id);
    let unlock = FileExt::unlock(&file);
    match (result, unlock) {
        (Ok(events), Ok(())) => Ok(events),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => {
            Err(error).with_context(|| format!("Failed to unlock {}", path.display()))
        }
    }
}

fn read_run_events_forward(path: &Path, run_id: &str) -> Result<Vec<RunEventRecord>> {
    let mut events = Vec::new();
    let report = scan_jsonl_raw(path, &|| false, |raw| {
        let identity = parse_run_event_identity(raw, path)?;
        if identity.run_id == run_id {
            events.push(parse_run_event(raw, path)?);
        }
        Ok(())
    })?;
    if report.unterminated_final_record {
        bail!(
            "Refusing to inspect {} because its final JSONL record is not newline-terminated",
            path.display()
        );
    }
    Ok(events)
}

fn scan_run_events_reverse(file: &File, path: &Path, run_id: &str) -> Result<Vec<RunEventRecord>> {
    let mut file = file;
    let mut cursor = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("Failed to seek {}", path.display()))?;
    if cursor > 0 {
        file.seek(SeekFrom::End(-1))
            .with_context(|| format!("Failed to inspect the tail of {}", path.display()))?;
        let mut final_byte = [0u8; 1];
        file.read_exact(&mut final_byte)
            .with_context(|| format!("Failed to read the tail of {}", path.display()))?;
        if final_byte[0] != b'\n' {
            bail!(
                "Refusing to inspect {} because its final JSONL record is not newline-terminated",
                path.display()
            );
        }
    }
    let mut carry = Vec::new();
    let mut events = Vec::new();
    let mut found_queued = false;
    while cursor > 0 {
        let read_len = usize::try_from(cursor.min(REVERSE_RUN_READ_CHUNK as u64))
            .unwrap_or(REVERSE_RUN_READ_CHUNK);
        cursor -= read_len as u64;
        file.seek(SeekFrom::Start(cursor))
            .with_context(|| format!("Failed to seek {}", path.display()))?;
        let mut chunk = vec![0u8; read_len];
        file.read_exact(&mut chunk)
            .with_context(|| format!("Failed to read run-event tail {}", path.display()))?;
        chunk.extend_from_slice(&carry);
        let split_at = if cursor == 0 {
            0
        } else {
            chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(chunk.len(), |index| index + 1)
        };
        for record in chunk[split_at..].split(|byte| *byte == b'\n').rev() {
            if record.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            // Continue to byte zero after finding queued: fold_events can
            // reject an older duplicate only when this scan retains it. The
            // cheap identity prefilter avoids deserializing unrelated older
            // records. Decode the candidate JSON string, and fall back to the
            // authoritative parser when an escape could rewrite a key, so
            // valid JSON rewrites cannot hide duplicated events.
            if found_queued && !record_may_have_run_id(record, run_id) {
                continue;
            }
            let raw = RawJsonlRecord {
                bytes: record,
                line_number: 0,
                terminated: true,
            };
            let identity = parse_run_event_identity(raw, path)?;
            if identity.run_id != run_id {
                continue;
            }
            found_queued |= identity.event == EVENT_QUEUED;
            events.push(parse_run_event(raw, path)?);
        }
        carry = chunk[..split_at].to_vec();
    }
    events.reverse();
    Ok(events)
}

fn record_may_have_run_id(record: &[u8], run_id: &str) -> bool {
    const KEY: &[u8] = b"\"run_id\"";
    let mut search_from = 0;
    while search_from + KEY.len() <= record.len() {
        let Some(relative) = record[search_from..]
            .windows(KEY.len())
            .position(|window| window == KEY)
        else {
            return record.contains(&b'\\');
        };
        let mut cursor = search_from + relative + KEY.len();
        while record.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if record.get(cursor) != Some(&b':') {
            search_from = search_from + relative + 1;
            continue;
        }
        cursor += 1;
        while record.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let mut deserializer = serde_json::Deserializer::from_slice(&record[cursor..]);
        if String::deserialize(&mut deserializer).is_ok_and(|candidate| candidate == run_id) {
            return true;
        }
        search_from = search_from + relative + 1;
    }
    // Jig serializes field names literally. A backslash is therefore unusual
    // in a record that lacks a literal `run_id`, but it can encode that key;
    // conservatively parse the record instead of letting the fast prefilter
    // weaken lifecycle validation.
    record.contains(&b'\\')
}

fn parse_run_event_identity(
    raw: RawJsonlRecord<'_>,
    path: &std::path::Path,
) -> Result<RunEventIdentity> {
    #[cfg(test)]
    RUN_EVENT_IDENTITY_PARSE_COUNT.with(|counter| counter.set(counter.get() + 1));
    serde_json::from_slice(raw.bytes).with_context(|| {
        format!(
            "Failed to parse run event identity{} in {}",
            if raw.line_number > 0 {
                format!(" at line {}", raw.line_number)
            } else {
                String::new()
            },
            path.display()
        )
    })
}

fn append_simple_event(
    ctx: &RepoContext,
    run_id: &str,
    event: &str,
    target: Option<TargetId>,
    conclusion: Option<RunConclusion>,
) -> Result<()> {
    append_event(
        ctx,
        RunEventRecord {
            id: new_id("run_event"),
            run_id: run_id.to_owned(),
            event: event.into(),
            timestamp_ms: now_ms(),
            work_plan_id: None,
            plan: None,
            target,
            result: None,
            conclusion,
        },
    )
}

fn append_event(ctx: &RepoContext, event: RunEventRecord) -> Result<()> {
    append_jsonl(&ctx.state_file(RUNS_FILE), &event)
}

fn append_event_with_cursor(ctx: &RepoContext, event: RunEventRecord) -> Result<RunEventCursor> {
    append_jsonl_with_end_offset(&ctx.state_file(RUNS_FILE), &event).map(RunEventCursor)
}

fn parse_run_event(raw: RawJsonlRecord<'_>, path: &std::path::Path) -> Result<RunEventRecord> {
    #[cfg(test)]
    FULL_RUN_EVENT_PARSE_COUNT.with(|counter| counter.set(counter.get() + 1));
    serde_json::from_slice(raw.bytes).with_context(|| {
        format!(
            "Failed to parse run record {} in {}",
            raw.line_number,
            path.display()
        )
    })
}

fn fold_events(run_id: &str, events: Vec<RunEventRecord>) -> Result<DurableRun> {
    let mut run: Option<DurableRun> = None;
    for event in events {
        match event.event.as_str() {
            EVENT_QUEUED => {
                if run.is_some() {
                    bail!("run '{run_id}' has more than one queued event");
                }
                let plan = event
                    .plan
                    .ok_or_else(|| anyhow::anyhow!("run '{run_id}' queued event has no plan"))?;
                let targets = plan
                    .targets
                    .iter()
                    .map(|target| {
                        TargetRunResult::queued(
                            target.target.clone(),
                            plan.config_digest.clone(),
                            target.input_digest.clone(),
                        )
                    })
                    .collect();
                run = Some(DurableRun {
                    result: RunResult::queued(run_id, plan.id.clone(), event.timestamp_ms, targets),
                    plan,
                    work_plan_id: event.work_plan_id,
                    cancel_requested: false,
                });
            }
            EVENT_RUNNING => {
                let current = require_run(&mut run, run_id, EVENT_RUNNING)?;
                ensure_nonterminal(current, run_id, EVENT_RUNNING)?;
                current.result.status = RunStatus::Running;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_TARGET_STARTED => {
                let current = require_run(&mut run, run_id, EVENT_TARGET_STARTED)?;
                ensure_nonterminal(current, run_id, EVENT_TARGET_STARTED)?;
                let target = event.target.ok_or_else(|| {
                    anyhow::anyhow!("run '{run_id}' target_started event has no target")
                })?;
                let result = target_result_mut(current, run_id, &target)?;
                if result.status == RunStatus::Completed {
                    bail!("run '{run_id}' target '{target}' started after completion");
                }
                result.status = RunStatus::Running;
                result.started_at_ms = Some(event.timestamp_ms);
                current.result.status = RunStatus::Running;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_TARGET_COMPLETED => {
                let current = require_run(&mut run, run_id, EVENT_TARGET_COMPLETED)?;
                ensure_nonterminal(current, run_id, EVENT_TARGET_COMPLETED)?;
                let result = event.result.ok_or_else(|| {
                    anyhow::anyhow!("run '{run_id}' target_completed event has no result")
                })?;
                if event.target.as_ref() != Some(&result.target) {
                    bail!("run '{run_id}' target_completed identity does not match its result");
                }
                if result.status != RunStatus::Completed || result.conclusion.is_none() {
                    bail!(
                        "run '{run_id}' target '{}' has a nonterminal result",
                        result.target
                    );
                }
                let stored = target_result_mut(current, run_id, &result.target)?;
                if stored.status == RunStatus::Completed {
                    bail!(
                        "run '{run_id}' target '{}' completed more than once",
                        result.target
                    );
                }
                *stored = result;
                current.result.status = RunStatus::Running;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_COMPLETED => {
                let current = require_run(&mut run, run_id, EVENT_COMPLETED)?;
                ensure_nonterminal(current, run_id, EVENT_COMPLETED)?;
                if current
                    .result
                    .targets
                    .iter()
                    .any(|target| target.status != RunStatus::Completed)
                {
                    bail!("run '{run_id}' completed before every target reached a conclusion");
                }
                current.result.status = RunStatus::Completed;
                current.result.conclusion = Some(event.conclusion.ok_or_else(|| {
                    anyhow::anyhow!("run '{run_id}' completed event has no conclusion")
                })?);
                current.result.updated_at_ms = event.timestamp_ms;
            }
            EVENT_CANCEL_REQUESTED => {
                let current = require_run(&mut run, run_id, EVENT_CANCEL_REQUESTED)?;
                // Cancellation and terminal completion may race across MCP
                // request and worker threads. A physically later cancellation
                // event is an idempotent observation, not stream corruption.
                if current.result.status == RunStatus::Completed {
                    continue;
                }
                current.cancel_requested = true;
                current.result.updated_at_ms = event.timestamp_ms;
            }
            _ => {}
        }
    }
    run.ok_or_else(|| anyhow::anyhow!("run '{run_id}' was not found"))
}

fn require_run<'a>(
    run: &'a mut Option<DurableRun>,
    run_id: &str,
    event: &str,
) -> Result<&'a mut DurableRun> {
    run.as_mut()
        .ok_or_else(|| anyhow::anyhow!("run '{run_id}' has a {event} event before queued"))
}

fn ensure_nonterminal(run: &DurableRun, run_id: &str, event: &str) -> Result<()> {
    if run.result.status == RunStatus::Completed {
        bail!("run '{run_id}' has a {event} event after completion");
    }
    Ok(())
}

fn target_result_mut<'a>(
    run: &'a mut DurableRun,
    run_id: &str,
    target: &TargetId,
) -> Result<&'a mut TargetRunResult> {
    run.result
        .targets
        .iter_mut()
        .find(|result| &result.target == target)
        .ok_or_else(|| anyhow::anyhow!("run '{run_id}' references unplanned target '{target}'"))
}

mod archive;
pub(crate) use archive::runs_archive;
pub(super) use archive::{ensure_run_stream_replaceable, validate_run_stream};

#[cfg(test)]
mod tests;
