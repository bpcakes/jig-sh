//! Splitting a command string into shell tokens.

use super::syntax::{
    ShellLex, ShellSeparator, ShellToken, ShellWord, ShellWordOrigin, is_shell_separator_char,
};

pub(super) fn shell_tokens(command: &str) -> ShellLex {
    let mut state = ShellTokenState::new();
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = state.quote {
            consume_quoted_shell_char(ch, quote_ch, &mut state, &mut chars);
            continue;
        }
        consume_unquoted_shell_char(ch, &mut state, &mut chars);
    }

    state.finish()
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

pub(super) fn redirection_has_inline_target(redirection: &str) -> bool {
    let operator_len = [
        "&>>", "<<-", "<<<", "&>", ">>", "<<", ">&", "<&", ">|", "<>",
    ]
    .into_iter()
    .find(|operator| redirection.starts_with(operator))
    .map_or(1, str::len);
    redirection.len() > operator_len
}

struct ShellTokenState {
    tokens: Vec<ShellToken>,
    current: String,
    word_started: bool,
    origin: ShellWordOrigin,
    active_dollar: bool,
    literal_dollar: bool,
    dynamic: bool,
    quote: Option<char>,
    ambiguous: bool,
}

impl ShellTokenState {
    fn new() -> Self {
        Self {
            tokens: Vec::new(),
            current: String::new(),
            word_started: false,
            origin: ShellWordOrigin::default(),
            active_dollar: false,
            literal_dollar: false,
            dynamic: false,
            quote: None,
            ambiguous: false,
        }
    }

    fn push_word(&mut self) {
        push_shell_word(
            &mut self.tokens,
            &mut self.current,
            &mut self.word_started,
            &mut self.origin,
            &mut self.active_dollar,
            &mut self.literal_dollar,
            &mut self.dynamic,
        );
    }

    fn push_word_or_drop_fd_prefix(&mut self) {
        push_shell_word_or_drop_fd_prefix(
            &mut self.tokens,
            &mut self.current,
            &mut self.word_started,
            &mut self.origin,
            &mut self.active_dollar,
            &mut self.literal_dollar,
            &mut self.dynamic,
        );
    }

    fn finish(mut self) -> ShellLex {
        self.ambiguous |= self.quote.is_some();
        self.push_word();
        ShellLex {
            tokens: self.tokens,
            ambiguous: self.ambiguous,
        }
    }
}

fn consume_quoted_shell_char(
    ch: char,
    quote_ch: char,
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    if ch == quote_ch {
        state.quote = None;
        return;
    }
    if ch == '\\' && quote_ch == '"' {
        let Some(escaped) = chars.next() else {
            state.ambiguous = true;
            return;
        };
        if escaped == '\n' {
            return;
        }
        if escaped == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
            return;
        }
        state.literal_dollar |= escaped == '$';
        state.current.push(escaped);
        return;
    }
    if ch == '$' {
        if quote_ch == '\'' {
            state.literal_dollar = true;
        } else {
            state.active_dollar = true;
            if chars.peek() == Some(&'(') {
                state.ambiguous = true;
                state.dynamic = true;
                state.current.push(ch);
                push_command_substitution_tail(&mut state.current, chars);
                return;
            }
        }
    } else if ch == '`' && quote_ch != '\'' {
        state.dynamic = true;
        state.ambiguous = true;
        state.current.push(ch);
        push_backtick_substitution_tail(&mut state.current, chars);
        return;
    }
    state.current.push(ch);
}

fn consume_unquoted_shell_char(
    ch: char,
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    match ch {
        '\'' | '"' => begin_quoted_shell_word(ch, state),
        '\\' => consume_shell_escape(state, chars),
        '\n' | '\r' => consume_shell_line_break(ch, state, chars),
        ch if ch.is_whitespace() => state.push_word(),
        ';' | '(' | ')' => consume_shell_group_separator(ch, state),
        '&' if chars.peek() == Some(&'>') => consume_combined_redirection(state, chars),
        '&' | '|' => consume_shell_separator(ch, state, chars),
        '<' | '>' => consume_shell_redirection(ch, state, chars),
        '#' if !state.word_started => consume_shell_comment(state, chars),
        '$' => consume_unquoted_dollar(state, chars),
        '`' => consume_unquoted_backtick(state, chars),
        _ => {
            state.word_started = true;
            state.current.push(ch);
        }
    }
}

