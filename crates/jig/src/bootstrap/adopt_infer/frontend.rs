use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub(super) use jig_typescript::dev_script::script_looks_like_vite;
pub(super) use jig_typescript::workspace::segment_matches;
use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

use crate::frontend_metadata::resolve_frontend_metadata;

use super::super::FrontendApp;
use super::repo::safe_name;
use super::scan::{
    MAX_SCAN_DEPTH, entry_is_dir, push_scan_warning, read_dir_entries, read_json_for_inference,
    read_yaml_for_inference, relative_path_string, should_skip_dir, yaml_mapping_get,
};

// Matches the default frontend coverage threshold rendered by the bootstrap template.
const FRONTEND_COVERAGE_THRESHOLD: u32 = 80;
const REQUIRED_FRONTEND_SCRIPTS: &[&str] = &["lint", "typecheck", "build:bundle", "test:coverage"];

#[derive(Clone, Debug, Default)]
pub(super) struct FrontendAppsInference {
    pub(super) apps: Vec<FrontendApp>,
    pub(super) workspace_roots: Vec<String>,
    pub(super) profiles: Vec<FrontendAppProfile>,
    pub(super) sources: Vec<String>,
    pub(super) warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct FrontendAppProfile {
    pub(super) name: String,
    pub(super) dir: String,
    pub(super) kind: String,
    pub(super) preferred_dev_port: Option<u16>,
    pub(super) sources: Vec<String>,
}

pub(super) fn infer_frontend_apps_with_metadata(
    root: &Path,
    inferred_repo_name: Option<&str>,
    warnings: &mut Vec<String>,
) -> FrontendAppsInference {
    let workspace_declared = workspace_declaration_present(root, warnings);
    let workspace_candidates = workspace_package_dirs(root, warnings);
    let mut workspace_roots = workspace_candidates
        .iter()
        .map(|dir| relative_path_string(dir.strip_prefix(root).unwrap_or(dir)))
        .map(|dir| if dir.is_empty() { ".".into() } else { dir })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    if workspace_candidates.is_empty() && !workspace_declared {
        if root.join("package.json").is_file() {
            candidates.push(root.to_path_buf());
        }
        for glob in ["apps/*", "packages/*"] {
            expand_optional_workspace_glob(root, glob, &mut candidates, warnings);
        }
        for dir in ["web", "frontend"] {
            let candidate = root.join(dir);
            if candidate.join("package.json").is_file() {
                candidates.push(candidate);
            }
        }
    } else {
        candidates.extend(workspace_candidates);
    }
    candidates.sort();
    candidates.dedup();

    let mut apps = Vec::new();
    let mut profiles = Vec::new();
    let mut sources = Vec::new();
    let mut metadata_warnings = Vec::new();
    let mut names = BTreeSet::new();
    for dir in candidates {
        let package_path = dir.join("package.json");
        let Some(package) = read_json_for_inference(&package_path, warnings) else {
            continue;
        };
        let Some(scripts) = package.get("scripts").and_then(JsonValue::as_object) else {
            continue;
        };
        let Some(dev_script) = non_empty_script(scripts, "dev") else {
            continue;
        };
        let missing_scripts = REQUIRED_FRONTEND_SCRIPTS
            .iter()
            .copied()
            .filter(|script| non_empty_script(scripts, script).is_none())
            .collect::<Vec<_>>();
        if !missing_scripts.is_empty() {
            push_scan_warning(
                warnings,
                &package_path,
                &format!(
                    "frontend package has a dev script but is missing required CI scripts: {}",
                    missing_scripts.join(", ")
                ),
            );
            continue;
        }
        let relative = relative_path_string(dir.strip_prefix(root).unwrap_or(&dir));
        let dir_value = if relative.is_empty() {
            ".".into()
        } else {
            relative
        };
        let base_name = if dir_value == "." {
            inferred_repo_name.unwrap_or("web").to_string()
        } else {
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("web")
                .to_string()
        };
        let name = unique_frontend_name(safe_frontend_name(&base_name), &mut names);
        let (kind, explicit_role) = if script_looks_like_vite(dev_script) {
            ("vite", None)
        } else if script_looks_like_astro(dev_script) {
            ("env-port", Some("astro"))
        } else {
            ("env-port", Some("spa"))
        };
        let role = resolve_frontend_metadata(&name, Some(kind), explicit_role, None).role;
        let source = format!(
            "{} scripts: dev, {}",
            relative_path_string(package_path.strip_prefix(root).unwrap_or(&package_path)),
            REQUIRED_FRONTEND_SCRIPTS.join(", ")
        );
        let port_source = format!(
            "{} scripts.dev",
            relative_path_string(package_path.strip_prefix(root).unwrap_or(&package_path))
        );
        let mut profile_warnings = Vec::new();
        let preferred_dev_port =
            infer_dev_port(dev_script, &package_path, warnings, &mut profile_warnings);
        metadata_warnings.extend(profile_warnings);
        let mut profile_sources = vec![source.clone()];
        if preferred_dev_port.is_some() {
            profile_sources.push(port_source);
        }
        apps.push(FrontendApp {
            name: name.clone(),
            dir: dir_value.clone(),
            coverage_threshold: FRONTEND_COVERAGE_THRESHOLD,
            kind: kind.into(),
            role: role.into(),
        });
        profiles.push(FrontendAppProfile {
            name,
            dir: dir_value,
            kind: kind.into(),
            preferred_dev_port,
            sources: profile_sources,
        });
        sources.push(source);
    }
    apps.sort_by(|left, right| left.dir.cmp(&right.dir));
    workspace_roots.retain(|root| !apps.iter().any(|app| app.dir == *root));
    workspace_roots.sort();
    workspace_roots.dedup();
    profiles.sort_by(|left, right| left.dir.cmp(&right.dir));
    sources.sort();
    FrontendAppsInference {
        apps,
        workspace_roots,
        profiles,
        sources,
        warnings: metadata_warnings,
    }
}

fn workspace_package_dirs(root: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let mut globs = Vec::new();
    globs.extend(package_json_workspace_globs(root, warnings));
    globs.extend(pnpm_workspace_globs(root, warnings));
    let mut dirs = Vec::new();
    let mut exclusions = Vec::new();
    for glob in globs.iter().filter(|glob| !glob.starts_with('!')) {
        if glob_escapes_root(glob) {
            continue;
        }
        expand_workspace_glob(root, glob, &mut dirs, 0, warnings);
    }
    for glob in globs.iter().filter_map(|glob| glob.strip_prefix('!')) {
        if glob_escapes_root(glob) {
            continue;
        }
        expand_workspace_glob(root, glob, &mut exclusions, 0, warnings);
    }
    exclusions.sort();
    exclusions.dedup();
    dirs.retain(|dir| exclusions.binary_search(dir).is_err());
    dirs
}

fn workspace_declaration_present(root: &Path, warnings: &mut Vec<String>) -> bool {
    package_json_has_workspaces(root, warnings) || root.join("pnpm-workspace.yaml").is_file()
}

fn package_json_has_workspaces(root: &Path, warnings: &mut Vec<String>) -> bool {
    let package_path = root.join("package.json");
    if !package_path.is_file() {
        return false;
    }
    let Some(package) = read_json_for_inference(&package_path, warnings) else {
        return false;
    };
    package.get("workspaces").is_some()
}

fn package_json_workspace_globs(root: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    let package_path = root.join("package.json");
    if !package_path.is_file() {
        return Vec::new();
    }
    let Some(package) = read_json_for_inference(&package_path, warnings) else {
        return Vec::new();
    };
    let Some(workspaces) = package.get("workspaces") else {
        return Vec::new();
    };
    if let Some(items) = workspaces.as_array() {
        return items
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::to_string)
            .collect();
    }
    workspaces
        .get("packages")
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn pnpm_workspace_globs(root: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    let path = root.join("pnpm-workspace.yaml");
    if !path.is_file() {
        return Vec::new();
    }
    let Some(yaml) = read_yaml_for_inference(&path, warnings) else {
        return Vec::new();
    };
    let Some(packages) = yaml_mapping_get(&yaml, "packages") else {
        push_scan_warning(
            warnings,
            &path,
            "pnpm-workspace.yaml did not declare supported packages globs",
        );
        return Vec::new();
    };
    let mut globs = Vec::new();
    match packages {
        YamlValue::Sequence(items) => {
            for item in items {
                if let Some(glob) = yaml_workspace_glob(item) {
                    globs.push(glob);
                } else {
                    push_scan_warning(
                        warnings,
                        &path,
                        "pnpm-workspace.yaml contains unsupported non-string packages entries",
                    );
                }
            }
        }
        value => {
            if let Some(glob) = yaml_workspace_glob(value) {
                globs.push(glob);
            } else {
                push_scan_warning(
                    warnings,
                    &path,
                    "pnpm-workspace.yaml packages must be a string array",
                );
            }
        }
    }
    if globs.is_empty() {
        push_scan_warning(
            warnings,
            &path,
            "pnpm-workspace.yaml did not declare supported packages globs",
        );
    }
    globs
}

fn yaml_workspace_glob(value: &YamlValue) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            // Unquoted YAML entries like `!packages/private` parse as a tag
            // with a null value. Recover that shape as the pnpm exclusion glob
            // the user wrote.
            if let YamlValue::Tagged(tagged) = value
                && matches!(tagged.value, YamlValue::Null)
            {
                let tag = tagged.tag.to_string();
                return tag.starts_with('!').then_some(tag);
            }
            None
        })
}

