use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;

use crate::agent_guides::references::{
    Destination, GuideFiles, has_uri_scheme, is_missing, markdown_references, resolve_reference,
};
use crate::context::RepoContext;
use crate::repository_path::normalize_portable_repo_path;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Serialize)]
struct Diagnostic {
    severity: Severity,
    code: &'static str,
    guide: String,
    line: Option<usize>,
    reference: Option<String>,
    component: Option<String>,
    message: String,
}

#[derive(Serialize)]
struct GuideReport {
    ok: bool,
    guide_count: usize,
    missing_guides: Vec<String>,
    missing_guides_note: &'static str,
    missing_sections: Vec<String>,
    missing_entry_ref: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

mod legacy;

pub(super) fn check(ctx: &RepoContext) -> Result<Value> {
    if ctx.contract_version() < 8 {
        return legacy::check(ctx);
    }
    let files = GuideFiles::new(ctx.root())?;
    let mut guides: BTreeSet<String> = super::agent_map::list_guides(ctx.root())?
        .into_iter()
        .collect();
    let mut owners = BTreeMap::<String, Vec<String>>::new();
    let mut diagnostics = Vec::new();
    for component in ctx.component_specs() {
        let Some(guidance) = &component.guidance else {
            continue;
        };
        match owner_path(guidance) {
            Ok(path) => {
                guides.insert(path.clone());
                owners
                    .entry(path)
                    .or_default()
                    .push(component.id.to_string());
            }
            Err(error) => diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "owner_guide_invalid",
                guide: ".jig.toml".into(),
                line: None,
                reference: Some(guidance.clone()),
                component: Some(component.id.to_string()),
                message: error.to_string(),
            }),
        }
    }
    let mut guide_count = guides.len();
    for guide in guides {
        let text = match files.read(&guide) {
            Ok(text) => text,
            Err(error) => {
                if is_missing(&error) && !owners.contains_key(&guide) {
                    // Git can still list an optional guide deleted from the worktree.
                    guide_count -= 1;
                    continue;
                }
                if let Some(components) = owners.get(&guide) {
                    for component in components {
                        diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            code: if is_missing(&error) {
                                "owner_guide_missing"
                            } else {
                                "owner_guide_unreadable"
                            },
                            guide: ".jig.toml".into(),
                            line: None,
                            reference: Some(guide.clone()),
                            component: Some(component.clone()),
                            message: format!("cannot read the declared owner guide: {error}"),
                        });
                    }
                } else {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: "guide_unreadable",
                        guide: guide.clone(),
                        line: None,
                        reference: None,
                        component: None,
                        message: format!("cannot read guide: {error}"),
                    });
                }
                continue;
            }
        };
        check_references(&files, &guide, &text, &mut diagnostics);
        let suggested = [
            "## Purpose",
            "## Key entrypoints",
            "## Edit here for X",
            "## Invariants",
            "## Common commands",
        ];
        // These conventions describe nested AGENTS.md guides, not the managed
        // root policy or arbitrary authored component documentation.
        if guide.ends_with("/AGENTS.md")
            && !suggested
                .iter()
                .all(|heading| text.lines().any(|line| line.trim_end() == *heading))
        {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning, code: "guide_structure", guide,
                line: None, reference: None, component: None,
                message: "The five suggested guide headings are optional; keep ownership, entrypoints and invariants clear in the style that fits this area.".into(),
            });
        }
    }
    let report = GuideReport {
        ok: !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error),
        guide_count,
        missing_guides: Vec::new(),
        missing_guides_note: "placeholder backend-level AGENTS.md files are no longer required; existing guides and declared component guidance are validated",
        missing_sections: Vec::new(),
        missing_entry_ref: Vec::new(),
        diagnostics,
    };
    Ok(serde_json::to_value(report)?)
}

fn owner_path(guidance: &str) -> Result<String> {
    if has_uri_scheme(guidance) {
        bail!("component guidance must be a literal repository-relative file path, not a URI");
    }
    normalize_portable_repo_path(guidance, "component guidance")
}

fn check_references(
    files: &GuideFiles,
    guide: &str,
    text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for reference in markdown_references(text) {
        if let Some(problem) = reference.problem {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "reference_invalid",
                guide: guide.into(),
                line: Some(reference.line),
                reference: Some(reference.target),
                component: None,
                message: problem.into(),
            });
            continue;
        }
        let (severity, code, message) = match resolve_reference(Path::new(guide), &reference.target)
        {
            Ok(Destination::Fragment) => continue,
            Ok(Destination::External) => (
                Severity::Info,
                "external_reference",
                "External reference is unverified; guide checks do not access the network.".into(),
            ),
            Ok(Destination::Local(path)) => match files.check_target(&path, false) {
                Ok(()) => continue,
                Err(error) => (
                    Severity::Error,
                    if is_missing(&error) {
                        "reference_missing"
                    } else {
                        "reference_unsafe"
                    },
                    format!("{path}: {error}"),
                ),
            },
            Err(error) => (Severity::Error, "reference_invalid", error.to_string()),
        };
        diagnostics.push(Diagnostic {
            severity,
            code,
            guide: guide.into(),
            line: Some(reference.line),
            reference: Some(reference.target),
            component: None,
            message,
        });
    }
}

#[cfg(test)]
mod tests;
