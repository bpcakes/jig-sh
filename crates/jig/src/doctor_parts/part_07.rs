
fn command_database_url_scope(
    words: &[ShellWord],
    program_index: usize,
) -> CommandDatabaseUrlScope {
    let mut scope = CommandDatabaseUrlScope::Inherited;
    let mut index = 0;
    let mut allow_prefix_keyword = true;

    while index < program_index {
        let Some(word) = words.get(index) else {
            break;
        };
        if shell_word_is_assignment(word) {
            allow_prefix_keyword = false;
            apply_database_url_assignment(&mut scope, word, true);
        } else if !shell_word_is_prefix_keyword(word, allow_prefix_keyword) {
            break;
        }
        index += 1;
    }

    while index < program_index {
        let word = shell_word_value(&words[index]);
        match word.as_str() {
            "builtin" => match builtin_wrapper_target(words, index + 1) {
                WrapperTarget::Executable { index: target, .. }
                    if bash_builtin(&shell_word_value(&words[target])) =>
                {
                    index = target;
                }
                WrapperTarget::Executable { .. }
                | WrapperTarget::NoExternalExecutable
                | WrapperTarget::Ambiguous => return CommandDatabaseUrlScope::Ambiguous,
            },
            "command" => match command_wrapper_target(words, index + 1) {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if ambiguous {
                        scope = CommandDatabaseUrlScope::Ambiguous;
                    }
                    index = target;
                }
                WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                    return CommandDatabaseUrlScope::Ambiguous;
                }
            },
            "exec" => match exec_wrapper_target(words, index + 1) {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if exec_wrapper_clears_environment(words, index + 1, target) {
                        scope = CommandDatabaseUrlScope::Removed;
                    }
                    if ambiguous {
                        scope = CommandDatabaseUrlScope::Ambiguous;
                    }
                    index = target;
                }
                WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                    return CommandDatabaseUrlScope::Ambiguous;
                }
            },
            _ if executable_is_named(&word, "nohup") => {
                match nohup_wrapper_target(words, index + 1) {
                    WrapperTarget::Executable {
                        index: target,
                        ambiguous,
                    } => {
                        if ambiguous {
                            scope = CommandDatabaseUrlScope::Ambiguous;
                        }
                        index = target;
                    }
                    WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                        return CommandDatabaseUrlScope::Ambiguous;
                    }
                }
            }
            _ if executable_is_named(&word, "time") => {
                match time_wrapper_target(words, index + 1) {
                    WrapperTarget::Executable {
                        index: target,
                        ambiguous,
                    } => {
                        if ambiguous {
                            scope = CommandDatabaseUrlScope::Ambiguous;
                        }
                        index = target;
                    }
                    WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                        return CommandDatabaseUrlScope::Ambiguous;
                    }
                }
            }
            _ if executable_is_named(&word, "env") => {
                let env = parse_env_wrapper(words, index + 1);
                if env.ambiguous {
                    scope = CommandDatabaseUrlScope::Ambiguous;
                }
                if env.clears_environment
                    || env.unset_names.iter().any(|name| database_url_name(name))
                {
                    scope = CommandDatabaseUrlScope::Removed;
                }
                for assignment_index in env.assignment_indices {
                    apply_database_url_assignment(&mut scope, &words[assignment_index], false);
                }
                let Some(target) = env.target_index else {
                    return if env.ambiguous {
                        CommandDatabaseUrlScope::Ambiguous
                    } else {
                        scope
                    };
                };
                index = target;
            }
            _ => break,
        }
    }

    scope
}

fn apply_database_url_assignment(
    scope: &mut CommandDatabaseUrlScope,
    word: &ShellWord,
    require_plain_name: bool,
) {
    if require_plain_name && !word.assignment_name_plain {
        return;
    }
    let value = shell_word_value(word);
    let Some((raw_name, value)) = value.split_once('=') else {
        return;
    };
    let append = require_plain_name && raw_name.ends_with('+');
    let name = if require_plain_name {
        bash_assignment_base_name(raw_name).unwrap_or(raw_name)
    } else {
        raw_name
    };
    if database_url_name(name) {
        *scope = if append || require_plain_name && raw_name.contains('[') {
            CommandDatabaseUrlScope::Ambiguous
        } else {
            CommandDatabaseUrlScope::Assigned(word.with_value(value))
        };
    }
}

fn database_url_name(name: &str) -> bool {
    #[cfg(windows)]
    {
        name.eq_ignore_ascii_case("DATABASE_URL")
    }
    #[cfg(not(windows))]
    {
        name == "DATABASE_URL"
    }
}

