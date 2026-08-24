fn parse_shell_commands(command: &str) -> ShellParse {
    let (command, heredoc_ambiguous) = strip_heredoc_bodies(command);
    let mut lexed = shell_tokens(&command);
    lexed.ambiguous |= heredoc_ambiguous;
    let mut commands = Vec::new();
    let mut separators = Vec::new();
    let mut current = Vec::new();
    let mut skip_next_word = false;

    for token in lexed.tokens {
        match token {
            ShellToken::Word(word) => {
                if skip_next_word {
                    skip_next_word = false;
                } else {
                    current.push(word);
                }
            }
            ShellToken::Redirection(redirection) => {
                skip_next_word = !redirection_has_inline_target(&redirection);
            }
            ShellToken::Separator(separator) => {
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                    separators.push(separator);
                }
                skip_next_word = false;
            }
        }
    }

    if !current.is_empty() {
        commands.push(current);
    }

    let uses_control_flow = commands.iter().any(|words| {
        let uses_non_time_control_flow = words.iter().any(|word| {
            word.syntactically_plain
                && matches!(
                    shell_word_value(word).as_str(),
                    "[[" | "]]"
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
                        | "until"
                        | "while"
                        | "{"
                        | "}"
                )
        });
        let uses_time_keyword = matches!(
            shell_command_name(words),
            ShellCommandName::Executable {
                index,
                force_external: false,
                allow_keyword: true,
                ..
            } if shell_word_value(&words[index]) == "time" && shell_word_is_keyword(&words[index])
        );
        uses_non_time_control_flow || uses_time_keyword
    });
    ShellParse {
        commands,
        separators,
        ambiguous: lexed.ambiguous || uses_control_flow,
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ShellToken {
    Word(ShellWord),
    Separator(ShellSeparator),
    Redirection(String),
}

#[derive(Debug)]
struct ShellLex {
    tokens: Vec<ShellToken>,
    ambiguous: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct HeredocSpec {
    delimiter: String,
    strip_tabs: bool,
    expands_body: bool,
}

fn strip_heredoc_bodies(command: &str) -> (String, bool) {
    let mut rendered = String::with_capacity(command.len());
    let mut pending: VecDeque<HeredocSpec> = VecDeque::new();
    let mut ambiguous = false;

    for line_with_ending in command.split_inclusive('\n') {
        let line = line_with_ending
            .strip_suffix('\n')
            .unwrap_or(line_with_ending)
            .strip_suffix('\r')
            .unwrap_or_else(|| {
                line_with_ending
                    .strip_suffix('\n')
                    .unwrap_or(line_with_ending)
            });
        if let Some(spec) = pending.front() {
            let candidate = if spec.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if candidate == spec.delimiter {
                pending.pop_front();
            } else if spec.expands_body && heredoc_body_has_active_command_substitution(line) {
                ambiguous = true;
            }
            continue;
        }

        rendered.push_str(line_with_ending);
        let (specs, line_ambiguous) = heredoc_specs_on_line(line);
        pending.extend(specs);
        ambiguous |= line_ambiguous;
    }

    ambiguous |= !pending.is_empty();
    (rendered, ambiguous)
}

fn heredoc_specs_on_line(line: &str) -> (Vec<HeredocSpec>, bool) {
    let chars = line.chars().collect::<Vec<_>>();
    let mut specs = Vec::new();
    let mut ambiguous = false;
    let mut quote = None;
    let mut at_word_start = true;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                index += 1;
            }
            index += 1;
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                at_word_start = false;
                index += 1;
            }
            '\\' => {
                at_word_start = false;
                index = (index + 2).min(chars.len());
            }
            '#' if at_word_start => break,
            '<' if chars.get(index + 1) == Some(&'<') => {
                if chars.get(index + 2) == Some(&'<') {
                    ambiguous = true;
                    index += 3;
                    continue;
                }
                index += 2;
                let strip_tabs = chars.get(index) == Some(&'-');
                if strip_tabs {
                    index += 1;
                }
                while chars.get(index).is_some_and(|ch| matches!(ch, ' ' | '\t')) {
                    index += 1;
                }
                let mut delimiter = String::new();
                let mut delimiter_quote = None;
                let mut delimiter_was_quoted = false;
                while let Some(&delimiter_ch) = chars.get(index) {
                    if let Some(quote_ch) = delimiter_quote {
                        if delimiter_ch == quote_ch {
                            delimiter_quote = None;
                        } else if delimiter_ch == '\\' && quote_ch == '"' {
                            index += 1;
                            if let Some(escaped) = chars.get(index) {
                                delimiter.push(*escaped);
                            } else {
                                ambiguous = true;
                            }
                        } else {
                            delimiter.push(delimiter_ch);
                        }
                        index += 1;
                        continue;
                    }
                    if matches!(delimiter_ch, '\'' | '"') {
                        delimiter_was_quoted = true;
                        delimiter_quote = Some(delimiter_ch);
                        index += 1;
                        continue;
                    }
                    if delimiter_ch == '\\' {
                        delimiter_was_quoted = true;
                        index += 1;
                        if let Some(escaped) = chars.get(index) {
                            delimiter.push(*escaped);
                            index += 1;
                        } else {
                            ambiguous = true;
                        }
                        continue;
                    }
                    if delimiter_ch.is_whitespace()
                        || is_shell_separator_char(delimiter_ch)
                        || matches!(delimiter_ch, '<' | '>')
                    {
                        break;
                    }
                    delimiter.push(delimiter_ch);
                    index += 1;
                }
                if delimiter.is_empty() || delimiter_quote.is_some() {
                    ambiguous = true;
                } else {
                    specs.push(HeredocSpec {
                        delimiter,
                        strip_tabs,
                        expands_body: !delimiter_was_quoted,
                    });
                }
                at_word_start = true;
            }
            ch if ch.is_whitespace() || is_shell_separator_char(ch) => {
                at_word_start = true;
                index += 1;
            }
            _ => {
                at_word_start = false;
                index += 1;
            }
        }
    }
    ambiguous |= quote.is_some();
    (specs, ambiguous)
}

