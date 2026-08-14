//! A std-only HTTP/1.1 client for driving the daemon's native API.
//!
//! Same shape as the daemon's integration tests use (`Connection: close`, read
//! the socket to EOF): the bench must not pull a networking stack into the
//! workspace to issue four kinds of request, and a hand-rolled client keeps
//! the measurement path free of anything that could buffer, retry, or pool
//! behind our back.
//!
//! Nothing here is measured. The bench takes no timings at all — every
//! duration the gate is read from was recorded by the pager itself, around
//! the operation it names.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Read deadline for one request. Generous on purpose: a cold switch reloads
/// ~8 GB of weights, so a normal request can legitimately take seconds. It
/// exists so a *dead* daemon surfaces as a named error instead of hanging the
/// run forever — silence must be distinguishable from success.
const READ_TIMEOUT: Duration = Duration::from_secs(600);

pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    /// The body parsed as JSON, or a named error carrying what actually came
    /// back — a protocol-breaking reply is infrastructure failure, never data.
    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_str(&self.body)
            .map_err(|e| format!("daemon returned unparseable JSON ({e}): {}", self.body))
    }
}

pub struct Client {
    addr: String,
}

impl Client {
    /// `http://127.0.0.1:8181` (or a bare `host:port`) -> a client.
    pub fn new(daemon_url: &str) -> Result<Client, String> {
        let rest = daemon_url
            .strip_prefix("http://")
            .unwrap_or_else(|| daemon_url.strip_prefix("https://").unwrap_or(daemon_url));
        let addr = rest.trim_end_matches('/').to_string();
        if !addr.contains(':') {
            return Err(format!(
                "--daemon {daemon_url} has no port (want host:port)"
            ));
        }
        Ok(Client { addr })
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn request(&self, method: &str, path: &str, body: &str) -> Result<Response, String> {
        let mut stream =
            TcpStream::connect(&self.addr).map_err(|e| format!("connect {}: {e}", self.addr))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| format!("set read timeout: {e}"))?;
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: bloomery\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .map_err(|e| format!("write {method} {path}: {e}"))?;

        let mut raw = String::new();
        stream
            .read_to_string(&mut raw)
            .map_err(|e| format!("read {method} {path}: {e}"))?;
        let status: u16 = raw
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| format!("{method} {path}: no status line in {raw:?}"))?
            .parse()
            .map_err(|e| format!("{method} {path}: unparseable status ({e}) in {raw:?}"))?;
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        Ok(Response { status, body })
    }

    /// A request whose status must be `expect`. Anything else is an
    /// infrastructure failure and aborts the run with the daemon's own
    /// structured refusal quoted verbatim — a bench that swallowed a `409`
    /// and carried on would report a sample count that silently means
    /// something else.
    pub fn expect(
        &self,
        method: &str,
        path: &str,
        body: &str,
        expect: u16,
    ) -> Result<Response, String> {
        let response = self.request(method, path, body)?;
        if response.status != expect {
            return Err(format!(
                "{method} {path} -> {} (wanted {expect}): {}",
                response.status, response.body
            ));
        }
        Ok(response)
    }
}
