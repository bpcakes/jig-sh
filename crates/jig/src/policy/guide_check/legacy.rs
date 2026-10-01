// Preserve the guide policy recorded by contract epochs 2 through 7. Upgrading
// the runtime alone must not broaden discovery or introduce link failures.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::policy::agent_map::relative_string;

pub(super) fn check(ctx: &RepoContext) -> Result<Value> {
    // Backend guides intentionally use this exact repo-wide heading contract so
    // agents can scan every package or crate guide without learning local synonyms.
    let required = [
        "## Purpose",
        "## Key entrypoints",
        "## Edit here for X",
        "## Invariants",
        "## Common commands",
    ];
    let mut missing_sections = Vec::new();
    let mut missing_entry_ref = Vec::new();
    let guides = backend_guides(ctx)?;
    for (guide, languages) in &guides {
        let rel = relative_string(ctx.root(), guide)?;
        let text = fs::read_to_string(guide)?;
        for section in required {
            if !text.lines().any(|line| line.trim_end() == section) {
                missing_sections.push(format!("{rel}: missing section '{section}'"));
            }
        }
        for language in languages {
            let (has_entry_ref, expected) = match language {
                GuideLanguage::Go => (
                    has_backticked_go_entrypoint(&text),
                    "a backticked .go entrypoint",
                ),
                GuideLanguage::Rust => (
                    text.contains("`src/lib.rs`") || text.contains("`src/main.rs`"),
                    "src/lib.rs or src/main.rs entrypoint reference",
                ),
            };
            if !has_entry_ref {
                missing_entry_ref.push(format!("{rel}: missing {expected}"));
            }
        }
    }
    Ok(json!({
        "ok": missing_sections.is_empty() && missing_entry_ref.is_empty(),
        "guide_count": guides.len(),
        "missing_guides": [],
        "missing_guides_note": "placeholder backend-level AGENTS.md files are no longer required; existing guides are validated when present",
        "missing_sections": missing_sections,
        "missing_entry_ref": missing_entry_ref,
    }))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum GuideLanguage {
    Go,
    Rust,
}

fn backend_guides(ctx: &RepoContext) -> Result<BTreeMap<PathBuf, BTreeSet<GuideLanguage>>> {
    let mut guides = BTreeMap::new();
    if ctx.contract_version() < 6 {
        if ctx.is_go_backend() {
            for root in ["cmd", "internal"] {
                add_child_guides(&ctx.root().join(root), GuideLanguage::Go, &mut guides)?;
            }
        } else {
            for root in ctx.rust_crate_roots() {
                add_child_guides(&ctx.root().join(root), GuideLanguage::Rust, &mut guides)?;
            }
        }
        return Ok(guides);
    }

    for component in ctx.component_specs() {
        let component_root = ctx.component_root_path(component)?;
        if component.adapters.iter().any(|adapter| adapter == "go") {
            if component_root != ctx.root() {
                add_guide_if_present(&component_root, GuideLanguage::Go, &mut guides);
            }
            for root in ["cmd", "internal"] {
                add_child_guides(&component_root.join(root), GuideLanguage::Go, &mut guides)?;
            }
        }
        if component.adapters.iter().any(|adapter| adapter == "rust")
            && component_root != ctx.root()
        {
            add_guide_if_present(&component_root, GuideLanguage::Rust, &mut guides);
        }
    }
    for root in ctx
        .rust_crate_roots()
        .iter()
        .filter(|root| root.as_str() != ".")
    {
        add_fallback_rust_guides(&ctx.root().join(root), &mut guides)?;
    }
    Ok(guides)
}

fn add_fallback_rust_guides(
    backend_root: &Path,
    guides: &mut BTreeMap<PathBuf, BTreeSet<GuideLanguage>>,
) -> Result<()> {
    if !backend_root.is_dir() {
        return Ok(());
    }
    for entry in sorted_dirs(backend_root)? {
        let guide = entry.join("AGENTS.md");
        if guide.exists() {
            guides
                .entry(guide)
                .or_insert_with(|| BTreeSet::from([GuideLanguage::Rust]));
        }
    }
    Ok(())
}

fn add_child_guides(
    backend_root: &Path,
    language: GuideLanguage,
    guides: &mut BTreeMap<PathBuf, BTreeSet<GuideLanguage>>,
) -> Result<()> {
    if !backend_root.is_dir() {
        return Ok(());
    }
    // Backend roots contain first-level packages or crates; deeper AGENTS.md
    // files are covered by agent-map link validation rather than guide policy.
    for entry in sorted_dirs(backend_root)? {
        let guide = entry.join("AGENTS.md");
        if guide.exists() {
            guides.entry(guide).or_default().insert(language);
        }
    }
    Ok(())
}

fn add_guide_if_present(
    component_root: &Path,
    language: GuideLanguage,
    guides: &mut BTreeMap<PathBuf, BTreeSet<GuideLanguage>>,
) {
    let guide = component_root.join("AGENTS.md");
    if guide.exists() {
        guides.entry(guide).or_default().insert(language);
    }
}

fn has_backticked_go_entrypoint(text: &str) -> bool {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .any(|reference| {
            !reference.is_empty()
                && !reference.chars().any(char::is_whitespace)
                && reference.ends_with(".go")
        })
}

fn sorted_dirs(path: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}
