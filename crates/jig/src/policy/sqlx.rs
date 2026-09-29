use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::policy::SqlxTodoInput;

mod scanner;

use scanner::scan_sqlx_calls;

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
    for file in sqlx_rust_files(ctx)? {
        let text = fs::read_to_string(ctx.root().join(&file))
            .with_context(|| format!("cannot read SQLx inventory source {file}"))?;
        calls.extend(scan_sqlx_calls(&file, &text)?);
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
    body.push_str("Coverage is a source AST inventory, not complete Rust syntax coverage or compiler analysis. It detects direct SQLx function calls and checked macro invocations. It does not resolve aliases or shadowing, inspect macro input tokens or expansions, or evaluate arbitrary cfg expressions. Test classification uses conventional test paths and exact `#[cfg(test)]` attributes on files and inline modules; external module relationships are not resolved. Unreadable or unparseable Rust files fail the inventory.\n\n");
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
