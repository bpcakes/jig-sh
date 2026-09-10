use jig_contract::{ActionSpec, PlannedTarget};

use super::*;
use crate::repository::RepositoryCatalog;

pub(crate) struct ExecutionAuthorityGuard {
    root: Dir,
    configuration: Vec<String>,
    directories: files::DirectoryAuthority,
    runners: files::RunnerFileAuthority,
}

impl SourceSnapshot {
    pub(crate) fn execution_authority(
        &mut self,
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        action: &ActionSpec,
        invocation: &PlannedTarget,
        expected_authority: &str,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<ExecutionAuthorityGuard> {
        files::same_directory(&self.root, &files::open_root(ctx.root())?)?;
        // These paths attest cwd identity across execution. File creation in a
        // stable directory is handled by the unchanged global effect checks.
        let original_directories = std::mem::replace(
            &mut self.directories,
            files::DirectoryAuthority::for_execution(),
        );
        self.execution_runners = Some(files::RunnerFileAuthority::default());
        let authority =
            super::super::authority::collect(ctx, catalog, action, invocation, self, budget);
        let directories = std::mem::replace(&mut self.directories, original_directories);
        let runners = self
            .execution_runners
            .take()
            .expect("execution runner observation is installed");
        if authority?.digest != expected_authority {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::RunnerChanged,
                "bound invocation authority changed before execution",
            ));
        }
        let guard = ExecutionAuthorityGuard {
            root: self.root.try_clone().map_err(|_| {
                CollectionFailure::new(
                    FreshnessReasonCode::CollectionFailed,
                    "repository authority capability could not be retained",
                )
            })?,
            configuration: self.configuration.clone(),
            directories,
            runners,
        };
        guard.revalidate(ctx, budget)?;
        Ok(guard)
    }
}

impl ExecutionAuthorityGuard {
    pub(crate) fn revalidate(
        &self,
        ctx: &RepoContext,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        budget.ensure_active()?;
        files::same_directory(&self.root, &files::open_root(ctx.root())?)?;
        self.runners.revalidate(&self.root, budget)?;
        self.directories.revalidate(&self.root, budget)?;
        if self.configuration != files::configuration(&self.root, budget)? {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::SourceRaced,
                "execution configuration changed while the target ran",
            ));
        }
        budget.ensure_active()
    }
}
