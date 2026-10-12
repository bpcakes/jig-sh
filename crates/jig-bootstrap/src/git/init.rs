use super::*;

#[cfg(test)]
pub(super) fn init_git_repo(destination: &Path, default_branch: &str) -> Result<bool> {
    init_git_repo_with_validation(destination, default_branch, || Ok(()))
}

pub(crate) fn init_git_repo_with_validation(
    destination: &Path,
    default_branch: &str,
    mut validate_destination: impl FnMut() -> Result<()>,
) -> Result<bool> {
    let destination_git = destination.join(".git");
    match fs::symlink_metadata(&destination_git) {
        Ok(_) => {
            validate_existing_git_work_tree_at_boundary(
                destination,
                &mut validate_destination,
                "before accepting existing Git metadata",
            )?;
            return Ok(false);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect git metadata {}",
                    destination_git.display()
                )
            });
        }
    }

    // Git may leave a partial .git directory after any failed init/fallback
    // step. Build it in a private sibling working tree, then publish only the
    // completed metadata directory with a no-replace rename. A concurrently
    // created destination .git wins without being modified.
    validate_destination().context("Git init destination validation failed before staging")?;
    let staged = private_tempdir_in(destination, ".jig-git-init-").with_context(|| {
        format!(
            "Failed to create private git init staging directory in {}",
            destination.display()
        )
    })?;
    let staged_destination = jig_repository::shell::git_env_path(staged.path())?;
    let staged_git = staged_destination.join(".git");

    let git_program = git_program();
    let initialization = (|| {
        staged.require_identity("before preparing the private Git template")?;
        let template_dir =
            prepare_private_git_template(&git_program, &staged_destination, &staged_git)?;
        staged.require_identity("after preparing the private Git template")?;
        let mut with_branch_command =
            staged_repository_command(&git_program, &staged_destination, &staged_git);
        apply_private_git_template(&mut with_branch_command, template_dir.as_deref());
        staged.require_identity("before running git init")?;
        let with_branch = with_branch_command
            .args(["init", "-b", default_branch])
            .output()
            .with_context(|| format!("Failed to start {git_program}"))?;
        staged.require_identity("after running git init")?;
        if !with_branch.status.success() {
            if !git_init_branch_flag_unsupported(&with_branch) {
                bail!(
                    "git init -b {default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&with_branch.stdout),
                    String::from_utf8_lossy(&with_branch.stderr)
                );
            }

            let mut fallback_command =
                staged_repository_command(&git_program, &staged_destination, &staged_git);
            apply_private_git_template(&mut fallback_command, template_dir.as_deref());
            staged.require_identity("before running fallback git init")?;
            let fallback = fallback_command
                .arg("init")
                .output()
                .with_context(|| format!("Failed to start {git_program}"))?;
            staged.require_identity("after running fallback git init")?;
            require_success(&fallback, |output| {
                format!(
                    "git init failed.\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )
            })?;
            set_git_head_branch(
                &staged_destination,
                &staged_git,
                &git_program,
                default_branch,
            )?;
            staged.require_identity("after setting the fallback Git branch")?;
        }

        staged.require_identity("before validating initialized Git metadata")?;
        validate_staged_git_repository(
            &staged_destination,
            &staged_git,
            &git_program,
            default_branch,
        )?;
        staged.require_identity("after validating initialized Git metadata")?;
        let git_directory = crate::path::repository_directory_commit_at(&staged_git)
            .context("Failed to retain initialized Git metadata directory identity")?;
        require_staged_git_directory_identity(
            &staged,
            &git_directory,
            &staged_git,
            "after retaining initialized Git metadata",
        )?;
        Ok(git_directory)
    })();
    let (staged, staged_git_commit) =
        retain_staging_directory_on_success(staged, &staged_destination, initialization)?;

    if let Err(error) = staged
        .require_identity("before destination validation for metadata staging")
        .and_then(|()| {
            require_staged_git_directory_identity(
                &staged,
                &staged_git_commit,
                &staged_git,
                "before destination validation for metadata staging",
            )
        })
        .and_then(|()| {
            validate_destination()
                .context("Git init destination validation failed before metadata staging")
        })
        .and_then(|()| staged.require_identity("after destination validation for metadata staging"))
        .and_then(|()| {
            require_staged_git_directory_identity(
                &staged,
                &staged_git_commit,
                &staged_git,
                "after destination validation for metadata staging",
            )
        })
    {
        return close_staging_directory(staged, &staged_destination, Err(error));
    }
    let metadata_stage =
        match private_tempdir_in(destination, ".jig-git-metadata-").with_context(|| {
            format!(
                "Failed to create private git metadata staging directory in {}",
                destination.display()
            )
        }) {
            Ok(metadata_stage) => metadata_stage,
            Err(error) => {
                return close_staging_directory(staged, &staged_destination, Err(error));
            }
        };
    let metadata_stage_path = jig_repository::shell::git_env_path(metadata_stage.path())?;

    let transfer = (|| {
        staged.require_identity("before transferring initialized Git metadata")?;
        metadata_stage.require_identity("before receiving initialized Git metadata")?;
        let permissions = fs::symlink_metadata(&staged_git)
            .with_context(|| {
                format!(
                    "Failed to inspect staged git metadata permissions {}",
                    staged_git.display()
                )
            })?
            .permissions();
        move_directory_contents(
            &staged_git,
            &metadata_stage_path,
            &staged,
            &staged_git_commit,
            &metadata_stage,
        )?;
        staged.require_identity("after transferring initialized Git metadata")?;
        metadata_stage.require_identity("after receiving initialized Git metadata")?;
        validate_staged_git_repository(
            &staged_destination,
            &metadata_stage_path,
            &git_program,
            default_branch,
        )?;
        staged.require_identity("after validating the disposable Git worktree")?;
        metadata_stage.require_identity("after validating staged Git metadata")?;
        Ok(permissions)
    })();
    let final_git_permissions = match transfer {
        Ok(permissions) => permissions,
        Err(error) => {
            let error =
                close_staging_directory::<()>(metadata_stage, &metadata_stage_path, Err(error))
                    .expect_err("an error remains an error after metadata staging cleanup");
            return close_staging_directory(staged, &staged_destination, Err(error));
        }
    };

    if let Err(error) = close_worktree_staging_before_publication(staged, &staged_destination) {
        return close_staging_directory(metadata_stage, &metadata_stage_path, Err(error));
    }

    // All disposable worktree cleanup has succeeded. Publication keeps this
    // guard through the no-replace rename, disarming it only after success and
    // explicitly closing it on every failure or contention path.
    if let Err(error) = metadata_stage
        .require_identity("before destination validation for Git metadata publication")
        .and_then(|()| {
            validate_destination()
                .context("Git init destination validation failed before .git publication")
        })
        .and_then(|()| {
            metadata_stage
                .require_identity("after destination validation for Git metadata publication")
        })
    {
        return close_staging_directory(metadata_stage, &metadata_stage_path, Err(error));
    }
    publish_staged_git_directory(
        metadata_stage,
        &destination_git,
        final_git_permissions,
        &mut validate_destination,
    )
}

fn git_init_branch_flag_unsupported(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("unknown switch `b")
        || stderr.contains("unknown option `b")
        || stderr.contains("unknown option `initial-branch")
        || stderr.contains("unknown option `initial branch")
}

fn set_git_head_branch(
    work_tree: &Path,
    git_dir: &Path,
    git_program: &str,
    default_branch: &str,
) -> Result<()> {
    let output = staged_repository_command(git_program, work_tree, git_dir)
        .args([
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ])
        .output()
        .with_context(|| format!("Failed to start {git_program}"))?;
    require_success(&output, |output| {
        format!(
            "git symbolic-ref HEAD refs/heads/{default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
