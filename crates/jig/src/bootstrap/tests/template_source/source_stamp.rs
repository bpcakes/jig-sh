use super::*;

#[cfg(unix)]
#[test]
fn native_binary_header_policy_accepts_pe_and_rejects_malformed_mz_files() {
    let temp = tempdir().unwrap();
    let valid_pe = temp.path().join("jig.exe");
    let malformed_pe = temp.path().join("malformed.exe");
    let mut bytes = vec![0_u8; 132];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&128_u32.to_le_bytes());
    bytes[128..132].copy_from_slice(b"PE\0\0");
    fs::write(&valid_pe, &bytes).unwrap();
    bytes[128..132].copy_from_slice(b"NOPE");
    fs::write(&malformed_pe, bytes).unwrap();
    let installer = include_str!("../../embedded_template_snapshots/scripts/install-jig.sh.jinja");
    let start = installer
        .find("native_binary_header_is_supported() {")
        .expect("installer should define native binary header validation");
    let end = installer[start..]
        .find("\nresolve_compatible_path_jig() {")
        .map(|offset| start + offset)
        .expect("PATH resolution should follow native header validation");
    let script = format!(
        "set -euo pipefail\nrequire_python3() {{ command -v python3 >/dev/null; }}\n{}\nnative_binary_header_is_supported \"$1\"\n",
        &installer[start..end]
    );

    let valid = Command::new("bash")
        .args(["-c", &script, "pe-header"])
        .arg(&valid_pe)
        .output()
        .unwrap();
    assert!(
        valid.status.success(),
        "valid PE header was rejected: {}",
        String::from_utf8_lossy(&valid.stderr)
    );

    let malformed = Command::new("bash")
        .args(["-c", &script, "pe-header"])
        .arg(&malformed_pe)
        .output()
        .unwrap();
    assert!(!malformed.status.success());
}

#[cfg(unix)]
#[test]
fn local_source_stamp_fails_closed_when_git_diff_fails() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let repo = temp.path().join("source");
    fs::create_dir_all(repo.join("crates/jig/src")).unwrap();
    fs::write(repo.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    fs::write(repo.join("Cargo.lock"), "version = 3\n").unwrap();
    fs::write(repo.join("crates/jig/src/main.rs"), "fn main() {}\n").unwrap();
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "source fixture"]).unwrap();

    let real_git = std::env::split_paths(
        &std::env::var_os("PATH").expect("the test environment should define PATH"),
    )
    .map(|directory| directory.join("git"))
    .find(|candidate| candidate.is_file())
    .expect("git should be available on PATH");
    let shim_dir = temp.path().join("bin");
    fs::create_dir(&shim_dir).unwrap();
    let shim = shim_dir.join("git");
    fs::write(
        &shim,
        r#"#!/bin/sh
case " $* " in
  *" diff "*)
    printf '%s\n' 'simulated diff diagnostic' >&2
    exit 65
    ;;
esac
exec "$JIG_TEST_REAL_GIT" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(shim_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").expect("the test environment should define PATH"),
    )))
    .unwrap();
    let installer = include_str!("../../embedded_template_snapshots/scripts/install-jig.sh.jinja");

    let output = local_source_stamp_command(installer, &repo)
        .env("PATH", path)
        .env("JIG_TEST_REAL_GIT", real_git)
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[cfg(unix)]
#[test]
fn local_source_stamp_rejects_a_tracked_file_replaced_by_a_worktree_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let repo = temp.path().join("source");
    let source_file = repo.join("crates/jig/src/main.rs");
    fs::create_dir_all(source_file.parent().unwrap()).unwrap();
    fs::write(repo.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    fs::write(repo.join("Cargo.lock"), "version = 3\n").unwrap();
    fs::write(&source_file, "fn main() {}\n").unwrap();
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "source fixture"]).unwrap();

    let external_target = temp.path().join("outside.rs");
    fs::write(&external_target, "fn main() { println!(\"first\"); }\n").unwrap();
    fs::remove_file(&source_file).unwrap();
    symlink(&external_target, &source_file).unwrap();
    let installer = include_str!("../../embedded_template_snapshots/scripts/install-jig.sh.jinja");

    let first = run_local_source_stamp(installer, &repo);
    fs::write(&external_target, "fn main() { println!(\"second\"); }\n").unwrap();
    let second = run_local_source_stamp(installer, &repo);

    for output in [first, second] {
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("tracked symbolic link"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(unix)]
#[test]
fn hash_stdin_falls_back_to_required_python() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    let python = std::env::split_paths(
        &std::env::var_os("PATH").expect("the test environment should define PATH"),
    )
    .map(|directory| directory.join("python3"))
    .find(|candidate| candidate.is_file())
    .expect("python3 should be available on PATH");
    symlink(python, bin_dir.join("python3")).unwrap();
    let installer = include_str!("../../embedded_template_snapshots/scripts/install-jig.sh.jinja");
    let start = installer
        .find("hash_stdin() {")
        .expect("installer should define hash_stdin");
    let end = installer[start..]
        .find("\nlocal_source_stamp() {")
        .map(|offset| start + offset)
        .expect("installer should define local_source_stamp after hash_stdin");
    let script = format!(
        "set -euo pipefail\n{}\nprintf 'abc' | hash_stdin\n",
        &installer[start..end]
    );

    let output = Command::new("/bin/bash")
        .args(["-c", &script])
        .env("PATH", bin_dir)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "python hash fallback failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
    );
}

#[cfg(unix)]
#[test]
fn hash_stdin_rejects_a_malformed_python_fallback_digest() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    let fake_python = bin_dir.join("python3");
    fs::write(&fake_python, "#!/bin/sh\nprintf malformed-digest\n").unwrap();
    fs::set_permissions(&fake_python, fs::Permissions::from_mode(0o755)).unwrap();
    let installer = include_str!("../../embedded_template_snapshots/scripts/install-jig.sh.jinja");
    let start = installer
        .find("hash_stdin() {")
        .expect("installer should define hash_stdin");
    let end = installer[start..]
        .find("\nlocal_source_stamp() {")
        .map(|offset| start + offset)
        .expect("installer should define local_source_stamp after hash_stdin");
    let script = format!(
        "set -euo pipefail\n{}\nprintf 'abc' | hash_stdin\n",
        &installer[start..end]
    );

    let output = Command::new("/bin/bash")
        .args(["-c", &script])
        .env("PATH", bin_dir)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn hash_stdin_rejects_an_empty_hasher_result() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    let fake_hasher = bin_dir.join("sha256sum");
    fs::write(&fake_hasher, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&fake_hasher, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(bin_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").expect("the test environment should define PATH"),
    )))
    .unwrap();
    let installer = include_str!("../../embedded_template_snapshots/scripts/install-jig.sh.jinja");
    let start = installer
        .find("hash_stdin() {")
        .expect("installer should define hash_stdin");
    let end = installer[start..]
        .find("\nlocal_source_stamp() {")
        .map(|offset| start + offset)
        .expect("installer should define local_source_stamp after hash_stdin");
    let script = format!(
        "set -euo pipefail\n{}\nprintf 'abc' | hash_stdin\n",
        &installer[start..end]
    );

    let output = Command::new("/bin/bash")
        .args(["-c", &script])
        .env("PATH", path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
