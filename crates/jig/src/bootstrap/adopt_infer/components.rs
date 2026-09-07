use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, bail};
use clap::Args;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::bootstrap::FrontendApp;
use crate::bootstrap::crate_classification::non_production_crate_reason;
use crate::bootstrap::repository_model::frontend_component_id;
use crate::repository_path::{normalize_portable_repo_path, validate_repository_directory_path};

use super::metadata::Confidence;
use super::scan::{RepoScan, read_json_for_inference, read_limited_text, read_toml_for_inference};

mod authority;
mod workspace;

#[derive(Args, Clone, Debug, Default)]
pub struct ComponentSelectionOpts {
    #[arg(
        long = "include-component",
        value_name = "ROOT",
        help = "Include candidates at an exact repository-relative root; may be repeated"
    )]
    pub include: Vec<String>,
    #[arg(
        long = "exclude-component",
        value_name = "ROOT",
        help = "Exclude candidates at an exact repository-relative root; no globs; may be repeated"
    )]
    pub exclude: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::bootstrap) enum Disposition {
    Included,
    Excluded,
    ReviewRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::bootstrap) enum Ecosystem {
    Rust,
    Node,
    Go,
    Authored,
}

#[derive(Clone, Debug)]
pub(in crate::bootstrap) struct ComponentCandidate {
    pub root: String,
    pub proposed_id: String,
    pub ecosystem: Ecosystem,
    pub disposition: Disposition,
    pub reason: String,
    pub evidence: Vec<String>,
    confidence: Confidence,
    pub frontend_name: Option<String>,
    pub workspace: bool,
    pub valid_manifest: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ComponentCandidates {
    pub(in crate::bootstrap) preserved: bool,
    pub(in crate::bootstrap) refresh: bool,
    pub(in crate::bootstrap) candidates: Vec<ComponentCandidate>,
}

impl ComponentCandidates {
    pub(super) fn discover(
        root: &Path,
        scan: &RepoScan,
        frontend_apps: &[FrontendApp],
        frontend_workspace_roots: &[String],
        warnings: &mut Vec<String>,
    ) -> Self {
        let rust_workspace = workspace::CargoWorkspace::read(root, warnings);
        let mut candidates = Vec::new();
        for (filename, ecosystem) in [
            ("Cargo.toml", Ecosystem::Rust),
            ("package.json", Ecosystem::Node),
            ("go.mod", Ecosystem::Go),
        ] {
            for path in scan.named_files(filename) {
                let directory = path.parent().unwrap_or(root);
                let relative = directory.strip_prefix(root).unwrap_or(directory);
                let relative = if relative.as_os_str().is_empty() {
                    ".".to_owned()
                } else {
                    relative.to_string_lossy().replace('\\', "/")
                };
                if validate_directory(root, &relative).is_err() {
                    warnings.push(format!(
                        "component candidate {relative} traverses an unsafe directory; ignored"
                    ));
                    continue;
                }
                let mut candidate = ComponentCandidate {
                    proposed_id: proposed_id(&relative, ecosystem),
                    root: relative,
                    ecosystem,
                    disposition: Disposition::ReviewRequired,
                    reason: "manifest alone does not establish component ownership".into(),
                    evidence: vec![
                        path.strip_prefix(root)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .replace('\\', "/"),
                    ],
                    confidence: Confidence::Low,
                    frontend_name: None,
                    workspace: false,
                    valid_manifest: false,
                };
                let mut package_name = None;
                match ecosystem {
                    Ecosystem::Rust => {
                        if let Some(parsed) = read_toml_for_inference(path, warnings) {
                            candidate.workspace = parsed
                                .get("workspace")
                                .and_then(toml::Value::as_table)
                                .is_some();
                            candidate.valid_manifest = candidate.workspace
                                || parsed
                                    .get("package")
                                    .and_then(|p| p.get("name"))
                                    .and_then(toml::Value::as_str)
                                    .is_some_and(|name| !name.is_empty());
                            package_name = parsed
                                .get("package")
                                .and_then(|p| p.get("name"))
                                .and_then(toml::Value::as_str)
                                .map(str::to_owned);
                            if candidate.root == "." && candidate.valid_manifest {
                                candidate.include("root Cargo manifest", Confidence::High);
                                if candidate.workspace && parsed.get("package").is_none() {
                                    candidate.proposed_id = "workspace".into();
                                }
                            } else if rust_workspace.excludes(&candidate.root) {
                                candidate.disposition = Disposition::Excluded;
                                candidate.confidence = Confidence::High;
                                candidate.reason = "excluded by Cargo workspace declaration".into();
                                candidate
                                    .evidence
                                    .push("Cargo.toml [workspace].exclude".into());
                            } else if candidate.valid_manifest
                                && rust_workspace.includes(&candidate.root)
                            {
                                candidate
                                    .include("declared Cargo workspace member", Confidence::High);
                                candidate
                                    .evidence
                                    .push("Cargo.toml [workspace].members".into());
                            }
                        }
                    }
                    Ecosystem::Node => {
                        if let Some(parsed) = read_json_for_inference(path, warnings) {
                            candidate.valid_manifest = parsed.is_object();
                            package_name = parsed
                                .get("name")
                                .and_then(Value::as_str)
                                .map(str::to_owned);
                            candidate.workspace = candidate.root == "."
                                && (parsed.get("workspaces").is_some_and(|value| {
                                    value.is_array()
                                        || value.get("packages").is_some_and(Value::is_array)
                                }) || root.join("pnpm-workspace.yaml").is_file());
                            if candidate.workspace {
                                candidate.include(
                                    "root package workspace declaration",
                                    Confidence::High,
                                );
                            } else if frontend_workspace_roots.contains(&candidate.root) {
                                candidate
                                    .include("declared package workspace member", Confidence::High);
                                candidate
                                    .evidence
                                    .push("package workspace membership".into());
                            }
                            if let Some(app) =
                                frontend_apps.iter().find(|app| app.dir == candidate.root)
                            {
                                candidate.frontend_name = Some(app.name.clone());
                                if let Ok(id) = frontend_component_id(&app.name) {
                                    candidate.proposed_id = id.to_string();
                                    candidate.include(
                                        "recognized frontend application scripts",
                                        Confidence::High,
                                    );
                                }
                            }
                        }
                    }
                    Ecosystem::Authored => {
                        unreachable!("discovery only scans known manifest kinds")
                    }
                    Ecosystem::Go => {
                        if let Ok(text) = read_limited_text(path) {
                            candidate.valid_manifest = text.lines().any(|line| {
                                line.trim()
                                    .strip_prefix("module ")
                                    .is_some_and(|module| !module.trim().is_empty())
                            });
                            if candidate.root == "." && candidate.valid_manifest {
                                candidate.include("root Go module declaration", Confidence::High);
                            }
                        }
                    }
                }
                if candidate.disposition != Disposition::Excluded
                    && let Some(reason) = non_production_crate_reason(
                        Path::new(&candidate.root),
                        package_name.as_deref(),
                    )
                {
                    candidate.disposition = Disposition::ReviewRequired;
                    candidate.reason = reason;
                }
                if !candidate.valid_manifest {
                    candidate.reason =
                        "manifest could not be interpreted; review before including".into();
                }
                candidates.push(candidate);
            }
        }
        let mut result = Self {
            candidates,
            preserved: false,
            refresh: false,
        };
        result.stabilize_ids();
        result
    }

