use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use jig_owned_process::{require_success, run_checked_output};
use tempfile::TempDir;
use toml::{Table, Value as TomlValue};

use super::git::{
    disable_git_worktree_integrations, ensure_clean_git_work_tree, git_stdout, is_git_work_tree,
    scrub_known_repository_git_environment, scrub_remote_template_git_environment,
};
use super::path::absolute_path_from;
use super::{
    ANSWERS_FILE, GIT_BIN_ENV, REMOTE_TEMPLATE_MODE_ERROR, TEMPLATE_LOCAL_PATH_KEY,
    TEMPLATE_MODE_KEY, TemplateMode, UpdateOpts, external_program, read_answers_toml,
};
const COMMIT_KEY: &str = "_commit";
const SRC_PATH_KEY: &str = "_src_path";
// Keep this sentinel in sync with templates/project/scripts/install-jig.sh.jinja.
pub(super) const EMBEDDED_TEMPLATE_SOURCE: &str = "embedded:jig-sh";

#[derive(Clone, Debug, Default)]
pub(super) struct TemplateIdentity {
    source: String,
    resolved_commit: Option<String>,
    local_path: Option<String>,
}

impl TemplateIdentity {
    const fn new(
        source: String,
        resolved_commit: Option<String>,
        local_path: Option<String>,
    ) -> Self {
        Self {
            source,
            resolved_commit,
            local_path,
        }
    }

    fn source(&self) -> &str {
        &self.source
    }

    fn resolved_commit(&self) -> Option<&str> {
        self.resolved_commit.as_deref()
    }

    fn local_path(&self) -> Option<&str> {
        self.local_path.as_deref()
    }

