//! Strength policy for newly chosen master passphrases.
//!
//! The policy applies only where an operator chooses a new credential:
//! initialization and passphrase change. Unlock, migration, backup, and
//! restore never revalidate an existing credential, so vaults created under
//! an older, weaker floor stay usable. Completing an authenticated, recorded
//! passphrase change also uses its existing credential, even if the current
//! estimator would reject choosing it for a new change.
//!
//! A candidate must be at least [`MIN_MASTER_PASSPHRASE_LEN`] UTF-8 bytes and
//! the pinned zxcvbn 3.1.1 estimator must report at least
//! [`MIN_MASTER_PASSPHRASE_GUESSES`] guesses. The estimate is guesswork, not a
//! measured entropy guarantee or crack-time promise. Its limits are accepted:
//!
//! - Only the first 100 Unicode characters are estimated, while the KDF still
//!   derives keys from every byte of the exact, untrimmed input.
//! - Date and year scoring use the current UTC year, captured once per
//!   process, so a date-bearing candidate near the threshold may be
//!   classified differently in another year or process.
//! - Estimator scratch strings and match tokens are not zeroized upstream.
//!   The estimate runs in a narrow scope and only the guess count survives
//!   it; this is not complete memory erasure.
//!
//! Rejections use one value-free message and never expose the candidate,
//! the estimate, matched patterns, a score, feedback, or a suggestion.

use anyhow::Result as AnyResult;
use secrecy::{ExposeSecret, SecretString};

use crate::error::{classified, classified_kind};
use crate::{Result, VaultError, VaultErrorKind};

/// Minimum length of a newly chosen vault passphrase, in UTF-8 bytes.
pub const MIN_MASTER_PASSPHRASE_LEN: usize = 16;

/// Minimum zxcvbn estimated guesses for a newly chosen vault passphrase.
pub const MIN_MASTER_PASSPHRASE_GUESSES: u64 = 1 << 40;

/// The single, value-free message for a rejected new passphrase.
pub const NEW_VAULT_PASSPHRASE_POLICY: &str = "New vault passphrases must be at least 16 bytes and estimated by zxcvbn to need at least 2^40 guesses; choose a longer, less predictable passphrase.";

/// Validates a passphrase for new vault creation or passphrase change.
///
/// # Errors
///
/// Returns an `InvalidInput` error with [`NEW_VAULT_PASSPHRASE_POLICY`] when
/// the passphrase is shorter than [`MIN_MASTER_PASSPHRASE_LEN`] bytes or is
/// estimated to need fewer than [`MIN_MASTER_PASSPHRASE_GUESSES`] guesses.
pub fn validate_new_vault_passphrase(passphrase: &SecretString) -> Result<()> {
    validate_new_vault_passphrase_inner(passphrase).map_err(public_error)
}

/// Validates exact protected input bytes as a new vault passphrase.
///
/// # Errors
///
/// Returns an `InvalidInput` error when the bytes are not UTF-8 or fail
/// [`validate_new_vault_passphrase`]'s policy.
pub fn validate_new_vault_passphrase_bytes(passphrase: &[u8]) -> Result<()> {
    let passphrase = std::str::from_utf8(passphrase).map_err(|_| {
        VaultError::new(
            VaultErrorKind::InvalidInput,
            "New vault passphrases must be valid UTF-8.",
        )
    })?;
    check_new_passphrase(passphrase).map_err(public_error)
}

pub(crate) fn validate_new_vault_passphrase_inner(passphrase: &SecretString) -> AnyResult<()> {
    check_new_passphrase(passphrase.expose_secret())
}

fn check_new_passphrase(passphrase: &str) -> AnyResult<()> {
    if passphrase.len() < MIN_MASTER_PASSPHRASE_LEN
        || !meets_guess_floor(estimated_guesses(passphrase))
    {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            NEW_VAULT_PASSPHRASE_POLICY,
        ));
    }
    Ok(())
}

/// Returns only the guess count; the estimator's match tokens and feedback
/// are dropped before this function returns and never formatted.
fn estimated_guesses(passphrase: &str) -> u64 {
    #[cfg(any(test, feature = "test-utils"))]
    if let Some(guesses) = TEST_ESTIMATE.with(std::cell::Cell::get) {
        return guesses;
    }
    // Empty contextual inputs keep repository and user labels away from the
    // estimator.
    zxcvbn::zxcvbn(passphrase, &[]).guesses()
}

#[cfg(any(test, feature = "test-utils"))]
thread_local! {
    static TEST_ESTIMATE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Models a changed estimator result without changing the clock or policy
/// threshold. Scoped to the calling test thread and restored even on panic.
#[cfg(any(test, feature = "test-utils"))]
pub fn with_passphrase_estimate_for_test<T>(guesses: u64, operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<u64>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_ESTIMATE.with(|estimate| estimate.set(self.0));
        }
    }
    let _reset = Reset(TEST_ESTIMATE.with(|estimate| estimate.replace(Some(guesses))));
    operation()
}

const fn meets_guess_floor(guesses: u64) -> bool {
    guesses >= MIN_MASTER_PASSPHRASE_GUESSES
}

fn public_error(error: anyhow::Error) -> VaultError {
    VaultError::new(
        classified_kind(&error).unwrap_or(VaultErrorKind::InvalidInput),
        error.to_string(),
    )
}

#[cfg(test)]
mod tests;
