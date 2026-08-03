use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::Value;

use crate::file_ops;
use crate::host::route_hostname;
use crate::types::{AppKind, AppRunSpec, CommandSpec};

const MAX_WORKSPACE_FILE_BYTES: u64 = 256 * 1024;
const MAX_WORKSPACE_GLOB_DEPTH: usize = 16;
const MAX_WORKSPACE_GLOB_MATCHES: usize = 10_000;

pub(crate) fn discover(
    root: &Path,
    repo_name: &str,
    tld: &str,
    package_manager: &str,
) -> Result<Vec<AppRunSpec>> {
    let Some(workspace_root) = find_workspace_root(root) else {
        return Ok(Vec::new());
    };
    let globs = workspace_globs(&workspace_root)?;
    let mut specs = Vec::new();

    for dir in expand_globs(&workspace_root, &globs)? {
        let pkg_path = dir.join("package.json");
        let Ok(text) = read_workspace_text(&pkg_path) else {
            continue;
        };
        let Ok(pkg) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let dev_script = pkg
            .get("scripts")
            .and_then(Value::as_object)
            .and_then(|scripts| scripts.get("dev"))
            .and_then(Value::as_str);
        let Some(dev_script) = dev_script.filter(|value| !value.trim().is_empty()) else {
            continue;
        };

        let name = pkg
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                dir.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "app".into());
        let hostname = route_hostname(&name, repo_name, tld)?;
        let kind = if script_looks_like_vite(dev_script) {
            AppKind::Vite
        } else {
            AppKind::EnvPort
        };
        specs.push(AppRunSpec {
            name,
            dir,
            command: CommandSpec::Argv(vec![package_manager.into(), "run".into(), "dev".into()]),
            kind,
            hostname,
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: true,
        });
    }

    specs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(specs)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    if start.join("pnpm-workspace.yaml").exists() || package_json_has_workspaces(start) {
        Some(start.to_path_buf())
    } else {
        None
    }
}

fn package_json_has_workspaces(dir: &Path) -> bool {
    let Ok(text) = read_workspace_text(&dir.join("package.json")) else {
        return false;
    };
    let Ok(pkg) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    pkg.get("workspaces").is_some_and(|workspaces| {
        workspaces.is_array()
            || workspaces
                .get("packages")
                .and_then(Value::as_array)
                .is_some()
    })
}

