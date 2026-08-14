//! Shared std-only HTTP client for the daemon's integration tests: no
//! reqwest/hyper dependency, just a raw `TcpStream` that speaks enough
//! HTTP/1.1 to drive `bloomery-daemon`'s native API. Verbatim from the
//! Task 14 brief.

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
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}
