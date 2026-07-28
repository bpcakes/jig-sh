use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(not(unix))]
use std::sync::Mutex;

use anyhow::{Result, bail};

pub(crate) use self::termination_reason::TerminationReason;
use super::child_lifecycle::{AppProcessLease, terminate_and_reap};
use super::output::CapturedAppOutput;
use crate::dev_sessions::DevCleanupLease;
use crate::state::{ProcessRouteOwnership, STATE_LOCK_TIMEOUT, StateStore};

mod termination_reason;

const SESSION_INACTIVE: u8 = 0;
const SESSION_RUNNING: u8 = 1;
const SESSION_INTERRUPTING: u8 = 2;
const SESSION_FINALIZING: u8 = 3;
const RESOURCES_UNARMED: u8 = 0;
const RESOURCES_ARMED: u8 = 1;
const EXIT_WITHOUT_RESOURCES_CLAIMED: u8 = 2;
const HANDLER_QUIESCENCE_TIMEOUT: Duration = Duration::from_millis(250);

static SESSION_CLAIMED: AtomicBool = AtomicBool::new(false);
static SESSION_PHASE: AtomicU8 = AtomicU8::new(SESSION_INACTIVE);
static TERMINATION_SIGNAL: AtomicI32 = AtomicI32::new(0);
static FORCE_CLEANUP_REQUESTED: AtomicBool = AtomicBool::new(false);
static OWNED_RESOURCE_STATE: AtomicU8 = AtomicU8::new(RESOURCES_UNARMED);
static NEXT_SESSION_GENERATION: AtomicUsize = AtomicUsize::new(1);
static ACTIVE_SESSION_GENERATION: AtomicUsize = AtomicUsize::new(0);
static TERMINATION_HANDLERS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static TERMINATION_SESSION_POISONED: AtomicBool = AtomicBool::new(false);
static TERMINATION_SESSION_CONSUMED: AtomicBool = AtomicBool::new(false);

pub(super) type SharedRouteCleanupDeadline = Arc<OnceLock<Instant>>;

pub(super) fn new_route_cleanup_deadline() -> SharedRouteCleanupDeadline {
    Arc::new(OnceLock::new())
}

pub(super) fn shared_route_cleanup_deadline(deadline: &SharedRouteCleanupDeadline) -> Instant {
    *deadline.get_or_init(|| Instant::now() + STATE_LOCK_TIMEOUT)
}

#[cfg(not(unix))]
static CTRL_C_HANDLER_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(not(unix))]
static CTRL_C_INSTALL_LOCK: Mutex<()> = Mutex::new(());

pub(super) struct RunningChild {
    pub(super) name: String,
    pub(super) store: StateStore,
    pub(super) child: Child,
    pub(super) output: CapturedAppOutput,
    pub(super) _process_lease: AppProcessLease,
    pub(super) process_cleanup_armed: bool,
    pub(super) route_ownership: Option<ProcessRouteOwnership>,
    pub(super) route_cleanup_armed: bool,
    pub(super) route_cleanup_deadline: SharedRouteCleanupDeadline,
    pub(super) session_cleanup: DevCleanupLease,
}

impl RunningChild {
    fn cleanup_process(&mut self) -> bool {
        if !self.process_cleanup_armed {
            return true;
        }
        match terminate_and_reap(&mut self.child) {
            Ok(()) => self.process_cleanup_armed = false,
            Err(error) => eprintln!(
                "jig proxy could not fully clean up child process {} for '{}': {error:#}; cleanup remains armed for a bounded retry",
                self.child.id(),
                self.name
            ),
        }
        !self.process_cleanup_armed
    }

