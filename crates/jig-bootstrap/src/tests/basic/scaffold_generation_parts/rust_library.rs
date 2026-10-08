#[cfg(unix)]
#[test]
fn rust_library_init_generates_exact_buildable_neutral_workspace() {
    assert_rust_only_generated_repository(RustOnlyAcceptanceCase::library());
}
