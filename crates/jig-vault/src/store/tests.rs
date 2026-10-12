use super::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[cfg(target_os = "macos")]
mod acl;
mod path_resolution;

#[cfg(unix)]
#[test]
fn resolve_rejects_symlink_home() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let error = VaultStore::resolve_for_test(Some(link))
        .unwrap_err()
        .to_string();
    assert!(error.contains("must not be a symlink"));
}

#[cfg(unix)]
#[test]
fn resolve_rejects_symlink_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let error = VaultStore::resolve_for_test(Some(link.join("vault")))
        .unwrap_err()
        .to_string();
    assert!(error.contains("creation base"));
}

#[test]
fn resolve_rejects_regular_file_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("vault");
    fs::write(&home, "not a directory").unwrap();
    let error = VaultStore::resolve_for_test(Some(home))
        .unwrap_err()
        .to_string();
    assert!(error.contains("failed to create vault home"));
}

#[cfg(unix)]
#[test]
fn exists_refuses_symlinked_vault_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let target = temp.path().join("outside-vault.json");
    fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, store.vault_path()).unwrap();

    assert!(store.exists().is_err());
}

#[cfg(unix)]
#[test]
fn read_refuses_symlinked_vault_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let target = temp.path().join("outside-vault.json");
    fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, store.vault_path()).unwrap();

    let error = store.read_vault_text().unwrap_err().to_string();
    assert!(error.contains("refusing to read symlinked vault file"));
}

#[test]
fn read_rejects_oversized_vault_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
    let file = File::create(store.vault_path()).unwrap();
    file.set_len(VAULT_TEXT_READ_LIMIT + 1).unwrap();

    let error = store.read_vault_text().unwrap_err().to_string();

    assert!(error.contains("read limit"));
}

#[test]
fn resolve_rejects_empty_env_home() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = std::env::var_os(VAULT_HOME_ENV);
    unsafe {
        std::env::set_var(VAULT_HOME_ENV, "");
    }
    let error = VaultStore::resolve_for_test(None).unwrap_err().to_string();
    unsafe {
        if let Some(previous) = previous {
            std::env::set_var(VAULT_HOME_ENV, previous);
        } else {
            std::env::remove_var(VAULT_HOME_ENV);
        }
    }
    assert!(error.contains("must not be empty"));
}

#[cfg(unix)]
#[test]
fn resolve_rejects_non_utf8_env_home() {
    use std::os::unix::ffi::OsStringExt;

    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = std::env::var_os(VAULT_HOME_ENV);
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(PathBuf::from(std::ffi::OsString::from_vec(
        b"vault-\xff".to_vec(),
    )));
    unsafe {
        std::env::set_var(VAULT_HOME_ENV, home.as_os_str());
    }
    let result = VaultStore::resolve_for_test(None);
    unsafe {
        if let Some(previous) = previous {
            std::env::set_var(VAULT_HOME_ENV, previous);
        } else {
            std::env::remove_var(VAULT_HOME_ENV);
        }
    }

    let error = result.unwrap_err().to_string();
    assert!(error.contains("must be valid Unicode"));
}
