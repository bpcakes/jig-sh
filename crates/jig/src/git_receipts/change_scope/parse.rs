use super::*;

#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_receipts) struct RawDiffEntry {
    pub(super) old_mode: String,
    pub(super) new_mode: String,
    pub(super) old_oid: String,
    pub(super) new_oid: String,
    pub(super) status: String,
    pub(super) baseline_path: Option<Vec<u8>>,
    pub(super) current_path: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_receipts) struct IndexStageEntry {
    pub(in crate::git_receipts) mode: String,
    pub(in crate::git_receipts) stage: String,
    pub(in crate::git_receipts) path: Vec<u8>,
}

pub(in crate::git_receipts) fn parse_raw_diff_z(
    stdout: &[u8],
    limit: usize,
) -> Result<Vec<RawDiffEntry>> {
    require_nul_terminated(stdout, "Git raw diff -z")?;
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    let mut entries = Vec::new();
    while let Some(metadata) = fields.next() {
        if metadata.is_empty() {
            if fields.peek().is_some() {
                bail!("malformed Git raw diff: empty metadata field");
            }
            break;
        }
        if entries.len() == limit {
            bail!("Git raw diff exceeded the scope entry limit of {limit}");
        }
        let metadata =
            std::str::from_utf8(metadata).context("Git raw diff metadata was not UTF-8")?;
        let Some(metadata) = metadata.strip_prefix(':') else {
            bail!("malformed Git raw diff metadata");
        };
        let mut parts = metadata.split_ascii_whitespace();
        let old_mode = parts.next().unwrap_or_default();
        let new_mode = parts.next().unwrap_or_default();
        let old_oid = parts.next().unwrap_or_default();
        let new_oid = parts.next().unwrap_or_default();
        let status = parts.next().unwrap_or_default();
        if parts.next().is_some()
            || !valid_mode(old_mode)
            || !valid_mode(new_mode)
            || !valid_oid(old_oid)
            || !valid_oid(new_oid)
            || !valid_status(status)
        {
            bail!("malformed Git raw diff metadata");
        }
        let first_path = required_path(fields.next())?;
        let (baseline_path, current_path) = if status.starts_with('R') || status.starts_with('C') {
            (Some(first_path), required_path(fields.next())?)
        } else {
            (None, first_path)
        };
        entries.push(RawDiffEntry {
            old_mode: old_mode.to_owned(),
            new_mode: new_mode.to_owned(),
            old_oid: old_oid.to_ascii_lowercase(),
            new_oid: new_oid.to_ascii_lowercase(),
            status: status.to_owned(),
            baseline_path,
            current_path,
        });
    }
    Ok(entries)
}

pub(in crate::git_receipts) fn parse_index_stage_z(
    stdout: &[u8],
    limit: usize,
) -> Result<Vec<IndexStageEntry>> {
    require_nul_terminated(stdout, "git ls-files --stage -z")?;
    let mut entries = Vec::new();
    for record in stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        if entries.len() == limit {
            bail!("Git index enumeration exceeded the scope entry limit of {limit}");
        }
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("malformed git ls-files --stage record")?;
        let metadata = std::str::from_utf8(&record[..separator])
            .context("Git index metadata was not UTF-8")?;
        let mut parts = metadata.split_ascii_whitespace();
        let mode = parts.next().unwrap_or_default();
        let oid = parts.next().unwrap_or_default();
        let stage = parts.next().unwrap_or_default();
        let path = &record[separator + 1..];
        if parts.next().is_some()
            || !valid_mode(mode)
            || !valid_oid(oid)
            || !matches!(stage, "0" | "1" | "2" | "3")
            || path.is_empty()
        {
            bail!("malformed git ls-files --stage record");
        }
        entries.push(IndexStageEntry {
            mode: mode.to_owned(),
            stage: stage.to_owned(),
            path: path.to_vec(),
        });
    }
    Ok(entries)
}

fn required_path(field: Option<&[u8]>) -> Result<Vec<u8>> {
    field
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .context("malformed Git raw diff: missing path")
}

fn valid_mode(mode: &str) -> bool {
    mode.len() == 6 && mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
}

fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_status(status: &str) -> bool {
    let Some(first) = status.bytes().next() else {
        return false;
    };
    matches!(first, b'A' | b'C' | b'D' | b'M' | b'R' | b'T' | b'U')
        && status[1..].bytes().all(|byte| byte.is_ascii_digit())
}
