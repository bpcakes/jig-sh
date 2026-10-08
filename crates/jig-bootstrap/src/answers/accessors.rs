use super::*;

impl RenderAnswers {
    pub const fn authored_repository(&self) -> Option<&AuthoredRepositoryModel> {
        self.authored_repository.as_ref()
    }

    pub const fn authored_repository_commands(&self) -> &BTreeMap<String, String> {
        &self.authored_repository_commands
    }

    pub fn default_branch(&self) -> &str {
        &self.default_branch
    }

    pub fn template_source_url(&self) -> &str {
        &self.template_source_url
    }

    pub fn frontend_apps(&self) -> &[FrontendApp] {
        &self.frontend_apps
    }

    pub fn frontend_workspace_roots(&self) -> &[String] {
        &self.frontend_workspace_roots
    }

    pub fn rust_crate_roots(&self) -> &[String] {
        &self.rust_crate_roots
    }

    pub const fn harness_footprint(&self) -> HarnessFootprint {
        self.harness_footprint
    }

    pub const fn backend_language(&self) -> BackendLanguage {
        self.backend_language
    }

    pub const fn repository_projection_hint(&self) -> RepositoryProjectionHint {
        self.repository_projection_hint
    }
}
