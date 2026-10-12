use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const SIGNAL_QUIESCENCE_TIMEOUT: Duration = Duration::from_millis(500);

static RECORDED_SIGNAL: AtomicI32 = AtomicI32::new(0);
static SIGNAL_MASK: AtomicUsize = AtomicUsize::new(0);
pub(crate) static SIGNAL_GENERATION: AtomicUsize = AtomicUsize::new(0);
pub(crate) static ACTIVE_SIGNAL_GENERATION: AtomicUsize = AtomicUsize::new(0);
static NEXT_SIGNAL_GENERATION: AtomicUsize = AtomicUsize::new(0);
static SIGNAL_HANDLERS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static SIGNAL_SESSION_POISONED: AtomicBool = AtomicBool::new(false);
pub(crate) static SIGNAL_SESSION: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RecordedSignals {
    pub(crate) first: Option<libc::c_int>,
    pub(crate) mask: usize,
}

impl RecordedSignals {
    fn ordered(self) -> Vec<libc::c_int> {
        let mut signals = Vec::with_capacity(3);
        if let Some(first) = self.first {
            signals.push(first);
        }
        for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
            if Some(signal) != self.first && self.mask & signal_bit(signal) != 0 {
                signals.push(signal);
            }
        }
        signals
    }

    fn first(self) -> Option<libc::c_int> {
        self.first.or_else(|| {
            [libc::SIGINT, libc::SIGHUP, libc::SIGTERM]
                .into_iter()
                .find(|signal| self.mask & signal_bit(*signal) != 0)
        })
    }
}

pub(crate) const fn signal_bit(signal: libc::c_int) -> usize {
    match signal {
        libc::SIGINT => 1 << 0,
        libc::SIGHUP => 1 << 1,
        libc::SIGTERM => 1 << 2,
        _ => 0,
    }
}

#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_PAUSE_HANDLER: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_HANDLER_PAUSED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_RELEASE_HANDLER: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
pub(crate) static SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) extern "C" fn record_sqlx_probe_test_redelivery(signal: libc::c_int) {
    let index = SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
    if index < 3 {
        let code = match signal {
            libc::SIGINT => 1,
            libc::SIGHUP => 2,
            libc::SIGTERM => 3,
            _ => 0,
        };
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.fetch_or(code << (index * 2), Ordering::SeqCst);
    }
}

pub(crate) extern "C" fn record_signal(signal: libc::c_int) {
    #[cfg(test)]
    if SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.load(Ordering::SeqCst) {
        SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.store(true, Ordering::SeqCst);
        while !SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
    }

    // POSIX does not make disposition selection and the first user-space
    // handler instruction atomic across threads. Claim the generation that is
    // active when this callback actually enters: a delayed callback therefore
    // joins a later active session, while an idle callback fails closed below.
    SIGNAL_HANDLERS_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let generation = ACTIVE_SIGNAL_GENERATION.load(Ordering::SeqCst);

    #[cfg(test)]
    if SQLX_PROBE_TEST_PAUSE_HANDLER.load(Ordering::SeqCst) {
        SQLX_PROBE_TEST_HANDLER_PAUSED.store(true, Ordering::SeqCst);
        while !SQLX_PROBE_TEST_RELEASE_HANDLER.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
    }

    if generation != 0 && SIGNAL_GENERATION.load(Ordering::SeqCst) == generation {
        let _ = RECORDED_SIGNAL.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
        SIGNAL_MASK.fetch_or(signal_bit(signal), Ordering::SeqCst);
    }

    #[cfg(test)]
    if SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD.load(Ordering::SeqCst) {
        SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.store(true, Ordering::SeqCst);
        while !SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
    }

    SIGNAL_HANDLERS_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);

    if generation == 0 || SIGNAL_SESSION_POISONED.load(Ordering::SeqCst) {
        // A failed disposition restoration may leave this handler installed
        // after its session, and an unsafe retirement can no longer hand a
        // signal back through the restored disposition. Never swallow either
        // termination request.
        // SAFETY: `_exit` is async-signal-safe and this handler owns no
        // resources when there is no active probe generation.
        unsafe { libc::_exit(128 + signal) }
    }
}

struct ActiveSignalSession {
    generation: usize,
    previous_actions: Vec<(libc::c_int, libc::sigaction)>,
}

pub(crate) struct SignalSession {
    _guard: MutexGuard<'static, ()>,
    active: Option<ActiveSignalSession>,
}

