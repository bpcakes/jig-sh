use std::fs;
use std::path::Path;

use super::scan::{RepoScan, read_limited_text};

/// Whether `root` still has the committed-document tests that the generated
/// `openapi` check runs: Go's `TestOpenAPIIsCurrent` in `internal/httpapi`,
/// or the Rust public test anywhere in the workspace, plus the admin one
/// beside an admin document. Earlier scaffolds wrote them under these names,
/// but they belong to the application, which may have renamed or moved them.
pub(crate) fn has_scaffold_document_tests(root: &Path, go: bool) -> bool {
    if go {
        return fs::read_dir(root.join("internal/httpapi")).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.ends_with("_test.go"))
                    && read_limited_text(&entry.path())
                        .is_ok_and(|text| text.contains("func TestOpenAPIIsCurrent("))
            })
        });
    }
    let mut missing = vec!["fn committed_public_openapi_is_current("];
    if root.join("openapi/admin.json").is_file() {
        missing.push("fn committed_admin_openapi_is_current(");
    }
    let scan = RepoScan::collect(root, &mut Vec::new());
    for path in scan.files_with_extensions(&["rs"]) {
        let Ok(text) = read_limited_text(path) else {
            continue;
        };
        missing.retain(|test| !text.contains(test));
        if missing.is_empty() {
            return true;
        }
    }
    false
}
