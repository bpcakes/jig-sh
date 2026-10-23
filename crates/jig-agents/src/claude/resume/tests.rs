use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::claude::CONFIG_DIR_ENV;
use crate::test_env::{EnvVarGuard, lock_env};

const SESSION_ID: &str = "2f4c6e8a-1b3d-4f5a-8c7e-9d0b1a2c3e4f";

/// Discovered homes under `root`, with the native default current.
fn homes(root: &Path, names: &[&str]) -> Homes {
    for name in names {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    Homes {
        paths: names.iter().map(|name| root.join(name)).collect(),
        current: root.join(".claude"),
        warnings: Vec::new(),
        default: root.join(".claude"),
        current_is_default: true,
        complete: true,
    }
}

fn write_transcript(home: &Path, project: &str) {
    let project = home.join("projects").join(project);
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join(format!("{SESSION_ID}.jsonl")), "{}\n").unwrap();
}

fn lookup(homes: &Homes) -> Result<Home> {
    resolve_resume_home(homes, SESSION_ID, &|| false, &mut |_, _| {})
}

#[test]
fn session_ids_are_validated_and_normalized() {
    assert_eq!(
        normalize_session_id(&SESSION_ID.to_ascii_uppercase()).unwrap(),
        SESSION_ID
    );
    let error = normalize_session_id("not-a-session\u{1b}[2J")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Invalid Claude session ID"), "{error}");
    assert!(!error.contains('\u{1b}'), "{error}");
}

#[test]
fn lookup_finds_the_home_whose_projects_hold_the_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let homes = homes(temp.path(), &[".claude", ".claude-work"]);
    write_transcript(&temp.path().join(".claude-work"), "-tmp-ExampleProject");
    // Neither a subagent directory named for the session, a stray file, nor a
    // directory named like the transcript is a session.
    let projects = temp.path().join(".claude/projects");
    fs::create_dir_all(
        projects
            .join("-tmp-Other")
            .join(SESSION_ID)
            .join("subagents"),
    )
    .unwrap();
    fs::create_dir_all(
        projects
            .join("-tmp-Dir")
            .join(format!("{SESSION_ID}.jsonl")),
    )
    .unwrap();
    fs::write(projects.join("stray-file"), "").unwrap();

    let mut progress = Vec::new();
    let home = resolve_resume_home(&homes, SESSION_ID, &|| false, &mut |done, total| {
        progress.push((done, total))
    })
    .unwrap();

    assert_eq!(
        home.path,
        temp.path().join(".claude-work").canonicalize().unwrap()
    );
    assert!(!home.default_config);
    assert_eq!(progress, [(0, 2), (1, 2), (2, 2)]);
}

#[test]
fn homes_without_a_projects_directory_hold_no_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let homes = homes(temp.path(), &[".claude", ".claude-work"]);
    fs::write(temp.path().join(".claude-work/projects"), "").unwrap();

    let error = lookup(&homes).unwrap_err().to_string();

    assert!(
        error.contains(&format!("Claude session '{SESSION_ID}' was not found")),
        "{error}"
    );
    assert!(error.contains(".claude-work"), "{error}");
    assert!(error.contains("Pass --home HOME"), "{error}");
}

#[test]
fn default_directory_resumes_in_its_current_configuration_mode() {
    let temp = tempfile::tempdir().unwrap();
    let mut homes = homes(temp.path(), &[".claude", ".claude-work"]);
    write_transcript(&temp.path().join(".claude"), "-tmp-ExampleProject");
    let default = temp.path().join(".claude").canonicalize().unwrap();

    let native = lookup(&homes).unwrap();
    assert!(native.default_config);
    assert_eq!(native.path, default);

    homes.current = temp.path().join(".claude-work");
    homes.current_is_default = false;
    assert!(lookup(&homes).unwrap().default_config);

    homes.current = temp.path().join(".claude");
    let explicit = lookup(&homes).unwrap();
    assert!(!explicit.default_config);
    assert_eq!(explicit.path, default);
}

