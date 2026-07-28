pub(crate) const MAX_SESSION_ID_BYTES: usize = 128;

pub(crate) fn is_valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_BYTES
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::{MAX_SESSION_ID_BYTES, is_valid_session_id};

    #[test]
    fn accepts_the_session_id_protocol_alphabet_and_byte_boundary() {
        assert!(is_valid_session_id("session.01-A_b"));
        assert!(is_valid_session_id(&"a".repeat(MAX_SESSION_ID_BYTES)));
    }

    #[test]
    fn rejects_empty_oversized_or_non_protocol_session_ids() {
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id(&"a".repeat(MAX_SESSION_ID_BYTES + 1)));
        assert!(!is_valid_session_id("contains spaces"));
        assert!(!is_valid_session_id("unicode-é"));
    }
}
