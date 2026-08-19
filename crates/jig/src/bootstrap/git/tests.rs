use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

#[cfg(windows)]
use super::git_template_environment_path;
use super::{
    GIT_BIN_ENV, init_git_repo, init_git_repo_with_validation, validate_staged_git_repository,
};

const HELPER_DESTINATION: &str = "JIG_GIT_INIT_AMBIENT_HELPER_DESTINATION";
const HELPER_EXPECT_EXISTING_GIT_PRESERVED: &str =
    "JIG_GIT_INIT_AMBIENT_HELPER_EXPECT_EXISTING_GIT_PRESERVED";
const HELPER_EXPECT_EXISTING_GIT_ACCEPTED: &str =
    "JIG_GIT_INIT_AMBIENT_HELPER_EXPECT_EXISTING_GIT_ACCEPTED";
const HELPER_EXPECT_FAILURE: &str = "JIG_GIT_INIT_AMBIENT_HELPER_EXPECT_FAILURE";
const HELPER_TEST: &str = "bootstrap::git::tests::git_init_ambient_environment_helper";

#[cfg(windows)]
#[test]
fn git_template_environment_path_removes_verbatim_prefix() {
    assert_eq!(
        git_template_environment_path(Path::new(r"\\?\C:\repo\.jig-git-template")).unwrap(),
        PathBuf::from(r"C:\repo\.jig-git-template")
    );
}

#[test]
fn git_init_ambient_environment_helper() {
    let Some(destination) = std::env::var_os(HELPER_DESTINATION) else {
        return;
    };
    let destination = PathBuf::from(destination);
    if std::env::var_os(HELPER_EXPECT_EXISTING_GIT_ACCEPTED).is_some() {
        assert!(!init_git_repo(&destination, "main").unwrap());
        assert!(destination.join(".git").is_dir());
        return;
    }
    if let Some(expected) = std::env::var_os(HELPER_EXPECT_EXISTING_GIT_PRESERVED) {
        let error = init_git_repo(&destination, "main").unwrap_err();
        let expected = expected.to_string_lossy();
        assert!(
            format!("{error:#}").contains(expected.as_ref()),
            "unexpected existing-metadata failure: {error:#}"
        );
        assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
        return;
    }
    if let Some(expected) = std::env::var_os(HELPER_EXPECT_FAILURE) {
        let error = init_git_repo(&destination, "main").unwrap_err();
        let expected = expected.to_string_lossy();
        assert!(
            format!("{error:#}").contains(expected.as_ref()),
            "unexpected injected failure: {error:#}"
        );
        assert!(!destination.join(".git").exists());
        return;
    }
    assert!(init_git_repo(&destination, "main").unwrap());
    assert!(!init_git_repo(&destination, "main").unwrap());
    validate_staged_git_repository(&destination, &destination.join(".git"), "git", "main").unwrap();
}

#[test]
fn git_init_accepts_a_valid_existing_git_directory() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    create_existing_git_repository(&destination);

    assert!(!init_git_repo(&destination, "main").unwrap());
    assert!(destination.join(".git").is_dir());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_accepts_a_valid_existing_regular_gitfile() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let metadata = temp.path().join("separate-metadata");
    fs::create_dir(&destination).unwrap();
    create_existing_gitfile_repository(&destination, &metadata);

    assert!(!init_git_repo(&destination, "main").unwrap());
    assert!(
        fs::symlink_metadata(destination.join(".git"))
            .unwrap()
            .is_file()
    );
    assert!(metadata.is_dir());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_rejects_a_gitfile_that_resolves_an_outer_worktree() {
    let temp = tempdir().unwrap();
    let outer = temp.path().join("outer-repository");
    let destination = outer.join("nested-destination");
    fs::create_dir(&outer).unwrap();
    create_existing_git_repository(&outer);
    fs::create_dir(&destination).unwrap();
    let mut config = Command::new("git");
    super::scrub_git_repository_environment(&mut config);
    let status = config
        .args(["config", "--file"])
        .arg(outer.join(".git/config"))
        .arg("core.worktree")
        .arg(&outer)
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        destination.join(".git"),
        format!("gitdir: {}\n", outer.join(".git").display()),
    )
    .unwrap();

    let error = init_git_repo(&destination, "main").unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("different work-tree root"), "{message}");
    assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
    assert!(outer.join(".git/HEAD").is_file());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_rejects_unusable_existing_git_files_and_directories() {
    for kind in ["garbage-file", "partial-directory"] {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repository");
        fs::create_dir(&destination).unwrap();
        match kind {
            "garbage-file" => fs::write(destination.join(".git"), b"not a gitfile\n").unwrap(),
            "partial-directory" => fs::create_dir(destination.join(".git")).unwrap(),
            _ => unreachable!(),
        }

        let error = init_git_repo(&destination, "main").unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("usable"), "{kind}: {message}");
        assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
        assert_no_private_git_staging(&destination);
    }
}