#[derive(Clone, Copy)]
pub(crate) struct SignalCancellation {
    generation: usize,
}

impl SignalCancellation {
    pub(crate) fn cancelled(self) -> bool {
        SIGNAL_GENERATION.load(Ordering::SeqCst) == self.generation
            && RECORDED_SIGNAL.load(Ordering::SeqCst) != 0
    }
}

#[derive(Default)]
struct SignalRestoration {
    error: Option<std::io::Error>,
    handlers_may_remain: bool,
}

impl SignalSession {
    pub(crate) fn start() -> std::io::Result<Self> {
        let guard = SIGNAL_SESSION
            .lock()
            .map_err(|_| std::io::Error::other("the signal-session mutex is poisoned"))?;
        if SIGNAL_SESSION_POISONED.load(Ordering::SeqCst) {
            return Err(std::io::Error::other(
                "a prior signal session could not retire safely",
            ));
        }
        let previous_generation = NEXT_SIGNAL_GENERATION
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |generation| {
                generation.checked_add(1)
            })
            .map_err(|_| std::io::Error::other("signal-session generations are exhausted"))?;
        let generation = previous_generation + 1;
        RECORDED_SIGNAL.store(0, Ordering::SeqCst);
        SIGNAL_MASK.store(0, Ordering::SeqCst);
        SIGNAL_GENERATION.store(generation, Ordering::SeqCst);
        ACTIVE_SIGNAL_GENERATION.store(generation, Ordering::SeqCst);

        let mut session = Self {
            _guard: guard,
            active: Some(ActiveSignalSession {
                generation,
                previous_actions: Vec::new(),
            }),
        };
        for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
            // SAFETY: zero is a valid starting representation for sigaction;
            // the mask is then initialized with sigemptyset before use.
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = record_signal as *const () as usize;
            // SAFETY: `action.sa_mask` is writable storage owned by this call.
            if unsafe { libc::sigemptyset(&mut action.sa_mask) } == -1 {
                let error = std::io::Error::last_os_error();
                session.retire_after_failed_start();
                return Err(error);
            }
            action.sa_flags = 0;

            // SAFETY: `action` is initialized, `previous` points to writable
            // storage, and the signal is one of the supported termination
            // signals. The prior action is retained for scoped restoration.
            let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
            if unsafe { libc::sigaction(signal, &action, &mut previous) } == -1 {
                let error = std::io::Error::last_os_error();
                session.retire_after_failed_start();
                return Err(error);
            }
            session
                .active
                .as_mut()
                .expect("a starting signal session is active")
                .previous_actions
                .push((signal, previous));
        }
        Ok(session)
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancellation().cancelled()
    }

    pub(crate) fn cancellation(&self) -> SignalCancellation {
        SignalCancellation {
            generation: self.active.as_ref().map_or(0, |active| active.generation),
        }
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> usize {
        self.active.as_ref().map_or(0, |active| active.generation)
    }

    pub(crate) fn finish(mut self) -> std::io::Result<()> {
        let (signals, restored) = self.retire();
        complete_signal_retirement(signals, restored)
    }

    fn retire_after_failed_start(&mut self) {
        let (signals, restored) = self.retire();
        let _ = complete_signal_retirement(signals, restored);
    }

    fn retire(&mut self) -> (RecordedSignals, std::io::Result<()>) {
        let Some(active) = self.active.take() else {
            return (RecordedSignals::default(), Ok(()));
        };
        let generation = active.generation;
        let mut restoration = restore_handlers(&active.previous_actions);
        if ACTIVE_SIGNAL_GENERATION.compare_exchange(
            generation,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) != Ok(generation)
        {
            restoration.error.get_or_insert_with(|| {
                std::io::Error::other("the active signal-session generation changed unexpectedly")
            });
            restoration.handlers_may_remain = true;
        }
        let quiesced = wait_for_signal_quiescence(SIGNAL_QUIESCENCE_TIMEOUT);
        if !quiesced {
            restoration.error.get_or_insert_with(|| {
                std::io::Error::other("signal handlers did not become quiescent")
            });
        }

        let recorded_generation_retired = quiesced
            && SIGNAL_GENERATION.compare_exchange(
                generation,
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) == Ok(generation);
        if !recorded_generation_retired {
            restoration.error.get_or_insert_with(|| {
                std::io::Error::other("the recorded signal generation changed unexpectedly")
            });
        }
        let unsafe_retirement =
            !quiesced || !recorded_generation_retired || restoration.handlers_may_remain;
        if unsafe_retirement {
            // Publish the fail-closed claim before taking the recorded signal
            // snapshot. A handler that already passed the poison observation
            // recorded before this snapshot; a handler that has not passed it
            // will observe poison and terminate the process itself.
            SIGNAL_SESSION_POISONED.store(true, Ordering::SeqCst);
        }
        let signals = take_recorded_signals();
        (signals, restoration.error.map_or(Ok(()), Err))
    }
}