    fn cleanup_route(&mut self) -> bool {
        if !self.route_cleanup_armed {
            return true;
        }
        let deadline = shared_route_cleanup_deadline(&self.route_cleanup_deadline);
        let Some(ownership) = self.route_ownership.as_ref() else {
            eprintln!(
                "jig proxy lost the route cleanup identity for '{}'; cleanup remains armed",
                self.name
            );
            return false;
        };
        // A forced request may abandon a contended route rewrite only after the
        // owning process group is confirmed gone. Process-route liveness then
        // makes the persisted entry inert until the next prune.
        let may_cancel = !self.process_cleanup_armed;
        match self
            .store
            .remove_process_route_if_owned_cancelable_until(ownership, deadline, || {
                may_cancel && force_cleanup_requested()
            }) {
            Ok(true) => self.route_cleanup_armed = false,
            Ok(false) => {
                eprintln!(
                    "jig proxy could not confirm removal of contended route '{}' after forced cleanup; the dead process route is inert and can be pruned later",
                    ownership.hostname()
                );
            }
            Err(error) => eprintln!(
                "jig proxy could not remove route '{}' while cleaning up '{}': {error}",
                ownership.hostname(),
                self.name
            ),
        }
        !self.route_cleanup_armed
    }

    fn cleanup(&mut self) -> bool {
        if self.route_cleanup_armed {
            shared_route_cleanup_deadline(&self.route_cleanup_deadline);
        }
        self.cleanup_process();
        self.cleanup_route();
        self.confirm_session_cleanup_if_complete()
    }

    fn confirm_session_cleanup_if_complete(&mut self) -> bool {
        let complete = !self.process_cleanup_armed && !self.route_cleanup_armed;
        if complete {
            self.session_cleanup.confirm();
        }
        complete
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub(super) fn cleanup_children(children: &mut [RunningChild]) -> bool {
    if let Some(running) = children.iter().find(|running| running.route_cleanup_armed) {
        shared_route_cleanup_deadline(&running.route_cleanup_deadline);
    }
    // Terminate every owned tree before a route lock can delay cleanup of a
    // later child.
    for running in children.iter_mut() {
        running.cleanup_process();
    }
    // A failed bounded attempt remains armed for one explicit bounded retry so
    // the caller can make its success/failure decision before Drop runs.
    for running in children.iter_mut() {
        running.cleanup_process();
    }
    for running in children.iter_mut() {
        running.cleanup_route();
    }
    for running in children.iter_mut() {
        running.cleanup_route();
    }
    for running in children.iter_mut() {
        running.confirm_session_cleanup_if_complete();
    }
    children
        .iter()
        .all(|running| !running.process_cleanup_armed && !running.route_cleanup_armed)
}

pub(super) struct TerminationSession {
    generation: usize,
    #[cfg(unix)]
    previous_handlers: Vec<(i32, libc::sigaction)>,
}

impl TerminationSession {
    fn start() -> Result<Self> {
        claim_one_shot_termination_session()?;
        if TERMINATION_SESSION_POISONED.load(Ordering::SeqCst) {
            bail!(
                "a prior Jig foreground termination session could not retire its signal handlers safely"
            );
        }
        if SESSION_CLAIMED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            bail!("a Jig foreground termination session is already active in this process");
        }
        if OWNED_RESOURCE_STATE.load(Ordering::SeqCst) != RESOURCES_UNARMED {
            SESSION_CLAIMED.store(false, Ordering::SeqCst);
            bail!("a prior forced termination is still completing in this process");
        }

        let generation = match claim_next_session_generation() {
            Ok(generation) => generation,
            Err(error) => {
                SESSION_CLAIMED.store(false, Ordering::SeqCst);
                return Err(error);
            }
        };

        TERMINATION_SIGNAL.store(0, Ordering::SeqCst);
        FORCE_CLEANUP_REQUESTED.store(false, Ordering::SeqCst);
        SESSION_PHASE.store(SESSION_RUNNING, Ordering::SeqCst);
        ACTIVE_SESSION_GENERATION.store(generation, Ordering::SeqCst);

        #[cfg(unix)]
        {
            match install_unix_handlers() {
                Ok(previous_handlers) => Ok(Self {
                    generation,
                    previous_handlers,
                }),
                Err(error) => {
                    retire_failed_session_start(generation, error.handlers_may_remain);
                    Err(anyhow::anyhow!(
                        "jig proxy could not install termination cleanup handlers: {error}"
                    ))
                }
            }
        }
        #[cfg(not(unix))]
        {
            if let Err(error) = ensure_ctrl_c_handler() {
                reset_session_state();
                return Err(anyhow::anyhow!(
                    "jig proxy could not install Ctrl-C cleanup handler: {error}"
                ));
            }
            Ok(Self { generation })
        }
    }
}

fn claim_one_shot_termination_session() -> Result<()> {
    if TERMINATION_SESSION_CONSUMED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        bail!(
            "a Jig foreground termination session has already been started in this process; start a new process before running another foreground dev or proxy command"
        );
    }
    Ok(())
}

