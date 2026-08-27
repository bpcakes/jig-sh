//! File-backed prompt registry for `jig prompt`.
//!
//! User prompts live under `prompts/user/`, repo prompts under `.jig/prompts/`,
//! and prompt-pack prompts under `prompt-packs/<pack>/prompts/`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::PromptRenderRequest;
use time::OffsetDateTime;

const PROMPT_HOME_ENV: &str = "JIG_PROMPT_HOME";
const CLIPBOARD_COMMAND_ENV: &str = "JIG_PROMPT_CLIPBOARD_COMMAND";
const ARCHIVE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub(crate) struct PromptRegistry {
    user_dir: PathBuf,
    packs_dir: PathBuf,
    repo_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Selector {
    Unqualified(PromptName),
    User(PromptName),
    Repo(PromptName),
    Pack { pack: PackName, prompt: PromptName },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PromptName(String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PackName(String);

#[derive(Clone, Debug)]
struct LocatedPrompt {
    namespace: PromptNamespace,
    path: PathBuf,
    record: PromptRecord,
}

#[derive(Debug)]
struct PromptScan {
    prompts: Vec<LocatedPrompt>,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PromptNamespace {
    User,
    Repo,
    Pack(PackName),
}

#[derive(Clone, Debug)]
struct PromptRecord {
    name: PromptName,
    metadata: PromptMetadata,
    body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PromptMetadata {
    #[serde(default)]
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(default = "default_prompt_version")]
    version: u32,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_timestamp",
        skip_serializing_if = "Option::is_none"
    )]
    updated_at: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PromptArchive {
    schema_version: u32,
    prompts: Vec<PromptArchiveEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PromptArchiveEntry {
    namespace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pack: Option<String>,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    version: u32,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_timestamp",
        skip_serializing_if = "Option::is_none"
    )]
    updated_at: Option<i64>,
    body: String,
}

struct PlannedImport {
    namespace: PromptNamespace,
    path: PathBuf,
    metadata: PromptMetadata,
    body: String,
    overwritten: bool,
    original: Option<Vec<u8>>,
}

struct AppliedImport {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct PromptAddRequest {
    pub(crate) name: String,
    pub(crate) body: Option<String>,
    pub(crate) file: Option<PathBuf>,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
}

impl PromptRegistry {
    pub(crate) fn from_env(repo_root: Option<&Path>) -> Result<Self> {
        let base = if let Some(home) = std::env::var_os(PROMPT_HOME_ENV) {
            if home.is_empty() {
                bail!("{PROMPT_HOME_ENV} cannot be empty");
            }
            PathBuf::from(home)
        } else {
            dirs::config_dir()
                .ok_or_else(|| anyhow!("Could not resolve user config directory"))?
                .join("jig")
        };
        Ok(Self::new(base, repo_root.map(Path::to_path_buf)))
    }

    fn new(base: PathBuf, repo_root: Option<PathBuf>) -> Self {
        Self {
            user_dir: base.join("prompts/user"),
            packs_dir: base.join("prompt-packs"),
            repo_dir: repo_root.map(|root| root.join(".jig/prompts")),
        }
    }

    pub(crate) fn render_prompt(&self, request: PromptRenderRequest) -> Result<String> {
        let vars = parse_template_vars(&request.vars)?;
        let prompt = self.resolve_read(&parse_selector(&request.name)?)?;
        render_prompt_body(&prompt.record.body, &vars, request.raw)
    }

    pub(crate) fn copy_prompt(&self, request: PromptRenderRequest) -> Result<Value> {
        let vars = parse_template_vars(&request.vars)?;
        let prompt = self.resolve_read(&parse_selector(&request.name)?)?;
        let rendered = render_prompt_body(&prompt.record.body, &vars, request.raw)?;
        copy_to_clipboard(&rendered)?;
        Ok(json!({
            "ok": true,
            "command": "prompt copy",
            "name": prompt.record.name.0,
            "namespace": namespace_name(&prompt.namespace),
            "qualified_name": qualified_name(&prompt),
            "path": prompt.path,
            "raw": request.raw,
        }))
    }

    pub(crate) fn add_prompt(&self, request: PromptAddRequest) -> Result<Value> {
        let selector = parse_selector(&request.name)?;
        let (name, path, namespace) = self.resolve_write_target(selector, true)?;
        reject_name_dir_collision(&path, &name)?;
        let body = read_body(request.body, request.file.as_deref())?;
        let overwritten = path.exists();
        let existing = if overwritten {
            read_prompt_file(&path, name.clone()).ok()
        } else {
            None
        };
        let existing_metadata = existing.as_ref().map(|record| &record.metadata);
        let version =
            existing
                .as_ref()
                .map(|record| {
                    record.metadata.version.checked_add(1).ok_or_else(|| {
                        anyhow!("Prompt '{}' version is already at u32::MAX", name.0)
                    })
                })
                .transpose()?
                .unwrap_or(1);
        let metadata = PromptMetadata {
            name: name.0.clone(),
            description: request
                .description
                .or_else(|| existing_metadata.and_then(|metadata| metadata.description.clone())),
            tags: if request.tags.is_empty() {
                existing_metadata
                    .map(|metadata| metadata.tags.clone())
                    .unwrap_or_default()
            } else {
                normalize_tags(request.tags)
            },
            version,
            updated_at: Some(now_timestamp()),
        };
        write_prompt_file(&path, &metadata, &body)?;
        Ok(json!({
            "ok": true,
            "command": "prompt add",
            "namespace": namespace_name(&namespace),
            "name": name.0,
            "path": path,
            "overwritten": overwritten,
        }))
    }

    pub(crate) fn add_prompt_with_editor(&self, request: PromptAddRequest) -> Result<Value> {
        if request.body.is_some() || request.file.is_some() {
            bail!("prompt add editor mode requires BODY and --file to be omitted");
        }
        let selector = parse_selector(&request.name)?;
        let (name, path, namespace) = self.resolve_write_target(selector, true)?;
        reject_name_dir_collision(&path, &name)?;
        let overwritten = path.exists();
        let original = if overwritten {
            Some(fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?)
        } else {
            None
        };
        let existing = if overwritten {
            read_prompt_file(&path, name.clone()).ok()
        } else {
            None
        };
        let existing_metadata = existing.as_ref().map(|record| &record.metadata);
        let version =
            existing
                .as_ref()
                .map(|record| {
                    record.metadata.version.checked_add(1).ok_or_else(|| {
                        anyhow!("Prompt '{}' version is already at u32::MAX", name.0)
                    })
                })
                .transpose()?
                .unwrap_or(1);
        let metadata = PromptMetadata {
            name: name.0.clone(),
            description: request
                .description
                .or_else(|| existing_metadata.and_then(|metadata| metadata.description.clone())),
            tags: if request.tags.is_empty() {
                existing_metadata
                    .map(|metadata| metadata.tags.clone())
                    .unwrap_or_default()
            } else {
                normalize_tags(request.tags)
            },
            version,
            updated_at: Some(now_timestamp()),
        };
        let initial_body = existing
            .as_ref()
            .map(|record| record.body.as_str())
            .unwrap_or("");
        write_prompt_file(&path, &metadata, initial_body)?;
        if let Err(error) = open_editor(&path) {
            restore_after_failed_edit(&path, original.as_deref())?;
            return Err(error);
        }
        let record = match read_prompt_file(&path, name.clone()) {
            Ok(record) => record,
            Err(error) => {
                restore_after_failed_edit(&path, original.as_deref())?;
                return Err(error)
                    .with_context(|| format!("Edited prompt file is invalid: {}", path.display()));
            }
        };
        if record.body.trim().is_empty() {
            restore_after_failed_edit(&path, original.as_deref())?;
            if !overwritten {
                bail!(
                    "New prompt '{}' was empty after edit; no prompt was saved",
                    name.0
                );
            }
            bail!(
                "Prompt '{}' was empty after edit; original prompt was restored",
                name.0
            );
        }
        Ok(json!({
            "ok": true,
            "command": "prompt add",
            "namespace": namespace_name(&namespace),
            "name": name.0,
            "path": path,
            "overwritten": overwritten,
            "editor": true,
        }))
    }

    pub(crate) fn edit_prompt(&self, name: &str) -> Result<Value> {
        let selector = parse_selector(name)?;
        let (prompt_name, path, namespace) = self.resolve_edit_target(selector)?;
        let original = if path.exists() {
            Some(fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?)
        } else {
            None
        };
        if original.is_none() {
            let metadata = PromptMetadata {
                name: prompt_name.0.clone(),
                description: None,
                tags: Vec::new(),
                version: 1,
                updated_at: Some(now_timestamp()),
            };
            write_prompt_file(&path, &metadata, "")?;
        }
        if let Err(error) = open_editor(&path) {
            restore_after_failed_edit(&path, original.as_deref())?;
            return Err(error);
        }
        let record = match read_prompt_file(&path, prompt_name.clone()) {
            Ok(record) => record,
            Err(error) => {
                restore_after_failed_edit(&path, original.as_deref())?;
                return Err(error)
                    .with_context(|| format!("Edited prompt file is invalid: {}", path.display()));
            }
        };
        if record.metadata.name != prompt_name.0 {
            restore_after_failed_edit(&path, original.as_deref())?;
            bail!(
                "Edited prompt metadata name '{}' does not match prompt name '{}'",
                record.metadata.name,
                prompt_name.0
            );
        }
        if record.body.trim().is_empty() {
            restore_after_failed_edit(&path, original.as_deref())?;
            if original.is_none() {
                bail!(
                    "New prompt '{}' was empty after edit; no prompt was saved",
                    prompt_name.0
                );
            }
            bail!(
                "Prompt '{}' was empty after edit; original prompt was restored",
                prompt_name.0
            );
        }
        Ok(json!({
            "ok": true,
            "command": "prompt edit",
            "namespace": namespace_name(&namespace),
            "name": prompt_name.0,
            "path": path,
        }))
    }

    pub(crate) fn prompt_edit_target(&self, name: &str) -> Result<Value> {
        let selector = parse_selector(name)?;
        let (prompt_name, path, namespace) = self.resolve_edit_target(selector)?;
        Ok(json!({
            "ok": true,
            "command": "prompt edit",
            "namespace": namespace_name(&namespace),
            "name": prompt_name.0,
            "path": path,
            "editor": false,
            "exists": path.exists(),
        }))
    }

    pub(crate) fn remove_prompt(&self, name: &str) -> Result<Value> {
        let selector = parse_selector(name)?;
        let prompt = self.resolve_writable_existing(selector)?;
        fs::remove_file(&prompt.path)
            .with_context(|| format!("Failed to remove {}", prompt.path.display()))?;
        Ok(json!({
            "ok": true,
            "command": "prompt remove",
            "namespace": namespace_name(&prompt.namespace),
            "name": prompt.record.name.0,
            "path": prompt.path,
        }))
    }

    pub(crate) fn list_prompts(&self, include_packs: bool) -> Result<Value> {
        let scan = self.list_records(include_packs)?;
        Ok(json!({
            "ok": true,
            "command": "prompt list",
            "schema_version": 1,
            "prompts": scan.prompts.iter().map(prompt_summary_value).collect::<Vec<_>>(),
            "warnings": scan.warnings,
        }))
    }

    pub(crate) fn search_prompts(&self, query: &str, include_body: bool) -> Result<Value> {
        let needle = query.to_ascii_lowercase();
        let scan = self.list_records(true)?;
        let prompts = scan
            .prompts
            .into_iter()
            .filter(|prompt| prompt_matches(prompt, &needle, include_body))
            .collect::<Vec<_>>();
        Ok(json!({
            "ok": true,
            "command": "prompt search",
            "schema_version": 1,
            "query": query,
            "prompts": prompts.iter().map(prompt_summary_value).collect::<Vec<_>>(),
            "warnings": scan.warnings,
        }))
    }

    pub(crate) fn export_prompts(&self) -> Result<Value> {
        let scan = self.list_records(true)?;
        if !scan.warnings.is_empty() {
            bail!(
                "Cannot export prompt registry while invalid prompts exist:\n{}",
                scan.warnings.join("\n")
            );
        }
        let prompts = scan
            .prompts
            .into_iter()
            .map(|prompt| archive_entry(&prompt))
            .collect::<Vec<_>>();
        let archive = PromptArchive {
            schema_version: ARCHIVE_SCHEMA_VERSION,
            prompts,
        };
        serde_json::to_value(archive).context("Failed to serialize prompt archive")
    }

    pub(crate) fn import_prompts(&self, file: &Path) -> Result<Value> {
        let text = fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;
        let archive: PromptArchive = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse {}", file.display()))?;
        if archive.schema_version != ARCHIVE_SCHEMA_VERSION {
            bail!(
                "Unsupported prompt archive schema version {}; expected {}",
                archive.schema_version,
                ARCHIVE_SCHEMA_VERSION
            );
        }

        let mut planned = Vec::new();
        let mut destinations = BTreeSet::new();
        let mut folded_destinations = BTreeSet::new();
        for entry in archive.prompts {
            let name = validate_prompt_name(&entry.name)?;
            let metadata = PromptMetadata {
                name: name.0.clone(),
                description: entry.description,
                tags: normalize_tags(entry.tags),
                version: entry.version,
                updated_at: entry.updated_at.or_else(|| Some(now_timestamp())),
            };
            let (path, namespace) = match entry.namespace.as_str() {
                "user" => (
                    self.prompt_path(&self.user_dir, &name),
                    PromptNamespace::User,
                ),
                "repo" => {
                    let repo_dir = self.repo_dir.as_ref().ok_or_else(|| {
                        anyhow!("Cannot import repo prompt '{}' outside a Jig repo", name.0)
                    })?;
                    (self.prompt_path(repo_dir, &name), PromptNamespace::Repo)
                }
                "pack" => {
                    let pack = validate_pack_name(entry.pack.as_deref().ok_or_else(|| {
                        anyhow!("Prompt archive pack entry '{}' is missing pack", name.0)
                    })?)?;
                    (
                        self.pack_prompt_path(&pack, &name),
                        PromptNamespace::Pack(pack),
                    )
                }
                other => bail!("Unsupported prompt archive namespace '{other}'"),
            };
            let folded_destination = folded_path_key(&path);
            if destinations.contains(&path) || folded_destinations.contains(&folded_destination) {
                bail!(
                    "Prompt archive contains duplicate destination {}",
                    path.display()
                );
            }
            reject_name_dir_collision(&path, &name)?;
            reject_planned_name_dir_collision(&path, &name, &destinations, &folded_destinations)?;
            destinations.insert(path.clone());
            folded_destinations.insert(folded_destination);
            // This snapshot drives both the import report and rollback. If the
            // file changes between planning and rollback, the pre-import state
            // is intentionally restored.
            let original = if path.exists() {
                Some(fs::read(&path).with_context(|| {
                    format!("Failed to read existing prompt {}", path.display())
                })?)
            } else {
                None
            };
            let overwritten = original.is_some();
            planned.push(PlannedImport {
                namespace,
                path,
                metadata,
                body: entry.body,
                overwritten,
                original,
            });
        }

        let mut imported = Vec::new();
        let mut applied = Vec::new();
        for entry in planned {
            if let Err(error) = write_prompt_file(&entry.path, &entry.metadata, &entry.body) {
                if let Err(rollback_error) = rollback_import(applied) {
                    bail!(
                        "Failed to import prompt '{}': {error:#}\nAdditionally failed to roll back prior writes: {rollback_error:#}",
                        entry.metadata.name
                    );
                }
                return Err(error).with_context(|| {
                    format!(
                        "Failed to import prompt '{}'; prior writes were rolled back",
                        entry.metadata.name
                    )
                });
            }
            applied.push(AppliedImport {
                path: entry.path.clone(),
                original: entry.original.clone(),
            });
            imported.push(json!({
                "namespace": namespace_name(&entry.namespace),
                "pack": match &entry.namespace {
                    PromptNamespace::Pack(pack) => Some(pack.0.clone()),
                    _ => None,
                },
                "name": entry.metadata.name,
                "path": entry.path,
                "overwritten": entry.overwritten,
            }));
        }

        Ok(json!({
            "ok": true,
            "command": "prompt import",
            "imported": imported,
        }))
    }

    fn resolve_read(&self, selector: &Selector) -> Result<LocatedPrompt> {
        match selector {
            Selector::User(name) => self.read_required_at(
                PromptNamespace::User,
                self.prompt_path(&self.user_dir, name),
                name,
                format!("No user prompt named '{}'", name.0),
            ),
            Selector::Repo(name) => {
                let repo_dir = self
                    .repo_dir
                    .as_ref()
                    .ok_or_else(|| anyhow!("repo: prompts require running inside a Jig repo"))?;
                self.read_required_at(
                    PromptNamespace::Repo,
                    self.prompt_path(repo_dir, name),
                    name,
                    format!("No repo prompt named '{}'", name.0),
                )
            }
            Selector::Pack { pack, prompt } => self.read_required_at(
                PromptNamespace::Pack(pack.clone()),
                self.pack_prompt_path(pack, prompt),
                prompt,
                format!("No pack prompt named 'pack:{}/{}'", pack.0, prompt.0),
            ),
            Selector::Unqualified(name) => {
                let matches = self.read_unqualified(name)?;
                match matches.len() {
                    0 => bail!("No prompt named '{}'", name.0),
                    1 => Ok(matches.into_iter().next().unwrap()),
                    _ => bail!(
                        "Prompt name '{}' is ambiguous; use one of: {}",
                        name.0,
                        matches
                            .iter()
                            .map(qualified_name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }
            }
        }
    }

    fn resolve_writable_existing(&self, selector: Selector) -> Result<LocatedPrompt> {
        match selector {
            Selector::Pack { .. } => bail!("pack: prompts are read-only"),
            Selector::User(_) | Selector::Repo(_) => self.resolve_read(&selector),
            Selector::Unqualified(name) => {
                let matches = self
                    .read_unqualified(&name)?
                    .into_iter()
                    .filter(|prompt| !matches!(prompt.namespace, PromptNamespace::Pack(_)))
                    .collect::<Vec<_>>();
                match matches.len() {
                    0 => bail!("No writable prompt named '{}'", name.0),
                    1 => Ok(matches.into_iter().next().unwrap()),
                    _ => bail!(
                        "Writable prompt name '{}' is ambiguous; use user:{} or repo:{}",
                        name.0,
                        name.0,
                        name.0
                    ),
                }
            }
        }
    }

    fn resolve_edit_target(
        &self,
        selector: Selector,
    ) -> Result<(PromptName, PathBuf, PromptNamespace)> {
        match selector {
            Selector::Unqualified(name) => {
                let matches = self
                    .read_unqualified(&name)?
                    .into_iter()
                    .filter(|prompt| !matches!(prompt.namespace, PromptNamespace::Pack(_)))
                    .collect::<Vec<_>>();
                match matches.len() {
                    0 => Ok((
                        name.clone(),
                        self.prompt_path(&self.user_dir, &name),
                        PromptNamespace::User,
                    )),
                    1 => {
                        let prompt = matches.into_iter().next().unwrap();
                        Ok((prompt.record.name, prompt.path, prompt.namespace))
                    }
                    _ => bail!(
                        "Writable prompt name '{}' is ambiguous; use user:{} or repo:{}",
                        name.0,
                        name.0,
                        name.0
                    ),
                }
            }
            other => self.resolve_write_target(other, false),
        }
    }

    fn resolve_write_target(
        &self,
        selector: Selector,
        default_user: bool,
    ) -> Result<(PromptName, PathBuf, PromptNamespace)> {
        match selector {
            Selector::Unqualified(name) if default_user => Ok((
                name.clone(),
                self.prompt_path(&self.user_dir, &name),
                PromptNamespace::User,
            )),
            Selector::Unqualified(name) => bail!("Prompt '{}' does not exist", name.0),
            Selector::User(name) => Ok((
                name.clone(),
                self.prompt_path(&self.user_dir, &name),
                PromptNamespace::User,
            )),
            Selector::Repo(name) => {
                let repo_dir = self
                    .repo_dir
                    .as_ref()
                    .ok_or_else(|| anyhow!("repo: prompts require running inside a Jig repo"))?;
                Ok((
                    name.clone(),
                    self.prompt_path(repo_dir, &name),
                    PromptNamespace::Repo,
                ))
            }
            Selector::Pack { .. } => bail!("pack: prompts are read-only"),
        }
    }

    fn read_unqualified(&self, name: &PromptName) -> Result<Vec<LocatedPrompt>> {
        let mut matches = Vec::new();
        let mut invalid = Vec::new();
        match self.read_optional_at(
            PromptNamespace::User,
            self.prompt_path(&self.user_dir, name),
            name,
        ) {
            Ok(Some(prompt)) => matches.push(prompt),
            Ok(None) => {}
            Err(error) => invalid.push(error.to_string()),
        }
        if let Some(repo_dir) = &self.repo_dir {
            match self.read_optional_at(
                PromptNamespace::Repo,
                self.prompt_path(repo_dir, name),
                name,
            ) {
                Ok(Some(prompt)) => matches.push(prompt),
                Ok(None) => {}
                Err(error) => invalid.push(error.to_string()),
            }
        }
        let mut warnings = Vec::new();
        for pack in self.pack_names(&mut warnings) {
            match self.read_optional_at(
                PromptNamespace::Pack(pack.clone()),
                self.pack_prompt_path(&pack, name),
                name,
            ) {
                Ok(Some(prompt)) => matches.push(prompt),
                Ok(None) => {}
                Err(error) => invalid.push(error.to_string()),
            }
        }
        if !invalid.is_empty() {
            let valid = matches
                .iter()
                .map(qualified_name)
                .collect::<Vec<_>>()
                .join(", ");
            if valid.is_empty() {
                bail!(
                    "Prompt name '{}' has invalid matching files:\n{}",
                    name.0,
                    invalid.join("\n")
                );
            }
            bail!(
                "Prompt name '{}' has invalid matching files:\n{}\nValid matches also exist: {}",
                name.0,
                invalid.join("\n"),
                valid
            );
        }
        if matches.is_empty() && !warnings.is_empty() {
            bail!(
                "No prompt named '{}'. Warnings while scanning prompt packs:\n{}",
                name.0,
                warnings.join("\n")
            );
        }
        Ok(matches)
    }

    fn list_records(&self, include_packs: bool) -> Result<PromptScan> {
        let mut prompts = Vec::new();
        let mut warnings = Vec::new();
        prompts.extend(self.scan_dir(PromptNamespace::User, &self.user_dir, &mut warnings)?);
        if let Some(repo_dir) = &self.repo_dir {
            prompts.extend(self.scan_dir(PromptNamespace::Repo, repo_dir, &mut warnings)?);
        }
        if include_packs {
            for pack in self.pack_names(&mut warnings) {
                let prompts_dir = self.packs_dir.join(&pack.0).join("prompts");
                prompts.extend(self.scan_dir(
                    PromptNamespace::Pack(pack),
                    &prompts_dir,
                    &mut warnings,
                )?);
            }
        }
        prompts.sort_by_key(qualified_name);
        Ok(PromptScan { prompts, warnings })
    }

    fn scan_dir(
        &self,
        namespace: PromptNamespace,
        dir: &Path,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<LocatedPrompt>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        collect_markdown_files(dir, dir, &mut paths, warnings)?;
        let mut prompts = Vec::new();
        for (name, path) in paths {
            match self.read_at(namespace.clone(), path.clone(), &name) {
                Ok(prompt) => prompts.push(prompt),
                Err(error) => warnings.push(format!(
                    "Skipping invalid prompt {}: {error:#}",
                    path.display()
                )),
            }
        }
        Ok(prompts)
    }

    fn pack_names(&self, warnings: &mut Vec<String>) -> Vec<PackName> {
        if !self.packs_dir.exists() {
            return Vec::new();
        }
        let mut names = Vec::new();
        let entries = match fs::read_dir(&self.packs_dir) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(format!(
                    "Skipping prompt packs in {}: {error:#}",
                    self.packs_dir.display()
                ));
                return Vec::new();
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(format!(
                        "Skipping unreadable prompt pack entry in {}: {error:#}",
                        self.packs_dir.display()
                    ));
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(format!(
                        "Skipping prompt pack entry {}: {error:#}",
                        entry.path().display()
                    ));
                    continue;
                }
            };
            if file_type.is_dir() {
                let text = entry.file_name().to_string_lossy().to_string();
                match validate_pack_name(&text) {
                    Ok(name) => names.push(name),
                    Err(error) => warnings.push(format!(
                        "Skipping invalid prompt pack directory {}: {error:#}",
                        entry.path().display()
                    )),
                }
            }
        }
        names.sort();
        names
    }

    fn read_at(
        &self,
        namespace: PromptNamespace,
        path: PathBuf,
        name: &PromptName,
    ) -> Result<LocatedPrompt> {
        let record = read_prompt_file(&path, name.clone())?;
        Ok(LocatedPrompt {
            namespace,
            path,
            record,
        })
    }

    fn read_required_at(
        &self,
        namespace: PromptNamespace,
        path: PathBuf,
        name: &PromptName,
        missing_message: String,
    ) -> Result<LocatedPrompt> {
        if !path.exists() {
            bail!("{missing_message}");
        }
        self.read_at(namespace, path.clone(), name)
            .map_err(|error| anyhow!("Invalid prompt file {}: {error:#}", path.display()))
    }

    fn read_optional_at(
        &self,
        namespace: PromptNamespace,
        path: PathBuf,
        name: &PromptName,
    ) -> Result<Option<LocatedPrompt>> {
        if !path.exists() {
            return Ok(None);
        }
        self.read_at(namespace, path.clone(), name)
            .map(Some)
            .map_err(|error| anyhow!("Invalid prompt file {}: {error:#}", path.display()))
    }

    fn prompt_path(&self, dir: &Path, name: &PromptName) -> PathBuf {
        prompt_path(dir, name)
    }

    fn pack_prompt_path(&self, pack: &PackName, name: &PromptName) -> PathBuf {
        prompt_path(&self.packs_dir.join(&pack.0).join("prompts"), name)
    }
}

fn prompt_summary_value(prompt: &LocatedPrompt) -> Value {
    json!({
        "namespace": namespace_name(&prompt.namespace),
        "pack": match &prompt.namespace {
            PromptNamespace::Pack(pack) => Some(pack.0.clone()),
            _ => None,
        },
        "name": prompt.record.name.0,
        "qualified_name": qualified_name(prompt),
        "description": prompt.record.metadata.description,
        "tags": prompt.record.metadata.tags,
        "version": prompt.record.metadata.version,
        "updated_at": prompt.record.metadata.updated_at,
    })
}

fn archive_entry(prompt: &LocatedPrompt) -> PromptArchiveEntry {
    PromptArchiveEntry {
        namespace: namespace_name(&prompt.namespace).to_string(),
        pack: match &prompt.namespace {
            PromptNamespace::Pack(pack) => Some(pack.0.clone()),
            _ => None,
        },
        name: prompt.record.name.0.clone(),
        description: prompt.record.metadata.description.clone(),
        tags: prompt.record.metadata.tags.clone(),
        version: prompt.record.metadata.version,
        updated_at: prompt.record.metadata.updated_at,
        body: prompt.record.body.clone(),
    }
}

fn prompt_matches(prompt: &LocatedPrompt, needle: &str, include_body: bool) -> bool {
    let mut haystack = vec![
        qualified_name(prompt),
        prompt.record.name.0.clone(),
        prompt
            .record
            .metadata
            .description
            .clone()
            .unwrap_or_default(),
        prompt.record.metadata.tags.join(" "),
    ];
    if include_body {
        haystack.push(prompt.record.body.clone());
    }
    haystack.join("\n").to_ascii_lowercase().contains(needle)
}

fn qualified_name(prompt: &LocatedPrompt) -> String {
    match &prompt.namespace {
        PromptNamespace::User => format!("user:{}", prompt.record.name.0),
        PromptNamespace::Repo => format!("repo:{}", prompt.record.name.0),
        PromptNamespace::Pack(pack) => format!("pack:{}/{}", pack.0, prompt.record.name.0),
    }
}

const fn namespace_name(namespace: &PromptNamespace) -> &'static str {
    match namespace {
        PromptNamespace::User => "user",
        PromptNamespace::Repo => "repo",
        PromptNamespace::Pack(_) => "pack",
    }
}

fn parse_selector(raw: &str) -> Result<Selector> {
    if let Some((namespace, rest)) = raw.split_once(':') {
        return match namespace {
            "user" => Ok(Selector::User(validate_prompt_name(rest)?)),
            "repo" => Ok(Selector::Repo(validate_prompt_name(rest)?)),
            "pack" => {
                let (pack, prompt) = rest
                    .split_once('/')
                    .ok_or_else(|| anyhow!("pack: prompts must use pack:<pack>/<prompt>"))?;
                Ok(Selector::Pack {
                    pack: validate_pack_name(pack)?,
                    prompt: validate_prompt_name(prompt)?,
                })
            }
            other => bail!("Unsupported prompt namespace '{other}'"),
        };
    }
    Ok(Selector::Unqualified(validate_prompt_name(raw)?))
}

fn validate_prompt_name(raw: &str) -> Result<PromptName> {
    let name = validate_relative_name(raw, "prompt name")?;
    if name
        .split('/')
        .any(|part| part.to_ascii_lowercase().ends_with(".md"))
    {
        bail!("Prompt names should not include a .md extension: '{raw}'");
    }
    Ok(PromptName(name))
}

fn validate_pack_name(raw: &str) -> Result<PackName> {
    if raw.contains('/') {
        bail!("Invalid prompt pack name '{raw}'");
    }
    validate_relative_name(raw, "prompt pack name").map(PackName)
}

fn validate_relative_name(raw: &str, label: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("{label} cannot be empty");
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            bail!("Invalid {label} '{raw}'");
        }
        if !part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            bail!("Invalid {label} '{raw}'");
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn prompt_path(dir: &Path, name: &PromptName) -> PathBuf {
    let mut path = dir.to_path_buf();
    let mut parts = name.0.split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_some() {
            path.push(part);
        } else {
            path.push(format!("{part}.md"));
        }
    }
    path
}