    pub(in crate::bootstrap) fn stabilize_ids(&mut self) {
        self.candidates.sort_by(|left, right| {
            left.root
                .cmp(&right.root)
                .then(left.proposed_id.cmp(&right.proposed_id))
        });
        let mut ids = BTreeMap::new();
        for candidate in &self.candidates {
            *ids.entry(candidate.proposed_id.clone()).or_insert(0) += 1;
        }
        for candidate in &mut self.candidates {
            if ids[&candidate.proposed_id] > 1 || candidate.proposed_id == "repo" {
                let identity = format!("{}:{:?}", candidate.root, candidate.ecosystem);
                candidate.proposed_id = suffixed_id(&candidate.proposed_id, &identity);
            }
        }
    }

    pub(in crate::bootstrap) fn select(
        &mut self,
        root: &Path,
        opts: &ComponentSelectionOpts,
    ) -> Result<()> {
        let included = self.normalized_roots(root, &opts.include)?;
        let excluded = self.normalized_roots(root, &opts.exclude)?;
        if let Some(conflict) = included.intersection(&excluded).next() {
            bail!("component root '{conflict}' cannot be both included and excluded");
        }
        for candidate in &mut self.candidates {
            if excluded.contains(&candidate.root) {
                candidate.disposition = Disposition::Excluded;
                candidate.reason = "explicit --exclude-component".into();
            } else if included.contains(&candidate.root) {
                candidate.disposition = Disposition::Included;
                candidate.reason = "explicit --include-component".into();
            }
        }
        Ok(())
    }

