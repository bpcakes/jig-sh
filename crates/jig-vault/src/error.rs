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

#[derive(Debug)]
pub struct VaultError {
    kind: VaultErrorKind,
    message: String,
    source: Option<anyhow::Error>,
}

impl VaultError {
    pub fn new(kind: VaultErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    pub const fn kind(&self) -> VaultErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn from_anyhow(kind: VaultErrorKind, error: anyhow::Error) -> Self {
        let message = error.to_string();
        let has_distinct_source = error
            .downcast_ref::<ClassifiedVaultError>()
            .is_none_or(|error| error.source.is_some());
        let source = has_distinct_source.then_some(error);
        Self {
            kind,
            message,
            source,
        }
    }

    pub(crate) fn into_classified_anyhow(self) -> anyhow::Error {
        let Self {
            kind,
            message,
            source,
        } = self;
        ClassifiedVaultError {
            kind,
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
    message: String,
    source: Option<anyhow::Error>,
}

impl ClassifiedVaultError {
    fn new(kind: VaultErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
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
}