impl Drop for SignalSession {
    fn drop(&mut self) {
        if self.active.is_none() {
            return;
        }
        let (signals, restored) = self.retire();
        let _ = complete_signal_retirement(signals, restored);
    }
}

fn restore_handlers(previous_actions: &[(libc::c_int, libc::sigaction)]) -> SignalRestoration {
    let mut restoration = SignalRestoration::default();
    for (signal, action) in previous_actions.iter().rev() {
        // SAFETY: each action was returned by sigaction for this exact signal
        // when the scoped session started.
        if unsafe { libc::sigaction(*signal, action, std::ptr::null_mut()) } == -1 {
            let restore_error = std::io::Error::last_os_error();
            restoration.error.get_or_insert(restore_error);
            if install_default_signal_handler(*signal).is_err() {
                restoration.handlers_may_remain = true;
            }
        }
    }
    restoration
}

fn take_recorded_signals() -> RecordedSignals {
    let first = match RECORDED_SIGNAL.swap(0, Ordering::SeqCst) {
        0 => None,
        signal => Some(signal),
    };
    RecordedSignals {
        first,
        mask: SIGNAL_MASK.swap(0, Ordering::SeqCst),
    }
}

pub(crate) fn install_default_signal_handler(signal: libc::c_int) -> std::io::Result<()> {
    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = libc::SIG_DFL;
    action.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this call.
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `action` is fully initialized and the signal is one installed
    // by this scoped session.
    if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wait_for_signal_quiescence(timeout: Duration) -> bool {
    let deadline = Instant::now().checked_add(timeout);
    while SIGNAL_HANDLERS_IN_FLIGHT.load(Ordering::SeqCst) != 0 {
        let Some(remaining) =
            deadline.and_then(|deadline| deadline.checked_duration_since(Instant::now()))
        else {
            #[cfg(test)]
            if SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT.load(Ordering::SeqCst) {
                SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.store(true, Ordering::SeqCst);
                while !SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
            }
            return false;
        };
        std::thread::sleep(remaining.min(Duration::from_millis(1)));
    }
    true
}

fn redeliver_signal(signal: libc::c_int) {
    // The scoped handlers have been restored and the probe process tree is
    // already reaped. Raising now preserves the caller's original signal
    // semantics, including default termination and custom handlers.
    // SAFETY: `signal` was supplied by the OS to this process's handler.
    let _ = unsafe { libc::raise(signal) };
}

fn redeliver_signals(signals: RecordedSignals) {
    for signal in signals.ordered() {
        redeliver_signal(signal);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SignalFinishAction {
    Continue,
    Redeliver(RecordedSignals),
    Exit(libc::c_int),
}

pub(crate) fn signal_finish_action(signals: RecordedSignals, restored: bool) -> SignalFinishAction {
    match (signals.first(), restored) {
        (Some(_), true) => SignalFinishAction::Redeliver(signals),
        (Some(signal), false) => SignalFinishAction::Exit(128 + signal),
        (None, _) => SignalFinishAction::Continue,
    }
}

fn complete_signal_retirement(
    termination_signals: RecordedSignals,
    restored: std::io::Result<()>,
) -> std::io::Result<()> {
    match signal_finish_action(termination_signals, restored.is_ok()) {
        SignalFinishAction::Continue => {}
        SignalFinishAction::Redeliver(signals) => {
            redeliver_signals(signals);
        }
        SignalFinishAction::Exit(status) => {
            // Restoration failure means raising could invoke the Jig recorder
            // again and swallow termination. The probe tree is already gone.
            // SAFETY: this is the fail-closed process termination path.
            unsafe { libc::_exit(status) }
        }
    }
    restored
}

pub(crate) fn finish_signal_session(signal_session: SignalSession) -> std::io::Result<()> {
    // `finish` retains the process-wide mutex guard until handlers are restored
    // and every recorded signal has reached its prior disposition.
    signal_session.finish()
}