#[cfg(unix)]
#[test]
fn git_init_rejects_existing_git_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let valid = temp.path().join("valid");
    fs::create_dir(&valid).unwrap();
    create_existing_git_repository(&valid);

    for target in [valid.join(".git"), temp.path().join("missing-git-target")] {
        let destination = temp.path().join(format!(
            "repository-{}",
            if target.exists() { "valid" } else { "dangling" }
        ));
        fs::create_dir(&destination).unwrap();
        symlink(&target, destination.join(".git")).unwrap();

        let error = init_git_repo(&destination, "main").unwrap_err();

        let message = format!("{error:#}");
        assert!(message.contains("not a symlink"), "{message}");
        assert!(fs::symlink_metadata(destination.join(".git")).is_ok());
        assert_no_private_git_staging(&destination);
    }
}

#[test]
fn git_init_rejects_and_preserves_an_invalid_concurrent_git_winner() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    let calls = Cell::new(0);

    let error = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        if call == 3 {
            fs::write(destination.join(".git"), b"concurrent invalid metadata\n").unwrap();
        }
        Ok(())
    })
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("usable"), "{message}");
    assert_eq!(
        fs::read(destination.join(".git")).unwrap(),
        b"concurrent invalid metadata\n"
    );
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_accepts_a_valid_concurrent_git_winner() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    let calls = Cell::new(0);

    let initialized = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        if call == 3 {
            create_existing_git_repository(&destination);
        }
        Ok(())
    })
    .unwrap();

    assert!(!initialized);
    assert!(calls.get() >= 5);
    assert!(destination.join(".git").is_dir());
    assert_no_private_git_staging(&destination);
}

#[test]
fn existing_git_validation_retains_identity_across_the_final_boundary() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let displaced = temp.path().join("displaced-valid-git");
    fs::create_dir(&destination).unwrap();
    create_existing_git_repository(&destination);
    let calls = Cell::new(0);

    let error = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        if call == 2 {
            fs::rename(destination.join(".git"), &displaced).unwrap();
            fs::write(destination.join(".git"), b"foreign replacement\n").unwrap();
        }
        Ok(())
    })
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("changed concurrently"), "{message}");
    assert_eq!(
        fs::read(destination.join(".git")).unwrap(),
        b"foreign replacement\n"
    );
    assert!(displaced.join("HEAD").is_file());
}