fn expand_workspace_glob(
    root: &Path,
    glob: &str,
    out: &mut Vec<PathBuf>,
    depth: usize,
    warnings: &mut Vec<String>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let segments = glob.split('/').collect::<Vec<_>>();
    expand_segments(root, root, &segments, out, depth, warnings);
}

fn expand_optional_workspace_glob(
    root: &Path,
    glob: &str,
    out: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) {
    let first_segment = glob.split('/').next().unwrap_or_default();
    if first_segment.is_empty() || root.join(first_segment).is_dir() {
        expand_workspace_glob(root, glob, out, 0, warnings);
    }
}

fn expand_segments(
    root: &Path,
    base: &Path,
    segments: &[&str],
    out: &mut Vec<PathBuf>,
    depth: usize,
    warnings: &mut Vec<String>,
) {
    if depth > MAX_SCAN_DEPTH || (depth > 0 && should_skip_dir(base)) {
        return;
    }
    if segments.is_empty() {
        if base.join("package.json").is_file() {
            out.push(base.to_path_buf());
        }
        return;
    }
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    if *first == "**" {
        expand_segments(root, base, rest, out, depth + 1, warnings);
        for entry in read_dir_entries(base, warnings) {
            if entry_is_dir(&entry, warnings) {
                expand_segments(root, &entry.path(), segments, out, depth + 1, warnings);
            }
        }
        return;
    }
    if first.contains('*') {
        for entry in read_dir_entries(base, warnings) {
            if entry_is_dir(&entry, warnings)
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| segment_matches(first, name))
            {
                expand_segments(root, &entry.path(), rest, out, depth + 1, warnings);
            }
        }
    } else {
        let next = base.join(first);
        // `root` and `next` are lexical paths under the already resolved
        // destination, and directory symlinks are not followed during expansion.
        if next.starts_with(root) {
            expand_segments(root, &next, rest, out, depth + 1, warnings);
        }
    }
}