fn begin_quoted_shell_word(ch: char, state: &mut ShellTokenState) {
    state.word_started = true;
    state.origin.syntactically_plain = false;
    if !state.current.contains('=') {
        state.origin.assignment_name_plain = false;
    }
    state.quote = Some(ch);
}

fn consume_shell_escape(
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    let Some(escaped) = chars.next() else {
        state.ambiguous = true;
        return;
    };
    if escaped == '\n' {
        return;
    }
    if escaped == '\r' && chars.peek() == Some(&'\n') {
        chars.next();
        return;
    }
    state.origin.syntactically_plain = false;
    if !state.current.contains('=') {
        state.origin.assignment_name_plain = false;
    }
    state.literal_dollar |= escaped == '$';
    state.word_started = true;
    state.current.push(escaped);
}

fn consume_shell_line_break(
    ch: char,
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    state.push_word();
    if ch == '\r' && chars.peek() == Some(&'\n') {
        chars.next();
    }
    state
        .tokens
        .push(ShellToken::Separator(ShellSeparator::Sequence));
}

fn consume_shell_group_separator(ch: char, state: &mut ShellTokenState) {
    state.push_word();
    let separator = if ch == ';' {
        ShellSeparator::Sequence
    } else {
        ShellSeparator::Group
    };
    state.tokens.push(ShellToken::Separator(separator));
    state.ambiguous |= matches!(ch, '(' | ')');
}

fn consume_combined_redirection(
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    state.push_word();
    chars.next();
    let mut redirection = String::from("&>");
    if chars.peek() == Some(&'>') {
        redirection.push(chars.next().expect("peeked append redirection"));
    }
    finish_shell_redirection(state, redirection, chars);
}

fn consume_shell_separator(
    ch: char,
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    state.push_word();
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
    state.tokens.push(ShellToken::Separator(separator));
}

fn consume_shell_redirection(
    ch: char,
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    state.push_word_or_drop_fd_prefix();
    let mut redirection = String::from(ch);
    if chars.peek() == Some(&ch) {
        redirection.push(chars.next().expect("peeked redirection operator"));
        if ch == '<' && chars.peek().is_some_and(|next| matches!(*next, '-' | '<')) {
            redirection.push(chars.next().expect("peeked heredoc modifier"));
        }
    } else if (ch == '>' && chars.peek() == Some(&'|')) || (ch == '<' && chars.peek() == Some(&'>'))
    {
        redirection.push(chars.next().expect("peeked compound redirection"));
    }
    if chars.peek() == Some(&'&') {
        redirection.push(chars.next().expect("peeked redirection target marker"));
    }
    finish_shell_redirection(state, redirection, chars);
}

fn finish_shell_redirection(
    state: &mut ShellTokenState,
    mut redirection: String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    push_inline_redirection_target(&mut redirection, chars);
    state.ambiguous |= shell_fragment_has_active_command_substitution(&redirection);
    state.tokens.push(ShellToken::Redirection(redirection));
}

fn consume_shell_comment(
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    while let Some(comment_ch) = chars.next() {
        if matches!(comment_ch, '\n' | '\r') {
            if comment_ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            state
                .tokens
                .push(ShellToken::Separator(ShellSeparator::Sequence));
            break;
        }
    }
}

fn consume_unquoted_dollar(
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    state.word_started = true;
    state.active_dollar = true;
    state.current.push('$');
    if chars.peek() == Some(&'(') {
        state.ambiguous = true;
        state.dynamic = true;
        push_command_substitution_tail(&mut state.current, chars);
    }
}

fn consume_unquoted_backtick(
    state: &mut ShellTokenState,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    state.word_started = true;
    state.dynamic = true;
    state.ambiguous = true;
    state.current.push('`');
    push_backtick_substitution_tail(&mut state.current, chars);
}