fn workspace_globs(root: &Path) -> Result<Vec<String>> {
    let pnpm = root.join("pnpm-workspace.yaml");
    if pnpm.exists() {
        return parse_pnpm_workspace(&read_workspace_text(&pnpm)?);
    }
    let text = read_workspace_text(&root.join("package.json"))?;
    let pkg: Value = serde_json::from_str(&text)?;
    let Some(workspaces) = pkg.get("workspaces") else {
        return Ok(Vec::new());
    };
    if let Some(items) = workspaces.as_array() {
        return Ok(items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect());
    }
    Ok(workspaces
        .get("packages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default())
}

fn read_workspace_text(path: &Path) -> Result<String> {
    let mut file = match file_ops::open_read_no_follow(path) {
        Ok(file) => file,
        Err(_)
            if fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink()) =>
        {
            bail!("workspace config {} must not be a symlink", path.display());
        }
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if metadata.len() > MAX_WORKSPACE_FILE_BYTES {
        bail!(
            "workspace config {} is larger than {} bytes",
            path.display(),
            MAX_WORKSPACE_FILE_BYTES
        );
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

fn parse_pnpm_workspace(text: &str) -> Result<Vec<String>> {
    let mut globs = Vec::new();
    let mut in_packages = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("packages:") {
            let rest = strip_inline_yaml_comment(rest.trim()).trim();
            if rest.starts_with('[') && rest.ends_with(']') {
                return Ok(rest
                    .trim_matches(['[', ']'])
                    .split(',')
                    .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
                    .filter(|item| !item.is_empty())
                    .collect());
            }
            if rest.starts_with('[') {
                bail!("pnpm-workspace.yaml uses unsupported multi-line flow-style packages list");
            }
            if !rest.is_empty() {
                bail!("pnpm-workspace.yaml uses unsupported inline packages value");
            }
            in_packages = true;
            continue;
        }
        if in_packages {
            if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.starts_with('-') {
                in_packages = false;
            } else {
                if trimmed.starts_with('[') {
                    bail!(
                        "pnpm-workspace.yaml uses unsupported multi-line flow-style packages list"
                    );
                }
                if let Some(item) = trimmed.strip_prefix('-') {
                    let item = strip_inline_yaml_comment(item.trim())
                        .trim()
                        .trim_matches(['"', '\''])
                        .to_string();
                    if !item.is_empty() {
                        globs.push(item);
                    }
                } else if !trimmed.is_empty() {
                    bail!("pnpm-workspace.yaml uses unsupported non-list packages entry");
                }
            }
        }
    }
    Ok(globs)
}

fn strip_inline_yaml_comment(value: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut backslashes = 0usize;
    for (index, ch) in value.char_indices() {
        let escaped = in_double && backslashes % 2 == 1;
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single && !escaped => in_double = !in_double,
            '#' if !in_single
                && !in_double
                && (index == 0
                    || value[..index]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_whitespace)) =>
            {
                return value[..index].trim_end();
            }
            _ => {}
        }
        if ch == '\\' && in_double {
            backslashes += 1;
        } else {
            backslashes = 0;
        }
    }
    value.trim_end()
}

fn expand_globs(root: &Path, globs: &[String]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut excluded = Vec::new();
    let canonical_root = root.canonicalize().map_err(|error| {
        anyhow::anyhow!(
            "Failed to canonicalize workspace root {}: {error}",
            root.display()
        )
    })?;
    for glob in globs {
        if let Some(exclude) = glob.strip_prefix('!') {
            if glob_escapes_root(exclude) {
                bail!("workspace exclusion glob '!{exclude}' must stay within the repo root");
            }
            expand_segments(
                Some(canonical_root.as_path()),
                root,
                &exclude.split('/').collect::<Vec<_>>(),
                &mut excluded,
                0,
                true,
            )?;
        } else {
            if glob_escapes_root(glob) {
                bail!("workspace glob '{glob}' must stay within the repo root");
            }
            expand_segments(
                Some(canonical_root.as_path()),
                root,
                &glob.split('/').collect::<Vec<_>>(),
                &mut out,
                0,
                false,
            )?;
        }
    }
    let canonical_excluded = canonicalize_excluded_paths(&excluded)?;
    let mut kept = Vec::new();
    for path in out {
        let Ok(canonical_path) = path.canonicalize() else {
            continue;
        };
        if canonical_path == canonical_root.as_path()
            || !canonical_path.starts_with(&canonical_root)
            || canonical_excluded
                .iter()
                .any(|excluded| canonical_path == *excluded || canonical_path.starts_with(excluded))
        {
            continue;
        }
        kept.push(canonical_path);
    }
    kept.sort();
    kept.dedup();
    Ok(kept)
}

fn canonicalize_excluded_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut canonical = Vec::new();
    for path in paths {
        match path.canonicalize() {
            Ok(canonical_path) => canonical.push(canonical_path),
            Err(error) => match fs::symlink_metadata(path) {
                Ok(_) => bail!(
                    "Failed to canonicalize workspace exclusion path {}: {error}",
                    path.display()
                ),
                Err(metadata_error) if metadata_error.kind() == ErrorKind::NotFound => {}
                Err(metadata_error) => bail!(
                    "Failed to inspect workspace exclusion path {} after canonicalization failed ({error}): {metadata_error}",
                    path.display()
                ),
            },
        }
    }
    Ok(canonical)
}

fn glob_escapes_root(glob: &str) -> bool {
    Path::new(glob).is_absolute()
        || Path::new(glob)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
}

