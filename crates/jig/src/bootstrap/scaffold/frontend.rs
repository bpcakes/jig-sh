pub(super) use app::{
    FrontendBackendContext, FrontendDatabaseContext, FrontendDevProxyContext, FrontendScaffold,
};
pub(super) use workspace::{
    DATABASE_CONFIG_GUARD, frontend_workspace_relative_paths_for_backend,
    render_frontend_workspace_files_for_backend, scaffold_bootstrap_command,
};

mod app;
mod templates;
#[cfg(test)]
mod tests;
mod workspace;
