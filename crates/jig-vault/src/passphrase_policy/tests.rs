use super::*;

/// Strong 15-byte prefix; appending one more byte meets the length floor.
const STRONG_15: &str = "Qz7#vR2!pL9@xK4";

fn accepted(candidate: &str) -> bool {
    check_new_passphrase(candidate).is_ok()
}

#[test]
fn guess_floor_is_exactly_two_to_the_fortieth() {
    assert_eq!(MIN_MASTER_PASSPHRASE_GUESSES, 1_099_511_627_776);
    assert!(!meets_guess_floor(MIN_MASTER_PASSPHRASE_GUESSES - 1));
    assert!(meets_guess_floor(MIN_MASTER_PASSPHRASE_GUESSES));
    assert!(meets_guess_floor(MIN_MASTER_PASSPHRASE_GUESSES + 1));
    assert!(meets_guess_floor(u64::MAX));
    assert!(!meets_guess_floor(0));
}

#[test]
fn length_floor_counts_utf8_bytes_not_characters() {
    assert_eq!(STRONG_15.len(), 15);
    assert!(meets_guess_floor(estimated_guesses(STRONG_15)));
    assert!(
        !accepted(STRONG_15),
        "a strong estimate cannot waive length"
    );
    assert!(accepted(&format!("{STRONG_15}$")));

    // 13 characters but 15 bytes stays short; 14 characters and 16 bytes
    // meets the floor even though it has fewer than 16 characters.
    let short_multibyte = "Qz7#vR2!pL9@界";
    assert_eq!(
        (short_multibyte.len(), short_multibyte.chars().count()),
        (15, 13)
    );
    assert!(!accepted(short_multibyte));
    let multibyte = "Qz7#vR2!pL9@x界";
    assert_eq!((multibyte.len(), multibyte.chars().count()), (16, 14));
    assert!(accepted(multibyte));
}

#[test]
fn input_is_neither_trimmed_nor_normalized() {
    // A trailing space is part of the passphrase and of its length.
    assert!(accepted(&format!("{STRONG_15} ")));
    // Composed and decomposed spellings differ in bytes and are judged as
    // the exact input: only the decomposed one reaches 16 bytes.
    let composed = "Qz7#vR2!pL9@é";
    let decomposed = "Qz7#vR2!pL9@e\u{301}";
    assert_eq!((composed.len(), decomposed.len()), (14, 15));
    assert!(!accepted(&format!("{composed}x")));
    assert!(accepted(&format!("{decomposed}x")));
}

#[test]
fn common_repetitive_and_predictable_long_inputs_are_rejected() {
    for candidate in [
        "passwordpasswordpassword",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "1234567890123456",
        "qwertyuiopasdfghjklzxcvbnm",
        "abcdefghijklmnopqrstuvwxyz",
        "iloveyouiloveyouiloveyou",
        "                        ",
    ] {
        assert!(
            candidate.len() >= MIN_MASTER_PASSPHRASE_LEN,
            "{candidate:?}"
        );
        assert!(!accepted(candidate), "{candidate:?}");
    }
}

#[test]
fn year_independent_strong_examples_are_accepted() {
    for candidate in [
        "Qz7#vR2!pL9@xK4$",
        "correct horse battery staple",
        "otter-quartz-lantern-mosaic-velvet",
        "Tr0ub4dor&3-Tr0ub4dor&3",
    ] {
        assert!(accepted(candidate), "{candidate:?}");
    }
}

#[test]
fn date_bearing_candidates_are_judged_with_robust_margins() {
    // Far below the floor in any reference year: a dictionary word plus a
    // recent year, or a bare date.
    for candidate in [
        "ExampleProject2026",
        "password1999password",
        "12/31/1999-12/31/1999",
    ] {
        assert!(!accepted(candidate), "{candidate:?}");
    }
    // Far above the floor in any reference year: a year does not weaken an
    // otherwise strong candidate.
    for candidate in ["Qz7#vR2!pL9@xK4$1999", "otter-quartz-lantern-2031-mosaic"] {
        assert!(accepted(candidate), "{candidate:?}");
    }
}

#[test]
fn only_the_first_hundred_characters_are_estimated() {
    let weak_prefix = "a".repeat(100);
    assert!(!accepted(&format!("{weak_prefix}{STRONG_15}$")));
    let strong_prefix = format!("{STRONG_15}$");
    assert!(accepted(&format!("{strong_prefix}{}", "a".repeat(200))));
    let hundred_chars = format!("{strong_prefix}{}", "界".repeat(84));
    assert_eq!(hundred_chars.chars().count(), 100);
    assert!(accepted(&hundred_chars));
}

#[test]
fn rejections_are_value_free_and_use_one_message() {
    for candidate in ["short", "passwordpasswordpassword", STRONG_15] {
        let error =
            validate_new_vault_passphrase(&SecretString::from(candidate.to_owned())).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert_eq!(error.message(), NEW_VAULT_PASSPHRASE_POLICY);
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains(candidate), "{rendered}");
        for leak in [
            "score", "guesses=", "feedback", "pattern", "crack", "suggest",
        ] {
            assert!(!rendered.to_lowercase().contains(leak), "{rendered}");
        }
    }
    validate_new_vault_passphrase(&SecretString::from("Qz7#vR2!pL9@xK4$".to_owned())).unwrap();
}

#[test]
fn byte_validation_matches_string_validation_and_requires_utf8() {
    assert!(validate_new_vault_passphrase_bytes(b"Qz7#vR2!pL9@xK4$").is_ok());
    let weak = validate_new_vault_passphrase_bytes(b"passwordpasswordpassword").unwrap_err();
    assert_eq!(weak.message(), NEW_VAULT_PASSPHRASE_POLICY);
    let invalid = validate_new_vault_passphrase_bytes(b"\xff\xfeQz7#vR2!pL9@xK4$").unwrap_err();
    assert_eq!(invalid.kind(), VaultErrorKind::InvalidInput);
    assert!(!invalid.to_string().contains("Qz7"));
}
