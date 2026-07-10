use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

pub(super) const INVOCATION_CWD_ENV: &str = "JIG_INVOKE_CWD";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RepositoryFileLeaf {
    Missing,
    RegularFile,
    Symlink,
}

pub(super) fn validate_no_reserved_git_metadata_components(relative: &Path) -> Result<()> {
    if let Some(component) = reserved_git_metadata_component(relative) {
        bail!(
            "Unsafe repository path {}: component {component:?} aliases the reserved Git metadata component \".git\" under Git's NTFS/HFS path rules",
            relative.display()
        );
    }
    Ok(())
}

fn reserved_git_metadata_component(relative: &Path) -> Option<&str> {
    relative.components().find_map(|component| {
        let Component::Normal(component) = component else {
            return None;
        };
        let component = component.to_str()?;
        component.split('\\').find(|segment| {
            is_ntfs_git_metadata_alias(segment) || is_hfs_git_metadata_alias(segment)
        })
    })
}

// Behavioral reference only; this is an independent Rust implementation of Git's
// protections pinned at f60db8d575adb79761d363e026fb49bddf330c73:
// https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/path.c#L1394-L1449
// https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/utf8.c#L698-L787
// Upstream cases: https://github.com/git/git/blob/f60db8d575adb79761d363e026fb49bddf330c73/t/t0060-path-utils.sh#L438-L527
fn is_ntfs_git_metadata_alias(component: &str) -> bool {
    [".git", "git~1"].into_iter().any(|prefix| {
        let Some(remainder) = strip_ascii_case_prefix(component, prefix) else {
            return false;
        };
        remainder
            .split_once(':')
            .map_or(remainder, |(before_stream, _)| before_stream)
            .bytes()
            .all(|byte| byte == b'.' || byte == b' ')
    })
}

fn strip_ascii_case_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .then(|| &value[prefix.len()..])
}

fn is_hfs_git_metadata_alias(component: &str) -> bool {
    let mut normalized = component
        .chars()
        .filter(|character| !is_hfs_ignored(*character));
    let expected = ['.', 'g', 'i', 't'];
    expected.into_iter().all(|expected| {
        normalized
            .next()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
    }) && normalized.next().is_none()
}

fn is_hfs_ignored(character: char) -> bool {
    matches!(
        character,
        '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
    )
}

pub(super) fn validate_repository_relative_ancestors(root: &Path, relative: &Path) -> Result<()> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "Repository path must be a contained relative path: {}",
            relative.display()
        );
    }
    validate_no_reserved_git_metadata_components(relative)?;

    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("Failed to stat repository root {}", root.display()))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        bail!(
            "Repository root must be a real directory, not a symlink or non-directory: {}",
            root.display()
        );
    }

    let destination = root.join(relative);
    if !destination.starts_with(root) {
        bail!(
            "Repository path escapes root {}: {}",
            root.display(),
            relative.display()
        );
    }

    let mut ancestor = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            let Component::Normal(component) = component else {
                unreachable!("relative path components were validated above");
            };
            ancestor.push(component);
            let metadata = match fs::symlink_metadata(&ancestor) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to stat {}", ancestor.display()));
                }
            };
            if metadata.file_type().is_symlink() {
                bail!(
                    "Unsafe repository path {}: ancestor {} is a symlink",
                    relative.display(),
                    ancestor.display()
                );
            }
            if !metadata.is_dir() {
                bail!(
                    "Unsafe repository path {}: ancestor {} is not a directory",
                    relative.display(),
                    ancestor.display()
                );
            }
        }
    }

    Ok(())
}

pub(super) fn validate_repository_relative_file_leaf(
    root: &Path,
    relative: &Path,
) -> Result<RepositoryFileLeaf> {
    validate_repository_relative_ancestors(root, relative)?;

    let path = root.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(RepositoryFileLeaf::Missing);
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to stat {}", path.display()));
        }
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Ok(RepositoryFileLeaf::Symlink);
    }
    if file_type.is_file() {
        return Ok(RepositoryFileLeaf::RegularFile);
    }
    if file_type.is_dir() {
        bail!(
            "Unsafe repository path {}: destination leaf {} is a directory; managed paths must be missing, regular files, or symlinks",
            relative.display(),
            path.display()
        );
    }
    bail!(
        "Unsafe repository path {}: destination leaf {} has an unsupported file type; managed paths must be missing, regular files, or symlinks",
        relative.display(),
        path.display()
    )
}

pub(super) fn absolute_path_from(path: &Path, base: &Path) -> Result<PathBuf> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    if resolved.exists() {
        fs::canonicalize(&resolved)
            .with_context(|| format!("Failed to canonicalize {}", resolved.display()))
    } else {
        Ok(resolved)
    }
}

