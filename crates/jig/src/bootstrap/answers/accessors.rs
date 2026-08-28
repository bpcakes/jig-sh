use super::*;

impl RenderAnswers {
    pub(in crate::bootstrap) const fn authored_repository(
        &self,
    ) -> Option<&AuthoredRepositoryModel> {
        self.authored_repository.as_ref()
    }

    pub(in crate::bootstrap) const fn authored_repository_commands(
        &self,
    ) -> &BTreeMap<String, String> {
        &self.authored_repository_commands
    }

    pub(in crate::bootstrap) fn default_branch(&self) -> &str {
        &self.default_branch
    }

    pub(in crate::bootstrap) fn template_source_url(&self) -> &str {
        &self.template_source_url
    }

    pub(in crate::bootstrap) fn frontend_apps(&self) -> &[FrontendApp] {
        &self.frontend_apps
    }

    pub(in crate::bootstrap) fn frontend_workspace_roots(&self) -> &[String] {
        &self.frontend_workspace_roots
    }

    pub(in crate::bootstrap) fn rust_crate_roots(&self) -> &[String] {
        &self.rust_crate_roots
    }

    pub(in crate::bootstrap) const fn harness_footprint(&self) -> HarnessFootprint {
        self.harness_footprint
    }

    pub(in crate::bootstrap) const fn backend_language(&self) -> BackendLanguage {
        self.backend_language
    }
}