fn non_empty_script<'a>(
    scripts: &'a serde_json::Map<String, JsonValue>,
    name: &str,
) -> Option<&'a str> {
    scripts
        .get(name)
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn script_looks_like_astro(value: &str) -> bool {
    let tokens = value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')'))
        .map(|token| token.trim_matches(['"', '\'']))
        .filter(|token| !token.is_empty())
        .map(|token| token.rsplit('/').next().unwrap_or(token))
        .collect::<Vec<_>>();
    let Some(astro_index) = tokens
        .iter()
        .position(|token| *token == "astro" || token.starts_with("astro@"))
    else {
        return false;
    };
    !tokens[astro_index + 1..]
        .iter()
        .any(|token| matches!(*token, "build" | "check" | "preview" | "sync"))
}

fn infer_dev_port(
    script: &str,
    source: &Path,
    warnings: &mut Vec<String>,
    metadata_warnings: &mut Vec<String>,
) -> Option<u16> {
    // Supported conventions are Vite-style flags and common PORT env assignments.
    let tokens = script_tokens(script);
    for (index, token) in tokens.iter().enumerate() {
        let Some(value) = port_candidate_value(token, tokens.get(index + 1).copied()) else {
            continue;
        };
        if let Some(port) = valid_port_or_warn(value, source, warnings, metadata_warnings) {
            return Some(port);
        }
    }
    None
}

