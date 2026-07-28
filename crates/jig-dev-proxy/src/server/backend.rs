use anyhow::{Context, Result};
use bytes::Bytes;
use hyper::StatusCode;
use hyper::header::{CONTENT_LENGTH, HeaderName, HeaderValue};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::{
    BACKEND_BODY_DRAIN_TIMEOUT, MAX_BACKEND_DRAIN_BODY_BYTES, MAX_BACKEND_HEADER_BYTES,
    MAX_BACKEND_HEADER_COUNT, MAX_CHUNK_HEADER_BYTES,
};

pub(super) async fn read_backend_headers(
    stream: &mut TcpStream,
) -> Result<(StatusCode, Vec<(HeaderName, HeaderValue)>, Bytes)> {
    let mut buffer = Vec::new();
    let mut temp = [0u8; 1024];
    let mut header_scan_start = 0usize;
    loop {
        let n = stream.read(&mut temp).await?;
        if n == 0 {
            anyhow::bail!("Backend closed before completing response headers");
        }
        buffer.extend_from_slice(&temp[..n]);
        if buffer.len() > MAX_BACKEND_HEADER_BYTES {
            anyhow::bail!("Backend response headers exceeded {MAX_BACKEND_HEADER_BYTES} bytes");
        }
        if find_header_end(&buffer, header_scan_start).is_some() {
            break;
        }
        header_scan_start = buffer.len().saturating_sub(3);
    }
    let header_end = find_header_end(&buffer, 0).context("Incomplete backend response headers")?;
    let header_block = &buffer[..header_end - 4];
    let status_line_end = find_crlf(header_block, 0).unwrap_or(header_block.len());
    let status = parse_backend_status(&header_block[..status_line_end])?;
    let headers = parse_backend_headers(&header_block[status_line_end.min(header_block.len())..])?;
    Ok((
        status,
        headers,
        Bytes::copy_from_slice(&buffer[header_end..]),
    ))
}

pub(super) async fn complete_backend_body(
    stream: &mut TcpStream,
    headers: &[(HeaderName, HeaderValue)],
    buffered: Bytes,
) -> Result<Bytes> {
    let content_length = content_length(headers)?;
    let chunked = transfer_encoding_is_chunked(headers);
    if content_length.is_some() && chunked {
        anyhow::bail!("Backend response used both Content-Length and Transfer-Encoding: chunked");
    }
    if let Some(content_length) = content_length {
        if content_length > MAX_BACKEND_DRAIN_BODY_BYTES {
            anyhow::bail!(
                "Backend non-upgrade WebSocket response body exceeded {MAX_BACKEND_DRAIN_BODY_BYTES} bytes"
            );
        }
        let mut body = buffered.to_vec();
        let mut temp = [0u8; 8192];
        while body.len() < content_length {
            let remaining = content_length - body.len();
            let read_len = remaining.min(temp.len());
            let n = stream.read(&mut temp[..read_len]).await?;
            if n == 0 {
                anyhow::bail!("Backend closed before completing declared response body");
            }
            body.extend_from_slice(&temp[..n]);
        }
        debug_assert!(body.len() <= content_length);
        body.truncate(content_length);
        return Ok(Bytes::from(body));
    }

    if chunked {
        return read_chunked_body(stream, buffered).await;
    }

    let mut body = buffered.to_vec();
    read_to_end_limited(stream, &mut body).await?;
    Ok(Bytes::from(body))
}

pub(super) fn parse_backend_status(line: &[u8]) -> Result<StatusCode> {
    let line = std::str::from_utf8(line).context("Backend response status line was not UTF-8")?;
    let Some((version, rest)) = line.split_once(' ') else {
        if !line.starts_with("HTTP/") {
            anyhow::bail!("Backend response status line did not start with HTTP/");
        }
        anyhow::bail!("Backend response status line did not include a status code");
    };
    if !version.starts_with("HTTP/") {
        anyhow::bail!("Backend response status line did not start with HTTP/");
    }
    if version.as_bytes().iter().any(u8::is_ascii_whitespace) {
        anyhow::bail!("Backend response protocol version contained whitespace");
    }
    if rest.as_bytes().first().is_none_or(|byte| *byte == b' ') {
        anyhow::bail!("Backend response status line used invalid spacing");
    }
    let status_text = rest
        .get(..3)
        .context("Backend response status line did not include a complete status code")?;
    if rest.as_bytes().get(3).is_some_and(|byte| *byte != b' ') {
        anyhow::bail!("Backend response status code was not followed by a single space");
    }
    let status_code = status_text
        .parse::<u16>()
        .context("Backend response status code was not numeric")?;
    StatusCode::from_u16(status_code).context("Invalid backend response status code")
}