fn heredoc_body_has_active_command_substitution(line: &str) -> bool {
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '`' => return true,
            '$' if chars.peek() == Some(&'(') => return true,
            _ => {}
        }
    }
    false
}

fn shell_tokens(command: &str) -> ShellLex {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut word_started = false;
    let mut origin = ShellWordOrigin::default();
    let mut active_dollar = false;
    let mut literal_dollar = false;
    let mut dynamic = false;
    let mut quote = None;
    let mut ambiguous = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                if let Some(escaped) = chars.next() {
                    if escaped == '\n' {
                        continue;
                    }
                    if escaped == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                        continue;
                    }
                    if escaped == '$' {
                        literal_dollar = true;
                    }
                    current.push(escaped);
                } else {
                    ambiguous = true;
                }
            } else {
                if ch == '$' {
                    if quote_ch == '\'' {
                        literal_dollar = true;
                    } else {
                        active_dollar = true;
                        if chars.peek() == Some(&'(') {
                            ambiguous = true;
                            dynamic = true;
                            current.push(ch);
                            push_command_substitution_tail(&mut current, &mut chars);
                            continue;
                        }
                    }
                } else if ch == '`' && quote_ch != '\'' {
                    dynamic = true;
                    ambiguous = true;
                    current.push(ch);
                    push_backtick_substitution_tail(&mut current, &mut chars);
                    continue;
                }
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                word_started = true;
                origin.syntactically_plain = false;
                if !current.contains('=') {
                    origin.assignment_name_plain = false;
                }
                quote = Some(ch);
            }
            '\\' => {
                if let Some(escaped) = chars.next() {
                    if escaped == '\n' {
                        continue;
                    }
                    if escaped == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                        continue;
                    }
                    origin.syntactically_plain = false;
                    if !current.contains('=') {
                        origin.assignment_name_plain = false;
                    }
                    if escaped == '$' {
                        literal_dollar = true;
                    }
                    word_started = true;
                    current.push(escaped);
                } else {
                    ambiguous = true;
                }
            }
            '\n' | '\r' => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                tokens.push(ShellToken::Separator(ShellSeparator::Sequence));
            }
            ch if ch.is_whitespace() => push_shell_word(
                &mut tokens,
                &mut current,
                &mut word_started,
                &mut origin,
                &mut active_dollar,
                &mut literal_dollar,
                &mut dynamic,
            ),
            ';' | '(' | ')' => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                tokens.push(ShellToken::Separator(if ch == ';' {
                    ShellSeparator::Sequence
                } else {
                    ShellSeparator::Group
                }));
                ambiguous |= matches!(ch, '(' | ')');
            }
            '&' if chars.peek() == Some(&'>') => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                chars.next();
                let mut redirection = String::from("&>");
                if chars.peek() == Some(&'>') {
                    redirection.push(chars.next().expect("peeked append redirection"));
                }
                push_inline_redirection_target(&mut redirection, &mut chars);
                ambiguous |= shell_fragment_has_active_command_substitution(&redirection);
                tokens.push(ShellToken::Redirection(redirection));
            }
            '&' | '|' => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                let doubled = chars.peek() == Some(&ch);
                if doubled {
                    chars.next();
                }
                let separator = match (ch, doubled) {
                    ('&', true) => ShellSeparator::And,
                    ('|', true) => ShellSeparator::Or,
                    ('&', false) => ShellSeparator::Background,
                    ('|', false) => ShellSeparator::Pipe,
                    _ => unreachable!(),
                };
                tokens.push(ShellToken::Separator(separator));
            }
            '<' | '>' => {
                push_shell_word_or_drop_fd_prefix(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                let mut redirection = String::from(ch);
                if chars.peek() == Some(&ch) {
                    redirection.push(chars.next().expect("peeked redirection operator"));
                    if ch == '<' && chars.peek().is_some_and(|next| matches!(*next, '-' | '<')) {
                        redirection.push(chars.next().expect("peeked heredoc modifier"));
                    }
                } else if (ch == '>' && chars.peek() == Some(&'|'))
                    || (ch == '<' && chars.peek() == Some(&'>'))
                {
                    redirection.push(chars.next().expect("peeked compound redirection"));
                }
                if chars.peek() == Some(&'&') {
                    redirection.push(chars.next().expect("peeked redirection target marker"));
                }
                push_inline_redirection_target(&mut redirection, &mut chars);
                ambiguous |= shell_fragment_has_active_command_substitution(&redirection);
                tokens.push(ShellToken::Redirection(redirection));
            }
            '#' if !word_started => {
                while let Some(comment_ch) = chars.next() {
                    if matches!(comment_ch, '\n' | '\r') {
                        if comment_ch == '\r' && chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        tokens.push(ShellToken::Separator(ShellSeparator::Sequence));
                        break;
                    }
                }
            }
            '$' => {
                word_started = true;
                active_dollar = true;
                current.push(ch);
                if chars.peek() == Some(&'(') {
                    ambiguous = true;
                    dynamic = true;
                    push_command_substitution_tail(&mut current, &mut chars);
                }
            }
            '`' => {
                word_started = true;
                dynamic = true;
                ambiguous = true;
                current.push(ch);
                push_backtick_substitution_tail(&mut current, &mut chars);
            }
            _ => {
                word_started = true;
                current.push(ch);
            }
        }
    }

    ambiguous |= quote.is_some();
    push_shell_word(
        &mut tokens,
        &mut current,
        &mut word_started,
        &mut origin,
        &mut active_dollar,
        &mut literal_dollar,
        &mut dynamic,
    );
    ShellLex { tokens, ambiguous }
}