#[test]
fn git_metadata_transfer_rejects_entries_added_after_its_sorted_snapshot() {
    let temp = tempdir().unwrap();
    let source_root = super::private_tempdir_in(temp.path(), ".source-stage-").unwrap();
    let destination_root = super::private_tempdir_in(temp.path(), ".destination-stage-").unwrap();
    let source = source_root.path().join(".git");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("z-last"), b"last").unwrap();
    fs::write(source.join("a-first"), b"first").unwrap();
    let source_commit = super::super::path::repository_directory_commit_at(&source).unwrap();

    let error = super::move_directory_contents_with(
        &source,
        destination_root.path(),
        &source_root,
        &source_commit,
        &destination_root,
        || {
            fs::write(source.join("late-entry"), b"preserve me").unwrap();
            Ok(())
        },
    )
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(
        message.contains("appeared after the transfer snapshot"),
        "{message}"
    );
    assert!(source_root.preserve_for_recovery.get());
    assert_eq!(fs::read(source.join("late-entry")).unwrap(), b"preserve me");
    assert_eq!(
        fs::read(destination_root.path().join("a-first")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(destination_root.path().join("z-last")).unwrap(),
        b"last"
    );
}

#[test]
fn invalid_existing_git_cannot_be_authorized_by_an_ambient_repository_redirect() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let external = temp.path().join("external");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&external).unwrap();
    create_existing_git_repository(&external);
    fs::write(destination.join(".git"), b"invalid local metadata\n").unwrap();

    let output = git_init_helper_command(&destination)
        .env(HELPER_EXPECT_EXISTING_GIT_PRESERVED, "usable")
        .env("GIT_DIR", external.join(".git"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "existing Git validation helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(destination.join(".git")).unwrap(),
        b"invalid local metadata\n"
    );
    assert!(external.join(".git/HEAD").is_file());
    assert_no_private_git_staging(&destination);
}

#[test]
fn existing_git_validation_ignores_ambient_global_worktree_redirection() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let external = temp.path().join("external-worktree");
    let global_config = temp.path().join("ambient.gitconfig");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&external).unwrap();
    create_existing_git_repository(&destination);
    fs::write(
        &global_config,
        format!("[core]\n\tworktree = {}\n", external.display()),
    )
    .unwrap();

    let output = git_init_helper_command(&destination)
        .env(HELPER_EXPECT_EXISTING_GIT_ACCEPTED, "1")
        .env("GIT_CONFIG_GLOBAL", &global_config)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "existing Git config-scrub helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join(".git/HEAD").is_file());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_scrubs_ambient_git_dir_in_a_subprocess() {
    run_ambient_repository_redirect_regression("GIT_DIR");
}

#[test]
fn git_init_scrubs_ambient_git_common_dir_in_a_subprocess() {
    run_ambient_repository_redirect_regression("GIT_COMMON_DIR");
}

#[test]
fn git_init_scrubs_ambient_git_object_directory_in_a_subprocess() {
    run_ambient_repository_redirect_regression("GIT_OBJECT_DIRECTORY");
}

