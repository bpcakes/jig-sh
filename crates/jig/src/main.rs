fn main() {
    if let Err(error) = jig::run() {
        if !jig::error_is_structured_command_failure(&error) && !jig::stderr_delivery_abandoned() {
            eprintln!("{error:#}");
        }
        std::process::exit(jig::error_exit_code(&error).unwrap_or(1));
    }
}