fn push_command_substitution_tail(
    rendered: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    let Some(opener) = chars.next() else {
        return;
    };
    debug_assert_eq!(opener, '(');
    rendered.push(opener);
    let mut depth = 1usize;
    let mut quote = None;
    while let Some(ch) = chars.next() {
        rendered.push(ch);
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\'
                && quote_ch != '\''
                && let Some(escaped) = chars.next()
            {
                rendered.push(escaped);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '\\' => {
                if let Some(escaped) = chars.next() {
                    rendered.push(escaped);
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            _ => {}
        }
    }
}

fn push_backtick_substitution_tail(
    rendered: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    while let Some(ch) = chars.next() {
        rendered.push(ch);
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                rendered.push(escaped);
            }
        } else if ch == '`' {
            return;
        }
    }
}

fn push_shell_word(
    tokens: &mut Vec<ShellToken>,
    current: &mut String,
    word_started: &mut bool,
    origin: &mut ShellWordOrigin,
    active_dollar: &mut bool,
    literal_dollar: &mut bool,
    dynamic: &mut bool,
) {
    if *word_started {
        tokens.push(ShellToken::Word(ShellWord {
            value: std::mem::take(current),
            syntactically_plain: origin.syntactically_plain,
            assignment_name_plain: origin.assignment_name_plain,
            active_dollar: std::mem::take(active_dollar),
            literal_dollar: std::mem::take(literal_dollar),
            dynamic: std::mem::take(dynamic),
        }));
    }
    *word_started = false;
    *origin = ShellWordOrigin::default();
}

