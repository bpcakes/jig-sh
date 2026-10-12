use super::*;

impl RenderAnswers {
    /// The saved document check's command, which generation keeps, as it
    /// keeps the other backend commands.
    pub(crate) fn openapi_check_command(&self) -> Option<&str> {
        self.openapi_check_command.as_deref()
    }

    /// Whether a saved model from before the document check belongs to a
    /// repository whose backend no longer has the scaffold's document tests,
    /// so generation leaves out the check that would run them.
    pub(crate) const fn legacy_document_tests_missing(&self) -> bool {
        self.legacy_document_tests_missing
    }
}

/// Whether the saved model predates the document check, which generation
/// would add, and the repository at `root` lacks the tests the check runs.
/// The tests belong to the application, which may have renamed or moved them;
/// the model's drift check still exports the documents from the backend.
pub(super) fn legacy_document_tests_missing(
    raw: &RawAnswers,
    commands: Option<&BTreeMap<String, String>>,
    root: &Path,
) -> bool {
    let (Some(repository), Some(commands)) = (raw.repository.as_ref(), commands) else {
        return false;
    };
    let go = raw.backend_language == Some(BackendLanguage::Go);
    repository.is_complete()
        && repository.predates_document_check(commands, go)
        && !crate::adopt_infer::has_scaffold_document_tests(root, go)
}
