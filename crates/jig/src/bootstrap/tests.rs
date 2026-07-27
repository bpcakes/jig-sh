use tempfile::{TempDir, tempdir};

use super::path;
use super::*;
use crate::test_env::{EnvVarGuard, lock_env};

fn template_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    copy_dir_recursive_inner(source, destination, Path::new(""));
}

fn copy_dir_recursive_inner(source: &Path, destination: &Path, relative: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let entry_name = entry.file_name();
        let entry_relative = relative.join(&entry_name);
        if skip_template_fixture_path(&entry_relative) {
            continue;
        }

        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().unwrap();

        if file_type.is_dir() {
            copy_dir_recursive_inner(&source_path, &destination_path, &entry_relative);
            continue;
        }

        if file_type.is_symlink() {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let target = fs::read_link(&source_path).unwrap();
            create_symlink(&target, &destination_path).unwrap();
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::copy(&source_path, &destination_path).unwrap();
    }
}

fn skip_template_fixture_path(relative: &Path) -> bool {
    matches!(
        relative.to_str(),
        Some(".git")
            | Some("target")
            | Some(".agent/.cache")
            | Some(".agent/plans")
            | Some(".agent/state")
    )
}

fn materialize_template_worktree() -> TempDir {
    let temp = tempdir().unwrap();
    copy_dir_recursive(
        &template_repo_root().join("templates"),
        &temp.path().join("templates"),
    );
    temp
}

fn materialize_template_git_worktree() -> TempDir {
    let temp = materialize_template_worktree();
    init_git_repo_for_test(temp.path());
    git(temp.path(), ["add", "."]).unwrap();
    git(temp.path(), ["commit", "-m", "template"]).unwrap();
    temp
}

fn init_git_repo_for_test(path: &Path) {
    git(path, ["init", "-b", "main"]).unwrap();
    git(path, ["config", "user.name", "Fixture"]).unwrap();
    git(path, ["config", "user.email", "fixture@example.com"]).unwrap();
}

fn write_test_crate_guide(repo: &Path) {
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
}

fn with_test_build_template_pin_policy<T>(
    policy: BuildTemplatePinPolicy,
    run: impl FnOnce() -> T,
) -> T {
    struct Guard(Option<BuildTemplatePinPolicy>);

    impl Drop for Guard {
        fn drop(&mut self) {
            TEST_BUILD_TEMPLATE_PIN_POLICY.with(|slot| slot.set(self.0));
        }
    }

    let previous = TEST_BUILD_TEMPLATE_PIN_POLICY.with(|slot| {
        let previous = slot.get();
        slot.set(Some(policy));
        previous
    });
    let _guard = Guard(previous);
    run()
}

fn adopt_repo_for_test(repo: &Path, template: &Path, template_mode: TemplateMode) {
    run_adopt(AdoptOpts {
        path: repo.to_path_buf(),
        template: Some(template.display().to_string()),
        template_mode: Some(template_mode),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
}

fn commit_template_root_guide(template: &Path, contents: &str, message: &str) -> String {
    fs::write(
        template.join("templates/project/AGENTS.md.jinja"),
        format!(
            "# Repository Guidelines\n\n<!-- BEGIN JIG MANAGED BLOCK -->\n{}<!-- END JIG MANAGED BLOCK -->\n",
            contents
        ),
    )
    .unwrap();
    git(template, ["add", "templates/project/AGENTS.md.jinja"]).unwrap();
    git(template, ["commit", "-m", message]).unwrap();
    git_stdout(template, ["rev-parse", "HEAD"]).unwrap()
}

fn push_template_main(template: &Path, remote_url: &str) {
    git(template, ["push", remote_url, "HEAD:refs/heads/main"]).unwrap();
}

struct NormalizedRemoteCommittedFixture {
    _root: TempDir,
    repo: PathBuf,
    template: TempDir,
    remote_url: String,
    answers_path: PathBuf,
}

impl NormalizedRemoteCommittedFixture {
    fn new(legacy_committed_state: bool) -> Self {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let remote = root.path().join("template-remote.git");
        let template = materialize_template_git_worktree();
        let remote_url = format!("file://{}", remote.display());

        write_test_crate_guide(&repo);
        // Build the fixture remote through Git's normal receive path. A local
        // `clone --bare --no-hardlinks` intermittently loses its temporary pack
        // index under parallel test load, even though these repositories are
        // independent.
        git(
            template.path(),
            [
                "init",
                "--bare",
                "--initial-branch=main",
                &remote.display().to_string(),
            ],
        )
        .unwrap();
        push_template_main(template.path(), &remote_url);

        adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        let answers_path = repo.join(".jig.toml");
        let mut answers = read_answers_toml(&answers_path).unwrap();
        answers.insert("_src_path".into(), TomlValue::String(remote_url.clone()));
        if legacy_committed_state {
            answers.remove(TEMPLATE_LOCAL_PATH_KEY);
        }
        write_answers_toml(&answers_path, &answers).unwrap();
        git(&repo, ["add", ".jig.toml"]).unwrap();
        git(&repo, ["commit", "-m", "normalize source"]).unwrap();

        Self {
            _root: root,
            repo,
            template,
            remote_url,
            answers_path,
        }
    }
}

mod basic;
mod committed;
mod frontend_adoption;
mod status_provider;
mod template_mode;
mod template_source;
mod windows_dependency_checker;