fn exec_wrapper_clears_environment(
    words: &[ShellWord],
    mut index: usize,
    program_index: usize,
) -> bool {
    while index < program_index {
        let word = shell_word_value(&words[index]);
        if word == "--" {
            return false;
        }
        if word == "-a" {
            index += 2;
            continue;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            return false;
        };
        if !options.chars().all(|option| matches!(option, 'c' | 'l')) {
            return false;
        }
        if options.contains('c') {
            return true;
        }
        index += 1;
    }
    false
}

fn sqlx_driver_from_command_value(
    value: &ShellWord,
    source: SqlxDriverSource,
    ambient_database_url: Option<&OsStr>,
) -> SqlxDriverResolution {
    let text = value.value.trim();
    if matches!(text, "$DATABASE_URL" | "${DATABASE_URL}") {
        if value.literal_dollar || !value.active_dollar {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is quoted or escaped literally",
            );
        }
        if source == SqlxDriverSource::CommandFlag && value.syntactically_plain {
            return SqlxDriverResolution::Indeterminate(
                "an unquoted DATABASE_URL command option can split or expand into multiple arguments",
            );
        }
        let Some(database_url) = ambient_database_url else {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is unset",
            );
        };
        let Some(database_url) = database_url.to_str() else {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is not valid UTF-8",
            );
        };
        return sqlx_driver_from_literal(database_url, source);
    }
    if text.is_empty()
        || value.active_dollar
        || value.literal_dollar
        || value.dynamic
        || text.contains('`')
        || text.contains("$(")
    {
        return SqlxDriverResolution::Indeterminate(
            "an explicit database URL is empty or dynamically expanded",
        );
    }
    sqlx_driver_from_literal(text, source)
}

fn sqlx_driver_from_literal(value: &str, source: SqlxDriverSource) -> SqlxDriverResolution {
    SqlxDriver::from_database_url(value)
        .map(|driver| SqlxDriverResolution::Known(SqlxDriverRequirement { driver, source }))
        .unwrap_or(SqlxDriverResolution::Indeterminate(
            "DATABASE_URL does not identify a supported SQLx driver",
        ))
}

fn shell_command_mutates_database_url(words: &[ShellWord]) -> bool {
    shell_command_may_persist_variable_change(words, &database_url_name)
}

fn shell_command_may_persist_path_change(words: &[ShellWord]) -> bool {
    shell_command_may_persist_variable_change(words, &path_variable_name)
}

fn shell_command_may_change_dispatch_or_inject_execution(words: &[ShellWord]) -> bool {
    let ShellCommandName::Executable {
        index,
        force_external: false,
        ..
    } = shell_command_name(words)
    else {
        return false;
    };
    let command = shell_word_value(&words[index]);
    let arguments = &words[index + 1..];
    if arguments
        .iter()
        .any(|word| word.active_dollar || word.dynamic)
    {
        return matches!(command.as_str(), "hash" | "enable" | "trap");
    }
    match command.as_str() {
        "hash" => hash_command_may_change_dispatch(arguments),
        "enable" => enable_command_may_change_dispatch(arguments),
        "trap" => trap_command_may_inject_execution(arguments),
        _ => false,
    }
}

fn hash_command_may_change_dispatch(arguments: &[ShellWord]) -> bool {
    let mut query_or_delete_only = false;
    for argument in arguments {
        let value = shell_word_value(argument);
        if value == "--" {
            query_or_delete_only = false;
            continue;
        }
        if let Some(options) = value
            .strip_prefix('-')
            .filter(|options| !options.is_empty())
        {
            if options.contains('p') {
                return true;
            }
            if !options
                .chars()
                .all(|option| matches!(option, 'd' | 'l' | 'r' | 't'))
            {
                return true;
            }
            query_or_delete_only = options
                .chars()
                .any(|option| matches!(option, 'd' | 'l' | 't'));
            continue;
        }
        if !query_or_delete_only {
            return true;
        }
    }
    false
}

fn enable_command_may_change_dispatch(arguments: &[ShellWord]) -> bool {
    for argument in arguments {
        let value = shell_word_value(argument);
        if value == "--" {
            continue;
        }
        if let Some(options) = value
            .strip_prefix('-')
            .filter(|options| !options.is_empty())
        {
            if options
                .chars()
                .any(|option| matches!(option, 'd' | 'f' | 'n'))
            {
                return true;
            }
            if !options
                .chars()
                .all(|option| matches!(option, 'a' | 'p' | 's'))
            {
                return true;
            }
            continue;
        }
        return true;
    }
    false
}

