use std::fmt::Write;

pub(crate) fn push_length_prefixed_field(output: &mut String, name: &str, value: &str) {
    // Lengths are UTF-8 byte counts, not character counts. This encoding is
    // authenticated persisted data and must remain byte-for-byte stable.
    writeln!(output, "{name}:{}:{value}", value.len()).expect("writing to String cannot fail");
}

#[cfg(test)]
mod tests {
    use super::push_length_prefixed_field;

    #[test]
    fn field_length_is_the_utf8_byte_count() {
        let mut output = String::new();

        push_length_prefixed_field(&mut output, "label", "é🙂");

        assert_eq!(output, "label:6:é🙂\n");
    }
}
