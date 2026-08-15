use std::{
    fmt,
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use jig_vault::{MAX_SECRET_VALUE_LEN, MIN_MASTER_PASSPHRASE_LEN, SecretBytes};
use zeroize::Zeroizing;

/// A bounded, non-cloneable protected editor backed by preallocated zeroizing
/// storage.
pub(crate) struct SecretInput {
    bytes: SecretBytes,
    encoding: InputEncoding,
}

impl SecretInput {
    pub(crate) fn new() -> Self {
        Self {
            bytes: SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN),
            encoding: InputEncoding::empty(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn push_char(&mut self, character: char) -> Result<(), InputTooLong> {
        let mut encoded = Zeroizing::new([0_u8; 4]);
        self.bytes
            .extend_from_slice(character.encode_utf8(&mut *encoded).as_bytes())
            .map_err(|_| InputTooLong)?;
        self.encoding.append_char();
        Ok(())
    }

    /// Appends one paste atomically. A paste that does not fit is rejected in
    /// full so the operator never saves a silently truncated value.
    pub(crate) fn paste(&mut self, value: &str) -> Result<(), InputTooLong> {
        self.bytes
            .extend_from_slice(value.as_bytes())
            .map_err(|_| InputTooLong)?;
        self.encoding.append_str(value);
        Ok(())
    }

    pub(crate) fn backspace(&mut self) {
        let len = self.bytes.len();
        if len == 0 {
            return;
        }
        let new_len = self.encoding.backspace_len(self.bytes.as_slice());
        self.bytes.truncate(new_len);
    }

    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.encoding = InputEncoding::empty();
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.bytes.as_slice() == other.bytes.as_slice()
    }

    pub(crate) fn validate_new_vault_passphrase(&self) -> Result<(), String> {
        if self.len() < MIN_MASTER_PASSPHRASE_LEN {
            return Err(format!(
                "New vault passphrases must contain at least {MIN_MASTER_PASSPHRASE_LEN} bytes."
            ));
        }
        Ok(())
    }

    pub(crate) fn take(&mut self) -> SecretBytes {
        self.encoding = InputEncoding::empty();
        std::mem::replace(
            &mut self.bytes,
            SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN),
        )
    }

    /// Loads exact bytes from a bounded regular file without growing the
    /// protected allocation. Symlinks and non-regular files are rejected.
    pub(crate) fn from_regular_file(path: &Path) -> Result<Self, SecretInputFileError> {
        let path_label = path.to_path_buf();
        let mut file = open_value_file(path).map_err(|error| {
            if is_no_follow_error(&error) {
                invalid_file_type(path_label.clone())
            } else {
                SecretInputFileError::new(path_label.clone(), "open", error.kind())
            }
        })?;
        let opened = file.metadata().map_err(|error| {
            SecretInputFileError::new(path_label.clone(), "inspect opened", error.kind())
        })?;
        if !is_real_regular_file(&opened) {
            return Err(invalid_file_type(path_label));
        }
        if opened.len() > MAX_SECRET_VALUE_LEN as u64 {
            return Err(SecretInputFileError::invalid(
                path_label,
                format!("value source exceeds the {MAX_SECRET_VALUE_LEN} byte vault value limit"),
            ));
        }

        let mut input = Self::new();
        let mut chunk = Zeroizing::new([0_u8; 8 * 1024]);
        loop {
            let read = file.read(&mut chunk[..]).map_err(|error| {
                SecretInputFileError::new(path.to_path_buf(), "read", error.kind())
            })?;
            if read == 0 {
                break;
            }
            input.bytes.extend_from_slice(&chunk[..read]).map_err(|_| {
                SecretInputFileError::invalid(
                    path.to_path_buf(),
                    format!(
                        "value source exceeds the {MAX_SECRET_VALUE_LEN} byte vault value limit"
                    ),
                )
            })?;
        }
        input.encoding = InputEncoding::inspect(input.bytes.as_slice());
        Ok(input)
    }

    /// Returns bullets and a byte count only. No source character is copied
    /// into the returned render string.
    pub(crate) fn render_label(&self) -> String {
        if self.is_empty() {
            return "(empty)".to_owned();
        }
        let characters = self.encoding.display_characters(self.len());
        let shown = characters.min(24);
        let mut label = "•".repeat(shown);
        if characters > shown {
            label.push('…');
        }
        label.push_str(&format!("  ({} bytes)", self.len()));
        label
    }
}

#[derive(Clone, Copy)]
enum InputEncoding {
    Utf8 {
        characters: usize,
    },
    Binary {
        valid_prefix_bytes: usize,
        valid_prefix_characters: usize,
    },
}

impl InputEncoding {
    const fn empty() -> Self {
        Self::Utf8 { characters: 0 }
    }

    fn inspect(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(value) => Self::Utf8 {
                characters: value.chars().count(),
            },
            Err(error) => {
                let valid_prefix_bytes = error.valid_up_to();
                let valid_prefix = std::str::from_utf8(&bytes[..valid_prefix_bytes])
                    .expect("Utf8Error::valid_up_to always ends at a valid UTF-8 boundary");
                Self::Binary {
                    valid_prefix_bytes,
                    valid_prefix_characters: valid_prefix.chars().count(),
                }
            }
        }
    }

    fn append_char(&mut self) {
        if let Self::Utf8 { characters } = self {
            *characters += 1;
        }
    }

    fn append_str(&mut self, value: &str) {
        if let Self::Utf8 { characters } = self {
            *characters += value.chars().count();
        }
    }

    fn backspace_len(&mut self, bytes: &[u8]) -> usize {
        match self {
            Self::Utf8 { characters } => {
                *characters -= 1;
                previous_utf8_boundary(bytes)
            }
            Self::Binary {
                valid_prefix_bytes,
                valid_prefix_characters,
            } => {
                let new_len = bytes.len() - 1;
                if new_len == *valid_prefix_bytes {
                    *self = Self::Utf8 {
                        characters: *valid_prefix_characters,
                    };
                }
                new_len
            }
        }
    }

    const fn display_characters(self, byte_len: usize) -> usize {
        match self {
            Self::Utf8 { characters } => characters,
            Self::Binary { .. } => byte_len,
        }
    }
}

