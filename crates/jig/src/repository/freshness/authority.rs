use std::path::{Component, Path, PathBuf};

use jig_contract::freshness::FreshnessReasonCode;
use jig_contract::{ActionRunner, ActionSpec, ArgvValue, PlannedTarget};
use serde_json::json;

use super::{
    CollectionBudget, CollectionFailure, CollectionResult, RepositoryCatalog,
    encoding::IdentityEncoder, source::SourceSnapshot,
};
use crate::context::RepoContext;

/// Bump when native result semantics change within the freshness epoch.
const NATIVE_IMPLEMENTATION_REVISION: &str = "jig-native-actions-v1";

pub(super) struct AuthorityDigest {
    pub(super) digest: String,
    pub(super) runner_digest: String,
    pub(super) invocation_digest: String,
}

pub(super) fn collect(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    action: &ActionSpec,
    invocation: &PlannedTarget,
    source: &mut SourceSnapshot,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<AuthorityDigest> {
    budget.ensure_active()?;
    let arguments = super::super::arguments::bind(
        catalog.contract_version(),
        action,
        invocation.arguments.clone(),
    )
    .map_err(|_| unavailable("bound invocation arguments are invalid or incomplete"))?;
    let mut runner = IdentityEncoder::new("jig-target-authority-v1", catalog.contract_version());
    runner.text("runner-component");
    let mut resolved = action.runner.clone();
    match &mut resolved {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            let command = ctx
                .command_for_key(command)
                .map_err(|_| unavailable("command runner authority could not be resolved"))?;
            runner.text(command);
        }
        ActionRunner::Argv {
            program,
            args,
            working_directory,
            environment,
        } => {
            if super::source::uses_file_projection(action) {
                let paths = repository_programs(
                    ctx,
                    program,
                    working_directory.as_deref(),
                    environment.get("PATH").map(String::as_str),
                    budget,
                )?;
                runner.number(paths.len() as u64);
                for path in paths {
                    source.require_runner_candidate(action, &path, budget)?;
                    runner.text(&path);
                }
            }
            let mut bound = Vec::new();
            for value in args.iter() {
                match value {
                    ArgvValue::Literal(value) => bound.push(ArgvValue::Literal(value.clone())),
                    ArgvValue::Argument { argument } => {
                        if let Some(value) = arguments.get(argument) {
                            bound.push(ArgvValue::Literal(value.clone()));
                        }
                    }
                }
            }
            *args = bound;
        }
        ActionRunner::Native { operation, .. } => {
            runner.text(NATIVE_IMPLEMENTATION_REVISION);
            if operation == jig_contract::tool::FILE_BUDGET {
                let prepared = invocation
                    .prepared_native_input
                    .as_ref()
                    .ok_or_else(|| unavailable("native file-budget authority was not prepared"))?;
                if prepared.schema_version != jig_contract::PreparedNativeInputV1::SCHEMA_VERSION
                    || !matches!(
                        prepared.policy,
                        jig_contract::PolicyPreparationV1::Ready { .. }
                    )
                    || !matches!(
                        prepared.comparison,
                        jig_contract::ComparisonPreparationV1::Ready { .. }
                    )
                {
                    return Err(unavailable(
                        "native prepared policy or comparison authority is unavailable",
                    ));
                }
            }
            runner.field(
                &serde_json::to_vec(&invocation.prepared_native_input)
                    .map_err(|_| unavailable("native authority could not be encoded"))?,
            );
        }
    }
    match &mut resolved {
        ActionRunner::Shell {
            working_directory, ..
        }
        | ActionRunner::Argv {
            working_directory, ..
        } => {
            let path = normalized_working_directory(working_directory.as_deref())?;
            // The configured path itself is authority; absolute checkout paths
            // never enter a digest. A symlinked cwd cannot create scoped proof.
            if super::source::uses_file_projection(action) {
                source.observe_working_directory(&path, budget)?;
            }
            *working_directory = Some(path);
        }
        _ => {}
    }
    runner.field(
        &serde_json::to_vec(&resolved)
            .map_err(|_| unavailable("runner authority could not be encoded"))?,
    );
    let runner_digest = runner.finish();

    let mut inputs = action.inputs.clone();
    inputs.sort();
    inputs.dedup();
    let mut effects = action.effects.clone();
    effects.sort();
    effects.dedup();
    let mut dependencies = action.depends_on.clone();
    dependencies.sort();
    dependencies.dedup();
    let timeout_seconds = action
        .timeout_seconds
        .unwrap_or_else(|| ctx.command_timeout().as_secs());
    let invocation_value = json!({
        "target": action.target, "intent": action.intent, "effects": effects,
        "inputs_policy": action.inputs_policy.unwrap_or_default(), "inputs": inputs,
        "depends_on": dependencies, "arguments": action.arguments, "bound_arguments": arguments,
        "result_parser": action.result_parser, "timeout_seconds": timeout_seconds,
        "output_limit_bytes": ctx.command_output_limit().bytes(),
    });
    let mut invocation_hash =
        IdentityEncoder::new("jig-target-authority-v1", catalog.contract_version());
    invocation_hash.text("invocation-component");
    invocation_hash.field(
        &serde_json::to_vec(&invocation_value)
            .map_err(|_| unavailable("invocation authority could not be encoded"))?,
    );
    let invocation_digest = invocation_hash.finish();
    let mut authority = IdentityEncoder::new("jig-target-authority-v1", catalog.contract_version());
    authority.text(catalog.config_digest());
    authority.text(&runner_digest);
    authority.text(&invocation_digest);
    budget.ensure_active()?;
    Ok(AuthorityDigest {
        digest: authority.finish(),
        runner_digest,
        invocation_digest,
    })
}

