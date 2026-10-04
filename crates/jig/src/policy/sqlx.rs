use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::policy::SqlxTodoInput;

mod modules;
mod scanner;

#[cfg(test)]
use scanner::scan_sqlx_calls;
use scanner::scan_sqlx_file;

const DEFAULT_SQLX_TODO_PATH: &str = "docs/sqlx-unchecked-queries-todo.md";

pub(super) fn generate_todo(ctx: &RepoContext, opts: &SqlxTodoInput) -> Result<Value> {
    let output = crate::repository_path::normalize_repo_relative_path(
        &opts
            .output
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SQLX_TODO_PATH)),
        "SQLx TODO output path",
    )?;
    let report = sqlx_report(ctx, &output)?;
    if let Some(parent) = ctx.root().join(&output).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(ctx.root().join(&output), report.body)?;
    Ok(json!({ "ok": true, "path": output, "non_test_count": report.non_test_count }))
}

pub(super) fn check_non_test(ctx: &RepoContext) -> Result<Value> {
    let report = sqlx_report(ctx, Path::new(DEFAULT_SQLX_TODO_PATH))?;
    Ok(json!({ "ok": report.non_test_count == 0, "non_test_count": report.non_test_count }))
}

struct SqlxReport {
    body: String,
    non_test_count: usize,
}

#[derive(Clone)]
struct SqlxCall {
    path: String,
    line: usize,
    function: String,
    checked: bool,
    is_test: bool,
}

fn sqlx_report(ctx: &RepoContext, prior_path: &Path) -> Result<SqlxReport> {
    let status_by_key = read_sqlx_statuses(&ctx.root().join(prior_path));
    let mut calls = Vec::new();
    let mut modules = BTreeMap::new();
    for file in sqlx_rust_files(ctx)? {
        let text = match fs::read_to_string(ctx.root().join(&file)) {
            Ok(text) => text,
            // `git ls-files` still lists a tracked file whose deletion has not
            // been staged, and an untracked listing races ordinary edits. An
            // absent path has no call sites; every other read error is real.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot read SQLx inventory source {file}"));
            }
        };
        let scan = scan_sqlx_file(&file, &text)?;
        calls.extend(scan.calls);
        modules.insert(file, scan.modules);
    }
    // A file that only a `#[cfg(test)]` module declaration reaches is test
    // code, exactly as the equivalent inline module already is.
    let test_only = modules::test_only_files(&modules);
    for call in &mut calls {
        call.is_test |= test_only.contains(&call.path);
    }
    calls.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    let checked_count = calls.iter().filter(|call| call.checked).count();
    let unchecked = calls
        .iter()
        .filter(|call| !call.checked)
        .collect::<Vec<_>>();
    let files_count = unchecked
        .iter()
        .map(|call| call.path.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let mut non_test_items = Vec::new();
    let mut test_items = Vec::new();
    for call in &unchecked {
        let key = format!("{}:{}|{}", call.path, call.line, call.function);
        let status = status_by_key.get(&key).copied().unwrap_or(' ');
        let item = format!(
            "- [{status}] `{}:{}`: `{}` -> `{}!`",
            call.path, call.line, call.function, call.function
        );
        if call.is_test {
            test_items.push(item);
        } else {
            non_test_items.push(item);
        }
    }
    let mut body = String::new();
    body.push_str("# SQLx Unchecked Queries TODO\n\n");
    body.push_str("This checklist tracks detected `sqlx::query*` call sites under the configured Rust crate roots that are not yet using compile-time checked SQLx macros.\n\n");
    body.push_str("Coverage is a source AST inventory, not complete Rust syntax coverage or compiler analysis. Sources are parsed as complete Rust files or expression fragments, as accepted by `include!`. Parsing, AST traversal, and destruction use a controlled stack and a conservative limit of 2,048 tokens along enclosing semicolon-separated regions (including delimiter groups and expression/type chains); sibling semicolon-separated declarations and statements do not accumulate toward that limit. Sources beyond that limit fail the inventory with a path-specific error; simplify nested syntax or split long expressions/declarations. It detects direct SQLx function calls and checked macro invocations. It does not resolve aliases or shadowing, expand macros, or evaluate arbitrary cfg expressions; macro input is read only for a fixed set of standard expression macros such as `vec!` and `assert!`. Test classification uses conventional test paths and exact `#[cfg(test)]` attributes on files and inline modules, and extends to a file that only `#[cfg(test)]` module declarations reach, following `mod` items and their `#[path]` attributes under Rust's module directory rules; a crate entrypoint, a file production code still reaches, and a file whose declaration cannot be resolved to an inventoried file stay classified by their own path and attributes, and `include!` relationships are not resolved. Paths Git lists that are absent from the worktree are skipped; other unreadable or unparseable Rust sources fail the inventory.\n\n");
    body.push_str("- Generated on: native jig\n");
    let _ = writeln!(body, "- Unchecked call sites: {}", unchecked.len());
    let _ = writeln!(
        body,
        "- Compile-checked macro call sites already present: {checked_count}"
    );
    let _ = writeln!(body, "- Files with unchecked call sites: {files_count}");
    let _ = writeln!(body, "- Non-test call sites: {}", non_test_items.len());
    let _ = write!(body, "- Test call sites: {}\n\n", test_items.len());
    body.push_str("## TODO Items (Non-Test Code - Priority)\n\n");
    if non_test_items.is_empty() {
        body.push_str("_None_\n");
    } else {
        body.push_str(&non_test_items.join("\n"));
        body.push('\n');
    }
    body.push_str("\n## TODO Items (Test Code)\n\n");
    if test_items.is_empty() {
        body.push_str("_None_\n");
    } else {
        body.push_str(&test_items.join("\n"));
        body.push('\n');
    }
    Ok(SqlxReport {
        body,
        non_test_count: non_test_items.len(),
    })
}

fn sqlx_rust_files(ctx: &RepoContext) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    // SQLx reports are a development TODO surface, so include both committed
    // files and non-ignored new Rust files under the configured crate roots.
    for file in super::git_list_files(ctx.root(), ctx.rust_crate_roots())? {
        if file.ends_with(".rs") {
            files.insert(file);
        }
    }
    if super::git_success(ctx.root(), &["rev-parse", "--is-inside-work-tree"])? {
        for file in git_untracked_files(ctx.root(), ctx.rust_crate_roots())? {
            if file.ends_with(".rs") {
                files.insert(file);
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn git_untracked_files(root: &Path, roots: &[String]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--others", "--exclude-standard", "--"];
    args.extend(roots.iter().map(String::as_str));
    Ok(super::split_nul(&super::git_output(root, &args)?))
}

fn read_sqlx_statuses(path: &Path) -> BTreeMap<String, char> {
    let mut map = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return map;
    };
    for line in text.lines() {
        if !(line.starts_with("- [ ] `")
            || line.starts_with("- [x] `")
            || line.starts_with("- [X] `"))
        {
            continue;
        }
        let status = if line.as_bytes().get(3).copied() == Some(b'x')
            || line.as_bytes().get(3).copied() == Some(b'X')
        {
            'x'
        } else {
            ' '
        };
        let parts = line.split('`').collect::<Vec<_>>();
        if parts.len() >= 5 {
            map.insert(format!("{}|{}", parts[1], parts[3]), status);
        }
    }
    map
}

#[cfg(test)]
mod tests;
