//! Minimal HTTP plumbing shared by the daemon's native API (`api_native`,
//! Task 14) and its OpenAI-compatible `/v1` surface (`api_v1`, Task 15):
//! parse a request's method, path, and (for `/v1`) its `X-Bloomery-Agent`
//! header into segments and a body, dispatch on the leading path segment,
//! and write the response back — JSON for both surfaces, plus SSE for
//! `/v1/chat/completions`'s `stream:true`.
//!
//! [`serve`] owns the whole request-serving story: bind, spin up a fixed
//! worker pool sharing one `Arc<Mutex<Pager<S>>>`, and hand back a
//! [`ServerHandle`] whose [`ServerHandle::shutdown`] actually stops every
//! worker rather than leaving them blocked in `Server::recv()` forever.

use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tiny_http::{Header, Request, Response, StatusCode};

use bloomery_substrate::Substrate;

use crate::api_native;
use crate::api_task;
use crate::api_v1::{self, V1Body, V1Result};
use crate::pager::Pager;
use crate::task::TaskRegistry;

/// One GPU, one pager, serialized inference: Phase 1's coarse lock (see the
/// Task 14 brief). Four workers are enough to keep the accept queue and a
/// slow client from blocking every other connection while every actual
/// pager call still runs one at a time, serialized by the `Mutex`.
const WORKER_COUNT: usize = 4;

/// Cap on a request body's size. 1 MiB is generous for this API (the
/// largest legitimate body is an `infer` prompt, and the pager's own
/// prompt-size gate refuses anything that wouldn't fit a context window
/// long before a client could usefully send megabytes of prompt). Without
/// this, `Content-Length` is entirely client-controlled: a request
/// declaring a multi-gigabyte body would grow `read_request`'s `String`
/// unbounded before any route-level validation ever ran.
const MAX_BODY_BYTES: u64 = 1_048_576;

/// A running HTTP server. Dropping this without calling [`shutdown`] leaks
/// the worker threads (each stays blocked in `Server::recv()` until the
/// process exits) — tests and `main.rs` should always call it or run
/// forever.
///
/// [`shutdown`]: ServerHandle::shutdown
pub struct ServerHandle {
    server: Arc<tiny_http::Server>,
    workers: Vec<JoinHandle<()>>,
    /// A scratch directory this handle's caller created (journal, image
    /// store, fixture `.gguf`) and wants removed on shutdown. `None` for
    /// `main.rs`'s real daemon, which owns `config.data_dir` for the life
    /// of the process; `Some` for test/bench fixtures that mint a fresh
    /// tempdir per run (`test_support::serve_fake` and friends) — without
    /// this, nothing ever cleans those up and they accumulate in the OS
    /// tempdir across every test run forever.
    scratch_dir: Option<PathBuf>,
}

impl ServerHandle {
    /// Registers `dir` to be best-effort removed (`remove_dir_all`, errors
    /// ignored — this is litter cleanup, not a source of truth) when
    /// [`shutdown`](Self::shutdown) runs.
    pub fn set_scratch_dir(&mut self, dir: PathBuf) {
        self.scratch_dir = Some(dir);
    }

