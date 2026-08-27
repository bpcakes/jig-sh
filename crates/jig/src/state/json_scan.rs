use anyhow::{Result, anyhow, bail};

pub(super) fn skip_json_value(input: &[u8], start: usize, end: usize) -> Result<usize> {
    let start = skip_whitespace(input, start, end);
    match input.get(start).copied() {
        Some(b'"') => skip_json_string(input, start, end),
        Some(b'{') | Some(b'[') => skip_json_composite(input, start, end),
        Some(_) => {
            let mut cursor = start;
            while cursor < end
                && !input[cursor].is_ascii_whitespace()
                && !matches!(input[cursor], b',' | b']' | b'}')
            {
                cursor += 1;
            }
            if cursor == start {
                bail!("Expected JSON value at byte {start}");
            }
            Ok(cursor)
        }
        None => bail!("Expected JSON value at byte {start}"),
    }
}

fn skip_json_composite(input: &[u8], start: usize, end: usize) -> Result<usize> {
    let mut cursor = start;
    let mut stack = Vec::new();
    while cursor < end {
        match input[cursor] {
            b'"' => cursor = skip_json_string(input, cursor, end)?,
            b'{' => {
                stack.push(b'}');
                cursor += 1;
            }
            b'[' => {
                stack.push(b']');
                cursor += 1;
            }
            b'}' | b']' => {
                let expected = stack
                    .pop()
                    .ok_or_else(|| anyhow!("Unexpected JSON delimiter at byte {cursor}"))?;
                if input[cursor] != expected {
                    bail!("Mismatched JSON delimiter at byte {cursor}");
                }
                cursor += 1;
                if stack.is_empty() {
                    return Ok(cursor);
                }
            }
            _ => cursor += 1,
        }
    }
    bail!("Unterminated JSON value at byte {start}")
}

pub(super) fn skip_json_string(input: &[u8], start: usize, end: usize) -> Result<usize> {
    let mut cursor = start
        .checked_add(1)
        .ok_or_else(|| anyhow!("JSON string offset overflow"))?;
    while cursor < end {
        match input[cursor] {
            b'\\' => {
                cursor = cursor
                    .checked_add(2)
                    .ok_or_else(|| anyhow!("JSON string offset overflow"))?;
            }
            b'"' => {
                return cursor
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("JSON string offset overflow"));
            }
            _ => cursor += 1,
        }
    }
    bail!("Unterminated JSON string at byte {start}")
}

pub(super) fn skip_whitespace(input: &[u8], mut cursor: usize, end: usize) -> usize {
    while cursor < end && input[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::skip_json_value;

    #[test]
    fn locates_nested_value_with_strings_and_escapes() {
        let input = br#"  {"items":[{"text":"a\\\"}]b"},null]} trailing"#;

        assert_eq!(skip_json_value(input, 0, input.len()).unwrap(), 38);
    }

    #[test]
    fn rejects_mismatched_nested_delimiters() {
        let error = skip_json_value(b"{]", 0, 2).unwrap_err();

        assert_eq!(error.to_string(), "Mismatched JSON delimiter at byte 1");
    }

    #[test]
    fn respects_the_bounded_range() {
        let error = skip_json_value(br#"["value"]"#, 0, 8).unwrap_err();

        assert_eq!(error.to_string(), "Unterminated JSON value at byte 0");
    }

    #[test]
    fn locates_primitive_value_after_whitespace() {
        assert_eq!(skip_json_value(b"  false,", 0, 8).unwrap(), 7);
    }
}