pub(super) fn bootstrap_invocation_cwd() -> Result<PathBuf> {
    let Some(value) = env::var_os(INVOCATION_CWD_ENV) else {
        let cwd = env::current_dir().context("Failed to resolve current directory")?;
        return fs::canonicalize(&cwd).with_context(|| {
            format!(
                "Failed to canonicalize current directory: {}",
                cwd.display()
            )
        });
    };

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        bail!(
            "{INVOCATION_CWD_ENV} must be an absolute path: {}",
            path.display()
        );
    }
    if !path.is_dir() {
        bail!(
            "{INVOCATION_CWD_ENV} is not a directory: {}",
            path.display()
        );
    }
    fs::canonicalize(&path).with_context(|| {
        format!(
            "Failed to canonicalize {INVOCATION_CWD_ENV}: {}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn repository_relative_ancestor_validation_allows_directories_and_missing_ancestors() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("existing")).unwrap();

        validate_repository_relative_ancestors(root.path(), Path::new("existing/file")).unwrap();
        validate_repository_relative_ancestors(root.path(), Path::new("missing/deep/file"))
            .unwrap();
    }

    #[test]
    fn repository_relative_ancestor_validation_rejects_escaping_paths() {
        let root = tempdir().unwrap();

        for relative in [Path::new("../outside"), root.path()] {
            let error = validate_repository_relative_ancestors(root.path(), relative)
                .unwrap_err()
                .to_string();
            assert!(error.contains("contained relative path"), "{error}");
        }
    }

    #[test]
    fn repository_relative_path_validation_rejects_reserved_git_metadata_aliases() {
        for relative in [
            ".git",
            ".git.",
            ".git ",
            "vendor/.GiT.../config",
            "vendor/.GIT. . /config",
            "GIT~1/config",
            "vendor/git~1. . /config",
            ".git:stream",
            ".git .:stream",
            ".git::$INDEX_ALLOCATION",
            ".git...:alternate-stream",
            "git~1::$DATA",
            ".g\u{200c}it/config",
            "\u{feff}.G\u{202e}i\u{206a}T/config",
            "vendor\\.GiT...\\config",
        ] {
            let error = validate_no_reserved_git_metadata_components(Path::new(relative))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("reserved Git metadata component"),
                "{relative}: {error}"
            );
            assert!(error.contains(relative), "{relative}: {error}");
        }
    }

    #[test]
    fn repository_relative_path_validation_allows_git_near_misses() {
        for relative in [
            ".github/workflows/check.yml",
            ".gitignore",
            ".gitkeep",
            "git/config",
            "git~2/config",
            "git~10/config",
            "git~1x/config",
            ".gitx. ",
            ".gitx:stream",
            ".git .config",
            ".git\u{a0}",
            ".git\u{200b}",
            ".gi\u{200b}t",
            ".git\u{2029}",
            ".git\u{2060}",
            ".git\u{2069}",
            ".g\u{200c}itx",
        ] {
            validate_no_reserved_git_metadata_components(Path::new(relative)).unwrap();
        }
    }

    #[test]
    fn repository_relative_path_validation_ignores_only_git_hfs_codepoints() {
        for ignored in [
            '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
            '\u{202d}', '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
            '\u{206f}', '\u{feff}',
        ] {
            let relative = format!(".g{ignored}it/config");
            validate_no_reserved_git_metadata_components(Path::new(&relative)).unwrap_err();
        }
    }

    #[test]
    fn repository_relative_ancestor_validation_rejects_non_directory_ancestors() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("blocking"), "file").unwrap();

        let error = validate_repository_relative_ancestors(root.path(), Path::new("blocking/file"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not a directory"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn repository_relative_ancestor_validation_rejects_symlink_ancestors() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("target")).unwrap();
        symlink("target", root.path().join("linked")).unwrap();

        let error = validate_repository_relative_ancestors(root.path(), Path::new("linked/file"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("is a symlink"), "{error}");
    }

    #[test]
    fn repository_relative_ancestor_validation_requires_a_real_directory_root() {
        let parent = tempdir().unwrap();
        let file_root = parent.path().join("file-root");
        fs::write(&file_root, "file").unwrap();

        let error = validate_repository_relative_ancestors(&file_root, Path::new("child"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("real directory"), "{error}");
    }

    #[test]
    fn repository_relative_file_leaf_validation_classifies_files_and_missing_paths() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("file"), "contents").unwrap();

        assert_eq!(
            validate_repository_relative_file_leaf(root.path(), Path::new("file")).unwrap(),
            RepositoryFileLeaf::RegularFile
        );
        assert_eq!(
            validate_repository_relative_file_leaf(root.path(), Path::new("missing")).unwrap(),
            RepositoryFileLeaf::Missing
        );
    }

    #[test]
    fn repository_relative_file_leaf_validation_rejects_directories() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("directory")).unwrap();

        let error = validate_repository_relative_file_leaf(root.path(), Path::new("directory"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("destination leaf"), "{error}");
        assert!(error.contains("is a directory"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn repository_relative_file_leaf_validation_accepts_leaf_symlinks() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        symlink("target", root.path().join("linked")).unwrap();

        assert_eq!(
            validate_repository_relative_file_leaf(root.path(), Path::new("linked")).unwrap(),
            RepositoryFileLeaf::Symlink
        );
    }

    #[cfg(unix)]
    #[test]
    fn repository_relative_ancestor_validation_rejects_a_symlink_root() {
        use std::os::unix::fs::symlink;

        let parent = tempdir().unwrap();
        fs::create_dir(parent.path().join("real-root")).unwrap();
        let root = parent.path().join("root");
        symlink("real-root", &root).unwrap();

        let error = validate_repository_relative_ancestors(&root, Path::new("child"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("real directory"), "{error}");
    }
}
