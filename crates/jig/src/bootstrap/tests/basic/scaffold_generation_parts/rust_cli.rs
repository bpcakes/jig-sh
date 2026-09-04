#[cfg(unix)]
#[test]
fn rust_cli_init_generates_exact_buildable_runnable_neutral_workspace() {
    assert_rust_only_generated_repository(RustOnlyAcceptanceCase::cli());
}