impl Drop for TerminationSession {
    fn drop(&mut self) {
        SESSION_PHASE.store(SESSION_FINALIZING, Ordering::SeqCst);
        #[cfg(unix)]
        let (generation_retired, restoration) = retire_session_and_restore_unix_handlers(
            self.generation,
            &self.previous_handlers,
            restore_unix_handler,
            install_default_unix_handler,
        );
        #[cfg(not(unix))]
        let generation_retired = retire_session_generation(self.generation);
        #[cfg(unix)]
        for warning in restoration.warnings {
            eprintln!("{warning}");
        }
        #[cfg(not(unix))]
        let handlers_may_remain = false;
        #[cfg(unix)]
        let handlers_may_remain = restoration.handlers_may_remain;

        let handlers_quiesced = wait_for_termination_handlers(HANDLER_QUIESCENCE_TIMEOUT);
        if handlers_may_remain || !generation_retired || !handlers_quiesced {
            TERMINATION_SESSION_POISONED.store(true, Ordering::SeqCst);
            eprintln!(
                "jig proxy could not retire its foreground termination handlers safely; later foreground sessions in this process are disabled"
            );
        }
        reset_session_state();
    }
}

fn claim_next_session_generation() -> Result<usize> {
    NEXT_SESSION_GENERATION
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |generation| {
            generation.checked_add(1)
        })
        .map_err(|_| anyhow::anyhow!("foreground termination session generation space exhausted"))
}

#[cfg(unix)]
fn retire_failed_session_start(generation: usize, handlers_may_remain: bool) {
    SESSION_PHASE.store(SESSION_FINALIZING, Ordering::SeqCst);
    let generation_retired = retire_session_generation(generation);
    let handlers_quiesced = wait_for_termination_handlers(HANDLER_QUIESCENCE_TIMEOUT);
    let pending_signal = TERMINATION_SIGNAL.load(Ordering::SeqCst);
    if handlers_may_remain || !generation_retired || !handlers_quiesced {
        TERMINATION_SESSION_POISONED.store(true, Ordering::SeqCst);
    }
    reset_session_state();
    if pending_signal != 0 {
        terminate_after_exit_claim(pending_signal);
    }
}

fn retire_session_generation(generation: usize) -> bool {
    retire_session_generation_with(generation, || {})
}

fn retire_session_generation_with(
    generation: usize,
    after_resources_unarmed: impl FnOnce(),
) -> bool {
    // TerminationSession is declared before every resource owner and therefore
    // drops after their bounded cleanup. Publish that no owned resource remains
    // before publishing generation zero: a handler entering in the interval
    // before reset must take the existing conventional-exit claim, not set a
    // force flag that reset would erase. Preserve an exit already in progress.
    let _ = OWNED_RESOURCE_STATE.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
        (state != EXIT_WITHOUT_RESOURCES_CLAIMED).then_some(RESOURCES_UNARMED)
    });
    after_resources_unarmed();
    ACTIVE_SESSION_GENERATION
        .compare_exchange(generation, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

#[cfg(unix)]
fn retire_session_and_restore_unix_handlers<T>(
    generation: usize,
    handlers: &[(i32, T)],
    restore: impl FnMut(i32, &T) -> std::result::Result<(), String>,
    install_default: impl FnMut(i32) -> std::result::Result<(), String>,
) -> (bool, RestoreReport) {
    // Once TerminationSession starts dropping, every resource owner declared
    // after it has already completed its bounded cleanup. Retire that ownership
    // before restoring dispositions so an in-flight Jig handler cannot mistake
    // the restoration window for a live-resource grace period.
    let generation_retired = retire_session_generation(generation);
    let restoration = restore_with_default_fallback(handlers, restore, install_default);
    (generation_retired, restoration)
}

fn wait_for_termination_handlers(timeout: Duration) -> bool {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        if TERMINATION_HANDLERS_IN_FLIGHT.load(Ordering::SeqCst) == 0 {
            return true;
        }
        if deadline.is_none_or(|deadline| Instant::now() >= deadline) {
            return false;
        }
        thread::yield_now();
    }
}

