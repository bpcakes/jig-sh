
fn resolve_literal_cd(root: &Path, cwd: &Path, words: &[ShellWord]) -> Option<PathBuf> {
    if shell_word_value(words.first()?) != "cd" {
        return None;
    }
    let path_word = match words {
        [_, path] => path,
        [_, option, path] if shell_word_value(option) == "--" => path,
        _ => return None,
    };
    let value = shell_word_value(path_word);
    if value.is_empty()
        || value.starts_with('~')
        || path_word.active_dollar
        || path_word.literal_dollar
        || path_word.dynamic
        || value.chars().any(|ch| matches!(ch, '*' | '?' | '['))
    {
        return None;
    }
    let candidate = PathBuf::from(value);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    };
    let root = fs::canonicalize(root).ok()?;
    let candidate = fs::canonicalize(candidate).ok()?;
    (candidate.is_dir() && candidate.starts_with(root)).then_some(candidate)
}

fn literal_exit_guard(words: &[ShellWord]) -> bool {
    match words {
        [command] => shell_word_value(command) == "exit",
        [command, status] => {
            shell_word_value(command) == "exit"
                && !status.active_dollar
                && !status.literal_dollar
                && !status.dynamic
                && !status.value.is_empty()
                && status.value.chars().all(|ch| ch.is_ascii_digit())
        }
        _ => false,
    }
}

fn reported_program(command_key: &str, program: &str) -> (String, bool) {
    let sqlx_safe_name = matches!(program, "cargo" | "cargo-sqlx" | "sqlx");
    let redact = (command_key == "sqlx_check_command" && !sqlx_safe_name)
        || credential_like_program(program);
    if redact {
        ("<redacted: command executable>".to_string(), true)
    } else {
        (program.to_string(), false)
    }
}

fn credential_like_program(program: &str) -> bool {
    let lowercase = program.to_ascii_lowercase();
    program.contains("://")
        || (program.contains('@') && program.contains(':'))
        || ["password", "passwd", "secret", "token", "apikey", "api_key"]
            .iter()
            .any(|marker| lowercase.contains(marker))
}

fn looks_like_shell_assignment(value: &str) -> bool {
    bash_assignment_name(value).is_some()
}

fn shell_command_prefix_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "do" | "done" | "elif" | "else" | "esac" | "fi" | "if" | "then" | "until" | "while"
    )
}

fn shell_word_is_prefix_keyword(word: &ShellWord, allow_prefix_keyword: bool) -> bool {
    allow_prefix_keyword && word.syntactically_plain && shell_command_prefix_keyword(&word.value)
}

fn shell_word_is_keyword(word: &ShellWord) -> bool {
    word.syntactically_plain && bash_keyword(&word.value)
}

fn bash_builtin(program: &str) -> bool {
    matches!(
        program,
        "." | ":"
            | "["
            | "alias"
            | "bg"
            | "bind"
            | "break"
            | "builtin"
            | "caller"
            | "cd"
            | "command"
            | "compgen"
            | "complete"
            | "compopt"
            | "continue"
            | "declare"
            | "dirs"
            | "disown"
            | "echo"
            | "enable"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "false"
            | "fc"
            | "fg"
            | "getopts"
            | "hash"
            | "help"
            | "history"
            | "jobs"
            | "kill"
            | "let"
            | "local"
            | "logout"
            | "mapfile"
            | "popd"
            | "printf"
            | "pushd"
            | "pwd"
            | "read"
            | "readarray"
            | "readonly"
            | "return"
            | "set"
            | "shift"
            | "shopt"
            | "source"
            | "suspend"
            | "test"
            | "times"
            | "trap"
            | "true"
            | "type"
            | "typeset"
            | "ulimit"
            | "umask"
            | "unalias"
            | "unset"
            | "wait"
    )
}

