//! Removing here-document bodies before tokenizing.

use std::collections::VecDeque;

use super::syntax::is_shell_separator_char;

#[derive(Debug, Eq, PartialEq)]
struct HeredocSpec {
    delimiter: String,
    strip_tabs: bool,
    expands_body: bool,
}

pub(super) fn strip_heredoc_bodies(command: &str) -> (String, bool) {
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
                let parsed = parse_heredoc_spec(&chars, index + 2);
                index = parsed.next_index;
                ambiguous |= parsed.ambiguous;
                specs.extend(parsed.spec);
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

struct ParsedHeredocSpec {
    next_index: usize,
    spec: Option<HeredocSpec>,
    ambiguous: bool,
}

fn parse_heredoc_spec(chars: &[char], mut index: usize) -> ParsedHeredocSpec {
    let strip_tabs = chars.get(index) == Some(&'-');
    index += usize::from(strip_tabs);
    while chars.get(index).is_some_and(|ch| matches!(ch, ' ' | '\t')) {
        index += 1;
    }
    let mut delimiter = String::new();
    let mut quote = None;
    let mut was_quoted = false;
    let mut ambiguous = false;
    while let Some(&ch) = chars.get(index) {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                index += 1;
                match chars.get(index) {
                    Some(escaped) => delimiter.push(*escaped),
                    None => ambiguous = true,
                }
            } else {
                delimiter.push(ch);
            }
            index += 1;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            was_quoted = true;
            quote = Some(ch);
            index += 1;
            continue;
        }
        if ch == '\\' {
            was_quoted = true;
            index += 1;
            match chars.get(index) {
                Some(escaped) => {
                    delimiter.push(*escaped);
                    index += 1;
                }
                None => ambiguous = true,
            }
            continue;
        }
        if ch.is_whitespace() || is_shell_separator_char(ch) || matches!(ch, '<' | '>') {
            break;
        }
        delimiter.push(ch);
        index += 1;
    }
    let invalid = delimiter.is_empty() || quote.is_some();
    ambiguous |= invalid;
    let spec = (!invalid).then_some(HeredocSpec {
        delimiter,
        strip_tabs,
        expands_body: !was_quoted,
    });
    ParsedHeredocSpec {
        next_index: index,
        spec,
        ambiguous,
    }
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