fn reset_session_state() {
    FORCE_CLEANUP_REQUESTED.store(false, Ordering::SeqCst);
    TERMINATION_SIGNAL.store(0, Ordering::SeqCst);
    ACTIVE_SESSION_GENERATION.store(0, Ordering::SeqCst);
    SESSION_PHASE.store(SESSION_INACTIVE, Ordering::SeqCst);
    SESSION_CLAIMED.store(false, Ordering::SeqCst);
}

pub(super) fn start_termination_cleanup_session() -> Result<TerminationSession> {
    TerminationSession::start()
}

pub(super) fn arm_owned_resources() -> Result<()> {
    if !SESSION_CLAIMED.load(Ordering::SeqCst) {
        // Tests with an injected interruption probe intentionally exercise the
        // runner without installing process-global handlers.
        return Ok(());
    }
    match OWNED_RESOURCE_STATE.compare_exchange(
        RESOURCES_UNARMED,
        RESOURCES_ARMED,
        Ordering::SeqCst,
        Ordering::SeqCst,
    ) {
        Ok(_) | Err(RESOURCES_ARMED) => Ok(()),
        Err(_) => Err(anyhow::anyhow!(
            "termination was forced before resources could start"
        )),
    }
}

pub(super) fn select_primary_outcome() {
    if SESSION_CLAIMED.load(Ordering::SeqCst) {
        SESSION_PHASE.store(SESSION_FINALIZING, Ordering::SeqCst);
    }
}

pub(super) fn select_interruption() {
    if SESSION_CLAIMED.load(Ordering::SeqCst) {
        SESSION_PHASE.store(SESSION_INTERRUPTING, Ordering::SeqCst);
    }
}

pub(super) fn termination_requested() -> Option<TerminationReason> {
    let phase = SESSION_PHASE.load(Ordering::SeqCst);
    if phase != SESSION_RUNNING && phase != SESSION_INTERRUPTING {
        return None;
    }
    let signal = TERMINATION_SIGNAL.load(Ordering::SeqCst);
    (signal != 0).then_some(TerminationReason::from_signal(signal))
}

pub(crate) fn force_cleanup_requested() -> bool {
    FORCE_CLEANUP_REQUESTED.load(Ordering::SeqCst)
}

#[cfg(test)]
fn record_termination(signal: i32) {
    record_termination_for_generation(signal, ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst));
}

fn record_termination_for_generation(signal: i32, generation: usize) {
    if generation == 0 {
        force_terminate_without_owned_resources(signal, None);
        return;
    }
    if ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst) != generation {
        // Foreground termination ownership is process-one-shot. In production,
        // a captured nonzero generation can therefore become stale only when
        // its session retires to zero, after publishing resources unarmed.
        // Take that now-safe exit claim before returning. Synthetic mismatches
        // with armed resources remain inert and cannot affect another owner.
        terminate_if_owned_resources_are_unarmed(signal);
        return;
    }
    let phase = SESSION_PHASE.load(Ordering::SeqCst);
    match phase {
        SESSION_RUNNING | SESSION_INTERRUPTING | SESSION_FINALIZING => {
            match TERMINATION_SIGNAL.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
            {
                // The first signal remains a graceful request even when a
                // primary outcome has already selected finalization. Cleanup is
                // already underway in that phase; only a later signal may
                // shorten its bounded grace periods. Once resource retirement
                // has won, however, the same first signal must take the
                // conventional-exit claim instead of being cleared by teardown.
                Ok(_) if phase == SESSION_FINALIZING => {
                    // Do not re-check the old generation here. Retirement may
                    // publish generation zero after the handler's initial
                    // validation but before this resource-state claim.
                    terminate_if_owned_resources_are_unarmed(signal);
                }
                Ok(_) => {}
                Err(first_signal) => {
                    // The first reason remains sticky, but any later termination
                    // request is an explicit escalation to forced cleanup.
                    force_terminate_without_owned_resources(first_signal, Some(generation));
                }
            }
        }
        _ => force_terminate_without_owned_resources(signal, Some(generation)),
    }
}

