//! Shared std-only HTTP client for the daemon's integration tests: no
//! reqwest/hyper dependency, just a raw `TcpStream` that speaks enough
//! HTTP/1.1 to drive `bloomery-daemon`'s native API. Verbatim from the
//! Task 14 brief, plus the chunked-response decoding described on
//! [`dechunk`].

use std::io::{Read, Write};

/// Sends one `{method} {path}` request with `body` as its JSON payload
/// (`Connection: close`, so reading the socket to EOF collects the whole
/// response) and returns `(status, body)`.
pub fn http(addr: &str, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).unwrap();
    let status: u16 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
    // `split_once` rather than `split(..).nth(1)`: the head ends at the FIRST
    // blank line and everything after it is body, including a chunked
    // response's own `0\r\n\r\n` terminator.
    let (head, raw_body) = buf.split_once("\r\n\r\n").unwrap_or((buf.as_str(), ""));
    let is_chunked = head
        .lines()
        .any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.contains("chunked"));
    let body = if is_chunked {
        dechunk(raw_body)
    } else {
        raw_body.to_string()
    };
    (status, body)
}

/// Strips HTTP/1.1 chunked framing: `{hex-size}\r\n{data}\r\n` repeated, then
/// a `0` chunk.
///
/// tiny_http answers a response larger than its write buffer (8 KiB) with
/// `Transfer-Encoding: chunked` instead of a `Content-Length`, so such a body
/// is not JSON until the framing comes off — a `GET .../task/{id}` whose steps
/// carry real file observations passes that size easily. Without this, a large
/// response failed to parse with a bare `trailing characters` serde error that
/// named neither chunking nor the size that caused it.
///
/// Byte-oriented on purpose: a chunk boundary may fall in the middle of a
/// multi-byte UTF-8 character, so the chunks are concatenated as bytes and
/// decoded once at the end, never sliced as `&str` at a boundary the server
/// chose.
fn dechunk(raw: &str) -> String {
    let mut out: Vec<u8> = Vec::new();
    let mut rest: &[u8] = raw.as_bytes();
    while let Some(eol) = rest.windows(2).position(|w| w == b"\r\n") {
        let size = match std::str::from_utf8(&rest[..eol])
            .ok()
            .and_then(|s| usize::from_str_radix(s.trim(), 16).ok())
        {
            Some(n) => n,
            // Not a chunk header: stop rather than guess. The caller's own
            // parse then fails on the partial body, which is the honest
            // outcome for a response this helper cannot read.
            None => break,
        };
        let data = &rest[eol + 2..];
        if size == 0 || data.len() < size {
            break;
        }
        out.extend_from_slice(&data[..size]);
        rest = match data[size..].strip_prefix(b"\r\n") {
            Some(r) => r,
            None => break,
        };
    }
    String::from_utf8_lossy(&out).into_owned()
}
