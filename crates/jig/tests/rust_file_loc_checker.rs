// agentic-loc-exception: checker behavior is characterized end to end across Git modes, thresholds, roots, and path edge cases.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use minijinja::{Environment, UndefinedBehavior, syntax::SyntaxConfig};
use serde_json::json;
use tempfile::{TempDir, tempdir};

const CHECKER_TEMPLATE: &str =
    include_str!("../../../templates/project/scripts/check-rust-file-loc.sh.jinja");
const EMBEDDED_CHECKER_TEMPLATE: &str = include_str!(
    "../src/bootstrap/embedded_template_snapshots/scripts/check-rust-file-loc.sh.jinja"
);
const SOURCE_CHECKER: &str = include_str!("../../../scripts/check-rust-file-loc.sh");

struct Fixture {
    root: TempDir,
    checker: PathBuf,
}

impl Fixture {
    fn new(roots: &[&str]) -> Self {
        let root = tempdir().unwrap();
        git(root.path(), &["init", "--initial-branch=main"]);
        git(
            root.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        git(root.path(), &["config", "user.name", "Fixture"]);
        let checker = root.path().join("check-rust-file-loc.sh");
        fs::write(&checker, render_checker(roots)).unwrap();
        Self { root, checker }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn write_lines(&self, relative: &str, count: usize) {
        self.write_contents(relative, &physical_lines(count, "\n", true));
    }

    fn write_contents(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn commit_all(&self, message: &str) -> String {
        git(self.path(), &["add", "."]);
        git(self.path(), &["commit", "-m", message]);
        git_output(self.path(), &["rev-parse", "HEAD"])
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_shell(Path::new("bash"), args)
    }

    fn run_with_shell(&self, shell: &Path, args: &[&str]) -> Output {
        let mut command = Command::new(shell);
        command
            .arg(&self.checker)
            .args(args)
            .current_dir(self.path())
            .env_remove("JIG_DEFAULT_BRANCH");
        isolate_git_config(&mut command);
        command.output().unwrap()
    }

    fn run_with_xpg_echo(&self, args: &[&str]) -> Output {
        let mut command = Command::new("bash");
        command
            .args(["-O", "xpg_echo"])
            .arg(&self.checker)
            .args(args)
            .current_dir(self.path())
            .env_remove("JIG_DEFAULT_BRANCH");
        isolate_git_config(&mut command);
        command.output().unwrap()
    }
}

fn isolate_git_config(command: &mut Command) {
    command
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

fn installed_bash_3_2() -> Option<PathBuf> {
    let mut candidates = std::env::var_os("JIG_BASH_3_2")
        .map(PathBuf::from)
        .into_iter()
        .collect::<Vec<_>>();
    candidates.extend(
        [
            "bash3.2",
            "bash-3.2",
            "/usr/local/bin/bash",
            "/opt/homebrew/bin/bash",
            "/bin/bash",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    candidates.into_iter().find(|candidate| {
        Command::new(candidate)
            .arg("--version")
            .output()
            .ok()
            .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains("version 3.2"))
    })
}

fn render_checker(roots: &[&str]) -> String {
    let mut environment = Environment::new();
    environment.set_syntax(
        SyntaxConfig::builder()
            .block_delimiters("[%", "%]")
            .variable_delimiters("<<[", "]>>")
            .comment_delimiters("<#", "#>")
            .build()
            .unwrap(),
    );
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment
        .render_str(
            CHECKER_TEMPLATE,
            json!({
                "rust_crate_roots": roots,
                "rust_crate_root_shell_args": roots
                    .iter()
                    .map(|root| shell_quote(root))
                    .collect::<Vec<_>>()
            }),
        )
        .unwrap()
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '.' | '_' | '-' | ':')
        })
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn physical_lines(count: usize, ending: &str, terminated: bool) -> String {
    if count == 0 {
        return String::new();
    }
    let mut contents = (0..count)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join(ending);
    if terminated {
        contents.push_str(ending);
    }
    contents
}

fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    isolate_git_config(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    isolate_git_config(&mut command);
    let output = command.output().unwrap();
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_failed_for(output: &Output, expected: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
    assert!(
        stderr(output).contains(expected),
        "stdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
}

#[test]
fn managed_and_embedded_checker_templates_are_identical_and_portable() {
    assert_eq!(CHECKER_TEMPLATE, EMBEDDED_CHECKER_TEMPLATE);
    assert_eq!(
        render_checker(&["crates"]),
        SOURCE_CHECKER.strip_suffix('\n').unwrap_or(SOURCE_CHECKER)
    );
    for forbidden in [
        "declare -A",
        "mapfile",
        "readarray",
        "rename_old",
        "rename_new",
    ] {
        assert!(!CHECKER_TEMPLATE.contains(forbidden));
    }

    let fixture = Fixture::new(&[]);
    fixture.write_lines("source/example.rs", 401);
    fixture.commit_all("fixture");
    let syntax = Command::new("bash")
        .args(["-n", fixture.checker.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(syntax.status.success(), "{}", stderr(&syntax));

    let output = fixture.run(&["--all"]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains("source/example.rs is 401 LOC"));

    if let Some(bash) = installed_bash_3_2() {
        let output = fixture.run_with_shell(&bash, &["--all"]);
        assert!(
            output.status.success(),
            "{} failed\nstdout: {}\nstderr: {}",
            bash.display(),
            stdout(&output),
            stderr(&output)
        );
    } else {
        eprintln!(
            "real Bash 3.2 is unavailable; syntax guards and ordinary Bash coverage remain active"
        );
    }
}

#[test]
fn invocation_modes_are_exclusive_and_missing_refs_fail_closed() {
    let fixture = Fixture::new(&["src"]);
    for args in [
        &[][..],
        &["--changed-against"][..],
        &["--staged", "--all"][..],
        &["--all", "extra"][..],
    ] {
        let output = fixture.run(args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "args: {args:?}\nstderr: {}",
            stderr(&output)
        );
        assert!(stderr(&output).contains("Usage:"));
    }

    let output = fixture.run(&["--changed-against", "missing-ref"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("comparison ref does not resolve to a tree"));

    let unmatched_root = Fixture::new(&["missing-rust-root"]);
    unmatched_root.write_lines("source/example.rs", 1);
    unmatched_root.commit_all("fixture");
    let output = unmatched_root.run(&["--all"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("configured Rust roots match no tracked files"));

    let rust_empty = Fixture::new(&["future-crates"]);
    rust_empty.write_contents("README.md", "# Example\n");
    rust_empty.commit_all("fixture");
    let output = rust_empty.run(&["--all"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("Rust LOC policy passed."));
}

#[test]
fn configured_roots_are_literal_git_pathspecs() {
    let fixture = Fixture::new(&[":(exclude)crates"]);
    fixture.write_lines(":(exclude)crates/src/checked.rs", 1);
    fixture.write_lines("other/src/oversized.rs", 801);
    fixture.commit_all("fixture");

    let output = fixture.run(&["--all"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("Rust LOC policy passed."));
    assert!(!stderr(&output).contains("other/src/oversized.rs"));
}

#[test]
fn changed_staged_all_and_default_branch_modes_enforce_candidates() {
    let changed = Fixture::new(&["src"]);
    changed.write_lines("src/base.rs", 1);
    let base = changed.commit_all("base");
    git(
        changed.path(),
        &["update-ref", "refs/remotes/origin/main", &base],
    );
    changed.write_lines("src/changed.rs", 801);
    changed.commit_all("changed");

    let explicit = changed.run(&["--changed-against", &base]);
    assert_failed_for(&explicit, "src/changed.rs is 801 LOC");
    let default_branch = changed.run(&["main"]);
    assert_failed_for(&default_branch, "src/changed.rs is 801 LOC");
    assert!(stdout(&default_branch).contains("Using Rust LOC base ref:"));

    let working_tree = Fixture::new(&["src"]);
    working_tree.write_lines("src/working.rs", 1);
    let working_base = working_tree.commit_all("base");
    working_tree.write_lines("src/working.rs", 801);
    assert_failed_for(
        &working_tree.run(&["--changed-against", &working_base]),
        "src/working.rs is 801 LOC",
    );

    let staged = Fixture::new(&["src"]);
    staged.write_lines("src/base.rs", 1);
    staged.commit_all("base");
    staged.write_lines("src/staged.rs", 801);
    git(staged.path(), &["add", "src/staged.rs"]);
    staged.write_lines("src/staged.rs", 1);
    assert_failed_for(&staged.run(&["--staged"]), "src/staged.rs is 801 LOC");

    let staged_without_head = Fixture::new(&["src"]);
    staged_without_head.write_lines("src/initial.rs", 801);
    git(staged_without_head.path(), &["add", "src/initial.rs"]);
    assert_failed_for(
        &staged_without_head.run(&["--staged"]),
        "src/initial.rs is 801 LOC",
    );

    let staged_rename = Fixture::new(&["src"]);
    staged_rename.write_lines("src/legacy.rs", 900);
    staged_rename.write_lines("src/absolute.rs", 1001);
    staged_rename.commit_all("base");
    git(
        staged_rename.path(),
        &["mv", "src/legacy.rs", "src/renamed.rs"],
    );
    git(
        staged_rename.path(),
        &["mv", "src/absolute.rs", "src/renamed-absolute.rs"],
    );
    let staged_rename_output = staged_rename.run(&["--staged"]);
    assert!(staged_rename_output.status.success());
    assert!(
        stdout(&staged_rename_output).contains(
            "src/renamed.rs remains above the hard limit at 900 LOC but did not increase"
        )
    );
    assert!(stdout(&staged_rename_output).contains(
        "src/renamed-absolute.rs remains above the absolute max at 1001 LOC but did not increase"
    ));

    let all = Fixture::new(&["src"]);
    all.write_lines("src/all.rs", 801);
    git(all.path(), &["add", "."]);
    assert_failed_for(&all.run(&["--all"]), "src/all.rs is 801 LOC");
}

#[test]
fn default_branch_mode_preserves_local_parent_and_empty_tree_fallbacks() {
    let local_parent = Fixture::new(&["src"]);
    local_parent.write_lines("src/base.rs", 1);
    local_parent.commit_all("base");
    local_parent.write_lines("src/changed.rs", 801);
    local_parent.commit_all("changed");
    assert_failed_for(&local_parent.run(&["main"]), "src/changed.rs is 801 LOC");
    let base_tree = git_output(local_parent.path(), &["rev-parse", "HEAD^^{tree}"]);
    assert_failed_for(
        &local_parent.run(&["--changed-against", &base_tree]),
        "src/changed.rs is 801 LOC",
    );

    let empty_tree = Fixture::new(&["src"]);
    empty_tree.write_lines("src/initial.rs", 801);
    empty_tree.commit_all("initial");
    assert_failed_for(&empty_tree.run(&["main"]), "src/initial.rs is 801 LOC");

    let invalid = empty_tree.run(&["bad branch"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(stderr(&invalid).contains("Invalid default branch name"));
}

#[test]
fn exact_policy_bands_and_physical_line_boundaries_are_preserved() {
    let fixture = Fixture::new(&["src"]);
    let counts = [400, 401, 500, 501, 600, 601, 800, 801, 1000, 1001];
    for count in counts {
        fixture.write_lines(&format!("src/lines_{count}.rs"), count);
        fixture.write_contents(
            &format!("src/crlf_{count}.rs"),
            &physical_lines(count, "\r\n", true),
        );
        fixture.write_contents(
            &format!("src/unterminated_{count}.rs"),
            &physical_lines(count, "\n", false),
        );
    }
    fixture.write_contents("src/empty.rs", "");
    fixture.write_contents("src/crlf_empty.rs", "");
    git(fixture.path(), &["add", "."]);

    let output = fixture.run(&["--all"]);
    assert_eq!(output.status.code(), Some(1));
    let out = stdout(&output);
    let err = stderr(&output);
    for absent in [
        "lines_400.rs",
        "crlf_400.rs",
        "unterminated_400.rs",
        "empty.rs",
        "crlf_empty.rs",
    ] {
        assert!(
            !out.contains(absent) && !err.contains(absent),
            "{absent} unexpectedly reported"
        );
    }
    for prefix in ["lines", "crlf", "unterminated"] {
        for expected in [
            format!("{prefix}_401.rs is 401 LOC and is approaching the soft limit"),
            format!("{prefix}_500.rs is 500 LOC and is approaching the soft limit"),
            format!("{prefix}_501.rs is 501 LOC and is above the soft limit"),
            format!("{prefix}_600.rs is 600 LOC and is above the soft limit"),
            format!("{prefix}_601.rs is 601 LOC and is approaching the hard limit"),
            format!("{prefix}_800.rs is 800 LOC and is approaching the hard limit"),
        ] {
            assert!(out.contains(&expected), "missing {expected:?} in {out}");
        }
    }
    for prefix in ["lines", "crlf", "unterminated"] {
        for expected in [
            format!("{prefix}_801.rs is 801 LOC, above the hard limit of 800"),
            format!("{prefix}_1000.rs is 1000 LOC, above the hard limit of 800"),
            format!("{prefix}_1001.rs is 1001 LOC, above the absolute max of 1000"),
        ] {
            assert!(err.contains(&expected), "missing {expected:?} in {err}");
        }
    }
}

#[test]
fn first_forty_line_exception_and_generated_markers_preserve_hard_limit_behavior() {
    let fixture = Fixture::new(&["src"]);
    for (name, marker_line, marker) in [
        ("exception_first.rs", 1, "// agentic-loc-exception: fixture"),
        (
            "exception_fortieth.rs",
            40,
            "// agentic-loc-exception: fixture",
        ),
        ("generated.rs", 1, "// @generated"),
        ("exception_late.rs", 41, "// agentic-loc-exception: fixture"),
        (
            "absolute_with_exception.rs",
            1,
            "// agentic-loc-exception: does not waive the absolute max",
        ),
    ] {
        let count = if name == "absolute_with_exception.rs" {
            1001
        } else {
            801
        };
        let mut lines = (1..=count)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>();
        lines[marker_line - 1] = marker.into();
        fixture.write_contents(&format!("src/{name}"), &(lines.join("\n") + "\n"));
    }
    git(fixture.path(), &["add", "."]);

    let output = fixture.run(&["--all"]);
    assert_eq!(output.status.code(), Some(1));
    for expected in [
        "exception_first.rs",
        "exception_fortieth.rs",
        "generated.rs",
    ] {
        assert!(stdout(&output).contains(&format!(
            "{expected} is 801 LOC and uses an explicit exception"
        )));
    }
    assert!(stderr(&output).contains("exception_late.rs is 801 LOC, above the hard limit"));
    assert!(
        stderr(&output).contains("absolute_with_exception.rs is 1001 LOC, above the absolute max")
    );
}

#[test]
fn legacy_non_growth_and_rename_baselines_are_preserved() {
    let fixture = Fixture::new(&["src"]);
    fixture.write_lines("src/legacy.rs", 900);
    fixture.write_lines("src/absolute.rs", 1001);
    fixture.write_lines("src/same-path.rs", 900);
    let base = fixture.commit_all("base");
    git(
        fixture.path(),
        &["mv", "src/legacy.rs", "src/renamed legacy.rs"],
    );
    git(
        fixture.path(),
        &["mv", "src/absolute.rs", "src/renamed-absolute.rs"],
    );
    fixture.write_contents(
        "src/same-path.rs",
        &format!("changed first line\n{}", physical_lines(899, "\n", true)),
    );
    fixture.commit_all("rename");
    git(fixture.path(), &["config", "diff.renames", "false"]);

    let unchanged = fixture.run(&["--changed-against", &base]);
    assert!(
        unchanged.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&unchanged),
        stderr(&unchanged)
    );
    assert!(stdout(&unchanged).contains(
        "renamed legacy.rs remains above the hard limit at 900 LOC but did not increase"
    ));
    assert!(stdout(&unchanged).contains(
        "renamed-absolute.rs remains above the absolute max at 1001 LOC but did not increase"
    ));
    assert!(
        stdout(&unchanged)
            .contains("same-path.rs remains above the hard limit at 900 LOC but did not increase")
    );

    fixture.write_lines("src/renamed legacy.rs", 850);
    let decreased = fixture.run(&["--changed-against", &base]);
    assert!(decreased.status.success());
    assert!(stdout(&decreased).contains(
        "renamed legacy.rs remains above the hard limit at 850 LOC but did not increase"
    ));

    fixture.write_lines("src/renamed legacy.rs", 901);
    fixture.write_lines("src/renamed-absolute.rs", 1002);
    let grown = fixture.run(&["--changed-against", &base]);
    assert_failed_for(&grown, "renamed legacy.rs is 901 LOC, above the hard limit");
    assert!(stderr(&grown).contains("renamed-absolute.rs is 1002 LOC, above the absolute max"));
}

#[test]
fn deletions_are_ignored_and_new_oversized_files_fail() {
    let fixture = Fixture::new(&["src"]);
    fixture.write_lines("src/deleted.rs", 1001);
    let base = fixture.commit_all("base");
    fs::remove_file(fixture.path().join("src/deleted.rs")).unwrap();
    fixture.commit_all("delete");
    let deleted = fixture.run(&["--changed-against", &base]);
    assert!(
        deleted.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&deleted),
        stderr(&deleted)
    );
    assert!(!stdout(&deleted).contains("deleted.rs"));

    fixture.write_contents(
        "src/new.rs",
        &(0..801)
            .map(|index| format!("brand new source {index}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    fixture.commit_all("new");
    assert_failed_for(
        &fixture.run(&["--changed-against", &base]),
        "src/new.rs is 801 LOC",
    );
}

#[test]
fn copied_files_remain_candidates_under_ambient_copy_detection() {
    let fixture = Fixture::new(&["src"]);
    fixture.write_lines("src/original.rs", 900);
    let base = fixture.commit_all("base");
    fs::copy(
        fixture.path().join("src/original.rs"),
        fixture.path().join("src/copied.rs"),
    )
    .unwrap();
    git(fixture.path(), &["config", "diff.renames", "copies"]);
    fixture.commit_all("copy");

    assert_failed_for(
        &fixture.run(&["--changed-against", &base]),
        "src/copied.rs is 900 LOC, above the hard limit",
    );
}

#[test]
fn bulk_renames_are_normalized_in_one_pass() {
    let fixture = Fixture::new(&["src"]);
    for index in 0..64 {
        fixture.write_lines(&format!("src/legacy_{index}.rs"), 801);
    }
    let base = fixture.commit_all("base");
    for index in 0..64 {
        git(
            fixture.path(),
            &[
                "mv",
                &format!("src/legacy_{index}.rs"),
                &format!("src/renamed_{index}.rs"),
            ],
        );
    }
    fixture.commit_all("bulk rename");

    let output = fixture.run(&["--changed-against", &base]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert_eq!(
        stdout(&output)
            .matches("remains above the hard limit at 801 LOC")
            .count(),
        64
    );
}

#[test]
fn configured_roots_and_nul_delimited_unusual_paths_are_preserved() {
    let scoped = Fixture::new(&["src one", "src team's"]);
    scoped.write_lines("src one/inside.rs", 401);
    scoped.write_lines("src team's/quoted.rs", 401);
    scoped.write_lines("outside/ignored.rs", 1001);
    scoped.commit_all("fixture");
    let output = scoped.run(&["--all"]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stdout(&output).contains("src one/inside.rs is 401 LOC"));
    assert!(stdout(&output).contains("src team's/quoted.rs is 401 LOC"));
    assert!(!stdout(&output).contains("outside/ignored.rs"));

    let unscoped = Fixture::new(&[]);
    let unusual = "source/with space\nand newline.rs";
    unscoped.write_lines(unusual, 900);
    let unusual_base = unscoped.commit_all("fixture");
    let renamed = "source/renamed space\nand newline.rs";
    git(unscoped.path(), &["mv", unusual, renamed]);
    unscoped.commit_all("rename");
    let rename_output = unscoped.run(&["--changed-against", &unusual_base]);
    assert!(rename_output.status.success());
    assert!(stdout(&rename_output).contains("remains above the hard limit at 900 LOC"));
    if let Some(bash) = installed_bash_3_2() {
        let output = unscoped.run_with_shell(&bash, &["--changed-against", &unusual_base]);
        assert!(output.status.success());
        assert!(stdout(&output).contains("renamed space\\nand newline.rs"));
    }

    unscoped.write_lines(renamed, 901);
    let escaped = "source/escape\u{1b}[31m.rs";
    unscoped.write_lines(escaped, 801);
    unscoped.commit_all("control-byte path");
    let output = unscoped.run_with_xpg_echo(&["--all"]);
    assert_failed_for(&output, "is 901 LOC, above the hard limit");
    assert!(stderr(&output).contains("renamed space\\nand newline.rs"));
    assert!(stderr(&output).contains("escape\\x1b[31m.rs"));
    assert!(!stderr(&output).contains('\u{1b}'));

    let dot = Fixture::new(&["."]);
    dot.write_lines("anywhere/example.rs", 401);
    dot.commit_all("fixture");
    let output = dot.run(&["--all"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("anywhere/example.rs is 401 LOC"));
}