fn bash_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "[["
            | "]]"
            | "case"
            | "coproc"
            | "do"
            | "done"
            | "elif"
            | "else"
            | "esac"
            | "fi"
            | "for"
            | "function"
            | "if"
            | "in"
            | "select"
            | "then"
            | "time"
            | "until"
            | "while"
            | "{"
            | "}"
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProgramOrigin {
    ExplicitPath,
    SearchPath { entry: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramResolution {
    path: PathBuf,
    origin: ProgramOrigin,
}

fn search_path_is_cwd_independent(search_path: Option<&OsStr>) -> bool {
    let Some(search_path) = search_path else {
        return false;
    };
    env::split_paths(search_path).all(|entry| !entry.as_os_str().is_empty() && entry.is_absolute())
}

fn resolve_program(
    command_cwd: &Path,
    program: &str,
    search_path: Option<&OsStr>,
) -> Option<ProgramResolution> {
    if program_has_explicit_path(program) {
        let path = PathBuf::from(program);
        let path = if path.is_absolute() {
            path
        } else {
            command_cwd.join(path)
        };
        return executable_exists(&path).then_some(ProgramResolution {
                path,
                origin: ProgramOrigin::ExplicitPath,
            });
    }

    let search_path = search_path?;
    for entry in env::split_paths(search_path) {
        let directory = if entry.is_absolute() {
            entry.clone()
        } else {
            command_cwd.join(&entry)
        };
        let path = directory.join(program);
        if executable_exists(&path) {
            return Some(ProgramResolution {
                path,
                origin: ProgramOrigin::SearchPath { entry },
            });
        }
    }
    None
}

fn program_has_explicit_path(program: &str) -> bool {
    let path = Path::new(program);
    path.is_absolute()
        || path.components().count() > 1
        || program.contains('/')
}


fn trusted_sqlx_probe_executable(
    root: &Path,
    program: &str,
    resolution: &ProgramResolution,
) -> Option<PathBuf> {
    if !bare_sqlx_probe_program(program) {
        return None;
    }
    trusted_bare_path_executable(root, resolution)
}

fn trusted_bare_path_executable(root: &Path, resolution: &ProgramResolution) -> Option<PathBuf> {
    let ProgramOrigin::SearchPath { entry } = &resolution.origin else {
        return None;
    };
    if entry.as_os_str().is_empty() || !entry.is_absolute() {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    if entry.starts_with(&root) || resolution.path.starts_with(&root) {
        return None;
    }
    if path_has_symlink_or_reparse_component(&resolution.path)? {
        return None;
    }
    let entry = fs::canonicalize(entry).ok()?;
    let executable = fs::canonicalize(&resolution.path).ok()?;
    if entry.starts_with(&root) || executable.starts_with(&root) {
        return None;
    }
    Some(executable)
}

fn cargo_sqlx_dispatch_issue(
    root: &Path,
    command: &str,
    environment: &DoctorEnvironment,
) -> &'static str {
    if environment.cargo_alias_sqlx.is_some() {
        return "CARGO_ALIAS_SQLX is set";
    }
    if cargo_sqlx_command_changes_dispatch_environment(command) {
        return "the command changes Cargo alias or home environment";
    }
    if cargo_sqlx_command_has_inline_config(command) {
        return "the cargo command has an inline --config override";
    }
    if effective_cargo_config_obscures_sqlx_alias(root, command, environment) {
        return "an effective Cargo config may change subcommand dispatch";
    }
    "an external cargo path does not prove which sqlx subcommand will run"
}

fn cargo_sqlx_command_changes_dispatch_environment(command: &str) -> bool {
    parse_shell_commands(command)
        .commands
        .iter()
        .any(|words| shell_command_changes_cargo_dispatch_environment(words))
}

fn shell_command_changes_cargo_dispatch_environment(words: &[ShellWord]) -> bool {
    if shell_command_may_persist_variable_change(words, &cargo_dispatch_environment_name) {
        return true;
    }
    let command_name = shell_command_name(words);
    let (program_index, force_external) = match command_name {
        ShellCommandName::Executable {
            index,
            force_external,
            ..
        } => (Some(index), force_external),
        ShellCommandName::NoExternalExecutable { .. }
        | ShellCommandName::AmbiguousWrapper { .. } => (None, false),
    };
    let mut leading_assignment_end = 0;
    while words
        .get(leading_assignment_end)
        .is_some_and(shell_word_is_assignment)
    {
        if bash_assignment_name(&shell_word_value(&words[leading_assignment_end]))
            .is_some_and(cargo_dispatch_environment_name)
        {
            return true;
        }
        leading_assignment_end += 1;
    }
    let prefix_end = program_index.unwrap_or(words.len());
    if words[..prefix_end].iter().any(|word| {
        shell_assignment_name(&shell_word_value(word)).is_some_and(cargo_dispatch_environment_name)
    }) {
        return true;
    }
    if program_index.is_none()
        && words.iter().any(|word| {
            shell_assignment_name(&shell_word_value(word))
                .is_some_and(cargo_dispatch_environment_name)
        })
    {
        return true;
    }

    let Some(program_index) = program_index else {
        return false;
    };
    let program = shell_word_value(&words[program_index]);
    if !force_external
        && matches!(
            program.as_str(),
            "export" | "local" | "readonly" | "typeset"
        )
        && words[program_index + 1..].iter().any(|word| {
            let word = shell_word_value(word);
            cargo_dispatch_environment_name(shell_assignment_name(&word).unwrap_or(word.as_str()))
        })
    {
        return true;
    }
    if !force_external
        && program == "unset"
        && words[program_index + 1..]
            .iter()
            .map(shell_word_value)
            .any(|name| cargo_dispatch_environment_name(&name))
    {
        return true;
    }

    let mut saw_env = false;
    let mut index = 0;
    while index < program_index {
        let word = shell_word_value(&words[index]);
        if word == "exec" && exec_wrapper_clears_environment(words, index + 1, program_index) {
            return true;
        }
        if executable_is_named(&word, "env") {
            saw_env = true;
        } else if saw_env {
            if matches!(word.as_str(), "-" | "-i" | "--ignore-environment") {
                return true;
            }
            if shell_assignment_name(&word).is_some_and(cargo_dispatch_environment_name) {
                return true;
            }
            if word == "-u" || word == "--unset" {
                index += 1;
                if words
                    .get(index)
                    .map(shell_word_value)
                    .as_deref()
                    .is_some_and(cargo_dispatch_environment_name)
                {
                    return true;
                }
            } else if word
                .strip_prefix("--unset=")
                .or_else(|| word.strip_prefix("-u"))
                .is_some_and(cargo_dispatch_environment_name)
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn cargo_dispatch_environment_name(name: &str) -> bool {
    const NAMES: [&str; 3] = ["CARGO_ALIAS_SQLX", "CARGO_HOME", "HOME"];
    NAMES.contains(&name)
}

fn cargo_sqlx_command_has_inline_config(command: &str) -> bool {
    parse_shell_commands(command).commands.iter().any(|words| {
        let Some(program_index) = command_program_index(words) else {
            return false;
        };
        if !executable_is_named(&shell_word_value(&words[program_index]), "cargo") {
            return false;
        }
        let Some(sqlx_index) = cargo_subcommand_index(words, program_index + 1) else {
            return false;
        };
        if shell_word_value(&words[sqlx_index]) != "sqlx" {
            return false;
        }
        let mut index = program_index + 1;
        while index < sqlx_index {
            let value = shell_word_value(&words[index]);
            let config = if value == "--config" {
                index += 1;
                words.get(index).cloned()
            } else {
                value
                    .strip_prefix("--config=")
                    .map(|value| words[index].with_value(value))
            };
            if config
                .as_ref()
                .is_some_and(cargo_config_override_obscures_sqlx_alias)
            {
                return true;
            }
            index += 1;
        }
        false
    })
}

fn cargo_config_override_obscures_sqlx_alias(config: &ShellWord) -> bool {
    if config.active_dollar || config.literal_dollar || config.dynamic {
        return true;
    }
    let text = config.value.trim();
    if !text.contains('=') {
        return true;
    }
    let Ok(config) = text.parse::<toml::Table>() else {
        return true;
    };
    if config.contains_key("include") {
        return true;
    }
    match config.get("alias") {
        None => false,
        Some(toml::Value::Table(aliases)) => aliases.contains_key("sqlx"),
        Some(_) => true,
    }
}

fn effective_cargo_config_obscures_sqlx_alias(
    root: &Path,
    command: &str,
    environment: &DoctorEnvironment,
) -> bool {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(invocation_directories) = cargo_sqlx_invocation_directories(&root, command) else {
        return true;
    };
    for invocation_directory in invocation_directories {
        for directory in invocation_directory.ancestors() {
            if cargo_config_directory_obscures_sqlx_alias(&directory.join(".cargo")) {
                return true;
            }
        }
    }

    let cargo_home = if let Some(cargo_home) = environment.cargo_home.as_deref() {
        let cargo_home = PathBuf::from(cargo_home);
        if !cargo_home.is_absolute() {
            return true;
        }
        Some(cargo_home)
    } else if let Some(home) = environment.home.as_deref() {
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return true;
        }
        Some(home.join(".cargo"))
    } else {
        None
    };
    cargo_home
        .as_deref()
        .is_some_and(cargo_config_directory_obscures_sqlx_alias)
}

fn cargo_sqlx_invocation_directories(root: &Path, command: &str) -> Option<Vec<PathBuf>> {
    let parsed = parse_shell_commands(command);
    if parsed.ambiguous {
        return None;
    }
    let mut cwd = root.to_path_buf();
    let mut directories = Vec::new();
    for (command_index, words) in parsed.commands.iter().enumerate() {
        let incoming = command_index
            .checked_sub(1)
            .and_then(|index| parsed.separators.get(index))
            .copied();
        let outgoing = parsed.separators.get(command_index).copied();
        if shell_command_changes_directory(words) {
            if !matches!(incoming, None | Some(ShellSeparator::Sequence))
                || outgoing != Some(ShellSeparator::And)
            {
                return None;
            }
            cwd = resolve_literal_cd(root, &cwd, words)?;
            continue;
        }
        let Some(program_index) = command_program_index(words) else {
            continue;
        };
        if executable_is_named(&shell_word_value(&words[program_index]), "cargo")
            && cargo_subcommand(words, program_index + 1).as_deref() == Some("sqlx")
        {
            directories.push(cwd.clone());
        }
    }
    (!directories.is_empty()).then_some(directories)
}

fn cargo_config_directory_obscures_sqlx_alias(directory: &Path) -> bool {
    [directory.join("config.toml"), directory.join("config")]
        .into_iter()
        .any(|path| cargo_config_obscures_sqlx_alias(&path))
}

fn cargo_config_obscures_sqlx_alias(path: &Path) -> bool {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let Ok(config) = text.parse::<toml::Value>() else {
        return true;
    };
    if config.get("include").is_some() {
        return true;
    }
    match config.get("alias") {
        None => false,
        Some(toml::Value::Table(aliases)) => aliases.contains_key("sqlx"),
        Some(_) => true,
    }
}

fn path_has_symlink_or_reparse_component(path: &Path) -> Option<bool> {
    use std::path::Component;

    if !path.is_absolute() {
        return None;
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
            }
            Component::CurDir => {}
            // Parent components make the raw lookup identity harder to
            // reason about. Bare PATH entries do not need them.
            Component::ParentDir => return None,
            Component::Normal(_) => {
                current.push(component.as_os_str());
                let metadata = fs::symlink_metadata(&current).ok()?;
                if metadata_is_symlink_or_reparse_point(&metadata) {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}

fn metadata_is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn bare_sqlx_probe_program(program: &str) -> bool {
    if program_has_explicit_path(program) {
        return false;
    }
    matches!(program, "sqlx" | "cargo-sqlx")
}

fn sanitized_probe_search_path(root: &Path, executable: &Path) -> Option<OsString> {
    let root = fs::canonicalize(root).ok()?;
    let executable = fs::canonicalize(executable).ok()?;
    let directory = executable.parent()?;
    if !directory.is_absolute() || directory.starts_with(root) {
        return None;
    }
    env::join_paths([directory]).ok()
}