fn expand_segments(
    canonical_root: Option<&Path>,
    base: &Path,
    segments: &[&str],
    out: &mut Vec<PathBuf>,
    depth: usize,
    include_dirs_without_package: bool,
) -> Result<()> {
    if out.len() >= MAX_WORKSPACE_GLOB_MATCHES {
        bail!(
            "workspace glob expansion exceeded {MAX_WORKSPACE_GLOB_MATCHES} matches; narrow workspace globs before running discovery"
        );
    }
    if depth > MAX_WORKSPACE_GLOB_DEPTH {
        return Ok(());
    }
    if path_is_symlink(base) || !path_is_within_root(canonical_root, base) {
        return Ok(());
    }
    if segments.is_empty() {
        if include_dirs_without_package || base.join("package.json").exists() {
            push_workspace_match(out, base)?;
        }
        return Ok(());
    }
    let (first, rest) = segments.split_first().expect("checked non-empty");
    if *first == "**" {
        if !rest.is_empty() {
            expand_segments(
                canonical_root,
                base,
                rest,
                out,
                depth,
                include_dirs_without_package,
            )?;
        }
        let Ok(entries) = fs::read_dir(base) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if should_recurse_into(&entry) && entry_is_real_dir(&entry) {
                if rest.is_empty() {
                    expand_segments(
                        canonical_root,
                        &path,
                        rest,
                        out,
                        depth + 1,
                        include_dirs_without_package,
                    )?;
                }
                expand_segments(
                    canonical_root,
                    &path,
                    segments,
                    out,
                    depth + 1,
                    include_dirs_without_package,
                )?;
            }
        }
        return Ok(());
    }
    if first.contains('*') {
        let Ok(entries) = fs::read_dir(base) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry_is_real_dir(&entry)
                && entry
                    .file_name()
                    .to_str()
                    .map(|name| segment_matches(first, name))
                    .unwrap_or(false)
            {
                expand_segments(
                    canonical_root,
                    &path,
                    rest,
                    out,
                    depth + 1,
                    include_dirs_without_package,
                )?;
            }
        }
    } else {
        expand_segments(
            canonical_root,
            &base.join(first),
            rest,
            out,
            depth + 1,
            include_dirs_without_package,
        )?;
    }
    Ok(())
}

fn push_workspace_match(out: &mut Vec<PathBuf>, path: &Path) -> Result<()> {
    if out.len() >= MAX_WORKSPACE_GLOB_MATCHES {
        bail!(
            "workspace glob expansion exceeded {MAX_WORKSPACE_GLOB_MATCHES} matches; narrow workspace globs before running discovery"
        );
    }
    out.push(path.to_path_buf());
    Ok(())
}

fn should_recurse_into(entry: &fs::DirEntry) -> bool {
    entry.file_name().to_str().is_some_and(|name| {
        !matches!(name, "node_modules" | "target" | "dist" | "build") && !name.starts_with('.')
    })
}

fn entry_is_real_dir(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|file_type| file_type.is_dir())
}

fn path_is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn path_is_within_root(canonical_root: Option<&Path>, path: &Path) -> bool {
    let Some(canonical_root) = canonical_root else {
        return false;
    };
    path.canonicalize()
        .is_ok_and(|canonical_path| canonical_path.starts_with(canonical_root))
}

fn segment_matches(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    // Package workspace patterns support the common single-wildcard segment
    // shape (`apps/*`, `pkg-*-shared`), not full shell glob semantics.
    let Some(index) = pattern.find('*') else {
        return pattern == name;
    };
    let (prefix, suffix) = pattern.split_at(index);
    let suffix = suffix.trim_start_matches('*');
    name.starts_with(prefix) && name.ends_with(suffix)
}

fn script_looks_like_vite(value: &str) -> bool {
    let tokens: Vec<_> = value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')'))
        .filter_map(normalized_script_token)
        .collect();
    let Some(vite_index) = tokens.iter().position(|token| is_vite_token(token)) else {
        return false;
    };
    !tokens[vite_index + 1..]
        .iter()
        .any(|token| matches!(*token, "build" | "preview" | "optimize"))
}

fn normalized_script_token(token: &str) -> Option<&str> {
    let token = token.trim_matches(['"', '\'']);
    if token.is_empty() {
        return None;
    }
    Some(token.rsplit('/').next().unwrap_or(token))
}

fn is_vite_token(token: &str) -> bool {
    token == "vite" || token.starts_with("vite@")
}

#[cfg(test)]
mod tests;
