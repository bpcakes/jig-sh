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
    Integrity,
}

const fn default_recovery(kind: VaultErrorKind) -> Option<VaultRecovery> {
    match kind {
        VaultErrorKind::AuditTampered => Some(VaultRecovery::Integrity),
        _ => None,
    }
}

/// Recovery metadata for a diagnostic whose kind is owned by its caller.
/// This also lets combined failures retain secondary recovery information
/// without replacing the primary operation's I/O or process classification.
#[derive(Debug)]
struct RecoveryContext {
    recovery: Option<VaultRecovery>,
    message: String,
}

impl fmt::Display for RecoveryContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RecoveryContext {}

fn recovery_from_anyhow(error: &anyhow::Error) -> Option<VaultRecovery> {
    error
        .downcast_ref::<ClassifiedVaultError>()
        .and_then(|error| error.recovery)
        .or_else(|| {
            error
                .downcast_ref::<VaultError>()
                .and_then(|error| error.recovery)
        })
        .or_else(|| {
            error
                .downcast_ref::<RecoveryContext>()
                .and_then(|error| error.recovery)
        })
}

pub(crate) fn recovery_error(recovery: VaultRecovery, message: impl Into<String>) -> anyhow::Error {
    RecoveryContext {
        recovery: Some(recovery),
        message: message.into(),
    }
    .into()
}

pub(crate) fn context_with_secondary_recovery(
    primary: anyhow::Error,
    secondary: &anyhow::Error,
    message: impl Into<String>,
) -> anyhow::Error {
    let recovery = recovery_from_anyhow(&primary).or_else(|| recovery_from_anyhow(secondary));
    primary.context(RecoveryContext {
        recovery,
        message: message.into(),
    })
}

/// Add fallback recovery guidance without changing the caller-owned kind or
/// replacing a more specific action already attached to the failure.
pub(crate) fn with_recovery(error: anyhow::Error, recovery: VaultRecovery) -> anyhow::Error {
    if recovery_from_anyhow(&error).is_some() {
        return error;
    }
    let message = error.to_string();
    error.context(RecoveryContext {
        recovery: Some(recovery),
        message,
    })
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
            recovery: default_recovery(kind),
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
        let recovery = recovery_from_anyhow(&error).or(default_recovery(kind));
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
            recovery: default_recovery(kind),
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
            recovery: recovery_from_anyhow(&source).or(default_recovery(kind)),
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
    // Preserve the cause's recovery instruction before applying this wrapper's
    // explicit instruction; a kind-based default is only a fallback.
    let recovery = recovery_from_anyhow(&source).unwrap_or(recovery);
    let mut error = ClassifiedVaultError::with_source(kind, message, source);
    error.recovery = Some(recovery);
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
    fn integrity_metadata_survives_public_and_internal_wrappers() {
        use super::{VaultRecovery, classify_recovery_source, vault_error_from_anyhow};

        let direct = VaultError::new(VaultErrorKind::AuditTampered, "audit missing");
        assert_eq!(direct.recovery(), Some(VaultRecovery::Integrity));
        let wrapped = classify_recovery_source(
            VaultErrorKind::Io,
            VaultRecovery::Credential,
            "pending operation failed",
            classified(VaultErrorKind::AuditTampered, "journal mismatch")
                .context("recovery failed"),
        );
        let public = vault_error_from_anyhow(VaultErrorKind::Internal, wrapped);
        assert_eq!(public.kind(), VaultErrorKind::Io);
        assert_eq!(public.recovery(), Some(VaultRecovery::Integrity));
        let roundtrip =
            vault_error_from_anyhow(VaultErrorKind::Internal, public.into_classified_anyhow());
        assert_eq!(roundtrip.recovery(), Some(VaultRecovery::Integrity));
        let unclassified = VaultError::from_anyhow(
            VaultErrorKind::AuditTampered,
            anyhow::anyhow!("audit failed"),
        );
        assert_eq!(unclassified.recovery(), Some(VaultRecovery::Integrity));
    }

    #[test]
    fn explicit_recovery_takes_precedence_over_the_error_kind_default() {
        use super::{VaultRecovery, classify_recovery_source, vault_error_from_anyhow};

        let wrapped = classify_recovery_source(
            VaultErrorKind::AuditTampered,
            VaultRecovery::Credential,
            "authentication required",
            anyhow::anyhow!("credential needed"),
        );
        let error = vault_error_from_anyhow(VaultErrorKind::Internal, wrapped);
        assert_eq!(error.recovery(), Some(VaultRecovery::Credential));
    }

    #[test]
    fn combined_failures_retain_secondary_recovery_and_primary_classification() {
        use super::{
            VaultRecovery, context_with_secondary_recovery, recovery_error, vault_error_from_anyhow,
        };
        for kind in [VaultErrorKind::Io, VaultErrorKind::Process] {
            let secondary = classified(VaultErrorKind::AuditTampered, "mutation anchor missing");
            let combined = context_with_secondary_recovery(
                classified(kind, "operation failed"),
                &secondary,
                "operation and audit failed",
            );
            let error = vault_error_from_anyhow(VaultErrorKind::Internal, combined);
            assert_eq!(error.kind(), kind);
            assert_eq!(error.recovery(), Some(VaultRecovery::Integrity));
            let roundtrip =
                vault_error_from_anyhow(VaultErrorKind::Internal, error.into_classified_anyhow());
            assert_eq!(roundtrip.recovery(), Some(VaultRecovery::Integrity));
        }
        let secondary = recovery_error(VaultRecovery::Integrity, "journal malformed");
        let primary = super::classified_recovery(
            VaultErrorKind::AlreadyExists,
            VaultRecovery::StorageConflict,
            "target occupied",
        );
        let error = vault_error_from_anyhow(
            VaultErrorKind::Internal,
            context_with_secondary_recovery(primary, &secondary, "two failures"),
        );
        assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
        assert_eq!(error.recovery(), Some(VaultRecovery::StorageConflict));
    }

    #[test]
    fn recovery_only_diagnostics_preserve_the_callers_kind() {
        use super::{VaultRecovery, recovery_error, vault_error_from_anyhow};
        for kind in [VaultErrorKind::Io, VaultErrorKind::AuditTampered] {
            let error = vault_error_from_anyhow(
                kind,
                recovery_error(VaultRecovery::Integrity, "journal malformed"),
            );
            assert_eq!(error.kind(), kind);
            assert_eq!(error.recovery(), Some(VaultRecovery::Integrity));
        }
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