pub(super) fn parse_backend_headers(mut bytes: &[u8]) -> Result<Vec<(HeaderName, HeaderValue)>> {
    if bytes.starts_with(b"\r\n") {
        bytes = &bytes[2..];
    }
    let mut headers = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let line_end = find_crlf(bytes, pos).unwrap_or(bytes.len());
        let line = &bytes[pos..line_end];
        if !line.is_empty() {
            if headers.len() >= MAX_BACKEND_HEADER_COUNT {
                anyhow::bail!("Backend response exceeded {MAX_BACKEND_HEADER_COUNT} headers");
            }
            let colon = line
                .iter()
                .position(|byte| *byte == b':')
                .context("Backend response header did not contain ':'")?;
            let name = &line[..colon];
            if name.is_empty() || name.iter().any(|byte| matches!(*byte, b' ' | b'\t')) {
                anyhow::bail!(
                    "Backend response header name was empty or contained whitespace before ':'"
                );
            }
            let value = trim_ows(&line[colon + 1..]);
            if value.iter().any(|byte| matches!(*byte, b'\r' | b'\n')) {
                anyhow::bail!("Backend response header value contained a bare CR or LF");
            }
            headers.push((
                HeaderName::from_bytes(name).context("Invalid backend response header name")?,
                HeaderValue::from_bytes(value).context("Invalid backend response header value")?,
            ));
        }
        pos = line_end.saturating_add(2);
    }
    Ok(headers)
}

pub(super) fn trim_ows(mut bytes: &[u8]) -> &[u8] {
    while bytes
        .first()
        .is_some_and(|byte| matches!(*byte, b' ' | b'\t'))
    {
        bytes = &bytes[1..];
    }
    while bytes
        .last()
        .is_some_and(|byte| matches!(*byte, b' ' | b'\t'))
    {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

pub(super) fn content_length(headers: &[(HeaderName, HeaderValue)]) -> Result<Option<usize>> {
    let mut parsed = None;
    for (_, value) in headers.iter().filter(|(name, _)| *name == CONTENT_LENGTH) {
        if parsed.is_some() {
            anyhow::bail!("Backend response used multiple Content-Length values");
        }
        let value = value
            .to_str()
            .context("Backend Content-Length was not valid header text")?;
        if value.contains(',') {
            anyhow::bail!("Backend response used multiple Content-Length values");
        }
        parsed = Some(
            value
                .trim()
                .parse::<usize>()
                .context("Invalid backend Content-Length")?,
        );
    }
    Ok(parsed)
}

pub(super) fn transfer_encoding_is_chunked(headers: &[(HeaderName, HeaderValue)]) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.as_str().eq_ignore_ascii_case("transfer-encoding"))
        .is_some_and(|(_, value)| {
            value
                .to_str()
                .unwrap_or("")
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("chunked"))
        })
}

pub(super) async fn read_chunked_body(stream: &mut TcpStream, buffered: Bytes) -> Result<Bytes> {
    let mut raw = buffered.to_vec();
    let mut scanner = ChunkedMessageScanner::default();
    loop {
        if raw.len() > MAX_BACKEND_DRAIN_BODY_BYTES {
            anyhow::bail!("Backend chunked response exceeded {MAX_BACKEND_DRAIN_BODY_BYTES} bytes");
        }
        if let Some(end) = scanner.scan(&raw)? {
            return decode_chunked_body(&raw[..end]);
        }
        let mut temp = [0u8; 1024];
        let n = stream.read(&mut temp).await?;
        if n == 0 {
            anyhow::bail!("Backend closed before completing chunked response");
        }
        raw.extend_from_slice(&temp[..n]);
    }
}

#[derive(Default)]
pub(super) struct ChunkedMessageScanner {
    pos: usize,
    pending_chunk_end: Option<usize>,
}