fn trap_command_may_inject_execution(arguments: &[ShellWord]) -> bool {
    let Some(first) = arguments.first().map(shell_word_value) else {
        return false;
    };
    !matches!(first.as_str(), "-l" | "-p")
}

fn shell_command_may_persist_variable_change(
    words: &[ShellWord],
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut leading_assignment_end = 0;
    while words
        .get(leading_assignment_end)
        .is_some_and(shell_word_is_assignment)
    {
        leading_assignment_end += 1;
    }
    let leading_variable_assignment = words[..leading_assignment_end]
        .iter()
        .any(|word| bash_assignment_name(&shell_word_value(word)).is_some_and(variable_matches));
    if leading_assignment_end == words.len() {
        return leading_variable_assignment;
    }

    let index = match shell_command_name(words) {
        ShellCommandName::Executable {
            index,
            force_external: false,
            ..
        } => index,
        ShellCommandName::Executable {
            force_external: true,
            ..
        }
        | ShellCommandName::NoExternalExecutable { .. } => return false,
        ShellCommandName::AmbiguousWrapper { .. } => return true,
    };
    let command = shell_word_value(&words[index]);
    if matches!(command.as_str(), "." | "source" | "eval") {
        return true;
    }
    if matches!(
        command.as_str(),
        "declare" | "export" | "local" | "readonly" | "typeset" | "unset"
    ) {
        return leading_variable_assignment
            || declaration_uses_nameref(words, index + 1)
            || words
                .iter()
                .skip(index + 1)
                .any(|word| shell_word_names_variable(word, variable_matches));
    }
    if command == "read" {
        return leading_variable_assignment
            || read_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "printf" {
        return leading_variable_assignment
            || printf_mutates_variable(words, index + 1, variable_matches);
    }
    if matches!(command.as_str(), "mapfile" | "readarray") {
        return leading_variable_assignment
            || mapfile_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "getopts" {
        return leading_variable_assignment
            || getopts_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "let" {
        // Arithmetic expressions can assign through array subscripts and
        // namerefs. Reimplementing Bash arithmetic expansion here would risk
        // a false-negative driver or PATH result, so treat it conservatively.
        return true;
    }
    false
}

fn declaration_uses_nameref(words: &[ShellWord], mut index: usize) -> bool {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            return false;
        }
        let Some(options) = word
            .strip_prefix('-')
            .or_else(|| word.strip_prefix('+'))
            .filter(|options| !options.is_empty())
        else {
            return false;
        };
        if options.contains('n') {
            return true;
        }
        index += 1;
    }
    false
}

fn shell_word_names_variable(word: &ShellWord, variable_matches: &dyn Fn(&str) -> bool) -> bool {
    if word.active_dollar || word.dynamic {
        return true;
    }
    let word = shell_word_value(word);
    shell_variable_base_name(&word).is_some_and(variable_matches)
}

fn shell_variable_base_name(value: &str) -> Option<&str> {
    let candidate = value.split_once('=').map_or(value, |(name, _)| name);
    bash_assignment_base_name(candidate)
}

fn bash_assignment_base_name(candidate: &str) -> Option<&str> {
    let candidate = candidate.strip_suffix('+').unwrap_or(candidate);
    let candidate = candidate
        .split_once('[')
        .map_or(candidate, |(name, _)| name);
    let mut chars = candidate.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
    .then_some(candidate)
}

fn read_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut options_allowed = true;
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if options_allowed && value == "--" {
            options_allowed = false;
            index += 1;
            continue;
        }
        if options_allowed {
            if let Some(options) = value.strip_prefix('-').filter(|value| !value.is_empty()) {
                let chars = options.char_indices();
                let mut consumed_next = false;
                for (offset, option) in chars {
                    match option {
                        'e' | 'r' | 's' => {}
                        'a' | 'd' | 'i' | 'n' | 'N' | 'p' | 't' | 'u' => {
                            let value_offset = offset + option.len_utf8();
                            let attached = &options[value_offset..];
                            let argument = if attached.is_empty() {
                                let Some(argument) = words.get(index + 1) else {
                                    return true;
                                };
                                consumed_next = true;
                                argument
                            } else {
                                word
                            };
                            if option == 'a' {
                                if attached.is_empty() {
                                    if shell_word_names_variable(argument, variable_matches) {
                                        return true;
                                    }
                                } else if bash_assignment_base_name(attached)
                                    .is_some_and(variable_matches)
                                {
                                    return true;
                                }
                            }
                            break;
                        }
                        _ => return true,
                    }
                }
                index += usize::from(consumed_next) + 1;
                continue;
            }
        }
        options_allowed = false;
        if shell_word_names_variable(word, variable_matches) {
            return true;
        }
        index += 1;
    }
    false
}

