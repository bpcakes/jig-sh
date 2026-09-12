use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use pulldown_cmark::{BrokenLink, CowStr, Event, LinkType, Options, Parser, Tag};

use crate::repository_path::normalize_portable_repo_path;

pub(crate) const MAX_GUIDE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) struct Reference {
    pub(crate) line: usize,
    pub(crate) target: String,
    pub(crate) problem: Option<&'static str>,
}

/// CommonMark events exclude code spans, fenced/indented code and HTML examples.
/// Source offsets refer to the link use, including for reference-style links.
pub(crate) fn markdown_references(text: &str) -> Vec<Reference> {
    let newlines = text
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    let line = |offset| newlines.partition_point(|newline| *newline < offset) + 1;
    let mut undefined = Vec::new();
    let mut callback = |link: BrokenLink<'_>| {
        if matches!(link.link_type, LinkType::Reference | LinkType::Collapsed) {
            undefined.push(Reference {
                line: line(link.span.start),
                target: link.reference.into_string(),
                problem: Some("explicit Markdown reference has no link definition"),
            });
        }
        None::<(CowStr<'_>, CowStr<'_>)>
    };
    let mut references =
        Parser::new_with_broken_link_callback(text, Options::empty(), Some(&mut callback))
            .into_offset_iter()
            .filter_map(|(event, range)| match event {
                Event::Start(
                    Tag::Link {
                        link_type,
                        dest_url,
                        ..
                    }
                    | Tag::Image {
                        link_type,
                        dest_url,
                        ..
                    },
                ) => {
                    Some(Reference {
                        line: line(range.start),
                        // CommonMark email autolinks omit the scheme in parser
                        // events. Preserve their URI meaning for every caller.
                        target: if link_type == LinkType::Email {
                            format!("mailto:{dest_url}")
                        } else {
                            dest_url.into_string()
                        },
                        problem: None,
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
    references.extend(undefined);
    references.sort_by_key(|reference| reference.line);
    references
}

#[derive(Debug, PartialEq)]
pub(crate) enum Destination {
    Fragment,
    External,
    Local(String),
}

pub(crate) fn resolve_reference(guide: &Path, target: &str) -> Result<Destination> {
    if target.chars().any(char::is_control) {
        bail!("reference contains control characters");
    }
    if target.is_empty() || target.starts_with('#') {
        return Ok(Destination::Fragment);
    }
    let bytes = target.as_bytes();
    if target.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        bail!("reference must use portable repository paths, not drive or UNC paths");
    }
    if target.starts_with("//") || has_uri_scheme(target) {
        return Ok(Destination::External);
    }
    let path = target.split(['#', '?']).next().unwrap_or(target);
    let decoded = percent_decode(path)?;
    if decoded.contains('\\') || decoded.chars().any(char::is_control) {
        bail!("reference must use portable '/' separators without control characters");
    }
    let mut parts: Vec<&str> = if decoded.starts_with('/') {
        Vec::new()
    } else {
        guide
            .parent()
            .and_then(Path::to_str)
            .unwrap_or("")
            .split('/')
            .filter(|s| !s.is_empty())
            .collect()
    };
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    bail!("reference escapes outside repository");
                }
            }
            part => parts.push(part),
        }
    }
    let joined = if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    };
    Ok(Destination::Local(normalize_portable_repo_path(
        &joined,
        "guide reference",
    )?))
}

pub(crate) fn has_uri_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
}

fn percent_decode(value: &str) -> Result<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(|b| char::from(b).to_digit(16));
            let low = bytes.next().and_then(|b| char::from(b).to_digit(16));
            let (Some(high), Some(low)) = (high, low) else {
                bail!("malformed percent escape in reference")
            };
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).map_err(Into::into)
}

/// All filesystem access stays beneath this pinned root. Each ancestor is opened
/// separately without following symlinks, so replacement cannot redirect a read.
pub(crate) struct GuideFiles {
    root: Dir,
}

impl GuideFiles {
    pub(crate) fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            root: Dir::open_ambient_dir(root, cap_std::ambient_authority())?,
        })
    }

    fn parent(&self, relative: &str) -> Result<(Dir, PathBuf)> {
        let normalized = normalize_portable_repo_path(relative, "guide path")?;
        let path = Path::new(&normalized);
        let mut directory = self.root.try_clone()?;
        if let Some(parent) = path.parent() {
            for part in parent.components() {
                directory = directory.open_dir_nofollow(part.as_os_str())?;
            }
        }
        let name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("."));
        Ok((directory, PathBuf::from(name)))
    }

    pub(crate) fn check_target(&self, relative: &str, require_file: bool) -> Result<()> {
        let (directory, name) = self.parent(relative)?;
        let metadata = directory.symlink_metadata(&name)?;
        if metadata.file_type().is_symlink() {
            bail!("reference is a symlink; use a real repository file or directory");
        }
        if !(metadata.is_file() || (!require_file && metadata.is_dir())) {
            bail!(
                "reference must identify {}",
                if require_file {
                    "a regular guide file"
                } else {
                    "a regular file or directory"
                }
            );
        }
        Ok(())
    }

    pub(crate) fn read(&self, relative: &str) -> Result<String> {
        let (directory, name) = self.parent(relative)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = directory.open_with(&name, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            bail!("guide must be a regular file");
        }
        if metadata.len() > MAX_GUIDE_BYTES {
            bail!("guide exceeds the {MAX_GUIDE_BYTES}-byte reading limit");
        }
        let mut bytes = Vec::new();
        file.take(MAX_GUIDE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_GUIDE_BYTES {
            bail!("guide exceeds the {MAX_GUIDE_BYTES}-byte reading limit");
        }
        Ok(String::from_utf8(bytes)?)
    }
}

pub(crate) fn is_missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::NotFound)
}
