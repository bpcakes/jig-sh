use std::io::{self, Cursor, Read};

use jig_file_budget::{
    MeasurementBudgetV1, MeasurementErrorKindV1, MeasurementV1, measure_stream_v1,
};

fn measure(bytes: &[u8]) -> MeasurementV1 {
    let mut reader = Cursor::new(bytes);
    let mut budget = MeasurementBudgetV1::new(u64::MAX, u64::MAX);
    measure_stream_v1(&mut reader, &mut budget, || false).unwrap()
}

#[test]
fn counts_physical_lines_and_exact_bytes_bytewise() {
    let cases: &[(&[u8], u64)] = &[
        (b"", 0),
        (b"a", 1),
        (b"\n", 1),
        (b"a\n", 1),
        (b"a\nb", 2),
        (b"a\r\nb\r\n", 2),
        (b"\n\nunterminated", 3),
        (b"\0\xff\n\0", 2),
        (b"\xef\xbb\xbf", 1),
    ];
    for (bytes, expected_lines) in cases {
        assert_eq!(
            measure(bytes),
            MeasurementV1 {
                lines: *expected_lines,
                bytes: bytes.len() as u64,
            },
            "bytes: {bytes:?}"
        );
    }
}

struct Chunked<R> {
    inner: R,
    maximum: usize,
}

impl<R: Read> Read for Chunked<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer.len().min(self.maximum);
        self.inner.read(&mut buffer[..length])
    }
}

#[test]
fn streams_across_arbitrary_read_boundaries() {
    let bytes = b"first\r\nsecond\nthird\0unterminated";
    let expected = measure(bytes);
    for maximum in 1..bytes.len() {
        let mut reader = Chunked {
            inner: Cursor::new(bytes),
            maximum,
        };
        let mut budget = MeasurementBudgetV1::new(100, 100);
        assert_eq!(
            measure_stream_v1(&mut reader, &mut budget, || false).unwrap(),
            expected,
            "chunk size {maximum}"
        );
    }
}

#[test]
fn cancellation_is_checked_between_bounded_reads() {
    let mut reader = Chunked {
        inner: Cursor::new(b"abcdef"),
        maximum: 2,
    };
    let mut budget = MeasurementBudgetV1::new(100, 100);
    let mut checks = 0;
    let error = measure_stream_v1(&mut reader, &mut budget, || {
        checks += 1;
        checks == 2
    })
    .unwrap_err();
    assert_eq!(error.kind, MeasurementErrorKindV1::Cancelled);
    assert_eq!(error.observed_bytes, 2);
    assert_eq!(budget.total_bytes_read(), 2);
}

#[test]
fn enforces_per_file_and_aggregate_limits_without_accepting_partial_measurement() {
    let mut reader = Cursor::new(b"abcd");
    let mut budget = MeasurementBudgetV1::new(3, 100);
    let error = measure_stream_v1(&mut reader, &mut budget, || false).unwrap_err();
    assert_eq!(error.kind, MeasurementErrorKindV1::PerFileLimit);
    assert_eq!(error.limit_bytes, Some(3));
    assert_eq!(error.observed_bytes, 4);

    let mut aggregate = MeasurementBudgetV1::new(10, 5);
    assert_eq!(
        measure_stream_v1(&mut Cursor::new(b"abc"), &mut aggregate, || false)
            .unwrap()
            .bytes,
        3
    );
    let error = measure_stream_v1(&mut Cursor::new(b"def"), &mut aggregate, || false).unwrap_err();
    assert_eq!(error.kind, MeasurementErrorKindV1::TotalLimit);
    assert_eq!(error.limit_bytes, Some(5));
    assert_eq!(error.observed_bytes, 6);
}

#[test]
fn exact_limits_and_empty_files_at_zero_limit_pass() {
    let mut exact = MeasurementBudgetV1::new(3, 3);
    assert_eq!(
        measure_stream_v1(&mut Cursor::new(b"abc"), &mut exact, || false).unwrap(),
        MeasurementV1 { lines: 1, bytes: 3 }
    );
    let mut zero = MeasurementBudgetV1::new(0, 0);
    assert_eq!(
        measure_stream_v1(&mut Cursor::new(b""), &mut zero, || false).unwrap(),
        MeasurementV1 { lines: 0, bytes: 0 }
    );
}

#[test]
fn io_errors_remain_typed() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "hidden"))
        }
    }
    let error =
        measure_stream_v1(&mut Broken, &mut MeasurementBudgetV1::new(1, 1), || false).unwrap_err();
    assert_eq!(
        error.kind,
        MeasurementErrorKindV1::Read(io::ErrorKind::PermissionDenied)
    );
    assert!(!error.to_string().contains("hidden"));
}

#[test]
fn interrupted_reads_are_retried_without_consuming_budget() {
    struct InterruptedOnce {
        interrupted: bool,
        bytes: Cursor<&'static [u8]>,
    }
    impl Read for InterruptedOnce {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.bytes.read(buffer)
        }
    }
    let mut reader = InterruptedOnce {
        interrupted: false,
        bytes: Cursor::new(b"a\nb"),
    };
    let mut budget = MeasurementBudgetV1::new(3, 3);
    assert_eq!(
        measure_stream_v1(&mut reader, &mut budget, || false).unwrap(),
        MeasurementV1 { lines: 2, bytes: 3 }
    );
    assert_eq!(budget.total_bytes_read(), 3);
}

#[test]
fn over_reporting_readers_fail_before_mutating_counters() {
    struct OverReports;
    impl Read for OverReports {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            Ok(buffer.len() + 1)
        }
    }
    let mut budget = MeasurementBudgetV1::new(100, 100);
    let error = measure_stream_v1(&mut OverReports, &mut budget, || false).unwrap_err();
    assert_eq!(error.kind, MeasurementErrorKindV1::InvalidReadLength);
    assert_eq!(error.limit_bytes, Some(101));
    assert_eq!(error.observed_bytes, 102);
    assert_eq!(budget.total_bytes_read(), 0);
}