fn printf_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if value == "--" {
            return false;
        }
        if value == "-v" {
            let Some(variable) = words.get(index + 1) else {
                return true;
            };
            return shell_word_names_variable(variable, variable_matches);
        }
        if let Some(variable) = value.strip_prefix("-v").filter(|value| !value.is_empty()) {
            return bash_assignment_base_name(variable).is_some_and(variable_matches);
        }
        if value.starts_with('-') {
            index += 1;
            continue;
        }
        return false;
    }
    false
}

fn mapfile_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut options_allowed = true;
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if options_allowed && value == "--" {
            options_allowed = false;
            index += 1;
            continue;
        }
        if options_allowed {
            if let Some(options) = value.strip_prefix('-').filter(|value| !value.is_empty()) {
                let chars = options.char_indices();
                let mut consumed_next = false;
                for (offset, option) in chars {
                    match option {
                        't' => {}
                        'C' => {
                            // The callback is evaluated as Bash code and can
                            // mutate arbitrary variables through namerefs.
                            return true;
                        }
                        'c' | 'd' | 'n' | 'O' | 's' | 'u' => {
                            let value_offset = offset + option.len_utf8();
                            if options[value_offset..].is_empty() {
                                if words.get(index + 1).is_none() {
                                    return true;
                                }
                                consumed_next = true;
                            }
                            break;
                        }
                        _ => return true,
                    }
                }
                index += usize::from(consumed_next) + 1;
                continue;
            }
        }
        return shell_word_names_variable(word, variable_matches);
    }
    false
}

fn getopts_mutates_variable(
    words: &[ShellWord],
    index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let Some(optstring) = words.get(index) else {
        return false;
    };
    if optstring.active_dollar || optstring.dynamic {
        return true;
    }
    let Some(name) = words.get(index + 1) else {
        return false;
    };
    shell_word_names_variable(name, variable_matches)
}

fn path_variable_name(name: &str) -> bool {
    #[cfg(windows)]
    {
        name.eq_ignore_ascii_case("PATH")
    }
    #[cfg(not(windows))]
    {
        name == "PATH"
    }
}

fn env_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    (!name.is_empty()).then_some(name)
}

fn shell_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
    .then_some(name)
}

fn bash_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    bash_assignment_base_name(name)
}

fn shell_word_is_assignment(word: &ShellWord) -> bool {
    word.assignment_name_plain && bash_assignment_name(&word.value).is_some()
}

fn shell_path_assignment_is_literal(word: &ShellWord) -> bool {
    let Some((name, _)) = word.value.split_once('=') else {
        return false;
    };
    !name.ends_with('+')
        && !name.contains('[')
        && bash_assignment_base_name(name).is_some_and(path_variable_name)
        && path_assignment_is_literal(word)
}

fn path_assignment_is_literal(word: &ShellWord) -> bool {
    !word.active_dollar
        && !word.dynamic
        && path_assignment_value(word).is_some_and(|value| {
            env::split_paths(&value)
                .all(|entry| !entry.to_str().is_some_and(|entry| entry.starts_with('~')))
        })
}

fn path_assignment_value(word: &ShellWord) -> Option<OsString> {
    word.value
        .split_once('=')
        .map(|(_, value)| OsString::from(value))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ShellWord {
    value: String,
    syntactically_plain: bool,
    assignment_name_plain: bool,
    active_dollar: bool,
    literal_dollar: bool,
    dynamic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShellWordOrigin {
    syntactically_plain: bool,
    assignment_name_plain: bool,
}

impl Default for ShellWordOrigin {
    fn default() -> Self {
        Self {
            syntactically_plain: true,
            assignment_name_plain: true,
        }
    }
}

impl ShellWord {
    fn with_value(&self, value: &str) -> Self {
        Self {
            value: value.to_string(),
            syntactically_plain: self.syntactically_plain,
            assignment_name_plain: self.assignment_name_plain,
            active_dollar: self.active_dollar,
            literal_dollar: self.literal_dollar,
            dynamic: self.dynamic,
        }
    }
}

fn shell_word_value(word: &ShellWord) -> String {
    word.value.clone()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellSeparator {
    And,
    Or,
    Sequence,
    Pipe,
    Background,
    Group,
}

#[derive(Debug)]
struct ShellParse {
    commands: Vec<Vec<ShellWord>>,
    separators: Vec<ShellSeparator>,
    ambiguous: bool,
}