#[test]
fn git_init_scrubs_command_scoped_config_injection_in_a_subprocess() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let injected_template = temp.path().join("injected-template");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&injected_template).unwrap();
    fs::write(
        injected_template.join("injected-template-marker"),
        b"must not be copied",
    )
    .unwrap();
    let config_file = temp.path().join("injected-config");
    fs::write(&config_file, b"[core]\n\tbare = true\n").unwrap();

    run_init_helper(
        &destination,
        [
            (OsString::from("GIT_CONFIG_COUNT"), OsString::from("2")),
            (
                OsString::from("GIT_CONFIG_KEY_0"),
                OsString::from("core.bare"),
            ),
            (OsString::from("GIT_CONFIG_VALUE_0"), OsString::from("true")),
            (
                OsString::from("GIT_CONFIG_KEY_1"),
                OsString::from("init.templateDir"),
            ),
            (
                OsString::from("GIT_CONFIG_VALUE_1"),
                injected_template.as_os_str().to_owned(),
            ),
            (
                OsString::from("GIT_CONFIG_PARAMETERS"),
                OsString::from("malformed-command-config"),
            ),
            (
                OsString::from("GIT_CONFIG"),
                config_file.as_os_str().to_owned(),
            ),
        ],
    );

    assert!(!destination.join(".git/injected-template-marker").exists());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_cleanup_failure_cannot_follow_publication() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();

    let output = git_init_helper_command(&destination)
        .env(
            HELPER_EXPECT_FAILURE,
            "injected git init staging cleanup failure",
        )
        .env("JIG_TEST_GIT_STAGING_CLOSE_FAILURE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git init cleanup helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!destination.join(".git").exists());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_preserves_a_safe_ambient_template_through_private_staging() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let template = temp.path().join("template");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&template).unwrap();
    fs::write(template.join("custom-marker"), b"safe template contents").unwrap();

    run_init_helper(
        &destination,
        [(
            OsString::from("GIT_TEMPLATE_DIR"),
            template.as_os_str().to_owned(),
        )],
    );

    assert_eq!(
        fs::read(destination.join(".git/custom-marker")).unwrap(),
        b"safe template contents"
    );
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_preserves_a_safe_template_from_global_config() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let template = temp.path().join("template");
    let global_config = temp.path().join("global-config");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&template).unwrap();
    fs::write(template.join("configured-marker"), b"safe config template").unwrap();
    let mut config_command = Command::new("git");
    super::scrub_git_repository_environment(&mut config_command);
    let status = config_command
        .args(["config", "--file"])
        .arg(&global_config)
        .arg("init.templateDir")
        .arg(&template)
        .status()
        .unwrap();
    assert!(status.success());
    let mut object_format_command = Command::new("git");
    super::scrub_git_repository_environment(&mut object_format_command);
    let status = object_format_command
        .args(["config", "--file"])
        .arg(&global_config)
        .arg("init.defaultObjectFormat")
        .arg("sha256")
        .status()
        .unwrap();
    assert!(status.success());

    run_init_helper(
        &destination,
        [(
            OsString::from("GIT_CONFIG_GLOBAL"),
            global_config.as_os_str().to_owned(),
        )],
    );

    assert_eq!(
        fs::read(destination.join(".git/configured-marker")).unwrap(),
        b"safe config template"
    );
    let mut object_format = Command::new("git");
    super::scrub_git_repository_environment(&mut object_format);
    let output = object_format
        .arg("-C")
        .arg(&destination)
        .args(["config", "--local", "--get", "extensions.objectFormat"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "ambient global config leaked into mutating init: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_no_private_git_staging(&destination);
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn git_init_preserves_a_non_utf8_template_path_from_global_config() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let template = temp
        .path()
        .join(OsString::from_vec(b"template-\xff".to_vec()));
    let global_config = temp.path().join("global-config");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&template).unwrap();
    fs::write(template.join("non-utf8-marker"), b"preserved").unwrap();
    let mut config = b"[init]\n\ttemplateDir = ".to_vec();
    config.extend_from_slice(template.as_os_str().as_bytes());
    config.push(b'\n');
    fs::write(&global_config, config).unwrap();

    run_init_helper(
        &destination,
        [
            (
                OsString::from("GIT_CONFIG_GLOBAL"),
                global_config.as_os_str().to_owned(),
            ),
            (
                OsString::from("GIT_CONFIG_SYSTEM"),
                OsString::from("/dev/null"),
            ),
        ],
    );

    assert_eq!(
        fs::read(destination.join(".git/non-utf8-marker")).unwrap(),
        b"preserved"
    );
    assert_no_private_git_staging(&destination);
}

#[cfg(unix)]
#[test]
fn configured_git_path_preserves_non_utf8_bytes() {
    use std::os::unix::ffi::OsStrExt;

    let path = super::git_path_from_bytes(b"template-\xff".to_vec());
    assert_eq!(path.as_bytes(), b"template-\xff");
}

