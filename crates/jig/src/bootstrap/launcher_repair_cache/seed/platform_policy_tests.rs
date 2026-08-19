use crate::context::runtime_profile_cache_name;

use super::super::super::{EMBEDDED_TEMPLATE_SOURCE, HarnessFootprint};
use super::super::refresh::{
    FullRefreshRuntimePolicy, LAUNCHER_REPAIR_SEED_STAMP_HEADER, finish_full_refresh,
};
use super::*;

fn write_fake_pe(path: &Path) {
    let mut bytes = vec![0_u8; 132];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&128_u32.to_le_bytes());
    bytes[128..132].copy_from_slice(b"PE\0\0");
    fs::write(path, bytes).unwrap();
}

fn create_fake_git_install(root: &Path) -> WindowsGitBashInstallation {
    let bash = root.join("bin/bash.exe");
    let git = root.join("cmd/git.exe");
    fs::create_dir_all(bash.parent().unwrap()).unwrap();
    fs::create_dir_all(git.parent().unwrap()).unwrap();
    fs::create_dir_all(root.join("usr/bin")).unwrap();
    write_fake_pe(&bash);
    write_fake_pe(&git);
    WindowsGitBashInstallation {
        bash: fs::canonicalize(bash).unwrap(),
        root: fs::canonicalize(root).unwrap(),
    }
}

fn expected_repair_directory(path: &Path) -> PathBuf {
    let canonical = fs::canonicalize(path).unwrap();
    #[cfg(windows)]
    return crate::shell::windows_bash_compatible_path(&canonical).unwrap();
    #[cfg(not(windows))]
    canonical
}

#[test]
fn windows_bash_selection_rejects_repository_controlled_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    let repository_install = create_fake_git_install(&destination.join("tools/Git"));
    let external_install = create_fake_git_install(&temp.path().join("Git"));

    let selected = select_windows_git_bash_candidate(
        &destination,
        [repository_install.bash, external_install.bash.clone()],
    )
    .unwrap();

    assert_eq!(selected, external_install);
}

#[test]
fn windows_bash_selection_rejects_non_git_bash_layouts() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    fs::create_dir(&destination).unwrap();
    let cygwin_bash = temp.path().join("cygwin64/bin/bash.exe");
    fs::create_dir_all(cygwin_bash.parent().unwrap()).unwrap();
    write_fake_pe(&cygwin_bash);
    let git = create_fake_git_install(&temp.path().join("Git"));

    let selected =
        select_windows_git_bash_candidate(&destination, [cygwin_bash, git.bash.clone()]).unwrap();

    assert_eq!(selected, git);
}

#[test]
fn windows_bash_candidates_put_standard_git_roots_before_path() {
    let temp = tempfile::tempdir().unwrap();
    let standard = temp.path().join("Program Files");
    let ambient = temp.path().join("ambient");
    let search_path = env::join_paths([ambient.clone()]).unwrap();

    let candidates = windows_git_bash_candidates([standard.clone()], None, Some(&search_path));

    assert_eq!(candidates[0], standard.join("Git/bin/bash.exe"));
    assert_eq!(candidates[1], standard.join("Git/usr/bin/bash.exe"));
    assert_eq!(candidates.last(), Some(&ambient.join("bash.exe")));
}

#[test]
fn windows_helper_path_excludes_repository_and_relative_entries() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    let repository_tools = destination.join("tools");
    let git = create_fake_git_install(&temp.path().join("Git"));
    let git_bin = git.root.join("bin");
    let git_usr_bin = git.root.join("usr/bin");
    let python_bin = temp.path().join("Python");
    let unrelated_bin = temp.path().join("unrelated");
    for directory in [&repository_tools, &python_bin, &unrelated_bin] {
        fs::create_dir_all(directory).unwrap();
    }
    write_fake_pe(&python_bin.join("python3.exe"));
    let search_path = env::join_paths([
        repository_tools,
        PathBuf::from("relative-tools"),
        unrelated_bin.clone(),
        python_bin.clone(),
    ])
    .unwrap();

    let directories =
        windows_repair_tool_directories(&destination, &git, Some(&search_path)).unwrap();

    assert!(directories.contains(&expected_repair_directory(&git_bin)));
    assert!(directories.contains(&expected_repair_directory(&git_usr_bin)));
    assert!(directories.contains(&expected_repair_directory(&python_bin)));
    assert!(!directories.contains(&expected_repair_directory(&unrelated_bin)));
    let destination = expected_repair_directory(&destination);
    assert!(
        directories
            .iter()
            .all(|directory| !directory.starts_with(&destination))
    );
}

#[test]
fn windows_helper_path_rejects_a_non_pe_python_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    fs::create_dir(&destination).unwrap();
    let git = create_fake_git_install(&temp.path().join("Git"));
    let python_bin = temp.path().join("Python");
    fs::create_dir(&python_bin).unwrap();
    fs::write(python_bin.join("python3.exe"), "not a PE executable").unwrap();
    let search_path = env::join_paths([python_bin]).unwrap();

    let error = windows_repair_tool_directories(&destination, &git, Some(&search_path))
        .unwrap_err()
        .to_string();

    assert!(error.contains("native python3.exe"), "{error}");
}

#[test]
fn full_refresh_runtime_policy_encodes_source_and_footprint_together() {
    assert_eq!(
        FullRefreshRuntimePolicy::for_render(HarnessFootprint::Minimal, EMBEDDED_TEMPLATE_SOURCE,),
        FullRefreshRuntimePolicy::NoManagedRuntime
    );
    assert_eq!(
        FullRefreshRuntimePolicy::for_render(HarnessFootprint::Full, EMBEDDED_TEMPLATE_SOURCE,),
        FullRefreshRuntimePolicy::EmbeddedTemplate
    );
    assert_eq!(
        FullRefreshRuntimePolicy::for_render(
            HarnessFootprint::Full,
            "https://example.test/jig.git",
        ),
        FullRefreshRuntimePolicy::ConfiguredSource
    );
}

#[test]
fn failed_embedded_refresh_keeps_the_last_launcher_repair_seed() {
    let _env = crate::test_env::lock_env();
    let _seed_failure = crate::test_env::EnvVarGuard::set(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV, "1");
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    fs::create_dir(&destination).unwrap();
    let cache = runtime_cache_base(&destination).join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Runtime,
    ));
    fs::create_dir_all(&cache).unwrap();
    let stamp = cache.join(".jig-source-stamp");
    fs::write(&stamp, format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\n")).unwrap();

    let warnings = finish_full_refresh(
        &destination,
        FullRefreshRuntimePolicy::EmbeddedTemplate,
        crate::progress::CliProgress::disabled("test"),
        "done",
    );

    assert_eq!(warnings.len(), 1);
    assert!(stamp.exists(), "the last runnable repair seed was retired");
}

#[test]
fn embedded_seed_test_double_matches_the_compiled_profile_set() {
    let _env = crate::test_env::lock_env();
    let _seed_failure = crate::test_env::EnvVarGuard::remove(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV);
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    fs::create_dir(&destination).unwrap();
    seed_embedded_template_runtime(&destination, crate::context::CURRENT_CONTRACT_VERSION)
        .unwrap()
        .commit();
    let cache_base = runtime_cache_base(&destination);
    let runtime = cache_base.join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Runtime,
    ));
    let default = cache_base.join(runtime_profile_cache_name(
        crate::context::CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Default,
    ));

    assert!(runtime.is_dir());
    assert_eq!(default.is_dir(), cfg!(feature = "dev-proxy"));
}