#[test]
fn selection_fails_closed_when_uniqueness_cannot_be_proven() {
    let temp = tempfile::tempdir().unwrap();
    let mut homes = homes(temp.path(), &[".claude", ".claude-work"]);

    let error = select_resume_home(&homes, SESSION_ID, vec![Probe::Found, Probe::Found])
        .unwrap_err()
        .to_string();
    assert!(error.contains("exists in multiple homes"), "{error}");
    assert!(error.contains("pass --home HOME"), "{error}");

    let error = select_resume_home(
        &homes,
        SESSION_ID,
        vec![
            Probe::Found,
            Probe::Failed("permission denied\u{1b}[2J".into()),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("uniqueness could not be confirmed"),
        "{error}"
    );
    assert!(
        error.contains("some discovered homes could not be inspected"),
        "{error}"
    );
    assert!(error.contains("permission denied"), "{error}");
    assert!(!error.contains('\u{1b}'), "{error}");
    assert!(
        error.contains("Pass --home HOME to resume the confirmed home explicitly"),
        "{error}"
    );

    let error = select_resume_home(
        &homes,
        SESSION_ID,
        vec![Probe::Missing, Probe::Failed("permission denied".into())],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("lookup coverage was incomplete"), "{error}");
    assert!(!error.contains("was not found"), "{error}");

    homes.complete = false;
    homes.warnings = vec!["Failed to discover Claude homes: denied".into()];
    let error = select_resume_home(&homes, SESSION_ID, vec![Probe::Found, Probe::Missing])
        .unwrap_err()
        .to_string();
    assert!(error.contains("home discovery was incomplete"), "{error}");
    assert!(!error.contains("could not be inspected"), "{error}");
    assert!(
        error.contains("discovery: Failed to discover Claude homes"),
        "{error}"
    );

    let error = select_resume_home(&homes, SESSION_ID, vec![Probe::Missing, Probe::Missing])
        .unwrap_err()
        .to_string();
    assert!(error.contains("lookup coverage was incomplete"), "{error}");
}

#[cfg(unix)]
#[test]
fn unreadable_projects_leave_a_missing_session_unresolved() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let homes = homes(temp.path(), &[".claude", ".claude-work"]);
    let locked = temp.path().join(".claude-work/projects/-tmp-Locked");
    fs::create_dir_all(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let denied = fs::metadata(locked.join("probe"))
        .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied);
    let result = lookup(&homes);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if !denied {
        // Privileged runners bypass directory permissions.
        return;
    }

    let error = result.unwrap_err().to_string();
    assert!(error.contains("lookup coverage was incomplete"), "{error}");
    assert!(error.contains("-tmp-Locked"), "{error}");
}

#[test]
fn cancelled_lookup_stops_without_suggesting_a_home() {
    let temp = tempfile::tempdir().unwrap();
    let homes = homes(temp.path(), &[".claude", ".claude-work"]);
    write_transcript(&temp.path().join(".claude"), "-tmp-ExampleProject");
    write_transcript(&temp.path().join(".claude-work"), "-tmp-ExampleProject");

    for cancel_after in [0, 1] {
        let polls = AtomicUsize::new(0);
        let mut progress = Vec::new();
        let error = resolve_resume_home(
            &homes,
            SESSION_ID,
            &|| polls.fetch_add(1, Ordering::SeqCst) >= cancel_after,
            &mut |done, total| progress.push((done, total)),
        )
        .unwrap_err()
        .to_string();

        assert_eq!(error, SESSION_LOOKUP_CANCELLED);
        assert_eq!(progress, [(0, 2)], "cancelled after {cancel_after} polls");
    }
}

#[test]
fn only_candidates_that_could_hold_sessions_leave_discovery_incomplete() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join(".claude-work")).unwrap();
    fs::write(temp.path().join("file"), "").unwrap();
    let _home = EnvVarGuard::set("HOME", temp.path());

    for config in [temp.path().join("missing"), temp.path().join("file/child")] {
        let _config = EnvVarGuard::set(CONFIG_DIR_ENV, &config);
        let homes = discover_homes().unwrap();
        assert_eq!(homes.warnings.len(), 1, "{:?}", homes.warnings);
        assert!(homes.complete, "{:?}", homes.warnings);
    }

    let _unlistable_home = EnvVarGuard::set("HOME", temp.path().join("file"));
    let _config = EnvVarGuard::remove(CONFIG_DIR_ENV);
    let homes = discover_homes().unwrap();
    assert!(!homes.complete, "{:?}", homes.warnings);
}
