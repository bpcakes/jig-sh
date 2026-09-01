use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum JsonWriteMode {
    Cache,
    Durable,
}

#[derive(Clone)]
pub(super) struct JsonLocation {
    pub(super) root: PathBuf,
    pub(super) dir: PathBuf,
    pub(super) path: PathBuf,
    pub(super) lock_path: PathBuf,
    pub(super) write_mode: JsonWriteMode,
}

impl JsonLocation {
    pub(super) fn new(root: PathBuf, dir: PathBuf, name: &str, write_mode: JsonWriteMode) -> Self {
        Self {
            path: dir.join(format!("{name}.json")),
            lock_path: dir.join(format!("{name}.lock")),
            root,
            dir,
            write_mode,
        }
    }
}