    fn template_mode_answer(&self) -> Option<&'static str> {
        self.local_path
            .as_ref()
            .map(|_| TemplateMode::Committed.as_str())
    }

    fn template_local_path_answer(&self) -> Option<&str> {
        self.local_path()
    }

    fn candidates(&self) -> [Option<&str>; 2] {
        [Some(self.source()), self.local_path()]
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(super) struct PrivateAnswerOverrides {
    template_mode: Option<TemplateMode>,
    template_local_path: Option<String>,
}

#[cfg(test)]
impl PrivateAnswerOverrides {
    pub(super) fn template_mode_answer(&self) -> Option<&'static str> {
        self.template_mode.map(TemplateMode::as_str)
    }

    pub(super) fn template_local_path_answer(&self) -> Option<&str> {
        self.template_local_path.as_deref()
    }

    #[cfg(test)]
    pub(super) fn test_committed(template_local_path: impl Into<String>) -> Self {
        Self {
            template_mode: Some(TemplateMode::Committed),
            template_local_path: Some(template_local_path.into()),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PreparedTemplateSource {
    identity: TemplateIdentity,
    render_source: TemplateRenderSource,
    _checkout: Option<Arc<TempDir>>,
}

#[derive(Clone, Debug)]
pub(super) enum TemplateRenderSource {
    Filesystem(PathBuf),
    Embedded,
}

impl PreparedTemplateSource {
    const fn new(
        source: String,
        render_source: TemplateRenderSource,
        resolved_commit: Option<String>,
        local_path: Option<String>,
        checkout: Option<Arc<TempDir>>,
    ) -> Self {
        Self {
            identity: TemplateIdentity::new(source, resolved_commit, local_path),
            render_source,
            _checkout: checkout,
        }
    }

    pub(super) fn source(&self) -> &str {
        self.identity.source()
    }

    pub(super) const fn render_source(&self) -> &TemplateRenderSource {
        &self.render_source
    }

    pub(super) fn vcs_ref(&self) -> Option<&str> {
        self.identity.resolved_commit()
    }

    pub(super) fn template_mode_answer(&self) -> Option<&'static str> {
        self.identity.template_mode_answer()
    }

    pub(super) fn template_local_path_answer(&self) -> Option<&str> {
        self.identity.template_local_path_answer()
    }

    #[cfg(test)]
    pub(super) fn test_local(
        source: String,
        render_root: PathBuf,
        vcs_ref: Option<String>,
        private_answers: PrivateAnswerOverrides,
    ) -> Self {
        Self::new(
            source,
            TemplateRenderSource::Filesystem(render_root),
            vcs_ref,
            private_answers.template_local_path,
            None,
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct StoredTemplateState {
    identity: TemplateIdentity,
    template_mode: Option<TemplateMode>,
}

impl StoredTemplateState {
    pub(super) fn has_source_path(&self) -> bool {
        !self.identity.source.is_empty()
    }

    pub(super) fn source_path(&self) -> &str {
        self.identity.source()
    }

    fn commit(&self) -> Option<&str> {
        self.identity.resolved_commit()
    }

    fn template_local_path(&self) -> Option<&str> {
        self.identity.local_path()
    }

    #[cfg(test)]
    pub(super) fn test_committed(
        src_path: impl Into<String>,
        template_local_path: Option<String>,
    ) -> Self {
        Self {
            identity: TemplateIdentity::new(src_path.into(), None, template_local_path),
            template_mode: Some(TemplateMode::Committed),
        }
    }
}

struct ResolvedUpdateTemplateSource<'a> {
    template: &'a str,
    template_mode: Option<TemplateMode>,
}

impl<'a> ResolvedUpdateTemplateSource<'a> {
    const fn new(template: &'a str, template_mode: Option<TemplateMode>) -> Self {
        Self {
            template,
            template_mode,
        }
    }
}

pub(super) fn prepare_template_source_from_base(
    template: &str,
    template_mode: Option<TemplateMode>,
    vcs_ref: Option<&str>,
    path_base: &Path,
) -> Result<PreparedTemplateSource> {
    if is_embedded_template_source(template) {
        if template_mode.is_some() {
            bail!("--template-mode only applies to local git template paths.");
        }
        if vcs_ref.is_some() {
            bail!("--vcs-ref only applies to remote templates or local git template paths.");
        }
        return Ok(PreparedTemplateSource::new(
            EMBEDDED_TEMPLATE_SOURCE.into(),
            TemplateRenderSource::Embedded,
            None,
            None,
            None,
        ));
    }

    if is_remote_template_source(template) {
        if template_mode.is_some() {
            bail!(REMOTE_TEMPLATE_MODE_ERROR);
        }
        return prepare_remote_template_source(template, vcs_ref);
    }

    let local_template = absolute_path_from(Path::new(template), path_base)?;
    if !local_template.is_dir() {
        bail!(
            "Template path is not a directory: {}",
            local_template.display()
        );
    }

    if !local_template.join("templates/project").is_dir() {
        bail!(
            "Template path does not contain templates/project: {}",
            local_template.display()
        );
    }

    if !is_git_work_tree(&local_template) {
        if vcs_ref.is_some() {
            bail!(
                "--vcs-ref only applies to remote templates or local git template paths: {}",
                local_template.display()
            );
        }
        if template_mode.is_some() {
            bail!(
                "Local template mode requires a git working tree: {}",
                local_template.display()
            );
        }
        return Ok(PreparedTemplateSource::new(
            local_template.display().to_string(),
            TemplateRenderSource::Filesystem(local_template),
            vcs_ref.map(str::to_string),
            None,
            None,
        ));
    }

    prepare_committed_template_source(&local_template, vcs_ref)
}

fn prepare_remote_template_source(
    template: &str,
    vcs_ref: Option<&str>,
) -> Result<PreparedTemplateSource> {
    let checkout = Arc::new(clone_template_source(template)?);
    let render_root = checkout.path().join("template");
    if let Some(vcs_ref) = vcs_ref {
        git_checkout(&render_root, vcs_ref)?;
    }
    let resolved_vcs_ref = git_stdout(&render_root, ["rev-parse", "HEAD"])?;

    Ok(PreparedTemplateSource::new(
        template.to_string(),
        TemplateRenderSource::Filesystem(render_root),
        Some(resolved_vcs_ref),
        None,
        Some(checkout),
    ))
}

fn prepare_committed_template_source(
    template_root: &Path,
    vcs_ref: Option<&str>,
) -> Result<PreparedTemplateSource> {
    ensure_clean_git_work_tree(template_root)?;
    let resolved_vcs_ref = match vcs_ref {
        Some(value) => git_stdout(template_root, ["rev-parse", &format!("{value}^{{commit}}")])?,
        None => git_stdout(template_root, ["rev-parse", "HEAD"])?,
    };
    let (render_root, checkout) = if vcs_ref.is_some() {
        let checkout = Arc::new(clone_template_source(&template_root.display().to_string())?);
        let render_root = checkout.path().join("template");
        git_checkout(&render_root, &resolved_vcs_ref)?;
        (render_root, Some(checkout))
    } else {
        (template_root.to_path_buf(), None)
    };
    let template_path = template_root.display().to_string();

    Ok(PreparedTemplateSource::new(
        template_path.clone(),
        TemplateRenderSource::Filesystem(render_root),
        Some(resolved_vcs_ref),
        Some(template_path),
        checkout,
    ))
}

fn clone_template_source(template: &str) -> Result<TempDir> {
    let checkout = TempDir::new().context("Failed to create template checkout directory")?;
    let destination = checkout.path().join("template");
    let empty_git_template = checkout.path().join("empty-git-template");
    fs::create_dir(&empty_git_template).context("Failed to create private empty Git template")?;
    let git_program = external_program(GIT_BIN_ENV, "git");
    let mut command = Command::new(&git_program);
    scrub_remote_template_git_environment(&mut command);
    disable_git_worktree_integrations(&mut command);
    command.env("GIT_TEMPLATE_DIR", &empty_git_template);
    let output = command
        .args(["--no-replace-objects", "clone", "--quiet", template])
        .arg(destination.as_os_str())
        .output()
        .with_context(|| format!("Failed to start {git_program}"))?;
    require_success(&output, |output| {
        format!(
            "git clone {template} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok(checkout)
}

fn git_checkout(repo: &Path, vcs_ref: &str) -> Result<()> {
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    scrub_known_repository_git_environment(&mut command);
    disable_git_worktree_integrations(&mut command);
    command
        .current_dir(repo)
        .args(["--no-replace-objects", "checkout", "--quiet", vcs_ref]);
    run_checked_output(&mut command, |output| {
        format!(
            "git checkout {vcs_ref} failed in {}\nstdout:\n{}\nstderr:\n{}",
            repo.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok(())
}

pub(super) fn read_stored_template_state(answers_path: &Path) -> Result<StoredTemplateState> {
    let answers = read_answers_toml(answers_path)?;
    let template_mode = optional_answer_string(&answers, TEMPLATE_MODE_KEY)
        .map(|value| parse_template_mode_answer(&value))
        .transpose()?;

    Ok(StoredTemplateState {
        identity: TemplateIdentity::new(
            optional_answer_string(&answers, SRC_PATH_KEY).unwrap_or_default(),
            optional_answer_string(&answers, COMMIT_KEY),
            optional_answer_string(&answers, TEMPLATE_LOCAL_PATH_KEY),
        ),
        template_mode,
    })
}

pub(super) fn prepare_update_template_source(
    opts: &UpdateOpts,
    stored: &StoredTemplateState,
    path_base: &Path,
) -> Result<Option<PreparedTemplateSource>> {
    let source_override_requested = opts.template.is_some() || opts.template_mode.is_some();
    if !source_override_requested && opts.vcs_ref.is_none() {
        return prepare_default_update_template_source(stored, opts.recopy, path_base);
    }

    let resolved_source = resolve_update_template_source(opts, stored);
    let prepared = prepare_template_source_from_base(
        resolved_source.template,
        resolved_source.template_mode,
        opts.vcs_ref
            .as_deref()
            .or_else(|| recopy_vcs_ref(opts.recopy, stored)),
        path_base,
    )?;
    ensure_update_template_identity(stored, &prepared)?;
    Ok(Some(final_update_template_state(stored, &prepared)))
}

fn recopy_vcs_ref(recopy: bool, stored: &StoredTemplateState) -> Option<&str> {
    if recopy { stored.commit() } else { None }
}

fn ensure_update_template_identity(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> Result<()> {
    if !stored.has_source_path()
        || is_embedded_template_source(stored.source_path())
        || template_identities_match(stored, prepared)
    {
        return Ok(());
    }

    bail!(
        "jig update cannot switch template source paths in-place. Re-run with the existing source path, or re-adopt the repo from the new template source."
    )
}

fn optional_answer_string(answers: &Table, key: &str) -> Option<String> {
    answers
        .get(key)
        .and_then(TomlValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn resolve_update_template_source<'a>(
    opts: &'a UpdateOpts,
    stored: &'a StoredTemplateState,
) -> ResolvedUpdateTemplateSource<'a> {
    if let Some(template) = opts.template.as_deref() {
        return ResolvedUpdateTemplateSource::new(
            template,
            inherited_update_template_mode(template, opts.template_mode, stored),
        );
    }

    if opts.template_mode == Some(TemplateMode::Committed) {
        if let Some(template) = stored.template_local_path() {
            return ResolvedUpdateTemplateSource::new(template, Some(TemplateMode::Committed));
        }

        if is_remote_template_source(stored.source_path()) {
            return ResolvedUpdateTemplateSource::new(stored.source_path(), None);
        }
    }

    ResolvedUpdateTemplateSource::new(
        stored_update_source(stored),
        inherited_update_template_mode(stored_update_source(stored), opts.template_mode, stored),
    )
}

#[cfg(test)]
pub(super) fn test_resolve_update_template_source(
    opts: &UpdateOpts,
    stored: &StoredTemplateState,
) -> (String, Option<TemplateMode>) {
    let resolved = resolve_update_template_source(opts, stored);
    (resolved.template.to_string(), resolved.template_mode)
}

fn prepare_default_update_template_source(
    stored: &StoredTemplateState,
    recopy: bool,
    path_base: &Path,
) -> Result<Option<PreparedTemplateSource>> {
    if stored.source_path().is_empty() {
        return Ok(None);
    }

    let source = stored_update_source(stored);
    let mode = inherited_update_template_mode(source, None, stored);
    let vcs_ref = if recopy { stored.commit() } else { None };
    let prepared = prepare_template_source_from_base(source, mode, vcs_ref, path_base)?;
    Ok(Some(final_update_template_state(stored, &prepared)))
}

fn inherited_update_template_mode(
    template: &str,
    requested_mode: Option<TemplateMode>,
    stored: &StoredTemplateState,
) -> Option<TemplateMode> {
    if requested_mode.is_some()
        || is_remote_template_source(template)
        || is_embedded_template_source(template)
    {
        requested_mode
    } else {
        stored.template_mode
    }
}

fn stored_update_source(stored: &StoredTemplateState) -> &str {
    if stored.template_mode != Some(TemplateMode::Committed) {
        return stored.source_path();
    }

    stored
        .template_local_path()
        .filter(|template| Path::new(template).is_dir())
        .unwrap_or(stored.source_path())
}

fn parse_template_mode_answer(value: &str) -> Result<TemplateMode> {
    match value {
        "committed" => Ok(TemplateMode::Committed),
        "working-tree" => bail!(
            "Unsupported legacy template mode 'working-tree' in {ANSWERS_FILE}. Re-adopt the repo or update it from a committed template source before running jig update."
        ),
        other => bail!("Unsupported template mode '{other}' in {ANSWERS_FILE}"),
    }
}

fn template_identities_match(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> bool {
    let prepared_identities = [Some(prepared.source()), prepared.identity.local_path()];

    stored
        .identity
        .candidates()
        .into_iter()
        .flatten()
        .any(|stored_identity| identity_matches(stored_identity, prepared_identities))
}

fn identity_matches(stored_identity: &str, prepared_identities: [Option<&str>; 2]) -> bool {
    prepared_identities
        .into_iter()
        .flatten()
        .any(|prepared_identity| stored_identity == prepared_identity)
}

fn final_update_template_state(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> PreparedTemplateSource {
    let mut final_template = prepared.clone();
    let same_committed_template = stored.template_mode == Some(TemplateMode::Committed)
        && template_identities_match(stored, prepared);

    if same_committed_template && final_template.identity.local_path.is_none() {
        // Legacy normalized remote committed state can carry committed mode without
        // a local checkout path. Preserve the rendered private answer shape while
        // keeping source identity anchored on _src_path.
        final_template.identity.local_path =
            Some(stored.identity.local_path.clone().unwrap_or_default());
    }

    final_template
}

#[cfg(test)]
pub(super) fn test_final_update_template_state(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> PreparedTemplateSource {
    final_update_template_state(stored, prepared)
}

pub(super) fn is_remote_template_source(template: &str) -> bool {
    template.contains("://") || template.starts_with("git@") && template.contains(':')
}

pub(super) fn is_embedded_template_source(template: &str) -> bool {
    template == EMBEDDED_TEMPLATE_SOURCE
}