#[cfg(unix)]
fn terminate_after_exit_claim(signal: i32) -> ! {
    unsafe { libc::_exit(128 + signal) }
}

#[cfg(not(unix))]
fn terminate_after_exit_claim(signal: i32) -> ! {
    // ctrlc invokes this callback on a regular handler thread on non-Unix.
    std::process::exit(128 + signal)
}

fn force_terminate_without_owned_resources(signal: i32, generation: Option<usize>) {
    let Some(generation) = generation else {
        terminate_if_owned_resources_are_unarmed(signal);
        return;
    };
    if ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst) != generation {
        // The process-one-shot session rule means a real mismatch is retirement,
        // not reuse by a newer owner. Retirement disarms resources before
        // publishing generation zero, so preserve the sticky first signal by
        // claiming its conventional exit before this stale handler returns.
        // An armed synthetic mismatch cannot win this transition or set force.
        terminate_if_owned_resources_are_unarmed(signal);
        return;
    }
    terminate_if_owned_resources_are_unarmed(signal);
    // Only a handler still attributed to the active nonzero generation can
    // request forced cleanup. Generation zero denotes no session owner.
    FORCE_CLEANUP_REQUESTED.store(true, Ordering::SeqCst);
}

fn terminate_if_owned_resources_are_unarmed(signal: i32) {
    // Resource startup and immediate exit contend on one atomic transition.
    // Whichever wins makes the other impossible: this is stronger than a
    // load-then-act check during the one allowed foreground session.
    if OWNED_RESOURCE_STATE
        .compare_exchange(
            RESOURCES_UNARMED,
            EXIT_WITHOUT_RESOURCES_CLAIMED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
    {
        // On Unix this is the only non-atomic handler action and `_exit` is
        // async-signal-safe. The successful claim proves no resource arm won.
        terminate_after_exit_claim(signal);
    }
}

fn enter_termination_handler() -> usize {
    // This must remain the handler's first stateful operation. Teardown first
    // retires the active generation and then waits for this count to reach
    // zero before resetting the process-global atomics. Registrations are also
    // one-shot because a callback dispatched before this instruction cannot be
    // counted or assigned safely to a later session.
    TERMINATION_HANDLERS_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst)
}

fn leave_termination_handler() {
    TERMINATION_HANDLERS_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
}

fn handle_termination_signal(signal: i32) {
    let generation = enter_termination_handler();
    record_termination_for_generation(signal, generation);
    leave_termination_handler();
}

#[cfg(unix)]
extern "C" fn unix_termination_handler(signal: libc::c_int) {
    // Signal context uses lock-free atomics and, only before owned resources
    // exist, async-signal-safe `_exit`. Child termination, route locking,
    // output cleanup, and disposition restoration stay on the foreground thread.
    handle_termination_signal(signal);
}

#[cfg(unix)]
fn install_unix_handlers()
-> std::result::Result<Vec<(i32, libc::sigaction)>, TransactionalInstallError> {
    install_transactionally(
        &[
            (libc::SIGINT, "SIGINT"),
            (libc::SIGHUP, "SIGHUP"),
            (libc::SIGTERM, "SIGTERM"),
        ],
        install_unix_handler,
        restore_unix_handler,
    )
}

#[cfg(unix)]
fn install_unix_handler(signal: i32) -> std::result::Result<libc::sigaction, String> {
    // These C signal structures are required to start zero-initialized.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = unix_termination_handler as *const () as usize;
    #[cfg(target_os = "nto")]
    let flags = 0;
    #[cfg(not(target_os = "nto"))]
    let flags = libc::SA_RESTART;
    action.sa_flags = flags as _;
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(previous)
}

