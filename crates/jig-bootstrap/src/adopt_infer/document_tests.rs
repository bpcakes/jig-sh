use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::components::workspace::CargoWorkspace;
use super::scan::{RepoScan, read_limited_text, relative_path_string};

/// Go targets, which constrain a file whose name ends in `_GOOS` or
/// `_GOARCH`, before `_test.go`.
const GO_TARGETS: &[&str] = &[
    "aix",
    "android",
    "darwin",
    "dragonfly",
    "freebsd",
    "hurd",
    "illumos",
    "ios",
    "js",
    "linux",
    "nacl",
    "netbsd",
    "openbsd",
    "plan9",
    "solaris",
    "wasip1",
    "windows",
    "zos",
    "386",
    "amd64",
    "arm",
    "arm64",
    "loong64",
    "mips",
    "mipsle",
    "mips64",
    "mips64le",
    "ppc64",
    "ppc64le",
    "riscv64",
    "s390x",
    "wasm",
];

/// Whether `root` still has the committed-document tests that the generated
/// `openapi` check runs: Go's `TestOpenAPIIsCurrent` in a test file of
/// `internal/httpapi`, or the Rust public test in a package that
/// `cargo test --workspace` tests, plus the admin one beside an admin
/// document. Earlier scaffolds wrote them under these names, but they belong
/// to the application, which may have renamed, moved, or disabled them. Only
/// a test that the check would run counts: not one in a comment or a string,
/// an ignored or build-constrained one, or one outside the Cargo workspace.
pub(crate) fn has_scaffold_document_tests(root: &Path, go: bool) -> bool {
    if go {
        return has_go_document_test(root);
    }
    let mut missing = vec!["committed_public_openapi_is_current"];
    if root.join("openapi/admin.json").is_file() {
        missing.push("committed_admin_openapi_is_current");
    }
    let workspace = CargoWorkspace::read(root, &mut Vec::new());
    let scan = RepoScan::collect(root, &mut Vec::new());
    let packages = scan
        .named_files("Cargo.toml")
        .filter_map(|manifest| manifest.parent().map(Path::to_path_buf))
        .collect::<BTreeSet<_>>();
    for path in scan.files_with_extensions(&["rs"]) {
        if !owning_package(root, path, &packages).is_some_and(|package| workspace.tests(&package)) {
            continue;
        }
        let Ok(text) = read_limited_text(path) else {
            continue;
        };
        let code = Language::Rust.code(&text);
        missing.retain(|test| !defines_rust_test(&code, test));
        if missing.is_empty() {
            return true;
        }
    }
    false
}

/// The package that owns `file`: the nearest directory with a Cargo
/// manifest, relative to `root`.
fn owning_package(root: &Path, file: &Path, packages: &BTreeSet<PathBuf>) -> Option<String> {
    file.ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(root))
        .find(|dir| packages.contains(*dir))
        .map(|dir| match dir.strip_prefix(root) {
            Ok(relative) if !relative.as_os_str().is_empty() => relative_path_string(relative),
            _ => ".".into(),
        })
}

/// Whether `code` defines `name` as a test that runs: a function with a test
/// attribute and no `ignore`.
fn defines_rust_test(code: &str, name: &str) -> bool {
    code.match_indices(&format!("fn {name}("))
        .any(|(index, _)| {
            let before = &code[..index];
            if before.ends_with(is_identifier) {
                return false;
            }
            let item = &before[before.rfind([';', '{', '}']).map_or(0, |end| end + 1)..];
            let attributes = outer_attributes(item);
            attributes.iter().any(|attribute| {
                attribute
                    .split(|c: char| c == '(' || c == '=' || c.is_whitespace())
                    .next()
                    .is_some_and(|path| path.ends_with("test"))
            }) && !attributes.iter().any(|attribute| {
                attribute
                    .split(|c: char| !is_identifier(c))
                    .any(|word| word == "ignore")
            })
        })
}

/// The contents of the attributes in `item`.
fn outer_attributes(item: &str) -> Vec<&str> {
    let mut attributes = Vec::new();
    let mut rest = item;
    while let Some(start) = rest.find("#[") {
        let body = &rest[start + 2..];
        let mut depth = 1;
        let Some(end) = body.char_indices().find_map(|(index, c)| {
            match c {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(index)
        }) else {
            break;
        };
        attributes.push(&body[..end]);
        rest = &body[end + 1..];
    }
    attributes
}

/// Whether a test file of `internal/httpapi` that every build includes
/// defines `TestOpenAPIIsCurrent`, which the Go check runs.
fn has_go_document_test(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root.join("internal/httpapi")) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|name| name.strip_suffix("_test.go")) else {
            return false;
        };
        entry.file_type().is_ok_and(|kind| kind.is_file())
            && !stem.starts_with(['_', '.'])
            && !targets_one_platform(stem)
            && read_limited_text(&entry.path()).is_ok_and(|text| {
                let code = Language::Go.code(&text);
                !has_build_constraint(&text)
                    && code
                        .match_indices("func TestOpenAPIIsCurrent(")
                        .any(|(index, _)| !code[..index].ends_with(is_identifier))
            })
    })
}

