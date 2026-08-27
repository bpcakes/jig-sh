use std::fs;
use std::path::Path;

use serde_json::Value;

pub(super) const REPOSITORY_ROOT_REDACTION: &str = "<repository-root>";

pub(super) fn repository_root_spellings(root: &Path) -> Vec<String> {
    let mut spellings = vec![root.to_string_lossy().into_owned()];
    if let Ok(canonical) = fs::canonicalize(root) {
        spellings.push(canonical.to_string_lossy().into_owned());
    }
    for spelling in spellings.clone() {
        let portable = spelling.replace('\\', "/");
        if portable != spelling {
            spellings.push(portable);
        }
    }
    spellings.retain(|spelling| spelling.len() > 1);
    spellings.sort_by_key(|spelling| std::cmp::Reverse(spelling.len()));
    spellings.dedup();
    spellings
}

pub(super) fn redact_repository_root(value: &str, spellings: &[String]) -> String {
    spellings.iter().fold(value.to_string(), |redacted, root| {
        redact_path_bounded_occurrences(&redacted, root)
    })
}

pub(super) fn redact_repository_root_in_value(mut value: Value, spellings: &[String]) -> Value {
    fn redact(value: &mut Value, spellings: &[String]) {
        match value {
            Value::String(text) => *text = redact_repository_root(text, spellings),
            Value::Array(values) => {
                for value in values {
                    redact(value, spellings);
                }
            }
            Value::Object(values) => {
                for value in values.values_mut() {
                    redact(value, spellings);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    redact(&mut value, spellings);
    value
}

fn redact_path_bounded_occurrences(value: &str, root: &str) -> String {
    let mut redacted = value.to_string();
    let mut search_from = 0;
    while let Some(relative_start) = redacted[search_from..].find(root) {
        let start = search_from + relative_start;
        let end = start + root.len();
        let previous = redacted[..start].chars().next_back();
        let has_left_boundary = previous.is_none_or(is_left_token_boundary);
        let has_right_boundary = has_right_token_boundary(&redacted[end..], previous);
        if has_left_boundary && has_right_boundary {
            redacted.replace_range(start..end, REPOSITORY_ROOT_REDACTION);
            search_from = start + REPOSITORY_ROOT_REDACTION.len();
        } else {
            search_from = end;
        }
    }
    redacted
}

fn is_left_token_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '\0' | '"' | '\'' | '=' | ':' | ',' | ';' | '(' | '[' | '{' | '<'
        )
}

fn has_right_token_boundary(suffix: &str, previous: Option<char>) -> bool {
    let mut characters = suffix.chars();
    let Some(first) = characters.next() else {
        return true;
    };
    if matches!(first, '/' | '\\' | '\0') || first.is_whitespace() {
        return true;
    }
    if paired_delimiters(previous, first) {
        return true;
    }
    if !is_trailing_token_punctuation(first) {
        return false;
    }
    for character in characters {
        if character.is_whitespace() || character == '\0' {
            return true;
        }
        if !is_trailing_token_punctuation(character) {
            return false;
        }
    }
    true
}

fn paired_delimiters(previous: Option<char>, next: char) -> bool {
    matches!(
        (previous, next),
        (Some('"'), '"')
            | (Some('\''), '\'')
            | (Some('('), ')')
            | (Some('['), ']')
            | (Some('{'), '}')
            | (Some('<'), '>')
    )
}

fn is_trailing_token_punctuation(character: char) -> bool {
    matches!(
        character,
        '"' | '\'' | '=' | ':' | ',' | ';' | ')' | ']' | '}' | '>' | '.' | '!' | '?'
    )
}

#[cfg(test)]
mod tests {
    use super::{REPOSITORY_ROOT_REDACTION, redact_repository_root};

    #[test]
    fn repository_root_redaction_requires_a_path_boundary() {
        let root = "/example/repository".to_string();
        let value =
            "/example/repository/file /example/repository\\file /example/repository-backup/file";

        assert_eq!(
            redact_repository_root("/example/repository", std::slice::from_ref(&root)),
            REPOSITORY_ROOT_REDACTION
        );
        assert_eq!(
            redact_repository_root(value, &[root]),
            format!(
                "{REPOSITORY_ROOT_REDACTION}/file {REPOSITORY_ROOT_REDACTION}\\file /example/repository-backup/file"
            )
        );
    }

    #[test]
    fn repository_root_redaction_handles_multiple_bounded_occurrences() {
        let root = "/example/repository".to_string();

        assert_eq!(
            redact_repository_root("/example/repository/a:/example/repository/b", &[root]),
            format!("{REPOSITORY_ROOT_REDACTION}/a:{REPOSITORY_ROOT_REDACTION}/b")
        );
    }

    #[test]
    fn repository_root_redaction_requires_a_left_token_boundary() {
        let root = "/example/repository".to_string();
        let value = concat!(
            "/prefix/example/repository/file ",
            "suffix/example/repository/file ",
            "cwd=/example/repository/file ",
            "quoted=\"/example/repository/file\""
        );

        assert_eq!(
            redact_repository_root(value, &[root]),
            format!(
                "/prefix/example/repository/file suffix/example/repository/file cwd={REPOSITORY_ROOT_REDACTION}/file quoted=\"{REPOSITORY_ROOT_REDACTION}/file\""
            )
        );
    }

    #[test]
    fn repository_root_redaction_accepts_right_token_delimiters() {
        let root = "/example/repository".to_string();
        let value = concat!(
            "quoted=\"/example/repository\" ",
            "parenthesized=(/example/repository) ",
            "comma=/example/repository, ",
            "newline=/example/repository\n",
            "tab=/example/repository\t",
            "nul=/example/repository\0tail"
        );

        assert_eq!(
            redact_repository_root(value, &[root]),
            concat!(
                "quoted=\"<repository-root>\" ",
                "parenthesized=(<repository-root>) ",
                "comma=<repository-root>, ",
                "newline=<repository-root>\n",
                "tab=<repository-root>\t",
                "nul=<repository-root>\0tail"
            )
        );
    }

    #[test]
    fn repository_root_redaction_preserves_punctuation_prefixed_siblings() {
        let root = "/example/repository".to_string();
        let value = concat!(
            "/example/repository.backup ",
            "/example/repository,backup ",
            "/example/repository)backup"
        );

        assert_eq!(redact_repository_root(value, &[root]), value);
    }

    #[test]
    fn repository_root_redaction_accepts_sentence_and_unicode_boundaries() {
        let root = "/example/repository".to_string();
        let value = "/example/repository.\u{2003}/example/repository\u{2003}done";

        assert_eq!(
            redact_repository_root(value, &[root]),
            "<repository-root>.\u{2003}<repository-root>\u{2003}done"
        );
    }
}
