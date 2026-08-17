use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::*;
use crate::test_tempdir as tempdir;

#[cfg(unix)]
#[test]
fn verified_route_reports_when_post_write_rollback_cannot_be_confirmed() {
    struct RestorePermissions {
        path: PathBuf,
        permissions: fs::Permissions,
    }

    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            fs::set_permissions(&self.path, self.permissions.clone()).unwrap();
        }
    }

    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
    let original_permissions = fs::metadata(store.root()).unwrap().permissions();
    let restore = RestorePermissions {
        path: store.root().to_path_buf(),
        permissions: original_permissions,
    };
    let mut calls = 0usize;

    let error = store
        .add_verified_route(
            Route {
                hostname: "rollback-uncertain.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            },
            |_| {
                calls += 1;
                if calls == 2 {
                    fs::set_permissions(store.root(), fs::Permissions::from_mode(0o500)).unwrap();
                    Err(anyhow::anyhow!("listener changed after publish"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    drop(restore);

    let diagnostic = format!("{error:#}");
    assert!(diagnostic.contains("listener changed after publish"));
    assert!(diagnostic.contains("rollback also failed"));
    assert!(!diagnostic.contains("route was not published"));
}