fn collect_markdown_files(
    base: &Path,
    dir: &Path,
    paths: &mut Vec<(PromptName, PathBuf)>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_markdown_files(base, &path, paths, warnings)?;
        } else if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            let relative = path.strip_prefix(base).unwrap_or(&path);
            let mut name = relative
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            if name.ends_with('/') {
                name.pop();
            }
            match validate_prompt_name(&name) {
                Ok(name) => paths.push((name, path)),
                Err(error) => warnings.push(format!(
                    "Skipping invalid prompt path {}: {error:#}",
                    path.display()
                )),
            }
        }
    }
    Ok(())
}

fn read_prompt_file(path: &Path, fallback_name: PromptName) -> Result<PromptRecord> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let (metadata, body) = parse_prompt_text(&text, &fallback_name)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    if metadata.name != fallback_name.0 {
        bail!(
            "Prompt metadata name '{}' does not match path name '{}'",
            metadata.name,
            fallback_name.0
        );
    }
    Ok(PromptRecord {
        name: validate_prompt_name(&metadata.name)?,
        metadata,
        body,
    })
}

fn parse_prompt_text(text: &str, fallback_name: &PromptName) -> Result<(PromptMetadata, String)> {
    for (prefix, delimiter) in [("---\n", "\n---\n"), ("---\r\n", "\r\n---\r\n")] {
        let Some(rest) = text.strip_prefix(prefix) else {
            continue;
        };
        if let Some(index) = rest.find(delimiter) {
            return parse_frontmatter_and_body(
                &rest[..index],
                rest[index + delimiter.len()..].to_string(),
                fallback_name,
            );
        }
        let eof_delimiter = delimiter.trim_end_matches(['\r', '\n']);
        if let Some(frontmatter) = rest.strip_suffix(eof_delimiter) {
            return parse_frontmatter_and_body(frontmatter, String::new(), fallback_name);
        }
    }
    Ok((
        PromptMetadata {
            name: fallback_name.0.clone(),
            description: None,
            tags: Vec::new(),
            version: 1,
            updated_at: None,
        },
        text.to_string(),
    ))
}

