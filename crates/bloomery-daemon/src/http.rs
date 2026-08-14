//! Minimal HTTP plumbing shared by the daemon's native API (`api_native`,
//! Task 14) and its future OpenAI-compatible surface (Task 15): parse a
//! request's method and path into segments, read its body, and write JSON
//! back.
//!
//! [`serve`] owns the whole request-serving story: bind, spin up a fixed
//! worker pool sharing one `Arc<Mutex<Pager<S>>>`, and hand back a
//! [`ServerHandle`] whose [`ServerHandle::shutdown`] actually stops every
//! worker rather than leaving them blocked in `Server::recv()` forever.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tiny_http::{Header, Request, Response, StatusCode};

use bloomery_substrate::Substrate;

use crate::api_native;
use crate::pager::Pager;

/// One GPU, one pager, serialized inference: Phase 1's coarse lock (see the
/// Task 14 brief). Four workers are enough to keep the accept queue and a
/// slow client from blocking every other connection while every actual
/// pager call still runs one at a time, serialized by the `Mutex`.
const WORKER_COUNT: usize = 4;

/// A running HTTP server. Dropping this without calling [`shutdown`] leaks
/// the worker threads (each stays blocked in `Server::recv()` until the
/// process exits) — tests and `main.rs` should always call it or run
/// forever.
///
/// [`shutdown`]: ServerHandle::shutdown
pub struct ServerHandle {
    server: Arc<tiny_http::Server>,
    workers: Vec<JoinHandle<()>>,
}

impl ServerHandle {
    /// Stops every worker and waits for them to exit.
    ///
    /// `tiny_http::Server::unblock()` wakes exactly one thread blocked in
    /// `recv()` per call — it pushes a single `Unblock` control message into
    /// the request queue, which one `pop()` consumes. Calling it once would
    /// only ever free one of the [`WORKER_COUNT`] workers and leave the rest
    /// blocked forever, so this calls it once per worker before joining any
    /// of them.
    pub fn shutdown(self) {
        for _ in 0..self.workers.len() {
            self.server.unblock();
        }
        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

/// Binds `127.0.0.1:port` (`0` lets the OS pick an ephemeral port; the
/// actual bound port is returned), starts [`WORKER_COUNT`] worker threads
/// sharing `pager` behind an `Arc<Mutex<_>>`, and returns the bound port
/// plus a handle to stop them.
pub fn serve<S: Substrate + Send + 'static>(pager: Pager<S>, port: u16) -> (u16, ServerHandle) {
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("bloomery-daemon: failed to bind 127.0.0.1:{port}: {e}"));
    let bound_port = server
        .server_addr()
        .to_ip()
        .unwrap_or_else(|| panic!("bloomery-daemon only binds TCP sockets"))
        .port();

    let server = Arc::new(server);
    let pager = Arc::new(Mutex::new(pager));

    let workers = (0..WORKER_COUNT)
        .map(|_| {
            let server = Arc::clone(&server);
            let pager = Arc::clone(&pager);
            std::thread::spawn(move || worker_loop(&server, &pager))
        })
        .collect();

    (bound_port, ServerHandle { server, workers })
}

/// One worker's whole life: pull requests off the shared server until it is
/// told to stop, dispatch each one against the shared pager, and respond.
fn worker_loop<S: Substrate>(server: &tiny_http::Server, pager: &Mutex<Pager<S>>) {
    loop {
        let mut request = match server.recv() {
            Ok(r) => r,
            // Either a connection error or `ServerHandle::shutdown`'s
            // `unblock()` — either way this worker's done.
            Err(_) => return,
        };
        let (method, segments, body) = read_request(&mut request);
        match api_native::dispatch(pager, &method, &segments, &body) {
            (status, Some(value)) => respond_json(request, status, &value),
            (status, None) => respond_empty(request, status),
        }
    }
}

/// Method name, `/`-split non-empty path segments (query string dropped),
/// and body — read off `req` without consuming it, since routing happens
/// before `req.respond(..)` can be called.
fn read_request(req: &mut Request) -> (String, Vec<String>, String) {
    let method = req.method().as_str().to_string();
    let path = req.url().split('?').next().unwrap_or("");
    let segments = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let mut body = String::new();
    // A client that lied about Content-Length, or sent non-UTF-8 bytes,
    // leaves `body` short or empty rather than hanging or panicking the
    // worker — malformed JSON is a per-route 400, not a transport failure.
    let _ = req.as_reader().read_to_string(&mut body);
    (method, segments, body)
}

/// Serializes `value` as a JSON body and responds with `status`.
fn respond_json(req: Request, status: u16, value: &serde_json::Value) {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    let content_type = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header is valid ASCII");
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(content_type);
    let _ = req.respond(response);
}

/// Responds with `status` and an empty body (the `204`s).
fn respond_empty(req: Request, status: u16) {
    let _ = req.respond(Response::empty(StatusCode(status)));
}