#[cfg(unix)]
#[test]
fn git_init_rejects_a_template_symlink_before_git_can_mutate_its_target() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let template = temp.path().join("template");
    let external = temp.path().join("external-config");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&template).unwrap();
    fs::write(&external, b"external sentinel\n").unwrap();
    symlink(&external, template.join("config")).unwrap();

    run_failing_init_helper(
        &destination,
        [(
            OsString::from("GIT_TEMPLATE_DIR"),
            template.as_os_str().to_owned(),
        )],
        "symbolic link or special file",
    );

    assert_eq!(fs::read(&external).unwrap(), b"external sentinel\n");
    assert!(!destination.join(".git").exists());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_revalidates_destination_at_each_mutation_boundary() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    let calls = Cell::new(0);

    let error = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        #[cfg(unix)]
        if call > 1 {
            use std::os::unix::fs::PermissionsExt;

            let private = fs::read_dir(&destination)
                .unwrap()
                .map(|entry| entry.unwrap())
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with(".jig-git-init-") || name.starts_with(".jig-git-metadata-")
                })
                .collect::<Vec<_>>();
            assert_eq!(private.len(), 1, "expected one private staging root");
            assert_eq!(
                private[0].metadata().unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        if call == 3 {
            return Err(anyhow::anyhow!("destination changed before publication"));
        }
        Ok(())
    })
    .unwrap_err();

    assert_eq!(calls.get(), 3);
    assert!(
        error
            .to_string()
            .contains("destination validation failed before .git publication")
    );
    assert!(!destination.join(".git").exists());
    assert_no_private_git_staging(&destination);
}

#[test]
fn git_init_preserves_a_concurrently_replaced_worktree_staging_directory() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    let calls = Cell::new(0);
    let replaced = RefCell::new(None);

    let error = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        if call == 2 {
            let staging = private_staging_path(&destination, ".jig-git-init-");
            let displaced = destination.join("displaced-owned-worktree-stage");
            fs::rename(&staging, &displaced).unwrap();
            fs::create_dir(&staging).unwrap();
            fs::write(staging.join("foreign-sentinel"), b"preserve me").unwrap();
            *replaced.borrow_mut() = Some((staging, displaced));
        }
        Ok(())
    })
    .unwrap_err();

    assert_eq!(calls.get(), 2);
    assert!(format!("{error:#}").contains("was replaced concurrently"));
    let (replacement, displaced) = replaced.into_inner().unwrap();
    assert_eq!(
        fs::read(replacement.join("foreign-sentinel")).unwrap(),
        b"preserve me"
    );
    assert!(displaced.join(".git").is_dir());
    assert!(!destination.join(".git").exists());
}

#[test]
fn git_init_preserves_a_concurrently_replaced_staged_git_child() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    let calls = Cell::new(0);
    let replaced = RefCell::new(None);

    let error = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        if call == 2 {
            let staging = private_staging_path(&destination, ".jig-git-init-");
            let staged_git = staging.join(".git");
            let displaced = destination.join("displaced-owned-git-metadata");
            fs::rename(&staged_git, &displaced).unwrap();
            fs::create_dir(&staged_git).unwrap();
            fs::write(staged_git.join("foreign-sentinel"), b"preserve me").unwrap();
            *replaced.borrow_mut() = Some((staging, displaced));
        }
        Ok(())
    })
    .unwrap_err();

    assert_eq!(calls.get(), 2);
    assert!(format!("{error:#}").contains("metadata directory"));
    assert!(format!("{error:#}").contains("preserving the staging tree"));
    let (staging, displaced) = replaced.into_inner().unwrap();
    assert_eq!(
        fs::read(staging.join(".git/foreign-sentinel")).unwrap(),
        b"preserve me"
    );
    assert!(displaced.join("HEAD").is_file());
    assert!(!destination.join(".git").exists());
}