    /// Stops every worker, waits for them to exit, then removes the
    /// scratch directory if one was registered.
    ///
    /// `tiny_http::Server::unblock()` wakes exactly one thread blocked in
    /// `recv()` per call — it pushes a single `Unblock` control message into
    /// the request queue, which one `pop()` consumes. Calling it once would
    /// only ever free one of the [`WORKER_COUNT`] workers and leave the rest
    /// blocked forever, so this calls it once per worker before joining any
    /// of them. Do not "simplify" this to a single call.
    pub fn shutdown(self) {
        for _ in 0..self.workers.len() {
            self.server.unblock();
        }
        for worker in self.workers {
            let _ = worker.join();
        }
        if let Some(dir) = self.scratch_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Binds `127.0.0.1:port` (`0` lets the OS pick an ephemeral port; the
/// actual bound port is returned), starts [`WORKER_COUNT`] worker threads
/// sharing `pager` behind an `Arc<Mutex<_>>`, and returns the bound port
/// plus a handle to stop them.
///
/// Binding to `127.0.0.1` only (never `0.0.0.0`) is Phase 1's whole
/// perimeter: there is also no read timeout on a connection, so a client
/// that opens a socket and never finishes sending its request line/headers
/// can stall the worker that accepted it indefinitely. Accepted as a
/// localhost-only limit for now rather than fixed — the moment this binds
/// anything but loopback, that stall becomes a real remote DoS and needs a
/// real fix (a read deadline on the underlying stream).
pub fn serve<S: Substrate + Send + 'static>(pager: Pager<S>, port: u16) -> (u16, ServerHandle) {
    serve_shared(Arc::new(Mutex::new(pager)), port)
}

/// [`serve`] for a caller that needs to keep its own handle on the pager.
///
/// The boot-time POST (Task 16) is the reason this exists: assay probes the
/// daemon *through the socket* while the boot thread attaches the profiles
/// that come back and finally clears the `posting` flag, so both sides must
/// hold the same `Arc<Mutex<Pager<S>>>`. There is still deliberately no
/// `ServerHandle::into_pager` — the pager is shared from the start or not at
/// all, never extracted back out of a running server.
pub fn serve_shared<S: Substrate + Send + 'static>(
    pager: Arc<Mutex<Pager<S>>>,
    port: u16,
) -> (u16, ServerHandle) {
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("bloomery-daemon: failed to bind 127.0.0.1:{port}: {e}"));
    let bound_port = server
        .server_addr()
        .to_ip()
        .unwrap_or_else(|| panic!("bloomery-daemon only binds TCP sockets"))
        .port();

    let server = Arc::new(server);
    // One registry shared by every worker, exactly like `pager` — Task 5's
    // task surface needs a place to look up an in-flight task's status
    // regardless of which of the `WORKER_COUNT` workers services the poll.
    let registry = Arc::new(TaskRegistry::new());

    let workers = (0..WORKER_COUNT)
        .map(|_| {
            let server = Arc::clone(&server);
            let pager = Arc::clone(&pager);
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || worker_loop(&server, &pager, &registry))
        })
        .collect();

    (
        bound_port,
        ServerHandle {
            server,
            workers,
            scratch_dir: None,
        },
    )
}

/// One worker's whole life: pull requests off the shared server until it is
/// told to stop, dispatch each one against the shared pager, and respond.
///
/// Takes `pager` as the actual `&Arc<Mutex<Pager<S>>>` (not a bare
/// `&Mutex<Pager<S>>>`) so `api_task::dispatch` can `Arc::clone` it into a
/// background task-worker thread that outlives this request — the existing
/// `api_native`/`api_v1` calls below still take it as `&Mutex<Pager<S>>>`
/// unchanged, via the ordinary `&Arc<T> -> &T` deref coercion. `S: Send +
/// 'static` is a new bound on this function specifically because that
/// spawn requires it (`api_native`/`api_v1` never needed it).
fn worker_loop<S: Substrate + Send + 'static>(
    server: &tiny_http::Server,
    pager: &Arc<Mutex<Pager<S>>>,
    registry: &Arc<TaskRegistry>,
) {
    loop {
        let mut request = match server.recv() {
            Ok(r) => r,
            Err(e) if is_shutdown_signal(&e) => return,
            Err(e) => {
                // A real connection/accept fault, not `ServerHandle::shutdown`'s
                // deliberate `unblock()` — name it rather than letting this
                // worker vanish into a silent black hole.
                eprintln!("bloomery-daemon: http worker exiting on a fatal recv() error: {e}");
                return;
            }
        };
        // Read before `read_request` consumes `request`'s body via a `&mut`
        // reader — `headers()` only needs `&self`, so this borrow is over
        // and done before that mutable one starts.
        let agent_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("X-Bloomery-Agent"))
            .map(|h| h.value.as_str().to_string());
        match read_request(&mut request) {
            Ok((method, segments, body)) => {
                if segments.first().map(String::as_str) == Some("v1") {
                    let result =
                        api_v1::dispatch(pager, &method, &segments, &body, agent_header.as_deref());
                    respond_v1(request, result);
                } else if let Some((status, value)) =
                    api_task::dispatch(pager, registry, &method, &segments, &body)
                {
                    match value {
                        Some(value) => respond_json(request, status, &value),
                        None => respond_empty(request, status),
                    }
                } else {
                    match api_native::dispatch(pager, &method, &segments, &body) {
                        (status, Some(value)) => respond_json(request, status, &value),
                        (status, None) => respond_empty(request, status),
                    }
                }
            }
            Err(BodyTooLarge) => respond_json(
                request,
                413,
                &serde_json::json!({
                    "error": "body_too_large",
                    "max_bytes": MAX_BODY_BYTES,
                }),
            ),
        }
    }
}