fn previous_utf8_boundary(bytes: &[u8]) -> usize {
    let mut index = bytes.len() - 1;
    while index > 0 && bytes[index] & 0b1100_0000 == 0b1000_0000 {
        index -= 1;
    }
    index
}

impl Default for SecretInput {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretInput")
            .field("len", &self.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputTooLong;

#[derive(Debug)]
pub(crate) struct SecretInputFileError {
    path: PathBuf,
    message: String,
}

impl SecretInputFileError {
    fn new(path: PathBuf, operation: &str, kind: std::io::ErrorKind) -> Self {
        Self {
            path,
            message: format!("failed to {operation} value source ({kind:?})"),
        }
    }

    fn invalid(path: PathBuf, message: impl Into<String>) -> Self {
        Self {
            path,
            message: message.into(),
        }
    }
}

impl fmt::Display for SecretInputFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for SecretInputFileError {}

fn open_value_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    #[cfg(windows)]
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    options.open(path)
}

fn invalid_file_type(path: PathBuf) -> SecretInputFileError {
    SecretInputFileError::invalid(
        path,
        "value source must be a regular file and must not be a symlink or reparse point",
    )
}

#[cfg(unix)]
fn is_no_follow_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_no_follow_error(_error: &std::io::Error) -> bool {
    false
}

fn is_real_regular_file(metadata: &std::fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        ffi::CString,
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn fifo_open_is_nonblocking_and_rejected_as_non_regular() {
        let temp = tempdir().unwrap();
        let fifo = temp.path().join("value.fifo");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `fifo_path` remains a live, NUL-terminated CString for the
        // call, and the mode contains only valid permission bits.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);

        // Keep both peer ends open so the subject open cannot hang even if it
        // regresses to blocking mode. The descriptor flags below prove the
        // production invariant without depending on scheduler timing.
        let _reader = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let _writer = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();

        let opened = open_value_file(&fifo).unwrap();
        // SAFETY: `opened` owns a live descriptor for the duration of the call.
        let flags = unsafe { libc::fcntl(opened.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags, -1, "failed to inspect opened value-file flags");
        assert_ne!(flags & libc::O_NONBLOCK, 0);
        drop(opened);

        let error = SecretInput::from_regular_file(&fifo)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must be a regular file"), "{error}");
    }
}
