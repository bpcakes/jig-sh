use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

use super::{generate_todo, scan_sqlx_calls, sqlx_report};
use crate::context::RepoContext;
use crate::policy::SqlxTodoInput;
use crate::test_env::TestRepoBuilder;

#[test]
fn scan_sqlx_calls_marks_inline_cfg_test_module_calls_as_test() {
    let text = r#"
pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::query("select 1").execute(pool).await;
}

#[cfg(test)]
mod tests {
    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 2").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 2);
    assert!(!calls[0].is_test);
    assert!(calls[1].is_test);
}

#[test]
fn scan_sqlx_calls_keeps_cfg_test_module_open_across_lifetime_lines() {
    let text = r#"
#[cfg(test)]
mod tests {
    fn helper<'a>(value: &'a str) {
        let _ = value;
    }

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_marks_named_cfg_test_modules_as_test() {
    let text = r#"
#[cfg(test)] mod unit {
    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_clears_pending_cfg_after_inline_test_module() {
    let text = r#"
#[cfg(test)] mod unit {}

mod production {
    pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(!calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_keeps_outer_test_module_after_nested_test_module() {
    let text = r#"
#[cfg(test)]
mod outer {
    #[cfg(test)]
    mod nested {}

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_accepts_test_module_brace_on_next_line() {
    let text = r#"
#[cfg(test)]
mod tests
{
    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_clears_pending_cfg_after_external_test_module_decl() {
    let text = r#"
#[cfg(test)]
mod tests;

mod production {
    pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(!calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_accepts_spaced_cfg_test_attributes() {
    let text = r#"
#[ cfg ( test ) ]
pub mod integration {
    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_allows_comment_between_cfg_test_and_module() {
    let text = r#"
#[cfg(test)]
// A module-level explanation may sit between the cfg and module declaration.
mod tests {
    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_ignores_braces_inside_raw_strings_and_chars() {
    let text = r##"
#[cfg(test)]
mod tests {
    fn helper() {
        let _raw = r#"}"#;
        let _char = '{';
    }

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"##;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_ignores_multiline_raw_string_contents() {
    let text = r##"
#[cfg(test)]
mod tests {
    fn fixture() {
        let _text = r#"
mod production {
    let _ = sqlx::query("select string only");
}
"#;
    }

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"##;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_ignores_multi_hash_raw_string_contents() {
    let text = r####"
pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _raw = r###"sqlx::query("select raw only")"###;
    let _ = sqlx::query("select 1").execute(pool).await;
}
"####;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].line, 4);
}

#[test]
fn scan_sqlx_calls_ignores_multiline_block_comment_contents() {
    let text = r#"
#[cfg(test)]
mod tests {
    /*
    mod production {
        let _ = sqlx::query("select comment only");
    }
    */

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_ignores_nested_block_comment_braces() {
    let text = r#"
#[cfg(test)]
mod tests {
    /* outer { /* inner } */ still comment } */

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_ignores_braces_inside_byte_strings_and_byte_chars() {
    let text = r##"
#[cfg(test)]
mod tests {
    fn helper() {
        let _bytes = b"}";
        let _raw_bytes = br#"}"#;
        let _byte_char = b'{';
    }

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"##;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_keeps_test_module_open_after_single_lifetime() {
    let text = r#"
#[cfg(test)]
mod tests {
    fn helper<'a, T>() {
        let _ = std::marker::PhantomData::<&'a T>;
    }

    async fn checks(pool: &sqlx::Pool<sqlx::Postgres>) {
        let _ = sqlx::query("select 1").execute(pool).await;
    }
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert!(calls[0].is_test);
}

#[test]
fn scan_sqlx_calls_requires_sqlx_keyword_boundary() {
    let text = r#"
pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = my_sqlx::query("select 0");
    let _ = sqlx_utils::query("select 0");
    let _ = sqlx::query("select 1").execute(pool).await;
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].line, 5);
}

#[test]
fn scan_sqlx_calls_ignores_comments_and_strings() {
    let text = r##"
pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
    // sqlx::query("select comment")
    let _literal = "sqlx::query(\"select string\")";
    let _raw = r#"sqlx::query("select raw")"#;
    let _ = sqlx::query("select 1").execute(pool).await;
}
"##;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].line, 6);
}

#[test]
fn scan_sqlx_calls_detects_turbofish_calls_as_unchecked() {
    let text = r#"
pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::query_as::<_, User>("select * from users").fetch_one(pool).await;
    let _ = sqlx::query_scalar :: <i64>("select 1").fetch_one(pool).await;
    let _ = sqlx::query!("select 2").fetch_one(pool).await;
}
"#;

    let calls = scan_sqlx_calls("crates/app/src/lib.rs", text);

    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].function, "sqlx::query_as");
    assert!(!calls[0].checked);
    assert_eq!(calls[1].function, "sqlx::query_scalar");
    assert!(!calls[1].checked);
    assert_eq!(calls[2].function, "sqlx::query");
    assert!(calls[2].checked);
}

#[test]
fn sqlx_report_includes_untracked_rust_files() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("crates/app/src")).unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
    let status = Command::new("git")
        .current_dir(temp.path())
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        temp.path().join("crates/app/src/untracked.rs"),
        r#"pub async fn load(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::query("select 1").fetch_one(pool).await;
}
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let report = sqlx_report(&ctx, std::path::Path::new("missing-report.md")).unwrap();

    assert_eq!(report.non_test_count, 1);
    assert!(report.body.contains("crates/app/src/untracked.rs:2"));
}

#[test]
fn generate_todo_rejects_output_outside_repo() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = generate_todo(
        &ctx,
        &SqlxTodoInput {
            output: Some(PathBuf::from("../todo.md")),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("inside the repository"));
    assert!(!temp.path().parent().unwrap().join("todo.md").exists());
}

#[test]
fn generate_todo_rejects_absolute_output_paths() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = generate_todo(
        &ctx,
        &SqlxTodoInput {
            output: Some(PathBuf::from("/tmp/todo.md")),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("repository-relative"));
}