#[test]
fn git_init_preserves_a_concurrently_replaced_metadata_staging_directory() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    fs::create_dir(&destination).unwrap();
    let calls = Cell::new(0);
    let replaced = RefCell::new(None);

    let error = init_git_repo_with_validation(&destination, "main", || {
        let call = calls.get() + 1;
        calls.set(call);
        if call == 3 {
            let staging = private_staging_path(&destination, ".jig-git-metadata-");
            let displaced = destination.join("displaced-owned-metadata-stage");
            fs::rename(&staging, &displaced).unwrap();
            fs::create_dir(&staging).unwrap();
            fs::write(staging.join("foreign-sentinel"), b"preserve me").unwrap();
            *replaced.borrow_mut() = Some((staging, displaced));
        }
        Ok(())
    })
    .unwrap_err();

    assert_eq!(calls.get(), 3);
    assert!(format!("{error:#}").contains("was replaced concurrently"));
    let (replacement, displaced) = replaced.into_inner().unwrap();
    assert_eq!(
        fs::read(replacement.join("foreign-sentinel")).unwrap(),
        b"preserve me"
    );
    assert!(displaced.join("HEAD").is_file());
    assert!(!destination.join(".git").exists());
}

fn run_ambient_repository_redirect_regression(variable: &str) {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repository");
    let external = temp.path().join("external");
    fs::create_dir(&destination).unwrap();
    fs::create_dir(&external).unwrap();
    fs::write(external.join("sentinel"), b"unchanged").unwrap();

    run_init_helper(
        &destination,
        [(OsString::from(variable), external.as_os_str().to_owned())],
    );

    assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"unchanged");
    let entries = fs::read_dir(&external)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, [OsString::from("sentinel")]);
    assert_no_private_git_staging(&destination);
}

fn create_existing_git_repository(destination: &Path) {
    let mut command = Command::new("git");
    super::scrub_git_repository_environment(&mut command);
    let output = command
        .current_dir(destination)
        .env("GIT_CONFIG_GLOBAL", super::null_git_config_path())
        .env("GIT_CONFIG_SYSTEM", super::null_git_config_path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "init.templateDir=", "init"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to prepare existing Git repository\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_existing_gitfile_repository(destination: &Path, metadata: &Path) {
    let mut command = Command::new("git");
    super::scrub_git_repository_environment(&mut command);
    let output = command
        .current_dir(destination.parent().unwrap())
        .env("GIT_CONFIG_GLOBAL", super::null_git_config_path())
        .env("GIT_CONFIG_SYSTEM", super::null_git_config_path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "init.templateDir=", "init", "--separate-git-dir"])
        .arg(metadata)
        .arg(destination)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to prepare existing Git gitfile repository\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_init_helper<const N: usize>(destination: &Path, environment: [(OsString, OsString); N]) {
    let output = git_init_helper_command(destination)
        .envs(environment)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git init helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join(".git").is_dir());
}

#[cfg(unix)]
fn run_failing_init_helper<const N: usize>(
    destination: &Path,
    environment: [(OsString, OsString); N],
    expected: &str,
) {
    let output = git_init_helper_command(destination)
        .env(HELPER_EXPECT_FAILURE, expected)
        .envs(environment)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failing git init helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_init_helper_command(destination: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", HELPER_TEST, "--nocapture"])
        .env_remove(GIT_BIN_ENV)
        .env_remove("GIT_TEMPLATE_DIR")
        .env_remove("GIT_CONFIG_GLOBAL")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env_remove("GIT_CONFIG_NOSYSTEM")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove(HELPER_EXPECT_EXISTING_GIT_PRESERVED)
        .env_remove(HELPER_EXPECT_FAILURE)
        .env(HELPER_DESTINATION, destination);
    command
}

fn assert_no_private_git_staging(destination: &Path) {
    let private = fs::read_dir(destination)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| {
            let name = name.to_string_lossy();
            name.starts_with(".jig-git-init-") || name.starts_with(".jig-git-metadata-")
        })
        .collect::<Vec<_>>();
    assert!(private.is_empty(), "private staging leaked: {private:?}");
}

fn private_staging_path(destination: &Path, prefix: &str) -> PathBuf {
    let matches = fs::read_dir(destination)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(prefix))
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "expected one {prefix} staging root");
    matches.into_iter().next().unwrap()
}
