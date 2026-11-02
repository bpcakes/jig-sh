use std::io::{BufRead, Write};

use anyhow::{Context, Result, bail};
use jig_vault::{FieldKind, MAX_SECRET_VALUE_LEN, SecretBytes};
use zeroize::Zeroizing;

use super::lifecycle::{hidden_terminal_input_available, prompt_zeroizing};

pub(super) fn read_field_value_from_prompt(kind: FieldKind) -> Result<SecretBytes> {
    if !hidden_terminal_input_available() {
        bail!("--value-prompt requires an interactive terminal; use --value-stdin for automation");
    }
    if kind == FieldKind::Concealed {
        let mut value = prompt_zeroizing("Field value: ")
            .context("failed to read field value from terminal")?;
        return Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()));
    }
    let mut stderr = std::io::stderr().lock();
    write!(stderr, "Field value: ")?;
    stderr.flush()?;
    read_text_line(std::io::stdin().lock())
}

fn read_text_line(input: impl BufRead) -> Result<SecretBytes> {
    let mut value = Zeroizing::new(String::new());
    input
        .take((MAX_SECRET_VALUE_LEN + 2) as u64)
        .read_line(&mut value)
        .context("failed to read text field value from terminal")?;
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    if value.len() > MAX_SECRET_VALUE_LEN {
        bail!("text field value exceeds the maximum length");
    }
    Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_prompt_preserves_whitespace_and_accepts_empty_values() {
        for (input, expected) in [
            ("  example  \n", "  example  "),
            ("\r\n", ""),
            ("café\n", "café"),
        ] {
            assert_eq!(
                read_text_line(input.as_bytes()).unwrap().as_slice(),
                expected.as_bytes()
            );
        }
        assert!(read_text_line(vec![b'a'; MAX_SECRET_VALUE_LEN + 1].as_slice()).is_err());
    }
}
