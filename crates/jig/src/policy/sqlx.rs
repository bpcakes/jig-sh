use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::policy::SqlxTodoInput;

const DEFAULT_SQLX_TODO_PATH: &str = "docs/sqlx-unchecked-queries-todo.md";

pub(super) fn generate_todo(ctx: &RepoContext, opts: &SqlxTodoInput) -> Result<Value> {
    let output = super::normalize_repo_relative_path(
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
        let text = fs::read_to_string(ctx.root().join(&file)).unwrap_or_default();
        calls.extend(scan_sqlx_calls(&file, &text));
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
    body.push_str("This checklist tracks every `sqlx::query*` call site under the configured Rust crate roots that is not yet using compile-time checked SQLx macros.\n\n");
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

fn scan_sqlx_calls(path: &str, text: &str) -> Vec<SqlxCall> {
    // Match only when the function name is followed by a macro bang, call
    // paren, or turbofish; this avoids counting shorter prefixes inside longer
    // variants.
    let names = [
        "sqlx::query_file_scalar",
        "sqlx::query_file_as",
        "sqlx::query_file",
        "sqlx::query_scalar",
        "sqlx::query_as",
        "sqlx::query",
    ];
    let path_is_test = is_test_path(path);
    let mut pending_cfg_test = false;
    let mut pending_test_module_decl = false;
    let mut test_module_brace_depth: Option<isize> = None;
    let mut raw_string_hashes = None;
    let mut block_comment_depth = 0usize;
    let mut calls = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut line_is_test = path_is_test || test_module_brace_depth.is_some();
        let mut entered_test_module = false;
        let trimmed = line.trim_start();
        let code = line_code_with_state(line, &mut raw_string_hashes, &mut block_comment_depth);
        let cfg_test_line = is_cfg_test_attr_code(&code);

        if test_module_brace_depth.is_some() {
            pending_cfg_test = false;
            pending_test_module_decl = false;
        } else if pending_test_module_decl {
            if code.trim_start().starts_with('{') {
                line_is_test = true;
                test_module_brace_depth = Some(line_brace_delta_from_code(&code));
                entered_test_module = true;
                pending_test_module_decl = false;
            } else if !(trimmed.is_empty() || trimmed.starts_with("//")) {
                pending_test_module_decl = false;
            }
        } else if cfg_test_line {
            pending_cfg_test = true;
            if let Some(decl) = test_module_decl_code(&code) {
                pending_cfg_test = false;
                match decl {
                    TestModuleDecl::InlineBrace => {
                        line_is_test = true;
                        test_module_brace_depth = Some(line_brace_delta_from_code(&code));
                        entered_test_module = true;
                    }
                    TestModuleDecl::BraceNextLine => {
                        pending_test_module_decl = true;
                    }
                    TestModuleDecl::ExternalFile => {}
                }
            }
        } else if pending_cfg_test {
            if let Some(decl) = test_module_decl_code(&code) {
                match decl {
                    TestModuleDecl::InlineBrace => {
                        line_is_test = true;
                        test_module_brace_depth = Some(line_brace_delta_from_code(&code));
                        entered_test_module = true;
                    }
                    TestModuleDecl::BraceNextLine => {
                        pending_test_module_decl = true;
                    }
                    TestModuleDecl::ExternalFile => {}
                }
                pending_cfg_test = false;
            } else if !(trimmed.is_empty()
                || trimmed.starts_with("#[")
                || trimmed.starts_with("//"))
            {
                pending_cfg_test = false;
            }
        }

        if pending_test_module_decl {
            pending_cfg_test = false;
        }

        for name in names {
            let mut start = 0usize;
            while let Some(pos) = code[start..].find(name) {
                let absolute = start + pos;
                let after = &code[absolute + name.len()..];
                if is_keyword_boundary(code[..absolute].chars().next_back()) {
                    if let Some(checked) = sqlx_call_checked(after) {
                        calls.push(SqlxCall {
                            path: path.into(),
                            line: index + 1,
                            function: name.into(),
                            checked,
                            is_test: line_is_test,
                        });
                    }
                }
                start = absolute + name.len();
            }
        }

        let mut clear_test_module = false;
        if let Some(depth) = &mut test_module_brace_depth {
            if !entered_test_module {
                *depth += line_brace_delta_from_code(&code);
            }
            if *depth <= 0 {
                clear_test_module = true;
            }
        }
        if clear_test_module {
            test_module_brace_depth = None;
        }
    }
    calls
}

enum TestModuleDecl {
    InlineBrace,
    BraceNextLine,
    ExternalFile,
}

fn sqlx_call_checked(after_name: &str) -> Option<bool> {
    let after_name = after_name.trim_start();
    if after_name.starts_with('!') {
        Some(true)
    } else if after_name.starts_with('(') || starts_turbofish(after_name) {
        Some(false)
    } else {
        None
    }
}

fn starts_turbofish(after_name: &str) -> bool {
    after_name
        .strip_prefix("::")
        .is_some_and(|rest| rest.trim_start().starts_with('<'))
}

fn test_module_decl_code(code: &str) -> Option<TestModuleDecl> {
    let mod_index = find_keyword(code, "mod")?;
    let after_mod = code[mod_index + 3..].trim_start();
    let mut chars = after_mod.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    let mut name_end = first.len_utf8();
    for (index, ch) in chars {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            name_end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    let after_name = after_mod[name_end..].trim_start();
    if after_name.starts_with('{') {
        Some(TestModuleDecl::InlineBrace)
    } else if after_name.is_empty() {
        Some(TestModuleDecl::BraceNextLine)
    } else if after_name.starts_with(';') {
        Some(TestModuleDecl::ExternalFile)
    } else {
        None
    }
}

fn is_cfg_test_attr_code(code: &str) -> bool {
    let Some(rest) = code.trim_start().strip_prefix("#[") else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix("cfg") else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix('(') else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix("test") else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix(')') else {
        return false;
    };
    rest.trim_start().starts_with(']')
}

fn find_keyword(text: &str, keyword: &str) -> Option<usize> {
    let mut start = 0usize;
    while let Some(pos) = text[start..].find(keyword) {
        let index = start + pos;
        let before = text[..index].chars().next_back();
        let after = text[index + keyword.len()..].chars().next();
        if is_keyword_boundary(before) && is_keyword_boundary(after) {
            return Some(index);
        }
        start = index + keyword.len();
    }
    None
}

fn is_keyword_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !(ch == '_' || ch.is_ascii_alphanumeric()))
}