fn parse_frontmatter_and_body(
    frontmatter: &str,
    body: String,
    fallback_name: &PromptName,
) -> Result<(PromptMetadata, String)> {
    let mut metadata: PromptMetadata = serde_yaml_ng::from_str(frontmatter)?;
    if metadata.name.is_empty() {
        metadata.name = fallback_name.0.clone();
    }
    metadata.tags = normalize_tags(metadata.tags);
    Ok((metadata, body))
}

fn write_prompt_file(path: &Path, metadata: &PromptMetadata, body: &str) -> Result<()> {
    let frontmatter = serde_yaml_ng::to_string(metadata)?;
    let text = format!("---\n{frontmatter}---\n{body}");
    write_bytes_atomic(path, text.as_bytes())
}

fn read_body(body: Option<String>, file: Option<&Path>) -> Result<String> {
    match (body, file) {
        (Some(body), None) => Ok(body),
        (None, Some(file)) => fs::read_to_string(file)
            .with_context(|| format!("Failed to read prompt body from {}", file.display())),
        (None, None) => bail!("Prompt body is required; pass BODY or --file"),
        (Some(_), Some(_)) => bail!("Pass either BODY or --file, not both"),
    }
}

fn restore_after_failed_edit(path: &Path, original: Option<&[u8]>) -> Result<()> {
    if let Some(original) = original {
        write_bytes_atomic(path, original).with_context(|| {
            format!(
                "Failed to restore original prompt after edit failure: {}",
                path.display()
            )
        })
    } else if path.exists() {
        fs::remove_file(path).with_context(|| {
            format!(
                "Failed to remove new prompt after edit failure: {}",
                path.display()
            )
        })
    } else {
        Ok(())
    }
}

