#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminationReason {
    signal: i32,
    requested_stop: bool,
}

impl TerminationReason {
    pub(crate) const fn from_signal(signal: i32) -> Self {
        Self {
            signal,
            requested_stop: false,
        }
    }

    pub(crate) const fn requested_stop() -> Self {
        Self {
            signal: 0,
            requested_stop: true,
        }
    }

    pub(crate) const fn signal(self) -> i32 {
        self.signal
    }

    pub(crate) const fn exit_status(self) -> i32 {
        if self.requested_stop {
            0
        } else {
            128 + self.signal
        }
    }

    pub(crate) const fn is_requested_stop(self) -> bool {
        self.requested_stop
    }

    pub(crate) const fn label(self) -> &'static str {
        if self.requested_stop {
            return "dev stop";
        }
        #[cfg(unix)]
        match self.signal {
            libc::SIGINT => "SIGINT",
            libc::SIGHUP => "SIGHUP",
            libc::SIGTERM => "SIGTERM",
            _ => "signal",
        }
        #[cfg(not(unix))]
        {
            "Ctrl-C"
        }
    }
}
