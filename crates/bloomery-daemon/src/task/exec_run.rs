//! The `run` executor — the most security-sensitive executor in this
//! module, per the P3 brief. Split into its own file (rather than living in
//! `exec.rs` alongside `read`/`find`/`patch`) because the process-group
//! cleanup this file documents added enough to push `exec.rs` past its
//! 800-line cap.
//!
//! **A CRITICAL a security review found and this file fixes:** the
//! original implementation only ever signaled the *direct* child's own PID
//! (`Child::kill()`). A granted command that leaves behind a
//! pipe-inheriting descendant — a shell backgrounding a job
//! (`sh -c 'sleep 30 & exit 0'`), a test suite that daemonizes a server it
//! starts, anything that forks and does not itself exit — defeats that
//! entirely: `kill()` on the direct PID does nothing to the descendant, the
//! descendant still holds the stdout/stderr pipe's write end open, and
//! [`crate::task::run_capture`]'s drain threads (which read those pipes to
//! EOF) block forever waiting for a write end that will never close. Before
//! this fix, joining those threads unconditionally meant `exec_run` itself
//! could hang forever — wedging the task thread — while the descendant
//! leaked as a permanently running orphan. That is an unbounded DoS on
//! exactly the cargo/make/pytest-shaped workload this executor exists to
//! run, and it did not require a timeout to happen at all: a command whose
//! *direct* child exits cleanly and quickly (like the `sh -c 'sleep 30 &
//! exit 0'` example above) hit it too, with no kill ever triggered.
//!
//! Two independent fixes close both halves:
//!
//! 1. **Process-group kill, on every path, not just the timeout.** The
//!    child is spawned as the leader of its own new process group
//!    (`.process_group(0)`, [`std::os::unix::process::CommandExt`]) rather
//!    than inheriting the daemon's. `kill_process_group` then sends
//!    `SIGKILL` to the whole group via `libc::killpg`, not just the leader
//!    — reaching any descendant that itself never left the group (the
//!    common case: a plain `&` background job in a shell script does not
//!    call `setpgid`/`setsid`, so it stays in its parent's group). This
//!    runs unconditionally after the direct child is no longer running —
//!    on a timeout kill (to unblock the `wait` that follows it), on a
//!    `try_wait` OS error, *and* after an ordinary clean exit — because
//!    the clean-exit case is exactly the `sh -c 'sleep 30 & exit 0'`
//!    scenario above: the direct child's own exit code says nothing about
//!    whether it left anything running behind it.
//! 2. **Bounded, best-effort drain-join.** Even with the group kill,
//!    `exec_run` must not stake its own return on every pipe actually
//!    closing — a descendant that dup'd the pipe fd somewhere `killpg`
//!    cannot reach (handed it to a process outside the group, or one that
//!    escaped via `setsid` before the signal landed) would still be a
//!    "wait forever" bug even after the group is swept. So the drain
//!    threads are joined with a short deadline
//!    ([`DRAIN_JOIN_BUDGET`]); a thread still blocked past that deadline is
//!    *detached* (its `JoinHandle` dropped, not joined) rather than waited
//!    on further. The [`crate::task::run_capture::BoundedSink`] it writes
//!    into is behind a `Mutex` the main thread can still read regardless of
//!    whether the writer thread ever returns, so detaching costs nothing
//!    but that thread's own (bounded, capped-memory) OS resources — never
//!    `exec_run`'s own timeliness. This is what makes the *return* of
//!    `exec_run` unconditionally bounded (within `run_timeout_secs` plus a
//!    small, fixed margin) regardless of descendant behavior — the group
//!    kill above is what additionally makes that bound also apply to the
//!    processes it spawned, in the common case, but is not itself relied
//!    on for `exec_run`'s own liveness.
//!
//! **Named v1 limit, stated rather than assumed away** (matching this
//! crate's convention — see `bloomery_core::grant`'s own module docs for
//! the same discipline applied to the grant boundary): a process-group
//! kill is not a kernel namespace or cgroup. A descendant that calls
//! `setsid()`/`setpgid()` to leave the group before the signal arrives, or
//! that hands its inherited pipe fd to a process entirely outside the
//! group before then, can still survive the `killpg` and keep running.
//! Fix 2 (bounded join) is what keeps that scenario from also hanging
//! `exec_run` itself; it does not by itself kill the survivor. Closing
//! that fully would need a Linux PID namespace or a cgroup per `run`
//! action, tracked as future hardening, not silently claimed here.

use crate::task::exec::{describe, failed};
use crate::task::run_capture::{new_sink, spawn_drain_thread, take_captured};
use crate::task::{ExecBounds, Observation};
use bloomery_core::grant::Grant;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Fixed `PATH` every `run` action's subprocess gets, regardless of the
/// daemon's own environment. Never the invoking process's inherited
/// `PATH` — see [`exec_run`]'s doc comment for why that must not leak
/// through.
const RUN_PATH: &str = "/usr/bin:/bin";

