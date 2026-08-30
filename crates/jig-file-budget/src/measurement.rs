use std::fmt;
use std::io::{self, Read};

use serde::{Deserialize, Serialize};

const READ_BUFFER_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MeasurementV1 {
    pub lines: u64,
    pub bytes: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MeasurementBudgetV1 {
    max_file_bytes: u64,
    max_total_bytes: u64,
    total_bytes_read: u64,
}

impl MeasurementBudgetV1 {
    /// Create a byte budget. Every byte successfully read is charged to the
    /// aggregate total, including bytes read from a file that later exceeds
    /// its per-file limit.
    #[must_use]
    pub const fn new(max_file_bytes: u64, max_total_bytes: u64) -> Self {
        Self {
            max_file_bytes,
            max_total_bytes,
            total_bytes_read: 0,
        }
    }

    #[must_use]
    pub const fn max_file_bytes(&self) -> u64 {
        self.max_file_bytes
    }

    #[must_use]
    pub const fn max_total_bytes(&self) -> u64 {
        self.max_total_bytes
    }

    #[must_use]
    pub const fn total_bytes_read(&self) -> u64 {
        self.total_bytes_read
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementErrorKindV1 {
    Cancelled,
    InvalidReadLength,
    PerFileLimit,
    TotalLimit,
    CounterOverflow,
    Read(io::ErrorKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementErrorV1 {
    pub kind: MeasurementErrorKindV1,
    pub observed_bytes: u64,
    pub limit_bytes: Option<u64>,
}

impl fmt::Display for MeasurementErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            MeasurementErrorKindV1::Cancelled => formatter.write_str("measurement was cancelled"),
            MeasurementErrorKindV1::InvalidReadLength => formatter.write_str(
                "byte stream reported reading more bytes than the supplied buffer could hold",
            ),
            MeasurementErrorKindV1::PerFileLimit => write!(
                formatter,
                "file measurement exceeded its {}-byte limit",
                self.limit_bytes.unwrap_or_default()
            ),
            MeasurementErrorKindV1::TotalLimit => write!(
                formatter,
                "aggregate measurement exceeded its {}-byte limit",
                self.limit_bytes.unwrap_or_default()
            ),
            MeasurementErrorKindV1::CounterOverflow => {
                formatter.write_str("measurement counter overflowed u64")
            }
            MeasurementErrorKindV1::Read(kind) => {
                write!(formatter, "byte stream read failed with {kind:?}")
            }
        }
    }
}

impl std::error::Error for MeasurementErrorV1 {}

/// Measure physical LF-delimited lines and exact bytes in one bounded pass.
///
/// Cancellation is checked immediately before every bounded read. The caller
/// owns opening and validating the stream; this crate never traverses a
/// repository or follows a filesystem path.
pub fn measure_stream_v1(
    reader: &mut impl Read,
    budget: &mut MeasurementBudgetV1,
    mut is_cancelled: impl FnMut() -> bool,
) -> Result<MeasurementV1, MeasurementErrorV1> {
    let mut buffer = [0_u8; READ_BUFFER_BYTES];
    let mut bytes = 0_u64;
    let mut lf_count = 0_u64;
    let mut final_byte_was_lf = false;

    loop {
        if is_cancelled() {
            return Err(MeasurementErrorV1 {
                kind: MeasurementErrorKindV1::Cancelled,
                observed_bytes: bytes,
                limit_bytes: None,
            });
        }
        let file_probe = budget
            .max_file_bytes
            .saturating_sub(bytes)
            .saturating_add(1);
        let total_probe = budget
            .max_total_bytes
            .saturating_sub(budget.total_bytes_read)
            .saturating_add(1);
        let read_bound_u64 = file_probe.min(total_probe).min(READ_BUFFER_BYTES as u64);
        let read_bound = usize::try_from(read_bound_u64).expect("read bound fits usize");
        let read = match reader.read(&mut buffer[..read_bound]) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(MeasurementErrorV1 {
                    kind: MeasurementErrorKindV1::Read(error.kind()),
                    observed_bytes: bytes,
                    limit_bytes: None,
                });
            }
        };
        if read == 0 {
            break;
        }
        if read > read_bound {
            return Err(MeasurementErrorV1 {
                kind: MeasurementErrorKindV1::InvalidReadLength,
                observed_bytes: u64::try_from(read).unwrap_or(u64::MAX),
                limit_bytes: Some(read_bound_u64),
            });
        }
        let read = u64::try_from(read).expect("read length fits u64");
        bytes = bytes.checked_add(read).ok_or(MeasurementErrorV1 {
            kind: MeasurementErrorKindV1::CounterOverflow,
            observed_bytes: bytes,
            limit_bytes: None,
        })?;
        budget.total_bytes_read =
            budget
                .total_bytes_read
                .checked_add(read)
                .ok_or(MeasurementErrorV1 {
                    kind: MeasurementErrorKindV1::CounterOverflow,
                    observed_bytes: bytes,
                    limit_bytes: None,
                })?;
        if bytes > budget.max_file_bytes {
            return Err(MeasurementErrorV1 {
                kind: MeasurementErrorKindV1::PerFileLimit,
                observed_bytes: bytes,
                limit_bytes: Some(budget.max_file_bytes),
            });
        }
        if budget.total_bytes_read > budget.max_total_bytes {
            return Err(MeasurementErrorV1 {
                kind: MeasurementErrorKindV1::TotalLimit,
                observed_bytes: budget.total_bytes_read,
                limit_bytes: Some(budget.max_total_bytes),
            });
        }
        let read = usize::try_from(read).expect("read length originated as usize");
        let chunk = &buffer[..read];
        let chunk_lf = u64::try_from(chunk.iter().filter(|byte| **byte == b'\n').count())
            .expect("chunk LF count fits u64");
        lf_count = lf_count.checked_add(chunk_lf).ok_or(MeasurementErrorV1 {
            kind: MeasurementErrorKindV1::CounterOverflow,
            observed_bytes: bytes,
            limit_bytes: None,
        })?;
        final_byte_was_lf = chunk.last() == Some(&b'\n');
    }

    let lines = if bytes == 0 || final_byte_was_lf {
        lf_count
    } else {
        lf_count.checked_add(1).ok_or(MeasurementErrorV1 {
            kind: MeasurementErrorKindV1::CounterOverflow,
            observed_bytes: bytes,
            limit_bytes: None,
        })?
    };
    Ok(MeasurementV1 { lines, bytes })
}
