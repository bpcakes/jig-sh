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

use jig_vault::{MAX_SECRET_VALUE_LEN, SecretBytes};
use zeroize::Zeroizing;

/// A bounded, non-cloneable protected editor backed by preallocated zeroizing
/// storage.
pub(crate) struct SecretInput {
    bytes: SecretBytes,
}

impl SecretInput {
    pub(crate) fn new() -> Self {
        Self {
            bytes: SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN),
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
            .map_err(|_| InputTooLong)
    }

    /// Appends one paste atomically. A paste that does not fit is rejected in
    /// full so the operator never saves a silently truncated value.
    pub(crate) fn paste(&mut self, value: &str) -> Result<(), InputTooLong> {
        self.bytes
            .extend_from_slice(value.as_bytes())
            .map_err(|_| InputTooLong)
    }

    pub(crate) fn backspace(&mut self) {
        let len = self.bytes.len();
        if len == 0 {
            return;
        }
        let new_len = std::str::from_utf8(self.bytes.as_slice())
            .ok()
            .and_then(|value| value.char_indices().next_back().map(|(index, _)| index))
            .unwrap_or(len - 1);
        self.bytes.truncate(new_len);
    }

    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.bytes.as_slice() == other.bytes.as_slice()
    }

    pub(crate) fn take(&mut self) -> SecretBytes {
        std::mem::replace(
            &mut self.bytes,
            SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN),
        )
    }

    /// Loads exact bytes from a bounded regular file without growing the
    /// protected allocation. Symlinks and non-regular files are rejected.
    pub(crate) fn from_regular_file(path: &Path) -> Result<Self, SecretInputFileError> {
        let path_label = path.to_path_buf();
        let metadata = std::fs::symlink_metadata(path).map_err(|error| {
            SecretInputFileError::new(path_label.clone(), "inspect", error.kind())
        })?;
        if !is_real_regular_file(&metadata) {
            return Err(SecretInputFileError::invalid(
                path_label,
                "value source must be a regular file and must not be a symlink or reparse point",
            ));
        }
        if metadata.len() > MAX_SECRET_VALUE_LEN as u64 {
            return Err(SecretInputFileError::invalid(
                path_label,
                format!("value source exceeds the {MAX_SECRET_VALUE_LEN} byte vault value limit"),
            ));
        }
        let mut file = open_value_file(path)
            .map_err(|error| SecretInputFileError::new(path.to_path_buf(), "open", error.kind()))?;
        let opened = file.metadata().map_err(|error| {
            SecretInputFileError::new(path.to_path_buf(), "inspect opened", error.kind())
        })?;
        if !is_real_regular_file(&opened) {
            return Err(SecretInputFileError::invalid(
                path.to_path_buf(),
                "opened value source is not a real regular file",
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
        Ok(input)
    }

    /// Returns bullets and a byte count only. No source character is copied
    /// into the returned render string.
    pub(crate) fn render_label(&self) -> String {
        if self.is_empty() {
            return "(empty)".to_owned();
        }
        let characters = std::str::from_utf8(self.bytes.as_slice())
            .map(|value| value.chars().count())
            .unwrap_or_else(|_| self.len());
        let shown = characters.min(24);
        let mut label = "•".repeat(shown);
        if characters > shown {
            label.push('…');
        }
        label.push_str(&format!("  ({} bytes)", self.len()));
        label
    }
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
    options.custom_flags(libc::O_NOFOLLOW);
    #[cfg(windows)]
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    options.open(path)
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
