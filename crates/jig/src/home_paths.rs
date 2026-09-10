use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

pub(crate) fn expand_tilde_path(input: &Path, user_home: &Path) -> Option<PathBuf> {
    let mut components = input.components();
    if components.next() == Some(Component::Normal(OsStr::new("~"))) {
        return Some(user_home.join(components.as_path()));
    }
    None
}

pub(crate) fn has_tilde_prefix(input: &Path) -> bool {
    input.components().next() == Some(Component::Normal(OsStr::new("~")))
}

pub(crate) fn is_bare_home_name(input: &Path) -> bool {
    let mut components = input.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

pub(crate) fn canonical_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    canonical_key(left) == canonical_key(right)
}

pub(crate) fn home_name(path: &Path) -> String {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    name.strip_prefix('.').unwrap_or(&name).to_owned()
}

pub(crate) fn home_name_matches(path: &Path, requested: &OsStr) -> bool {
    let name = path.file_name().unwrap_or(path.as_os_str());
    let encoded = name.as_encoded_bytes();
    encoded.strip_prefix(b".").unwrap_or(encoded) == requested.as_encoded_bytes()
}

/// Builds a conventional alternate home without reinterpreting native name bytes.
pub(crate) fn prefixed_home(user_home: &Path, requested: &OsStr, prefix: &str) -> PathBuf {
    let mut name = OsString::from(".");
    if !requested.as_encoded_bytes().starts_with(prefix.as_bytes()) {
        name.push(prefix);
    }
    name.push(requested);
    user_home.join(name)
}