/// True for the specific `io::Error` `tiny_http::Server::recv()` returns
/// when `Server::unblock()` woke a blocked receiver on purpose (see
/// `ServerHandle::shutdown`). tiny_http 0.12 collapses that deliberate
/// signal and a real connection/accept fault into the same
/// `io::Result<Request>::Err`; this exact kind + message (`messages_queue`'s
/// `Control::Unblock` maps to `IoError::new(IoErrorKind::Other, "thread
/// unblocked")`) is the only thing that tells them apart.
fn is_shutdown_signal(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::Other && e.to_string() == "thread unblocked"
}

/// The request's declared or actual body exceeded [`MAX_BODY_BYTES`].
struct BodyTooLarge;

/// Method name, `/`-split non-empty path segments (query string dropped),
/// and body — read off `req` without consuming it, since routing happens
/// before `req.respond(..)` can be called.
///
/// `Content-Length` is client-controlled, so the read is bounded to
/// [`MAX_BODY_BYTES`] `+ 1` regardless of what the client declared: reading
/// one byte past the cap (rather than exactly at it) is what lets an
/// exactly-at-cap body be told apart from a genuinely oversized one without
/// ever buffering more than `MAX_BODY_BYTES + 1` bytes in memory.
fn read_request(req: &mut Request) -> Result<(String, Vec<String>, String), BodyTooLarge> {
    let method = req.method().as_str().to_string();
    let path = req.url().split('?').next().unwrap_or("");
    let segments = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let mut body = String::new();
    // A client that sent non-UTF-8 bytes leaves `body` short or empty
    // rather than hanging or panicking the worker — malformed JSON is a
    // per-route 400, not a transport failure.
    let _ = req
        .as_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_string(&mut body);
    if body.len() as u64 > MAX_BODY_BYTES {
        return Err(BodyTooLarge);
    }
    Ok((method, segments, body))
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

/// Writes a `/v1` response: JSON gets `Content-Type: application/json`
/// exactly like the native API, SSE gets `Content-Type: text/event-stream`
/// (Task 15's `stream:true`, D3) — this is the one place that content type
/// is chosen, so a route can never send SSE framing under a JSON header or
/// vice versa. Any headers the route asked for (`X-Bloomery-Template`) are
/// layered on afterward.
fn respond_v1(req: Request, result: V1Result) {
    let (content_type, body) = match result.body {
        V1Body::Json(value) => (
            "application/json",
            serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string()),
        ),
        V1Body::Sse(body) => ("text/event-stream", body),
    };
    let content_type = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("static header is valid ASCII");
    let mut response = Response::from_string(body)
        .with_status_code(StatusCode(result.status))
        .with_header(content_type);
    for (name, value) in result.headers {
        if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response = response.with_header(h);
        }
        // An invalid header value (non-ASCII) is dropped rather than
        // failing the whole response — `X-Bloomery-Template` is always one
        // of two static ASCII strings today, so this path is unreachable in
        // practice, not a silent data loss risk.
    }
    let _ = req.respond(response);
}
