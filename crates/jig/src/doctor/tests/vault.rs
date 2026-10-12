use serde_json::json;

use crate::doctor::vault::vault_detail;

#[test]
fn vault_detail_names_the_shared_main_checkout() {
    let shared = vault_detail(&json!({
        "vault_home": "/tmp/jig-vault/scopes/repo-1",
        "vault_scope": "repo",
        "vault_scope_id": "scope_1",
        "vault_main_checkout_root": "/tmp/ExampleProject",
    }));
    assert_eq!(
        shared,
        "vault_home=/tmp/jig-vault/scopes/repo-1 scope=repo scope_id=scope_1 main_checkout_root=/tmp/ExampleProject"
    );

    let checkout = vault_detail(&json!({
        "vault_home": "/tmp/jig-vault/scopes/repo-1",
        "vault_scope": "repo",
        "vault_scope_id": "scope_1",
        "vault_main_checkout_root": null,
    }));
    assert!(!checkout.contains("main_checkout_root"), "{checkout}");
    assert!(!checkout.contains("worktree_local"), "{checkout}");

    let worktree_local = vault_detail(&json!({
        "vault_home": "/tmp/jig-vault/scopes/repo-2",
        "vault_scope": "repo",
        "vault_scope_id": "scope_1",
        "vault_main_checkout_root": null,
        "vault_worktree_local": true,
    }));
    assert_eq!(
        worktree_local,
        "vault_home=/tmp/jig-vault/scopes/repo-2 scope=repo scope_id=scope_1 worktree_local=true"
    );
}
#[test]
fn vault_detail_reports_the_unauthenticated_header_format() {
    let detail = vault_detail(&json!({
        "vault_home": "/tmp/jig-vault/scopes/repo-1",
        "vault_scope": "repo",
        "format_version": 3,
    }));
    assert_eq!(
        detail,
        "vault_home=/tmp/jig-vault/scopes/repo-1 scope=repo format_version=3"
    );
    let absent = vault_detail(&json!({
        "vault_home": "/tmp/jig-vault/scopes/repo-1",
        "format_version": null,
    }));
    assert!(!absent.contains("format_version"), "{absent}");
}
