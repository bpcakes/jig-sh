use super::*;

impl RenderAnswers {
    /// The saved document check's command, which generation keeps, as it
    /// keeps the other backend commands.
    pub(crate) fn openapi_check_command(&self) -> Option<&str> {
        self.openapi_check_command.as_deref()
    }

    /// Whether generation leaves out the document check of a saved model from
    /// before it, whose owner has not asked for it.
    pub(crate) const fn document_check_deferred(&self) -> bool {
        self.document_check_deferred
    }
}

/// Whether generation leaves out the document check: the saved model predates
/// it, and its owner has not asked for it with `jig update
/// --add-openapi-check`. The check runs the scaffold's document tests by name,
/// and those belong to the application, which may have renamed, moved, or
/// disabled them.
pub(super) fn document_check_deferred(
    raw: &RawAnswers,
    commands: Option<&BTreeMap<String, String>>,
) -> bool {
    let (Some(repository), Some(commands)) = (raw.repository.as_ref(), commands) else {
        return false;
    };
    !raw.add_openapi_check
        && repository.is_complete()
        && repository
            .predates_document_check(commands, raw.backend_language == Some(BackendLanguage::Go))
}

/// What an update says about a saved model from before the document check:
/// how to add the check, or that a custom model keeps its own targets.
pub(super) fn document_check_note(raw: &RawAnswers, custom: bool) -> Option<String> {
    let repository = raw.repository.as_ref()?;
    if raw.document_check_deferred {
        return Some(
            "api:openapi, which checks the committed OpenAPI documents with the backend's document tests, is not part of this repository model yet; `jig update --add-openapi-check` adds it, running api_openapi_command when set."
                .into(),
        );
    }
    (raw.add_openapi_check && custom && !repository.has_document_check()).then(|| {
        "--add-openapi-check left the custom repository model as it is; declare api:openapi in .jig.toml to add the check."
            .into()
    })
}