fn port_candidate_value<'a>(token: &'a str, next: Option<&'a str>) -> Option<&'a str> {
    if matches!(token, "--port" | "-p") {
        next
    } else if let Some(value) = token.strip_prefix("--port=") {
        Some(value)
    } else if let Some(value) = token.strip_prefix("-p") {
        Some(value)
    } else {
        let (name, value) = token.split_once('=')?;
        matches!(name, "PORT" | "VITE_PORT" | "APP_PORT").then_some(value)
    }
}

fn valid_port_or_warn(
    value: &str,
    source: &Path,
    warnings: &mut Vec<String>,
    metadata_warnings: &mut Vec<String>,
) -> Option<u16> {
    match parse_port(value) {
        PortParse::Valid(port) => Some(port),
        PortParse::Invalid(message) => {
            push_profile_warning(warnings, metadata_warnings, source, &message);
            None
        }
        PortParse::NotNumeric => None,
    }
}

fn push_profile_warning(
    warnings: &mut Vec<String>,
    metadata_warnings: &mut Vec<String>,
    source: &Path,
    message: &str,
) {
    metadata_warnings.push(format!("{}: {message}", source.display()));
    push_scan_warning(warnings, source, message);
}

fn script_tokens(script: &str) -> Vec<&str> {
    script
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')'))
        .map(|token| token.trim_matches(['"', '\'']))
        .filter(|token| !token.is_empty())
        .collect()
}

enum PortParse {
    Valid(u16),
    Invalid(String),
    NotNumeric,
}

fn parse_port(value: &str) -> PortParse {
    let value = value.trim_matches(['"', '\'']);
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return PortParse::NotNumeric;
    }
    match value.parse::<u16>() {
        Ok(port) if port > 0 => PortParse::Valid(port),
        Ok(_) => PortParse::Invalid(
            "frontend dev port literal 0 is invalid; preferred_dev_port was not inferred from it"
                .into(),
        ),
        Err(error) => PortParse::Invalid(format!(
            "frontend dev port literal {value} is invalid; preferred_dev_port was not inferred from it: {error}"
        )),
    }
}

fn safe_frontend_name(value: &str) -> String {
    safe_name(value, "web")
}

fn unique_frontend_name(name: String, seen: &mut BTreeSet<String>) -> String {
    if seen.insert(name.clone()) {
        return name;
    }
    for index in 2.. {
        let candidate = format!("{name}-{index}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn glob_escapes_root(glob: &str) -> bool {
    Path::new(glob).is_absolute()
        || Path::new(glob)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
}