fn normalized_working_directory(configured: Option<&str>) -> CollectionResult<String> {
    crate::repository_path::normalize_portable_repo_path(
        configured.unwrap_or("."),
        "working_directory",
    )
    .map_err(|_| unavailable("working directory authority is invalid"))
}

fn repository_programs(
    ctx: &RepoContext,
    program: &str,
    working_directory: Option<&str>,
    declared_path: Option<&str>,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<Vec<String>> {
    budget.entries(1)?;
    let root = ctx
        .root()
        .canonicalize()
        .map_err(|_| unavailable("repository root authority is unavailable"))?;
    let cwd = root.join(normalized_working_directory(working_directory)?);
    let search_path = declared_path
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_else(|| super::super::runners::DEFAULT_ARGV_SEARCH_PATH.into());
    let candidates: Box<dyn Iterator<Item = PathBuf> + '_> = if program.contains('/') {
        Box::new(std::iter::once(cwd.join(program)))
    } else {
        Box::new(
            std::env::split_paths(&search_path).map(|directory| cwd.join(directory).join(program)),
        )
    };
    let mut repository_candidates = Vec::new();
    let mut executable_candidate = false;
    for candidate in candidates {
        budget.entries(1)?;
        let normalized = lexical_absolute(&candidate)?;
        // execve may skip an apparently executable file because its interpreter
        // is absent or access is denied. Include every reachable repository
        // candidate, including complete absence, instead of predicting success.
        if let Ok(relative) = normalized.strip_prefix(&root) {
            let path = relative
                .to_str()
                .ok_or_else(|| unavailable("runner path encoding is unsupported"))?;
            repository_candidates.push(path.to_owned());
        }
        // execve observes the original path. Collapsing '..' before resolving
        // a preceding symlink can select a different executable.
        let metadata = match std::fs::metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(_) => {
                return Err(unavailable(
                    "runner executable authority could not be resolved",
                ));
            }
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        executable_candidate = true;
        budget.entries(1)?;
        let physical = candidate
            .canonicalize()
            .map_err(|_| unavailable("runner path authority could not be resolved"))?;
        if physical.starts_with(&root) {
            reject_symlink_components(&candidate, budget)?;
            if normalized != physical {
                return Err(CollectionFailure::new(
                    FreshnessReasonCode::UnobservableInput,
                    "repository runner traverses a symlink",
                ));
            }
            continue;
        }
        if normalized.starts_with(&root) {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnobservableInput,
                "repository runner resolves outside the repository",
            ));
        }
        // External PATH tools are identified by the configured invocation in
        // v1; their installed bytes and ambient environment are outside scope.
    }
    if executable_candidate {
        Ok(repository_candidates)
    } else {
        Err(unavailable(
            "runner executable is missing or cannot be resolved",
        ))
    }
}

fn lexical_absolute(path: &Path) -> CollectionResult<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                if !result.pop() {
                    return Err(unavailable("runner path escapes its filesystem root"));
                }
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}

fn reject_symlink_components(
    path: &Path,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        budget.entries(1)?;
        prefix.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&prefix)
            .map_err(|_| unavailable("runner path authority could not be observed"))?;
        if metadata.file_type().is_symlink() {
            return Err(unavailable("repository runner traverses a symlink"));
        }
    }
    Ok(())
}

fn unavailable(message: &str) -> CollectionFailure {
    CollectionFailure::new(FreshnessReasonCode::UnobservableInput, message)
}
