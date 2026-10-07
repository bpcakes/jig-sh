use std::fmt;

pub type Result<T> = std::result::Result<T, VaultError>;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultErrorKind {
    AlreadyExists,
    AuditTampered,
    Authentication,
    InvalidInput,
    Io,
    NotFound,
    Process,
    Serialization,
    Internal,
}

/// Operator-owned action required to recover a vault operation.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultRecovery {
    StorageConflict,
    Credential,
}

#[derive(Debug)]
pub struct VaultError {
    kind: VaultErrorKind,
    recovery: Option<VaultRecovery>,
    message: String,
    source: Option<anyhow::Error>,
}

impl VaultError {
    pub fn new(kind: VaultErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            recovery: None,
            message: message.into(),
            source: None,
        }
    }

    pub const fn kind(&self) -> VaultErrorKind {
        self.kind
    }

    pub const fn recovery(&self) -> Option<VaultRecovery> {
        self.recovery
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn from_anyhow(kind: VaultErrorKind, error: anyhow::Error) -> Self {
        let message = error.to_string();
        let recovery = error
            .downcast_ref::<ClassifiedVaultError>()
            .and_then(|error| error.recovery);
        let has_distinct_source = error
            .downcast_ref::<ClassifiedVaultError>()
            .is_none_or(|error| error.source.is_some());
        let source = has_distinct_source.then_some(error);
        Self {
            kind,
            recovery,
            message,
            source,
        }
    }

    pub(crate) fn into_classified_anyhow(self) -> anyhow::Error {
        let Self {
            kind,
            recovery,
            message,
            source,
        } = self;
        ClassifiedVaultError {
            kind,
            recovery,
            message,
            source,
        }
        .into()
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for VaultError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
    }
}

#[derive(Debug)]
pub(crate) struct ClassifiedVaultError {
    kind: VaultErrorKind,
    recovery: Option<VaultRecovery>,
    message: String,
    source: Option<anyhow::Error>,
}

impl ClassifiedVaultError {
    fn new(kind: VaultErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            recovery: None,
            message: message.into(),
            source: None,
        }
    }

    fn with_source(
        kind: VaultErrorKind,
        message: impl Into<String>,
        source: anyhow::Error,
    ) -> Self {
        Self {
            kind,
            recovery: source
                .downcast_ref::<Self>()
                .and_then(|error| error.recovery),
            message: message.into(),
            source: Some(source),
        }
    }

    const fn kind(&self) -> VaultErrorKind {
        self.kind
    }
}

impl fmt::Display for ClassifiedVaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ClassifiedVaultError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
    }
}

pub(crate) fn classified(kind: VaultErrorKind, message: impl Into<String>) -> anyhow::Error {
    ClassifiedVaultError::new(kind, message).into()
}

pub(crate) fn classified_recovery(
    kind: VaultErrorKind,
    recovery: VaultRecovery,
    message: impl Into<String>,
) -> anyhow::Error {
    let mut error = ClassifiedVaultError::new(kind, message);
    error.recovery = Some(recovery);
    error.into()
}

pub(crate) fn classify_recovery_source(
    kind: VaultErrorKind,
    recovery: VaultRecovery,
    message: impl Into<String>,
    source: anyhow::Error,
) -> anyhow::Error {
    let mut error = ClassifiedVaultError::with_source(kind, message, source);
    // A specific storage conflict takes precedence over generic retry guidance.
    error.recovery = error.recovery.or(Some(recovery));
    error.into()
}

pub(crate) fn classify_source(
    kind: VaultErrorKind,
    message: impl Into<String>,
    source: anyhow::Error,
) -> anyhow::Error {
    ClassifiedVaultError::with_source(kind, message, source).into()
}

pub(crate) fn classified_kind(error: &anyhow::Error) -> Option<VaultErrorKind> {
    error
        .downcast_ref::<ClassifiedVaultError>()
        .map(ClassifiedVaultError::kind)
}

pub(crate) fn vault_error_from_anyhow(default: VaultErrorKind, error: anyhow::Error) -> VaultError {
    let kind = classified_kind(&error).unwrap_or(default);
    VaultError::from_anyhow(kind, error)
}

#[cfg(test)]
mod tests {
    use super::{VaultError, VaultErrorKind, classified, classify_source};

    #[test]
    fn simple_classified_errors_do_not_repeat_the_same_source() {
        let error = VaultError::from_anyhow(
            VaultErrorKind::NotFound,
            classified(VaultErrorKind::NotFound, "vault does not exist"),
        );

        assert_eq!(format!("{error:#}"), "vault does not exist");
    }

    #[test]
    fn classified_source_errors_keep_cause_context() {
        use std::error::Error;

        let error = VaultError::from_anyhow(
            VaultErrorKind::Serialization,
            classify_source(
                VaultErrorKind::Serialization,
                "failed to parse vault file",
                anyhow::anyhow!("expected value"),
            ),
        );

        let source = error.source().expect("classified source should be kept");
        assert_eq!(source.to_string(), "failed to parse vault file");
        let cause = source.source().expect("classified cause should be kept");
        assert_eq!(cause.to_string(), "expected value");
    }

    #[test]
    fn recovery_metadata_survives_context_wrapping_and_public_error_roundtrip() {
        use super::{
            VaultRecovery, classified_recovery, classify_recovery_source, vault_error_from_anyhow,
        };

        let conflict = classified_recovery(
            VaultErrorKind::AlreadyExists,
            VaultRecovery::StorageConflict,
            "restore target occupied",
        );
        let pending = classify_recovery_source(
            VaultErrorKind::Io,
            VaultRecovery::Credential,
            "restore did not finish",
            conflict.context("installation failed"),
        );
        let error = vault_error_from_anyhow(VaultErrorKind::Internal, pending);
        assert_eq!(error.kind(), VaultErrorKind::Io);
        assert_eq!(error.recovery(), Some(VaultRecovery::StorageConflict));
        let error =
            vault_error_from_anyhow(VaultErrorKind::Internal, error.into_classified_anyhow());
        assert_eq!(error.kind(), VaultErrorKind::Io);
        assert_eq!(error.recovery(), Some(VaultRecovery::StorageConflict));
    }
}