fn rollback_import(applied: Vec<AppliedImport>) -> Result<()> {
    let mut errors = Vec::new();
    for entry in applied.into_iter().rev() {
        let result = if let Some(original) = entry.original {
            write_bytes_atomic(&entry.path, &original)
        } else if entry.path.exists() {
            fs::remove_file(&entry.path)
                .with_context(|| format!("Failed to remove {}", entry.path.display()))
        } else {
            Ok(())
        };
        if let Err(error) = result {
            errors.push(format!("{}: {error:#}", entry.path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("Failed to roll back prompt import:\n{}", errors.join("\n"))
    }
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tags.into_iter()
        .filter(|tag| seen.insert(tag.clone()))
        .collect()
}

fn reject_name_dir_collision(path: &Path, name: &PromptName) -> Result<()> {
    reject_case_folded_file_collision(path, name)?;
    let stem_path = path.with_extension("");
    if stem_path.is_dir() {
        bail!(
            "Prompt name '{}' conflicts with existing prompt directory {}",
            name.0,
            stem_path.display()
        );
    }
    let mut root = path
        .parent()
        .ok_or_else(|| anyhow!("Prompt path has no parent: {}", path.display()))?
        .to_path_buf();
    let segment_count = name.0.split('/').count();
    for _ in 1..segment_count {
        if !root.pop() {
            return Ok(());
        }
    }
    if segment_count < 2 {
        return Ok(());
    }
    let mut prefix = PathBuf::new();
    for segment in name.0.split('/').take(segment_count - 1) {
        prefix.push(segment);
        let prefix_file = root.join(&prefix).with_extension("md");
        if prefix_file.exists() {
            bail!(
                "Prompt name '{}' conflicts with existing prompt file {}",
                name.0,
                prefix_file.display()
            );
        }
    }
    Ok(())
}

fn reject_case_folded_file_collision(path: &Path, name: &PromptName) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if !parent.exists() {
        return Ok(());
    }
    let Some(target_name) = path
        .file_name()
        .map(|file_name| file_name.to_string_lossy().to_ascii_lowercase())
    else {
        return Ok(());
    };
    for entry in
        fs::read_dir(parent).with_context(|| format!("Failed to read {}", parent.display()))?
    {
        let entry = entry?;
        let candidate = entry.path();
        if candidate == path {
            continue;
        }
        if entry.file_name().to_string_lossy().to_ascii_lowercase() == target_name {
            bail!(
                "Prompt name '{}' conflicts with existing prompt file {}",
                name.0,
                candidate.display()
            );
        }
    }
    Ok(())
}

fn reject_planned_name_dir_collision(
    path: &Path,
    name: &PromptName,
    planned_paths: &BTreeSet<PathBuf>,
    folded_planned_paths: &BTreeSet<String>,
) -> Result<()> {
    let stem_path = path.with_extension("");
    if let Some(existing) = planned_paths
        .iter()
        .find(|planned| path_starts_with_folded(planned, &stem_path))
    {
        bail!(
            "Prompt archive destination {} conflicts with nested prompt {}",
            path.display(),
            existing.display()
        );
    }

    let mut root = path
        .parent()
        .ok_or_else(|| anyhow!("Prompt path has no parent: {}", path.display()))?
        .to_path_buf();
    let segment_count = name.0.split('/').count();
    for _ in 1..segment_count {
        if !root.pop() {
            return Ok(());
        }
    }
    if segment_count < 2 {
        return Ok(());
    }
    let mut prefix = PathBuf::new();
    for segment in name.0.split('/').take(segment_count - 1) {
        prefix.push(segment);
        let prefix_file = root.join(&prefix).with_extension("md");
        if folded_planned_paths.contains(&folded_path_key(&prefix_file)) {
            bail!(
                "Prompt archive destination {} conflicts with parent prompt {}",
                path.display(),
                prefix_file.display()
            );
        }
    }
    Ok(())
}

fn folded_path_key(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

fn path_starts_with_folded(path: &Path, prefix: &Path) -> bool {
    let path_components = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let prefix_components = prefix
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>();
    path_components.starts_with(&prefix_components)
}

pub(crate) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".jig-prompt-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;
    temp.write_all(bytes)
        .with_context(|| format!("Failed to write temporary file for {}", path.display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary file for {}", path.display()))?;
    temp.persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn parse_template_vars(raw_vars: &[String]) -> Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for raw in raw_vars {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| anyhow!("Template variable must use key=value: {raw}"))?;
        validate_var_key(key)?;
        vars.insert(key.to_string(), value.to_string());
    }
    Ok(vars)
}

fn validate_var_key(key: &str) -> Result<()> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || key.as_bytes()[0].is_ascii_digit()
    {
        bail!("Invalid template variable key '{key}'");
    }
    Ok(())
}

fn render_prompt_body(body: &str, vars: &BTreeMap<String, String>, raw: bool) -> Result<String> {
    if raw {
        return Ok(body.to_string());
    }
    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    let template = env
        .template_from_str(body)
        .context("Failed to parse prompt template")?;
    template
        .render(vars)
        .context("Failed to render prompt template")
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    if let Ok(command) = std::env::var(CLIPBOARD_COMMAND_ENV) {
        // This override is intentionally shell-evaluated user configuration.
        // The prompt body is delivered through stdin, not interpolated.
        return pipe_to_command("sh", &["-c", &command], text);
    }

    #[cfg(target_os = "macos")]
    {
        pipe_to_command("pbcopy", &[], text)
    }

    #[cfg(not(target_os = "macos"))]
    {
        for (program, args) in [
            ("wl-copy", Vec::<&str>::new()),
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ] {
            if command_available(program) {
                return pipe_to_command(program, &args, text);
            }
        }

        bail!(
            "No supported clipboard command found; install wl-copy, xclip, or xsel, or set JIG_PROMPT_CLIPBOARD_COMMAND"
        )
    }
}

fn pipe_to_command(program: &str, args: &[&str], text: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start clipboard command `{program}`"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Clipboard command stdin was unavailable"))?;
        stdin.write_all(text.as_bytes())?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("Failed to wait for clipboard command `{program}`"))?;
    if !output.status.success() {
        bail!(
            "Clipboard command `{program}` exited with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn command_available(program: &str) -> bool {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return is_executable_file(program_path);
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| is_executable_file(&dir.join(program)))
}

