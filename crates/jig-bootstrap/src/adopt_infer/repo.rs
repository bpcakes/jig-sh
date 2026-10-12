use std::path::Path;

use super::super::git::{git_stdout, is_git_work_tree};
use super::scan::push_scan_warning;

#[derive(Clone, Debug)]
pub(super) struct RepoValueInference {
    pub(super) value: Option<String>,
    pub(super) source: Option<String>,
}

#[cfg(test)]
pub(super) fn infer_repo_name(root: &Path) -> Option<String> {
    infer_repo_name_with_metadata(root).value
}

pub(super) fn infer_repo_name_with_metadata(root: &Path) -> RepoValueInference {
    if let Some(value) = git_remote_repo_name(root) {
        return RepoValueInference {
            value: Some(safe_remote_repo_name(&value)),
            source: Some("git remote origin URL".into()),
        };
    }

    let value = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .map(|value| safe_repo_name(&value));
    let source = value
        .as_ref()
        .map(|_| "repository directory name".to_string());
    RepoValueInference { value, source }
}

fn git_remote_repo_name(root: &Path) -> Option<String> {
    let remote = try_git_stdout(root, ["remote", "get-url", "origin"])?;
    repo_name_from_remote_url(&remote)
}

pub(super) fn repo_name_from_remote_url(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last);
    Some(name.to_string()).filter(|value| !value.is_empty())
}

#[cfg(test)]
pub(super) fn infer_default_branch(root: &Path, warnings: &mut Vec<String>) -> Option<String> {
    infer_default_branch_with_metadata(root, warnings).value
}

pub(super) fn infer_default_branch_with_metadata(
    root: &Path,
    warnings: &mut Vec<String>,
) -> RepoValueInference {
    if !is_git_work_tree(root) {
        return RepoValueInference {
            value: None,
            source: None,
        };
    }
    let origin_head = git_stdout(
        root,
        ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    // This probe is optional: repositories without `origin/HEAD` fall through
    // to explicit origin branch refs and then a known local branch name.
    .ok()
    .and_then(|value| value.strip_prefix("origin/").map(str::to_string));
    if let Some(value) = origin_head.filter(|value| !value.is_empty()) {
        return RepoValueInference {
            value: Some(value),
            source: Some("git refs/remotes/origin/HEAD".into()),
        };
    }
    if let Some(value) = infer_default_branch_from_remote_refs(root, warnings) {
        return RepoValueInference {
            value: Some(value),
            source: Some("git refs/remotes/origin/{main,master,trunk}".into()),
        };
    }
    let value =
        infer_default_branch_from_local_head(root, warnings).filter(|value| !value.is_empty());
    let source = value
        .as_ref()
        .map(|_| "git symbolic-ref --short HEAD".to_string());
    RepoValueInference { value, source }
}

fn infer_default_branch_from_remote_refs(
    root: &Path,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let branches = ["main", "master", "trunk"]
        .into_iter()
        .filter(|branch| git_ref_exists(root, &format!("refs/remotes/origin/{branch}")))
        .collect::<Vec<_>>();
    let branch = branches.first()?;
    if branches.len() > 1 {
        push_scan_warning(
            warnings,
            root,
            &format!(
                "multiple origin default branch candidates detected ({}); using {}",
                branches.join(", "),
                branch
            ),
        );
    }
    Some((*branch).into())
}

fn infer_default_branch_from_local_head(root: &Path, warnings: &mut Vec<String>) -> Option<String> {
    // Local HEAD is only a fallback signal. If git cannot report it, adopt keeps
    // the template default unless the user supplies an explicit branch.
    let branch = git_stdout(root, ["symbolic-ref", "--short", "HEAD"]).ok()?;
    if matches!(branch.as_str(), "main" | "master" | "trunk") {
        return Some(branch);
    }
    push_scan_warning(
        warnings,
        root,
        &format!(
            "current branch {branch} is not a known default branch name; using template default unless overridden"
        ),
    );
    None
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    // Missing or unreadable refs are not adoption failures; they only remove
    // this branch name from the default-branch inference candidates.
    super::super::git::git_command(root, ["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn try_git_stdout<const N: usize>(root: &Path, args: [&str; N]) -> Option<String> {
    if !is_git_work_tree(root) {
        return None;
    }
    // Git probes here are optional inference only. Failure means the value is
    // unavailable, so callers fall back to directory names or template defaults.
    git_stdout(root, args)
        .ok()
        .filter(|value| !value.is_empty())
}

pub(super) fn safe_repo_name(value: &str) -> String {
    safe_name_with(value, "repo", |ch| {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
    })
}

fn safe_remote_repo_name(value: &str) -> String {
    // Remote repositories can legally contain dots (`owner/my.app.git`), while
    // directory fallback names stay DNS-label-shaped for existing local behavior.
    safe_name_with(value, "repo", |ch| {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
    })
}

pub(super) fn safe_name(value: &str, fallback: &str) -> String {
    safe_name_with(value, fallback, |ch| {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
    })
}

fn safe_name_with(value: &str, fallback: &str, safe_char: impl Fn(char) -> bool) -> String {
    let name = value
        .chars()
        .map(|ch| if safe_char(ch) { ch } else { '-' })
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_string();
    if name.is_empty() {
        fallback.into()
    } else {
        name
    }
}
