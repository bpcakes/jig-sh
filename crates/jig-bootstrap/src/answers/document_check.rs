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
/// how to add the check, or, when asking would leave a custom model as it
/// is, how to declare it.
pub(super) fn document_check_note(
    raw: &RawAnswers,
    commands: Option<&BTreeMap<String, String>>,
    custom: bool,
) -> Option<String> {
    let (Some(repository), Some(commands)) = (raw.repository.as_ref(), commands) else {
        return None;
    };
    if raw.add_openapi_check {
        return (custom && !repository.has_document_check()).then(|| {
            "--add-openapi-check left the custom repository model as it is; declare api:openapi in .jig.toml to add the check."
                .into()
        });
    }
    if !repository.is_complete() || !repository.lacks_document_check(commands) {
        return None;
    }
    // Asking adds the check, running a command its owner already set, unless
    // the model is custom for another reason.
    let mut asking = raw.clone();
    asking.add_openapi_check = true;
    asking.document_check_deferred = false;
    if asking.openapi_check_command.is_none() {
        asking.openapi_check_command = commands.get("api_openapi_command").cloned();
    }
    Some(
        if loaded_repository_model_is_custom(&asking, Some(commands)) {
            "api:openapi, which checks the committed OpenAPI documents with the backend's document tests, is not part of this custom repository model, which keeps its own targets; declare it in .jig.toml to add the check."
            .into()
        } else {
            "api:openapi, which checks the committed OpenAPI documents with the backend's document tests, is not part of this repository model yet; `jig update --add-openapi-check` adds it, running api_openapi_command when set."
            .into()
        },
    )
}