fn push_shell_word_or_drop_fd_prefix(
    tokens: &mut Vec<ShellToken>,
    current: &mut String,
    word_started: &mut bool,
    origin: &mut ShellWordOrigin,
    active_dollar: &mut bool,
    literal_dollar: &mut bool,
    dynamic: &mut bool,
) {
    if origin.syntactically_plain
        && !current.is_empty()
        && current.chars().all(|ch| ch.is_ascii_digit())
    {
        current.clear();
        *word_started = false;
        *origin = ShellWordOrigin::default();
        *active_dollar = false;
        *literal_dollar = false;
        *dynamic = false;
    } else {
        push_shell_word(
            tokens,
            current,
            word_started,
            origin,
            active_dollar,
            literal_dollar,
            dynamic,
        );
    }
}

fn push_inline_redirection_target(
    redirection: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    let mut quote = None;
    let mut substitution_quote = None;
    let mut substitution_depth = 0usize;
    while let Some(next) = chars.peek().copied() {
        if substitution_depth > 0 {
            let ch = chars.next().expect("peeked command substitution character");
            redirection.push(ch);
            if let Some(quote_ch) = substitution_quote {
                if ch == quote_ch {
                    substitution_quote = None;
                } else if ch == '\\'
                    && quote_ch != '\''
                    && let Some(escaped) = chars.next()
                {
                    redirection.push(escaped);
                }
                continue;
            }
            match ch {
                '\'' | '"' => substitution_quote = Some(ch),
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        redirection.push(escaped);
                    }
                }
                '(' => substitution_depth += 1,
                ')' => substitution_depth -= 1,
                _ => {}
            }
            continue;
        }
        if quote.is_none()
            && (next.is_whitespace() || is_shell_separator_char(next) || matches!(next, '<' | '>'))
        {
            break;
        }
        let ch = chars.next().expect("peeked redirection target");
        redirection.push(ch);
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                if let Some(escaped) = chars.next() {
                    redirection.push(escaped);
                }
            } else if ch == '$' && quote_ch != '\'' && chars.peek() == Some(&'(') {
                redirection.push(chars.next().expect("peeked command substitution opener"));
                substitution_depth = 1;
            } else if ch == '`' && quote_ch != '\'' {
                push_backtick_substitution_tail(redirection, chars);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '\\' => {
                if let Some(escaped) = chars.next() {
                    redirection.push(escaped);
                }
            }
            '$' if chars.peek() == Some(&'(') => {
                redirection.push(chars.next().expect("peeked command substitution opener"));
                substitution_depth = 1;
            }
            '`' => push_backtick_substitution_tail(redirection, chars),
            _ => {}
        }
    }
}

fn shell_fragment_has_active_command_substitution(fragment: &str) -> bool {
    let mut quote = None;
    let mut chars = fragment.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                chars.next();
            } else if quote_ch != '\'' && (ch == '`' || ch == '$' && chars.peek() == Some(&'(')) {
                return true;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '\\' => {
                chars.next();
            }
            '`' => return true,
            '$' if chars.peek() == Some(&'(') => return true,
            _ => {}
        }
    }
    false
}

fn redirection_has_inline_target(redirection: &str) -> bool {
    let operator_len = [
        "&>>", "<<-", "<<<", "&>", ">>", "<<", ">&", "<&", ">|", "<>",
    ]
    .into_iter()
    .find(|operator| redirection.starts_with(operator))
    .map_or(1, str::len);
    redirection.len() > operator_len
}

const fn is_shell_separator_char(ch: char) -> bool {
    matches!(ch, ';' | '&' | '|' | '(' | ')')
}

fn shell_command_changes_directory(words: &[ShellWord]) -> bool {
    let ShellCommandName::Executable {
        index,
        force_external: false,
        ..
    } = shell_command_name(words)
    else {
        return false;
    };
    matches!(
        shell_word_value(&words[index]).as_str(),
        "cd" | "pushd" | "popd"
    )
}
