use serde::{Deserialize, Deserializer, Serialize};

use super::super::CollectionDomain;

pub const SNAPSHOT_ERROR_SCOPES: &[&str] = &[
    "repository",
    "state.sessions",
    "state.plans",
    "state.decisions",
    "state.receipts",
    "loops",
    "gates",
    "body",
];
pub const SNAPSHOT_ERROR_CODES: &[&str] = &[
    "git_observation_failed",
    "stream_open_failed",
    "stream_read_failed",
    "record_too_large",
    "record_decode_failed",
    "loop_observation_failed",
    "gate_observation_failed",
    "body_not_found",
    "body_unsafe_path",
    "body_unsafe_type",
    "body_read_failed",
    "body_invalid_utf8",
    "unsupported_platform",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotError {
    scope: String,
    code: String,
    subject_id: Option<String>,
    message: String,
}

impl SnapshotError {
    #[must_use]
    pub fn new(
        domain: CollectionDomain,
        code: SnapshotErrorCode,
        subject_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            scope: domain.as_str().to_string(),
            code: code.as_str().to_string(),
            subject_id,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl<'de> Deserialize<'de> for SnapshotError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            scope: String,
            code: String,
            subject_id: Option<String>,
            message: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        if !SNAPSHOT_ERROR_SCOPES.contains(&wire.scope.as_str()) {
            return Err(serde::de::Error::custom("unknown snapshot error scope"));
        }
        if !SNAPSHOT_ERROR_CODES.contains(&wire.code.as_str()) {
            return Err(serde::de::Error::custom("unknown snapshot error code"));
        }
        Ok(Self {
            scope: wire.scope,
            code: wire.code,
            subject_id: wire.subject_id,
            message: wire.message,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotErrorCode {
    GitObservationFailed,
    StreamOpenFailed,
    StreamReadFailed,
    RecordTooLarge,
    RecordDecodeFailed,
    LoopObservationFailed,
    GateObservationFailed,
    BodyNotFound,
    BodyUnsafePath,
    BodyUnsafeType,
    BodyReadFailed,
    BodyInvalidUtf8,
    UnsupportedPlatform,
}

impl SnapshotErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GitObservationFailed => "git_observation_failed",
            Self::StreamOpenFailed => "stream_open_failed",
            Self::StreamReadFailed => "stream_read_failed",
            Self::RecordTooLarge => "record_too_large",
            Self::RecordDecodeFailed => "record_decode_failed",
            Self::LoopObservationFailed => "loop_observation_failed",
            Self::GateObservationFailed => "gate_observation_failed",
            Self::BodyNotFound => "body_not_found",
            Self::BodyUnsafePath => "body_unsafe_path",
            Self::BodyUnsafeType => "body_unsafe_type",
            Self::BodyReadFailed => "body_read_failed",
            Self::BodyInvalidUtf8 => "body_invalid_utf8",
            Self::UnsupportedPlatform => "unsupported_platform",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Observation<T> {
    pub data: Option<T>,
    pub error: Option<SnapshotError>,
}

impl<T> Observation<T> {
    #[must_use]
    pub const fn available(data: T) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    #[must_use]
    pub const fn unavailable(error: SnapshotError) -> Self {
        Self {
            data: None,
            error: Some(error),
        }
    }

    #[must_use]
    pub const fn partial(data: T, error: SnapshotError) -> Self {
        Self {
            data: Some(data),
            error: Some(error),
        }
    }
}
