
fn program_presence(root: &Path, program: &str, resolved: Option<&Path>) -> (bool, String) {
    match resolved {
        Some(_) => (true, format!("{program} is available")),
        None if program_has_explicit_path(program) => {
            let path = PathBuf::from(program);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            (
                false,
                format!("{} is missing or not executable", path.display()),
            )
        }
        None => (false, format!("{program} was not found on PATH")),
    }
}