#[cfg(not(target_os = "macos"))]
fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn open_editor(path: &Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string());
    let status = editor_command(&editor, path)
        .status()
        .with_context(|| format!("Failed to start editor `{editor}`"))?;
    if !status.success() {
        bail!(
            "Editor `{editor}` exited with {}",
            format_exit_status(&status)
        );
    }
    Ok(())
}

#[cfg(unix)]
fn editor_command(editor: &str, path: &Path) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        // EDITOR/VISUAL conventionally allow arguments, e.g. "code -w".
        // The value is user-controlled and intentionally evaluated by a shell.
        .arg(format!("exec {editor} \"$1\""))
        .arg("jig-prompt-editor")
        .arg(path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

#[cfg(not(unix))]
fn editor_command(editor: &str, path: &Path) -> Command {
    let mut command = Command::new(editor);
    command.arg(path);
    command
}

fn now_timestamp() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn format_exit_status(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("status {code}"),
        None => "signal termination".to_string(),
    }
}

const fn default_prompt_version() -> u32 {
    1
}

fn deserialize_optional_timestamp<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Timestamp {
        Number(i64),
        String(String),
    }

    let value = Option::<Timestamp>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Timestamp::Number(value)) => Ok(Some(value)),
        Some(Timestamp::String(value)) => value
            .parse::<i64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[cfg(test)]
mod tests;