/// A file named `*_GOOS` or `*_GOARCH`, before `_test.go`, builds for one
/// platform only.
fn targets_one_platform(stem: &str) -> bool {
    stem.rsplit_once('_')
        .is_some_and(|(prefix, last)| !prefix.is_empty() && GO_TARGETS.contains(&last))
}

/// Build constraints precede the package clause.
fn has_build_constraint(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with("package "))
        .any(|line| line.starts_with("//go:build") || line.starts_with("// +build"))
}

fn is_identifier(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[derive(Clone, Copy, PartialEq)]
enum Language {
    Rust,
    Go,
}

impl Language {
    /// `text` with its comments and the contents of its literals blanked, so
    /// that only code matches.
    fn code(self, text: &str) -> String {
        let chars = text.chars().collect::<Vec<_>>();
        let mut code = String::with_capacity(text.len());
        let mut index = 0;
        while index < chars.len() {
            if let Some(end) = self.comment_or_literal_end(&chars, index) {
                code.extend(
                    chars[index..end]
                        .iter()
                        .map(|&c| if c == '\n' { '\n' } else { ' ' }),
                );
                index = end;
            } else {
                code.push(chars[index]);
                index += 1;
            }
        }
        code
    }

    /// The end of the comment or literal that starts at `index`, if one does.
    fn comment_or_literal_end(self, chars: &[char], index: usize) -> Option<usize> {
        let at = |offset: usize| chars.get(index + offset).copied();
        match (chars[index], at(1)) {
            ('/', Some('/')) => Some(
                chars[index..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(chars.len(), |end| index + end),
            ),
            ('/', Some('*')) => Some(self.block_comment_end(chars, index)),
            ('"', _) => Some(quoted_end(chars, index + 1, '"')),
            ('`', _) if self == Self::Go => Some(
                chars[index + 1..]
                    .iter()
                    .position(|&c| c == '`')
                    .map_or(chars.len(), |end| index + end + 2),
            ),
            ('\'', Some('\\')) => Some(quoted_end(chars, index + 1, '\'')),
            // A Rust lifetime has no closing quote.
            ('\'', Some(_)) if at(2) == Some('\'') => Some(index + 3),
            ('r', Some('"' | '#')) if self == Self::Rust => raw_string_end(chars, index),
            _ => None,
        }
    }

    /// Rust block comments nest; Go's do not.
    fn block_comment_end(self, chars: &[char], index: usize) -> usize {
        let mut depth = 0;
        let mut position = index;
        while position + 1 < chars.len() {
            match (chars[position], chars[position + 1]) {
                ('/', '*') if depth == 0 || self == Self::Rust => {
                    depth += 1;
                    position += 2;
                }
                ('*', '/') => {
                    depth -= 1;
                    position += 2;
                    if depth == 0 {
                        return position;
                    }
                }
                _ => position += 1,
            }
        }
        chars.len()
    }
}

/// The end of a literal whose contents start at `start`, honouring escapes.
fn quoted_end(chars: &[char], start: usize, quote: char) -> usize {
    let mut position = start;
    while position < chars.len() {
        match chars[position] {
            '\\' => position += 2,
            c if c == quote => return position + 1,
            _ => position += 1,
        }
    }
    chars.len()
}

/// The end of a raw string such as `r#"…"#` that starts at `index`, if one
/// does: the `r` must begin a token, or follow a `b` or `c` prefix that does.
fn raw_string_end(chars: &[char], index: usize) -> Option<usize> {
    let token_start = |position: usize| {
        position
            .checked_sub(1)
            .is_none_or(|previous| !is_identifier(chars[previous]))
    };
    let prefixed = index
        .checked_sub(1)
        .is_some_and(|previous| matches!(chars[previous], 'b' | 'c') && token_start(previous));
    if !token_start(index) && !prefixed {
        return None;
    }
    let hashes = chars[index + 1..].iter().take_while(|&&c| c == '#').count();
    let open = index + 1 + hashes;
    if chars.get(open) != Some(&'"') {
        return None;
    }
    let closes = |position: usize| {
        chars[position] == '"'
            && chars[position + 1..]
                .iter()
                .take(hashes)
                .filter(|&&c| c == '#')
                .count()
                == hashes
    };
    Some(
        (open + 1..chars.len())
            .find(|&position| closes(position))
            .map_or(chars.len(), |position| position + 1 + hashes),
    )
}