fn line_code_with_state(
    line: &str,
    raw_string_hashes: &mut Option<usize>,
    block_comment_depth: &mut usize,
) -> String {
    let mut code = String::new();
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0usize;

    if let Some(hashes) = raw_string_hashes.take() {
        let Some(end) = raw_string_end(&chars, 0, hashes) else {
            *raw_string_hashes = Some(hashes);
            return code;
        };
        code.push(' ');
        index = end;
    }

    while index < chars.len() {
        if *block_comment_depth > 0 {
            if chars[index] == '/' && chars.get(index + 1).copied() == Some('*') {
                *block_comment_depth += 1;
                index += 2;
            } else if chars[index] == '*' && chars.get(index + 1).copied() == Some('/') {
                *block_comment_depth -= 1;
                index += 2;
                code.push(' ');
            } else {
                index += 1;
            }
            continue;
        }
        if chars[index] == '/' && chars.get(index + 1).copied() == Some('/') {
            break;
        }
        if chars[index] == '/' && chars.get(index + 1).copied() == Some('*') {
            *block_comment_depth = 1;
            index += 2;
            code.push(' ');
            continue;
        }
        if let Some((prefix_len, raw_hashes)) = raw_string_prefix(&chars, index) {
            let start = index + prefix_len + raw_hashes + 1;
            if let Some(end) = raw_string_end(&chars, start, raw_hashes) {
                index = end;
            } else {
                *raw_string_hashes = Some(raw_hashes);
                break;
            }
            code.push(' ');
            continue;
        }
        if let Some(quote_index) = quoted_string_start(&chars, index) {
            index = skip_quoted(&chars, quote_index, '"');
            code.push(' ');
            continue;
        }
        if let Some(char_end) = char_literal_end_at(&chars, index) {
            index = char_end;
            code.push(' ');
            continue;
        }
        code.push(chars[index]);
        index += 1;
    }
    code
}

fn line_brace_delta_from_code(code: &str) -> isize {
    code.chars().fold(0, |depth, ch| match ch {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

fn raw_string_prefix(chars: &[char], index: usize) -> Option<(usize, usize)> {
    let prefix_len = if chars.get(index).copied() == Some('r') {
        1
    } else if chars.get(index).copied() == Some('b') && chars.get(index + 1).copied() == Some('r') {
        2
    } else {
        return None;
    };
    let mut cursor = index + prefix_len;
    let mut hashes = 0usize;
    while chars.get(cursor).copied() == Some('#') {
        hashes += 1;
        cursor += 1;
    }
    (chars.get(cursor).copied() == Some('"')).then_some((prefix_len, hashes))
}

fn raw_string_end(chars: &[char], start: usize, hashes: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < chars.len() {
        if chars[cursor] == '"' && raw_string_closes(chars, cursor, hashes) {
            return Some(cursor + hashes + 1);
        }
        cursor += 1;
    }
    None
}

fn raw_string_closes(chars: &[char], quote_index: usize, hashes: usize) -> bool {
    (0..hashes).all(|offset| chars.get(quote_index + 1 + offset).copied() == Some('#'))
}

fn quoted_string_start(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index).copied() == Some('"') {
        Some(index)
    } else if chars.get(index).copied() == Some('b') && chars.get(index + 1).copied() == Some('"') {
        Some(index + 1)
    } else {
        None
    }
}

fn skip_quoted(chars: &[char], index: usize, quote: char) -> usize {
    let mut cursor = index + 1;
    let mut escaped = false;
    while cursor < chars.len() {
        if escaped {
            escaped = false;
        } else if chars[cursor] == '\\' {
            escaped = true;
        } else if chars[cursor] == quote {
            return cursor + 1;
        }
        cursor += 1;
    }
    chars.len()
}

fn char_literal_end_at(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index).copied() == Some('b') && chars.get(index + 1).copied() == Some('\'') {
        return char_literal_end(chars, index + 1);
    }
    char_literal_end(chars, index)
}

fn char_literal_end(chars: &[char], index: usize) -> Option<usize> {
    if chars.get(index).copied() != Some('\'') {
        return None;
    }
    let mut cursor = index + 1;
    if chars.get(cursor).copied() == Some('\\') {
        cursor += 1;
        if chars.get(cursor).copied() == Some('u') && chars.get(cursor + 1).copied() == Some('{') {
            cursor += 2;
            while cursor < chars.len() && chars[cursor] != '}' {
                cursor += 1;
            }
            cursor += 1;
        } else {
            cursor += 1;
        }
    } else {
        cursor += 1;
    }
    (chars.get(cursor).copied() == Some('\'')).then_some(cursor + 1)
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

fn is_test_path(path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    path.contains("/tests/")
        || matches!(basename, "tests.rs" | "test_support.rs")
        || basename.starts_with("tests_")
        || basename.ends_with("_tests.rs")
}

#[cfg(test)]
mod tests;