impl ChunkedMessageScanner {
    pub(super) fn scan(&mut self, raw: &[u8]) -> Result<Option<usize>> {
        loop {
            if let Some(chunk_end) = self.pending_chunk_end {
                if raw.len() < chunk_end {
                    return Ok(None);
                }
                if raw.get(chunk_end - 2..chunk_end) != Some(b"\r\n") {
                    anyhow::bail!("Invalid chunked response framing");
                }
                self.pos = chunk_end;
                self.pending_chunk_end = None;
                continue;
            }
            let Some(line_end) = find_crlf(raw, self.pos) else {
                if raw.len().saturating_sub(self.pos) > MAX_CHUNK_HEADER_BYTES {
                    anyhow::bail!("Backend chunk header exceeded {MAX_CHUNK_HEADER_BYTES} bytes");
                }
                return Ok(None);
            };
            if line_end.saturating_sub(self.pos) > MAX_CHUNK_HEADER_BYTES {
                anyhow::bail!("Backend chunk header exceeded {MAX_CHUNK_HEADER_BYTES} bytes");
            }
            let size = parse_chunk_size(&raw[self.pos..line_end]).context("Invalid chunk size")?;
            let data_start = line_end + 2;
            if size == 0 {
                if raw.get(data_start..data_start + 2) == Some(b"\r\n") {
                    return Ok(Some(data_start + 2));
                }
                return Ok(find_header_end(raw, data_start));
            }
            let data_end = data_start
                .checked_add(size)
                .context("Backend chunk size overflowed parser bounds")?;
            let chunk_end = data_end
                .checked_add(2)
                .context("Backend chunk size overflowed parser bounds")?;
            if raw.len() < chunk_end {
                self.pending_chunk_end = Some(chunk_end);
                return Ok(None);
            }
            if raw.get(data_end..chunk_end) != Some(b"\r\n") {
                anyhow::bail!("Invalid chunked response framing");
            }
            self.pos = chunk_end;
        }
    }
}

pub(super) async fn read_to_end_limited(stream: &mut TcpStream, body: &mut Vec<u8>) -> Result<()> {
    timeout(BACKEND_BODY_DRAIN_TIMEOUT, async {
        let mut temp = [0u8; 8192];
        loop {
            if body.len() >= MAX_BACKEND_DRAIN_BODY_BYTES {
                // Keep the buffered response at the documented maximum and
                // read one sentinel byte only to distinguish exact-limit EOF
                // from an over-limit backend body.
                let mut extra = [0u8; 1];
                let n = stream.read(&mut extra).await?;
                if n == 0 {
                    return Ok(());
                }
                anyhow::bail!(
                    "Backend non-upgrade WebSocket response body exceeded {MAX_BACKEND_DRAIN_BODY_BYTES} bytes"
                );
            }
            let remaining = MAX_BACKEND_DRAIN_BODY_BYTES - body.len();
            let read_len = remaining.min(temp.len());
            let n = stream.read(&mut temp[..read_len]).await?;
            if n == 0 {
                return Ok(());
            }
            body.extend_from_slice(&temp[..n]);
        }
    })
    .await
    .context("Timed out waiting for backend to close non-upgrade WebSocket response")?
}

pub(super) fn decode_chunked_body(raw: &[u8]) -> Result<Bytes> {
    let mut pos = 0usize;
    let mut decoded = Vec::new();
    loop {
        let line_end = find_crlf(raw, pos).context("Incomplete chunked response")?;
        let size = parse_chunk_size(&raw[pos..line_end]).context("Invalid chunk size")?;
        pos = line_end + 2;
        if size == 0 {
            if decoded.len() > MAX_BACKEND_DRAIN_BODY_BYTES {
                anyhow::bail!(
                    "Backend chunked response exceeded {MAX_BACKEND_DRAIN_BODY_BYTES} bytes"
                );
            }
            return Ok(Bytes::from(decoded));
        }
        let data_end = pos
            .checked_add(size)
            .context("Backend chunk size overflowed parser bounds")?;
        let chunk_end = data_end
            .checked_add(2)
            .context("Backend chunk size overflowed parser bounds")?;
        let decoded_len = decoded
            .len()
            .checked_add(size)
            .context("Backend chunked response size overflowed parser bounds")?;
        if decoded_len > MAX_BACKEND_DRAIN_BODY_BYTES {
            anyhow::bail!("Backend chunked response exceeded {MAX_BACKEND_DRAIN_BODY_BYTES} bytes");
        }
        if raw.len() < chunk_end || raw.get(data_end..chunk_end) != Some(b"\r\n") {
            anyhow::bail!("Incomplete chunked response");
        }
        decoded.extend_from_slice(&raw[pos..data_end]);
        pos = chunk_end;
    }
}

pub(super) fn parse_chunk_size(line: &[u8]) -> Option<usize> {
    let end = line
        .iter()
        .position(|byte| *byte == b';')
        .unwrap_or(line.len());
    let text = std::str::from_utf8(&line[..end]).ok()?.trim();
    usize::from_str_radix(text, 16).ok()
}

pub(super) fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|index| start + index)
}

pub(super) fn find_header_end(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| start + index + 4)
}
