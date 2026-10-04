use super::*;

const JOURNAL: &str = ".agent/state/runs.jsonl";

pub(super) fn check_local_history(
    root: &Path,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Option<DoctorCheck> {
    // Repositories without Git have no index or ignore policy to migrate.
    // A linked worktree has a .git file instead of a directory.
    if !root.ancestors().any(|path| path.join(".git").exists()) {
        return None;
    }
    let facts = (|| -> Result<(bool, bool)> {
        let tracked = git_path_matches(root, &["ls-files", "--error-unmatch"], cancelled)?;
        let ignored = git_path_matches(root, &["check-ignore", "--no-index", "-q"], cancelled)?;
        Ok((tracked, ignored))
    })();
    Some(match facts {
        Ok((tracked, ignored)) => {
            let mut result = check(
                "run_history",
                "Local run history",
                false,
                !tracked && ignored,
                if tracked { "tracked" } else if ignored { "local" } else { "not ignored" },
                if tracked {
                    "Git tracks runs.jsonl; routine checks change the checkout and can cause merge conflicts."
                } else if ignored {
                    "runs.jsonl stays local and is ignored by Git."
                } else {
                    "runs.jsonl is untracked but can still be added to Git."
                },
            ).with_data(json!({ "path": JOURNAL, "tracked": tracked, "ignored": ignored }));
            if tracked || !ignored {
                let mut fix = String::new();
                if !ignored {
                    fix.push_str(
                        "Add `.agent/state/runs.jsonl` to `.gitignore` in the Jig root (the directory containing `.jig.toml`). ",
                    );
                }
                if tracked {
                    fix.push_str("Run `git rm --cached -- .agent/state/runs.jsonl` from the Jig root (the directory containing `.jig.toml`) to keep the local file and stop tracking it. Before other clones pull the deletion commit, copy aside any `.agent/state/runs.jsonl` history they need to keep. ");
                }
                fix.push_str(
                    "Commit the tracking policy change; keep local run history out of commits.",
                );
                result = result.with_fix(&fix);
            }
            result
        }
        Err(error) => check(
            "run_history",
            "Local run history",
            false,
            false,
            "unverified",
            format!("Could not inspect run-history tracking: {error}"),
        )
        .with_fix("Check Git access from the Jig root containing `.jig.toml`, then rerun `scripts/jig doctor`."),
    })
}

fn git_path_matches(
    root: &Path,
    args: &[&str],
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<bool> {
    let mut command = Command::new("git");
    crate::bootstrap::scrub_known_repository_git_environment(&mut command);
    command
        .current_dir(root)
        .args(args)
        .args(["--", JOURNAL])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let output = run_owned_process_tree_with_output(&mut command, Duration::from_secs(5), || {
        cancelled.is_some_and(|cancelled| cancelled())
    })?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(anyhow!("git {} exited with {}", args[0], output.status)),
    }
}