/// Poll interval for [`exec_run`]'s timeout loop and for
/// [`join_or_detach`]'s bounded join — same value, same reasoning, as
/// `lens_py::PythonLens`'s `PY_COMPILE_TIMEOUT` loop and
/// `post::run_bounded`'s: short enough that a fast command isn't visibly
/// delayed, long enough not to busy-spin.
const RUN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Upper bound on how long `exec_run` waits for the stdout/stderr drain
/// threads to notice their pipe closed and return, once the direct child
/// (and, via [`kill_process_group`], its whole process group) is already
/// gone. Deliberately separate from `run_timeout_secs`: that bounds how
/// long the *command* may run; this bounds how long `exec_run` waits on an
/// OS pipe after nothing that should still be writing to it remains. See
/// this module's docs for the named v1 limit this budget exists to
/// contain — a descendant that escaped the process group can still hold a
/// drain thread open past this deadline, at which point that thread is
/// detached (not joined) so `exec_run` itself still returns on schedule.
const DRAIN_JOIN_BUDGET: Duration = Duration::from_secs(2);

/// Why [`exec_run`]'s poll loop can end without a normal exit status.
enum RunFailure {
    /// `bounds.run_timeout_secs` (floored — see [`exec_run`]) elapsed
    /// before the child exited; its process group has been killed and the
    /// direct child reaped.
    TimedOut,
    /// `Child::try_wait` itself returned an OS error (not the child's own
    /// exit — the `wait4`/`waitpid` call failed). Rare; handled
    /// fail-closed (group killed, direct child reaped best-effort) rather
    /// than left running.
    WaitFailed(std::io::Error),
}

/// Sends `SIGKILL` to every process in the group led by `pgid` — not just
/// one PID. `pgid` must be the *child's own* pid: [`exec_run`] spawns the
/// child with `.process_group(0)`, which makes it the leader of a
/// brand-new group, so `pgid == child.id()` always holds for every caller
/// in this file. A group with no living members (already exited, or the
/// call races a member's own natural exit) is a harmless `ESRCH`, not
/// checked here — this is best-effort cleanup, same posture as every
/// other `kill`/`wait` call in this crate (`lens_py::run_py_compile`,
/// `post::run_bounded`).
fn kill_process_group(pgid: i32) {
    // SAFETY: `killpg` is a plain libc call taking two integers and
    // touching no memory this side of the FFI boundary; `pgid` is always
    // the child's own pid per this fn's doc comment, so this can never
    // target a process group this call did not itself create.
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
}

/// Joins `handle` if it finishes before `deadline`; otherwise detaches it
/// (drops the `JoinHandle` without joining) so a thread still blocked on a
/// pipe read cannot hold `exec_run`'s own return hostage — see this
/// module's docs on why that detach is safe (the sink it writes into stays
/// readable regardless).
fn join_or_detach(handle: JoinHandle<()>, deadline: Instant) {
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            drop(handle); // detach — see this fn's doc comment
            return;
        }
        std::thread::sleep(RUN_POLL_INTERVAL);
    }
    let _ = handle.join();
}