    fn normalized_roots(&self, root: &Path, values: &[String]) -> Result<BTreeSet<String>> {
        values.iter().map(|value| {
            if value.contains(['*', '?', '[', ']', '{', '}']) {
                bail!("component selections require exact roots, not globs: {value}");
            }
            let normalized = normalize_portable_repo_path(value, "component root")?;
            if !self.candidates.iter().any(|candidate| candidate.root == normalized) {
                bail!("unknown component root '{normalized}'; choose a root from the adoption preview");
            }
            validate_directory(root, &normalized)?;
            Ok(normalized)
        }).collect()
    }

    pub(in crate::bootstrap) fn report(&self) -> Value {
        json!(
            self.candidates
                .iter()
                .map(|candidate| json!({
                    "root": candidate.root, "proposed_id": candidate.proposed_id,
                    "ecosystem": candidate.ecosystem, "disposition": candidate.disposition,
                    "confidence": candidate.confidence.as_str(), "evidence": candidate.evidence,
                    "reason": candidate.reason,
                }))
                .collect::<Vec<_>>()
        )
    }
}

impl ComponentCandidate {
    pub(in crate::bootstrap) fn review_line(&self) -> String {
        let disposition = serde_json::to_value(self.disposition).expect("disposition is a string");
        format!(
            "component {} at {}: {} ({} confidence; {}; evidence: {})",
            self.proposed_id,
            self.root,
            disposition.as_str().unwrap(),
            self.confidence.as_str(),
            self.reason,
            self.evidence.join(", ")
        )
    }

    fn include(&mut self, reason: &str, confidence: Confidence) {
        self.disposition = Disposition::Included;
        self.reason = reason.into();
        self.confidence = confidence;
    }
}

fn proposed_id(root: &str, ecosystem: Ecosystem) -> String {
    if root == "." {
        return match ecosystem {
            Ecosystem::Rust => "api",
            Ecosystem::Node => "node",
            Ecosystem::Go => "go",
            Ecosystem::Authored => "component",
        }
        .into();
    }
    let id = root
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let id = id.trim_matches('-');
    let id = if id.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        format!("component-{id}")
    } else {
        id.to_owned()
    };
    let id = id.as_str();
    if id.is_empty() {
        return suffixed_id("component", root);
    }
    if id.len() > 64 {
        suffixed_id(id, root)
    } else {
        id.to_owned()
    }
}

fn suffixed_id(prefix: &str, identity: &str) -> String {
    let mut end = prefix.len().min(51);
    while !prefix.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &prefix[..end];
    let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
    format!("{}-{}", prefix.trim_end_matches('-'), &digest[..12])
}

fn validate_directory(root: &Path, relative: &str) -> Result<()> {
    if relative == "." {
        if !std::fs::symlink_metadata(root)?.file_type().is_dir() {
            bail!("component root must be a real directory");
        }
    } else {
        validate_repository_directory_path(root, Path::new(relative))?;
    }
    if !root.join(relative).is_dir() {
        bail!("component root '{relative}' is not a directory");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
