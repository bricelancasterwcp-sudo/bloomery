//! Bounded, continuously-drained capture of a subprocess's combined
//! stdout+stderr, used by `exec::exec_run` (Task 3).
//!
//! **Why continuous draining, not read-after-exit:** a naive
//! wait-then-read (spawn, poll `try_wait` in a loop, only touch the pipes
//! once the child has exited) deadlocks against a child that writes more
//! than one pipe buffer's worth of output (64 KiB on Linux) before exiting
//! — once the kernel pipe buffer fills, the child blocks inside its own
//! `write(2)` until *something* reads the other end, but nothing is
//! reading until after `try_wait` reports the child dead, which it never
//! will while the child is blocked. [`spawn_drain_thread`] runs for the
//! full life of the pipe instead, so a chatty child can always keep
//! writing (and therefore keep running toward its own exit, or toward
//! `exec_run`'s timeout) no matter how much output it produces.
//!
//! **Why bounded, not `read_to_end`:** the whole point of continuous
//! draining is that it must tolerate a child that never stops producing
//! output until it is killed. Buffering everything read would make the
//! daemon's own memory use track the child's output volume 1:1 — exactly
//! the OOM this crate's other caps (`ExecBounds::read_cap_bytes`,
//! `find_result_cap`) exist to avoid for the read/find executors. Every
//! byte past [`BoundedSink`]'s cap is decoded off the pipe and dropped, so
//! the sink itself never grows past `cap` bytes regardless of how long the
//! child keeps running.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A byte sink shared between `exec_run`'s two pipe-reader threads (stdout,
/// stderr) and its main poll loop. Bytes from both streams interleave in
/// whatever order they actually arrive at [`BoundedSink::push`] — this is
/// what "combined stdout+stderr" means here, not a stdout-then-stderr
/// concatenation.
pub(crate) struct BoundedSink {
    bytes: Vec<u8>,
    cap: usize,
    truncated: bool,
}

impl BoundedSink {
    fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::new(),
            cap,
            truncated: false,
        }
    }

    /// Appends as much of `chunk` as still fits under `cap`; any remainder
    /// is discarded (never buffered) and flips `truncated`, matching this
    /// module's docs on why the sink itself must stay bounded.
    fn push(&mut self, chunk: &[u8]) {
        let room = self.cap.saturating_sub(self.bytes.len());
        let take = room.min(chunk.len());
        if take > 0 {
            self.bytes.extend_from_slice(&chunk[..take]);
        }
        if take < chunk.len() {
            self.truncated = true;
        }
    }
}

/// A handle to a fresh, `cap`-bounded [`BoundedSink`], ready to hand to two
/// [`spawn_drain_thread`] calls (one per pipe) and later to [`take_captured`].
pub(crate) type CaptureSink = Arc<Mutex<BoundedSink>>;

/// Builds a fresh, empty, `cap`-bounded sink.
pub(crate) fn new_sink(cap: usize) -> CaptureSink {
    Arc::new(Mutex::new(BoundedSink::new(cap)))
}

/// Spawns a background thread that reads `pipe` to EOF, pushing every chunk
/// into `sink` (bounded — see [`BoundedSink`]) as it arrives. See this
/// module's docs for why this drains continuously rather than only after
/// the owning child exits.
///
/// The lock is only ever held across a `Vec::extend_from_slice`-sized
/// critical section (inside [`BoundedSink::push`]), which cannot panic —
/// poisoning here would mean a bug elsewhere already brought the process
/// down, so `.unwrap()` on the lock is the same "truly unreachable state"
/// case this crate reserves `unwrap` for, not a runtime condition being
/// waved away.
pub(crate) fn spawn_drain_thread<R: Read + Send + 'static>(
    mut pipe: R,
    sink: CaptureSink,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) => return, // EOF: the write end closed (child exited or was reaped)
                Ok(n) => sink.lock().unwrap().push(&buf[..n]),
                Err(_) => return, // pipe/read error — nothing more to drain
            }
        }
    })
}

/// Reads out the sink's final bytes and whether the capture was truncated.
/// Callers join both drain threads before calling this, so nothing is
/// still writing to `sink` — see `exec_run`.
pub(crate) fn take_captured(sink: &CaptureSink) -> (Vec<u8>, bool) {
    let guard = sink.lock().unwrap();
    (guard.bytes.clone(), guard.truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins [`BoundedSink::push`]'s truncation math directly, independent
    /// of any real subprocess: a chunk landing exactly on the cap boundary
    /// keeps everything and does not mark truncated; a chunk one byte over
    /// keeps exactly `cap` bytes and does mark truncated. Without this, a
    /// future off-by-one in `push` could silently under- or over-truncate
    /// and nothing at the `exec_run` test level would necessarily pin the
    /// exact boundary (those tests assert "truncated", not "truncated at
    /// exactly this byte").
    #[test]
    fn push_keeps_exactly_cap_bytes_and_flags_truncation_only_when_over() {
        let mut sink = BoundedSink::new(4);
        sink.push(b"abcd");
        assert_eq!(sink.bytes, b"abcd");
        assert!(!sink.truncated, "exactly at cap must not be truncated");

        sink.push(b"e");
        assert_eq!(sink.bytes, b"abcd", "no room left — nothing more is kept");
        assert!(sink.truncated);
    }

    /// A chunk that overshoots the cap in a single `push` call (rather than
    /// arriving byte-by-byte, as a real pipe read of arbitrary chunk size
    /// would) must still be clipped to exactly `cap` bytes, not accepted
    /// whole or dropped whole.
    #[test]
    fn a_single_oversized_chunk_is_clipped_to_the_cap() {
        let mut sink = BoundedSink::new(3);
        sink.push(b"abcdefgh");
        assert_eq!(sink.bytes, b"abc");
        assert!(sink.truncated);
    }
}