/// Execute a `Run` action against `grant`.
///
/// `grant.check_command(argv)` runs **first**. On a violation this returns
/// a failed [`Observation`] and **nothing is ever spawned** — no `Command`
/// is even constructed on that path (`tests/task_exec_run_test.rs` proves
/// this against a canary file: an ungranted `rm`-shaped `argv` leaves the
/// file untouched).
///
/// On a granted command, `argv` runs **directly** —
/// `Command::new(&argv[0]).args(&argv[1..])` — never through a shell,
/// never `sh -c`: no `$VAR` expansion, no `;`/`|`/`&&` splitting into a
/// second command, no globbing. A model that puts shell metacharacters in
/// an argument gets that argument back byte-for-byte in the child's
/// `argv` (see `no_shell_interpretation` in the test file). If the
/// *grant* itself names a shell (an operator choice, not this executor's),
/// that shell can still background its own descendants — see this
/// module's docs for how those are cleaned up.
///
/// The environment is fully scrubbed and rebuilt from nothing:
/// `.env_clear()` then exactly `PATH` (fixed at [`RUN_PATH`], never the
/// daemon's own), `HOME` (`cwd`), and `LANG=C` — no proxy vars, no
/// inherited secrets. `stdin` is `/dev/null`; stdout+stderr are piped and
/// drained into one combined, bounded buffer — see
/// [`crate::task::run_capture`]'s docs for why that draining is
/// continuous (not read-after-exit) and capped (not `read_to_end`).
///
/// **Timeout and cleanup:** `bounds.run_timeout_secs` is floored to at
/// least 1 (`.max(1)`) — an operator-misconfigured `0` would otherwise
/// kill every run on the very first poll iteration, which is far more
/// likely to be a config mistake than an intentional "never allow this to
/// run" setting; the floor makes that failure mode "runs for up to ~1s"
/// instead of "never runs at all", and is documented rather than silently
/// special-cased. On expiry, [`kill_process_group`] (`SIGKILL`, not
/// `SIGTERM` — a child that ignores termination signals, the 2a
/// `llama-server` case this pattern was built to survive, still dies)
/// followed by `child.wait()` to reap the direct child (skipping that
/// `wait` would leave it a zombie for the daemon's life). The process
/// group is swept **again, unconditionally, after every exit path** —
/// including a clean, on-time exit — because a direct child that exited
/// promptly can still have left a backgrounded descendant running (see
/// this module's docs); this is what actually kills that descendant, not
/// just what unblocks a timed-out `wait`. Drain threads are then joined
/// with [`join_or_detach`]'s bounded deadline, not unconditionally — see
/// this module's docs for why an unconditional join was the other half of
/// the bug this file fixes.
///
/// **`failed` is `false` for any run that *completes*, at any exit code —
/// including non-zero.** A `run` action's verb is "run this command", not
/// "run it successfully": a non-zero exit is a legitimate observation the
/// model acts on, not an executor failure. `failed` is `true` only when
/// the verb itself was not carried out: a grant violation or spawn
/// failure (never ran), or a timeout (started but never finished).
///
/// This executor never touches the pager lock — the subprocess runs
/// entirely outside it; Task 4 decides whether/when a `run` action holds
/// or releases the pager while this executes.
pub fn exec_run(grant: &Grant, cwd: &Path, argv: &[String], bounds: &ExecBounds) -> Observation {
    if let Err(v) = grant.check_command(argv) {
        return failed(format!("grant violation: {}", describe(&v)));
    }
    // `check_command` rejects an empty `argv` (see
    // `bloomery_core::grant::command::check_command`), so `argv[0]` and
    // `argv[1..]` below can never panic.
    let program = argv[0].clone();

    let mut child = match Command::new(&program)
        .args(&argv[1..])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", RUN_PATH)
        .env("HOME", cwd)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Leader of a brand-new process group — see this module's docs
        // and `kill_process_group`'s doc comment for why this is what
        // lets a later `killpg` reach descendants the child itself forks,
        // which a plain `child.kill()` (signals only the direct pid)
        // cannot.
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return failed(format!("run failed: could not spawn {program:?}: {e}")),
    };
    // `.process_group(0)` above makes the child its own group's leader,
    // so its pid IS that group's pgid.
    let pgid = child.id() as i32;

    let stdout = child.stdout.take().expect("stdout piped above");
    let stderr = child.stderr.take().expect("stderr piped above");
    let sink = new_sink(bounds.run_output_cap_bytes);
    let out_thread = spawn_drain_thread(stdout, Arc::clone(&sink));
    let err_thread = spawn_drain_thread(stderr, Arc::clone(&sink));

    let run_timeout_secs = bounds.run_timeout_secs.max(1);
    let timeout = Duration::from_secs(run_timeout_secs);
    let started = Instant::now();
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                kill_process_group(pgid);
                let _ = child.wait();
                break Err(RunFailure::TimedOut);
            }
            Ok(None) => std::thread::sleep(RUN_POLL_INTERVAL),
            Err(e) => {
                kill_process_group(pgid);
                let _ = child.wait();
                break Err(RunFailure::WaitFailed(e));
            }
        }
    };
    // Unconditional final sweep — see this fn's doc comment on why a
    // clean, on-time exit still needs this: it is a no-op (ESRCH,
    // ignored) for the already-killed timeout/error paths above, and it
    // is the *only* cleanup a normal exit ever gets.
    kill_process_group(pgid);

    let join_deadline = Instant::now() + DRAIN_JOIN_BUDGET;
    join_or_detach(out_thread, join_deadline);
    join_or_detach(err_thread, join_deadline);
    let (bytes, truncated) = take_captured(&sink);
    let mut output = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        output.push_str(&format!(
            "\n[note: output truncated at {} bytes]",
            bounds.run_output_cap_bytes
        ));
    }

    match outcome {
        Ok(status) => {
            // `code()` is `None` when the child died from a signal rather
            // than exiting normally; -1 is not a real exit code and reads
            // as "no code", the same sentinel `post::run_bounded`'s sibling
            // uses for the identical case.
            let code = status.code().unwrap_or(-1);
            Observation {
                outcome: format!("ran {program} exit {code}"),
                content: format!("exit {code}\n{output}"),
                failed: false,
            }
        }
        Err(RunFailure::TimedOut) => Observation {
            outcome: format!("ran {program} timed out"),
            content: format!("timed out after {run_timeout_secs}s\n{output}"),
            failed: true,
        },
        Err(RunFailure::WaitFailed(e)) => {
            failed(format!("run failed: could not wait for {program:?}: {e}"))
        }
    }
}