#[cfg(unix)]
fn restore_unix_handler(
    signal: i32,
    previous: &libc::sigaction,
) -> std::result::Result<(), String> {
    if unsafe { libc::sigaction(signal, previous, std::ptr::null_mut()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(unix)]
fn install_default_unix_handler(signal: i32) -> std::result::Result<(), String> {
    // If restoring an inherited disposition fails, leaving Jig's scoped
    // handler installed would make it outlive its session. A default
    // disposition is the safest conventional fallback.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = libc::SIG_DFL;
    action.sa_flags = 0;
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(unix)]
struct RestoreReport {
    warnings: Vec<String>,
    handlers_may_remain: bool,
}

#[cfg(unix)]
fn restore_with_default_fallback<T>(
    handlers: &[(i32, T)],
    mut restore: impl FnMut(i32, &T) -> std::result::Result<(), String>,
    mut install_default: impl FnMut(i32) -> std::result::Result<(), String>,
) -> RestoreReport {
    let mut warnings = Vec::new();
    let mut handlers_may_remain = false;
    for (signal, previous) in handlers.iter().rev() {
        if let Err(restore_error) = restore(*signal, previous) {
            let fallback = match install_default(*signal) {
                Ok(()) => "installed the default disposition instead".to_string(),
                Err(fallback_error) => {
                    handlers_may_remain = true;
                    format!("default-disposition fallback also failed: {fallback_error}")
                }
            };
            warnings.push(format!(
                "jig proxy could not restore the previous {} disposition: {restore_error}; {fallback}",
                signal_label(*signal)
            ));
        }
    }
    RestoreReport {
        warnings,
        handlers_may_remain,
    }
}

#[derive(Debug)]
struct TransactionalInstallError {
    message: String,
    handlers_may_remain: bool,
}

impl std::fmt::Display for TransactionalInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn install_transactionally<T>(
    signals: &[(i32, &'static str)],
    mut install: impl FnMut(i32) -> std::result::Result<T, String>,
    mut restore: impl FnMut(i32, &T) -> std::result::Result<(), String>,
) -> std::result::Result<Vec<(i32, T)>, TransactionalInstallError> {
    let mut installed = Vec::with_capacity(signals.len());
    for (signal, label) in signals {
        match install(*signal) {
            Ok(previous) => installed.push((*signal, previous)),
            Err(error) => {
                let mut rollback_errors = Vec::new();
                for (installed_signal, previous) in installed.iter().rev() {
                    if let Err(rollback_error) = restore(*installed_signal, previous) {
                        rollback_errors.push(format!(
                            "{}: {rollback_error}",
                            signal_label(*installed_signal)
                        ));
                    }
                }
                let rollback = if rollback_errors.is_empty() {
                    String::new()
                } else {
                    format!("; rollback also failed for {}", rollback_errors.join(", "))
                };
                return Err(TransactionalInstallError {
                    message: format!("failed to register {label}: {error}{rollback}"),
                    handlers_may_remain: !rollback_errors.is_empty(),
                });
            }
        }
    }
    Ok(installed)
}

fn signal_label(signal: i32) -> &'static str {
    #[cfg(unix)]
    match signal {
        libc::SIGINT => "SIGINT",
        libc::SIGHUP => "SIGHUP",
        libc::SIGTERM => "SIGTERM",
        _ => "signal",
    }
    #[cfg(not(unix))]
    {
        let _ = signal;
        "Ctrl-C"
    }
}

#[cfg(not(unix))]
fn ensure_ctrl_c_handler() -> std::result::Result<(), String> {
    if CTRL_C_HANDLER_INSTALLED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let _guard = CTRL_C_INSTALL_LOCK
        .lock()
        .map_err(|_| "Ctrl-C installation lock was poisoned".to_string())?;
    if CTRL_C_HANDLER_INSTALLED.load(Ordering::SeqCst) {
        return Ok(());
    }
    const CTRL_C_SIGNAL: i32 = 2;
    ctrlc::set_handler(|| handle_termination_signal(CTRL_C_SIGNAL))
        .map_err(|error| error.to_string())?;
    CTRL_C_HANDLER_INSTALLED.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests;
