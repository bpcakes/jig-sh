use std::fmt;

use secrecy::SecretString;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecretBytesCapacityError;

impl fmt::Display for SecretBytesCapacityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("secret byte extension would reallocate")
    }
}

impl std::error::Error for SecretBytesCapacityError {}

pub struct SecretBytes {
    value: Zeroizing<Vec<u8>>,
}

impl SecretBytes {
    pub fn new(value: Vec<u8>) -> Self {
        Self {
            value: Zeroizing::new(value),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self::new(Vec::with_capacity(capacity))
    }

    pub(crate) fn zeroed(len: usize) -> Self {
        Self::new(vec![0; len])
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.value.as_slice()
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        self.value.as_mut_slice()
    }

    /// Appends bytes without allowing the backing allocation to grow.
    ///
    /// # Errors
    ///
    /// Returns [`SecretBytesCapacityError`] when the new length overflows or
    /// exceeds the existing allocation capacity.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) -> Result<(), SecretBytesCapacityError> {
        let Some(new_len) = self.len().checked_add(bytes.len()) else {
            return Err(SecretBytesCapacityError);
        };
        if new_len > self.value.capacity() {
            return Err(SecretBytesCapacityError);
        }
        self.value.extend_from_slice(bytes);
        Ok(())
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.value.truncate(len);
    }

    /// Converts the bytes into a protected UTF-8 string.
    ///
    /// # Errors
    ///
    /// Returns the original [`SecretBytes`] when its contents are not valid
    /// UTF-8.
    pub fn into_secret_string(self) -> std::result::Result<SecretString, Self> {
        self.into_zeroizing_string().map(|mut value| {
            let owned = std::mem::take(&mut *value);
            SecretString::from(owned)
        })
    }

    pub(crate) fn into_zeroizing_string(mut self) -> std::result::Result<Zeroizing<String>, Self> {
        // `String::from_utf8` takes ownership of the bytes, so they briefly
        // leave the `Zeroizing` wrapper while being converted and are wrapped
        // again on both success and failure paths.
        let bytes = std::mem::take(&mut *self.value);
        match String::from_utf8(bytes) {
            Ok(value) => Ok(Zeroizing::new(value)),
            Err(error) => Err(Self::new(error.into_bytes())),
        }
    }

    pub(crate) fn zeroize(&mut self) {
        self.value.zeroize();
    }
}

impl AsRef<[u8]> for SecretBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretBytes")
            .field("len", &self.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}
